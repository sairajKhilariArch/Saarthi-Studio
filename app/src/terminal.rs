//! Interactive terminal sessions for the bottom panel.
//!
//! One PTY per session, keyed by an id. The shell starts in the caller-supplied
//! directory (the active project's folder). Sessions survive panel hiding and
//! project switches — one session per directory, reused when that project
//! returns. Output is broadcast on a session channel; each SSE subscriber gets
//! a live stream of JSON-quoted chunks (JSON so PTY control bytes cannot break
//! the framing). A wait thread reaps the child when it exits, and the close
//! endpoint drops the PTY master so the shell receives SIGHUP. On application
//! exit the kernel closes the master fds, which SIGHUPs every running shell.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json as AxumJson, Router};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};

pub(crate) type Hub = Arc<TerminalHub>;

pub(crate) struct TerminalHub {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<u64, TerminalSession>>,
}

struct TerminalSession {
    id: u64,
    tx: tokio::sync::broadcast::Sender<String>,
    alive: Arc<AtomicBool>,
    master: Box<dyn MasterPty + Send>,
    writer: Mutex<Option<Box<dyn std::io::Write + Send>>>,
    cwd: String,
}

impl TerminalSession {
    fn alive(&self) -> &AtomicBool {
        &self.alive
    }
}

impl TerminalHub {
    fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (status, AxumJson(json!({ "ok": false, "error": message }))).into_response()
}

fn shell_command(cwd: &str) -> CommandBuilder {
    #[cfg(target_os = "windows")]
    let mut cmd = CommandBuilder::new("powershell.exe");
    #[cfg(not(target_os = "windows"))]
    let mut cmd = CommandBuilder::new(
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
    );
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd
}

pub(crate) fn hub() -> Hub {
    Arc::new(TerminalHub::new())
}

async fn terminal_open(State(state): State<crate::AppState>, AxumJson(input): AxumJson<Value>) -> Response {
    let hub = state.terminal.clone();
    let cwd = input
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/')
        .to_string();
    let cols = input.get("cols").and_then(Value::as_u64).unwrap_or(80).max(20) as u16;
    let rows = input.get("rows").and_then(Value::as_u64).unwrap_or(24).max(5) as u16;
    if cwd.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "Set a project folder before opening a terminal.");
    }
    if !std::path::Path::new(&cwd).is_dir() {
        return json_error(StatusCode::BAD_REQUEST, format!("Folder does not exist: {cwd}").as_str());
    }
    {
        // One session per directory; switching projects leaves the old one
        // running so returning to that project keeps its scrollback.
        let sessions = hub.sessions.lock().unwrap();
        if let Some(found) = sessions.values().find(|s| s.cwd == cwd && s.alive().load(Ordering::SeqCst)) {
            return AxumJson(json!({
                "ok": true, "id": found.id, "cwd": found.cwd, "reused": true
            }))
            .into_response();
        }
    }

    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };

    let child = match pair.slave.spawn_command(shell_command(&cwd)) {
        Ok(child) => child,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };
    let reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };
    let writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };

    let id = hub.next_id.fetch_add(1, Ordering::SeqCst);
    let alive = Arc::new(AtomicBool::new(true));
    let (tx, _) = tokio::sync::broadcast::channel::<String>(1024);

    // Fan the PTY master out to every subscriber, JSON-escaped so neither a
    // namespace nor control bytes can break the SSE framing.
    let task_tx = tx.clone();
    let task_alive = alive.clone();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read_count) => {
                    let chunk = String::from_utf8_lossy(&buffer[..read_count]);
                    let _ = task_tx.send(serde_json::to_string(chunk.as_ref()).unwrap_or_default());
                }
                Err(error) => {
                    if error.kind() != std::io::ErrorKind::Interrupted {
                        break;
                    }
                }
            }
        }
        task_alive.store(false, Ordering::SeqCst);
    });

    // Reap the child (and finally drop our reference to it) on exit.
    let reap_alive = alive.clone();
    let reap_tx = tx.clone();
    tokio::task::spawn_blocking(move || {
        let mut child = child;
        let _ = child.wait();
        reap_alive.store(false, Ordering::SeqCst);
        let _ = reap_tx.send("\"__exit__\"".to_string());
    });

        hub.sessions.lock().unwrap().insert(
            id,
            TerminalSession {
                id,
                tx,
                alive: alive.clone(),
                master: pair.master,
                writer: Mutex::new(Some(writer)),
                cwd: cwd.clone(),
            },
        );

    AxumJson(json!({ "ok": true, "id": id, "cwd": cwd, "reused": false })).into_response()
}

async fn terminal_stream(
    State(state): State<crate::AppState>,
    Path(id): Path<u64>,
) -> Response {
    let hub = state.terminal.clone();
    let session = hub
        .sessions
        .lock()
        .unwrap()
        .get(&id)
        .map(|s| (s.tx.subscribe(), s.alive.clone()));
    let Some((rx, alive)) = session else {
        return json_error(StatusCode::NOT_FOUND, "No such terminal session. Open the panel again.");
    };
    let stream = futures_util::stream::unfold((rx, alive), |(mut rx, alive)| async move {
        if !alive.load(Ordering::SeqCst) {
            return Some((Ok::<_, std::convert::Infallible>(SseEvent::default().event("done")), (rx, alive)));
        }
        match rx.recv().await {
            Ok(data) => Some((Ok(SseEvent::default().data(data)), (rx, alive))),
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                Some((
                    Ok(SseEvent::default().data("\"… output skipped while output was fast …\"")),
                    (rx, alive),
                ))
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

async fn terminal_input(
    State(state): State<crate::AppState>,
    Path(id): Path<u64>,
    AxumJson(input): AxumJson<Value>,
) -> Response {
    let data = match input.get("data").and_then(Value::as_str) {
        Some(data) => data.to_string(),
        None => return json_error(StatusCode::BAD_REQUEST, "Missing terminal input."),
    };
    let hub = state.terminal.clone();
    let send_result = hub.sessions.lock().unwrap().get(&id).and_then(|session| {
        let mut writer_guard = session.writer.lock().unwrap();
        writer_guard.as_mut().map(|writer| writer.write_all(data.as_bytes()).and_then(|_| writer.flush()))
    });
    match send_result {
        Some(Ok(())) => AxumJson(json!({ "ok": true })).into_response(),
        Some(Err(error)) => json_error(StatusCode::BAD_REQUEST, &error.to_string()),
        None => json_error(StatusCode::NOT_FOUND, "No such terminal session."),
    }
}

async fn terminal_resize(
    State(state): State<crate::AppState>,
    Path(id): Path<u64>,
    AxumJson(input): AxumJson<Value>,
) -> Response {
    let cols = input.get("cols").and_then(Value::as_u64).unwrap_or(80).clamp(20, 500) as u16;
    let rows = input.get("rows").and_then(Value::as_u64).unwrap_or(24).clamp(5, 200) as u16;
    let hub = state.terminal.clone();
    let result = hub.sessions.lock().unwrap().get(&id).map(|session| {
        session
            .master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
    });
    match result {
        Some(Ok(())) => AxumJson(json!({ "ok": true })).into_response(),
        Some(Err(error)) => json_error(StatusCode::BAD_REQUEST, &error.to_string()),
        None => json_error(StatusCode::NOT_FOUND, "No such terminal session."),
    }
}

async fn terminal_close(
    State(state): State<crate::AppState>,
    Path(id): Path<u64>,
) -> Response {
    let hub = state.terminal.clone();
    let closed = hub.sessions.lock().unwrap().remove(&id).is_some();
    // Dropping the master closes the PTY; the shell SIGHUPs and exits.
    if closed {
        AxumJson(json!({ "ok": true })).into_response()
    } else {
        json_error(StatusCode::NOT_FOUND, "No such terminal session.")
    }
}

/// Mutating routes are local-origin guarded like every other POST route; the
/// stream endpoint is read-only and therefore outside the guard.
pub(crate) fn router(port: u16) -> Router<crate::AppState> {
    let guarded = Router::new()
        .route("/api/terminal/open", post(terminal_open))
        .route("/api/terminal/:id/input", post(terminal_input))
        .route("/api/terminal/:id/resize", post(terminal_resize))
        .route("/api/terminal/:id/close", post(terminal_close))
        .route_layer(axum::middleware::from_fn_with_state(
            crate::workspace::LocalOriginGuard::new(port),
            crate::workspace::guard_local_origin,
        ));
    let read_only = Router::new().route("/api/terminal/:id/stream", get(terminal_stream));
    guarded.merge(read_only)
}
