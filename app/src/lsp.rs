//! Phase 9 — language-server diagnostics.
//!
//! Phase 9's remaining half asks for what a language server knows. Compiler
//! output already reaches the Problems pane through the runner; this module adds
//! the second source: a bounded LSP client that starts the server installed for
//! the open file's language, initializes it, opens the buffer, waits a short
//! while for `textDocument/publishDiagnostics`, and shuts it down.
//!
//! Deliberate limits, all reported rather than hidden:
//!
//!   * one request starts one process and asks for one file — there is no
//!     pooled, long-lived session and no background re-check;
//!   * five seconds to publish, ten to answer `initialize`; a server that stays
//!     quiet is reported as "published nothing", not as "no problems";
//!   * a language with no mapping, or a server that is not on `PATH`, answers
//!     with the reason instead of an empty list;
//!   * no debugger. This is diagnostics only.
//!
//! Nothing is inferred from a model, a filename or a previous run: every row is
//! a `publishDiagnostics` item the server sent for the URI we opened.

use crate::{json_error, AppState};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// A buffer larger than this is refused before a server is started.
const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024;
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(600);
pub(crate) const MAX_DIAGNOSTICS: usize = 500;
const MAX_MESSAGE_CHARS: usize = 500;
const MAX_SOURCE_CHARS: usize = 40;
/// Two servers at once is already unusual; more would only queue up processes.
const MAX_CONCURRENT_DIAGNOSTICS: usize = 2;
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// One server, the file types it answers for and the LSP language id to send.
#[derive(Clone, Copy)]
struct ServerSpec {
    program: &'static str,
    args: &'static [&'static str],
    language_id: &'static str,
    extensions: &'static [&'static str],
}

const SERVERS: &[ServerSpec] = &[
    ServerSpec {
        program: "rust-analyzer",
        args: &[],
        language_id: "rust",
        extensions: &["rs"],
    },
    ServerSpec {
        program: "typescript-language-server",
        args: &["--stdio"],
        language_id: "typescript",
        extensions: &["ts", "tsx", "mts", "cts"],
    },
    ServerSpec {
        program: "typescript-language-server",
        args: &["--stdio"],
        language_id: "javascript",
        extensions: &["js", "jsx", "mjs", "cjs"],
    },
    ServerSpec {
        program: "pylsp",
        args: &[],
        language_id: "python",
        extensions: &["py"],
    },
    ServerSpec {
        program: "gopls",
        args: &["serve"],
        language_id: "go",
        extensions: &["go"],
    },
    ServerSpec {
        program: "clangd",
        args: &[],
        language_id: "c",
        extensions: &["c"],
    },
    ServerSpec {
        program: "clangd",
        args: &[],
        language_id: "cpp",
        extensions: &["cpp", "cc", "cxx", "hpp"],
    },
];

fn spec_for(path: &str) -> Option<&'static ServerSpec> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    let extension = extension.to_ascii_lowercase();
    SERVERS
        .iter()
        .find(|spec| spec.extensions.contains(&extension.as_str()))
}

fn clean_path(value: &str) -> Result<String, String> {
    let path = value.trim().replace('\\', "/");
    if path.is_empty() {
        return Err("Open a file first.".to_string());
    }
    if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        return Err("Use a path relative to the open folder.".to_string());
    }
    if path.chars().any(|ch| ch.is_control()) {
        return Err("That path contains a control character.".to_string());
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err("That path leaves the open folder.".to_string());
    }
    Ok(path)
}

/// A `file:` URI an LSP server accepts: forward slashes, a leading slash before
/// a drive letter, and everything but unreserved characters percent-encoded.
fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || "-._~/".contains(ch) || ch == ':' {
            uri.push(ch);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

fn cap(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[derive(Debug)]
struct Launch {
    program: PathBuf,
    args: Vec<String>,
}

/// The language ids a saved server command may name. Kept beside the built-in
/// table so a save can never promise a language this build cannot serve.
pub(crate) const LANGUAGES: &[&str] = &[
    "rust",
    "typescript",
    "javascript",
    "python",
    "go",
    "c",
    "cpp",
];

/// A saved `language :: command` line, split into the program and its arguments.
/// Double quotes group an argument, so a Windows path with a space survives; no
/// shell is involved, so nothing is expanded or interpreted.
fn split_command(command: &str) -> Option<(String, Vec<String>)> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in command.trim().chars() {
        match ch {
            '"' => quoted = !quoted,
            ch if ch.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            ch => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    if tokens.is_empty() || quoted {
        return None;
    }
    let program = tokens.remove(0);
    Some((program, tokens))
}

/// How to start the server for `spec`: a command the user saved for that
/// language wins, and the built-in program name is found on `PATH` otherwise.
/// The error is the sentence the route shows, so "configured but missing" and
/// "not installed" cannot be confused.
fn launch_for(spec: &ServerSpec, servers: &[(String, String)]) -> Result<Launch, String> {
    if let Some((_, command)) = servers
        .iter()
        .find(|(language, _)| language == spec.language_id)
    {
        let (program, args) = split_command(command).ok_or_else(|| {
            format!(
                "The saved language-server command for {} is empty or has an unclosed quote.",
                spec.language_id
            )
        })?;
        let resolved = crate::find_on_path(&program)
            .or_else(|| {
                let direct = std::path::PathBuf::from(&program);
                direct.is_file().then_some(direct)
            })
            .ok_or_else(|| {
                format!(
                    "{program} (saved for {} language diagnostics) was not found on PATH or at \
                     that path.",
                    spec.language_id
                )
            })?;
        return Ok(Launch {
            program: resolved,
            args,
        });
    }
    crate::find_on_path(spec.program)
        .map(|program| Launch {
            program,
            args: spec.args.iter().map(|arg| arg.to_string()).collect(),
        })
        .ok_or_else(|| {
            format!(
                "{} is not on PATH, so no language-server diagnostics were read.",
                spec.program
            )
        })
}

/// One `Content-Length` framed JSON-RPC message, or `None` at end of stream.
fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some(value) = header
            .strip_prefix("Content-Length:")
            .or_else(|| header.strip_prefix("content-length:"))
        {
            length = value.trim().parse().ok()?;
        }
    }
    // A header without a usable length, or one that asks for an unreasonable
    // body, ends the conversation instead of allocating it.
    if length == 0 || length > 4 * 1024 * 1024 {
        return None;
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadFailure {
    Timeout,
    Closed,
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
    buffered: VecDeque<Value>,
    reader: Option<std::thread::JoinHandle<()>>,
    next_id: u64,
}

impl Session {
    fn start(root: &Path, launch: &Launch) -> Result<Self, String> {
        // `spawn` cannot run a Windows `.cmd` shim directly (npm installs one
        // for typescript-language-server), so those go through the command
        // interpreter with the path and arguments passed untouched.
        let batch = launch
            .program
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| matches!(extension.to_ascii_lowercase().as_str(), "cmd" | "bat"))
            .unwrap_or(false);
        let mut command = if batch {
            let mut command = Command::new("cmd");
            command.arg("/C").arg(&launch.program);
            command
        } else {
            Command::new(&launch.program)
        };
        command
            .args(&launch.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|error| format!("Could not start {}: {error}", launch.program.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "The language server has no stdin.".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "The language server has no stdout.".to_string())?;
        let (sender, messages) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut source = BufReader::new(stdout);
            while let Some(message) = read_message(&mut source) {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            messages,
            buffered: VecDeque::new(),
            reader: Some(reader),
            next_id: 0,
        })
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        let body = serde_json::to_string(message).map_err(|error| error.to_string())?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stdin
            .write_all(header.as_bytes())
            .and_then(|_| self.stdin.write_all(body.as_bytes()))
            .and_then(|_| self.stdin.flush())
            .map_err(|error| format!("Could not write to the language server: {error}"))
    }

    fn next_message(&mut self, deadline: Instant) -> Result<Value, ReadFailure> {
        if let Some(value) = self.buffered.pop_front() {
            return Ok(value);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ReadFailure::Timeout);
        }
        match self.messages.recv_timeout(remaining) {
            Ok(value) => Ok(value),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ReadFailure::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ReadFailure::Closed),
        }
    }

    fn request(&mut self, method: &str, params: Value, deadline: Instant) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        // Every message is examined exactly once: what earlier phases buffered
        // first, then each new arrival. A non-matching message is set aside,
        // never re-queued into the loop that reads it — re-queuing would spin
        // on it forever while the answer waits unread in the channel, and the
        // deadline would never be consulted. The aside messages return to the
        // buffer on exit so a later phase still sees them, in arrival order.
        let mut queue = std::mem::take(&mut self.buffered);
        let mut aside: VecDeque<Value> = VecDeque::new();
        let outcome = loop {
            let message = match queue.pop_front() {
                Some(message) => message,
                None => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break Err(format!("{method} did not answer in time."));
                    }
                    match self.messages.recv_timeout(remaining) {
                        Ok(message) => message,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            break Err(format!("{method} did not answer in time."))
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            break Err(format!("The language server stopped during {method}."))
                        }
                    }
                }
            };
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    let text = error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("the server returned an error");
                    break Err(format!("{method}: {text}"));
                }
                break Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            aside.push_back(message);
        };
        self.buffered = aside;
        self.buffered.extend(queue);
        outcome
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The client owns the process: a timed-out or aborted request must not
        // leave a language server running behind it.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// One row per published item, in the shape the Problems pane already renders.
fn diagnostic_rows(value: &Value, path: &str) -> Vec<Value> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .take(MAX_DIAGNOSTICS)
        .filter_map(|item| {
            let message = item
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if message.is_empty() {
                return None;
            }
            let line = item
                .pointer("/range/start/line")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                + 1;
            let column = item
                .pointer("/range/start/character")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                + 1;
            let severity = match item.get("severity").and_then(Value::as_u64) {
                Some(1) => "error",
                Some(2) => "warning",
                Some(3) => "info",
                _ => "hint",
            };
            let code = item
                .get("code")
                .map(|code| match code {
                    Value::String(text) => cap(text, MAX_SOURCE_CHARS),
                    other => cap(&other.to_string(), MAX_SOURCE_CHARS),
                })
                .unwrap_or_default();
            Some(json!({
                "path": path,
                "line": line,
                "column": column,
                "severity": severity,
                "message": cap(message, MAX_MESSAGE_CHARS),
                "source": cap(
                    item.get("source").and_then(Value::as_str).unwrap_or(""),
                    MAX_SOURCE_CHARS,
                ),
                "code": code,
            }))
        })
        .collect()
}

#[derive(Debug)]
struct Outcome {
    diagnostics: Vec<Value>,
    published: bool,
    elapsed_ms: u64,
}

fn run(
    root: &Path,
    path: &str,
    spec: &ServerSpec,
    content: &str,
    launch: &Launch,
) -> Result<Outcome, String> {
    let started = Instant::now();
    let mut session = Session::start(root, launch)?;
    let root_uri = file_uri(root);
    let document = file_uri(&root.join(path));
    session.request(
        "initialize",
        json!({
            "processId": null,
            "clientInfo": {
                "name": "sarthi-studio",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "rootUri": root_uri,
            "capabilities": {},
            "workspaceFolders": [{ "uri": root_uri, "name": "workspace" }],
        }),
        Instant::now() + INITIALIZE_TIMEOUT,
    )?;
    session.send(&json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {},
    }))?;
    session.send(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": document,
                "languageId": spec.language_id,
                "version": 1,
                "text": content,
            },
        },
    }))?;

    let deadline = Instant::now() + DIAGNOSTIC_TIMEOUT;
    let mut published = None;
    // The same one-pass rule as `request`: handshake-buffered messages are
    // examined once, then each new arrival. A publish for another file is set
    // aside, not re-queued into the loop that reads it, so the wait keeps
    // reading until the open buffer's own publish arrives or time runs out.
    let mut queue = std::mem::take(&mut session.buffered);
    let mut aside: VecDeque<Value> = VecDeque::new();
    loop {
        let message = match queue.pop_front() {
            Some(message) => message,
            None => match session.next_message(deadline) {
                Ok(message) => message,
                Err(ReadFailure::Timeout) => break,
                Err(ReadFailure::Closed) => {
                    return Err(
                        "The language server stopped before publishing diagnostics.".to_string()
                    )
                }
            },
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        if method == "textDocument/publishDiagnostics" {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            // Only the file we opened: a workspace-wide publish for
            // other files is not an answer to this request.
            if params.get("uri").and_then(Value::as_str) == Some(document.as_str()) {
                let items = params
                    .get("diagnostics")
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                let empty = items.as_array().map_or(true, |rows| rows.is_empty());
                published = Some(items);
                if !empty {
                    break;
                }
                // An empty publish is the server's opening state, not its
                // answer: rust-analyzer clears first and sends the real rows
                // once the crate graph loads. Keep the empty list as the
                // answer so far and keep reading; a non-empty publish wins,
                // and if the deadline arrives first the empty list stands.
            }
        }
        aside.push_back(message);
    }
    session.buffered = aside;
    session.buffered.extend(queue);

    // Best effort: a server that does not answer `shutdown` is killed by Drop.
    let _ = session.request("shutdown", Value::Null, Instant::now() + SHUTDOWN_TIMEOUT);
    let _ = session.send(&json!({ "jsonrpc": "2.0", "method": "exit" }));

    let diagnostics = published
        .as_ref()
        .map(|value| diagnostic_rows(value, path))
        .unwrap_or_default();
    Ok(Outcome {
        diagnostics,
        published: published.is_some(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

async fn diagnostics_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => return error.into_response(),
    };
    let raw_path = input.get("path").and_then(Value::as_str).unwrap_or("");
    let path = match clean_path(raw_path) {
        Ok(path) => path,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message).into_response(),
    };
    let content = match input.get("content").and_then(Value::as_str) {
        Some(text) => text.to_string(),
        None => match std::fs::read_to_string(root.join(&path)) {
            Ok(text) => text,
            Err(error) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    format!("Could not read {path}: {error}"),
                )
                .into_response()
            }
        },
    };
    if content.len() > MAX_CONTENT_BYTES {
        return json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "That buffer is larger than 2 MiB.",
        )
        .into_response();
    }
    let Some(spec) = spec_for(&path) else {
        return Json(json!({
            "ok": true,
            "path": path,
            "server": null,
            "installed": false,
            "diagnostics": [],
            "note": "No language server is mapped to this file type.",
        }))
        .into_response();
    };
    let servers = crate::language_servers(&state);
    let launch = match launch_for(spec, &servers) {
        Ok(launch) => launch,
        Err(note) => {
            return Json(json!({
                "ok": true,
                "path": path,
                "server": spec.program,
                "installed": false,
                "diagnostics": [],
                "note": note,
            }))
            .into_response();
        }
    };
    if RUNNING.fetch_add(1, Ordering::SeqCst) >= MAX_CONCURRENT_DIAGNOSTICS {
        RUNNING.fetch_sub(1, Ordering::SeqCst);
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "Two diagnostics runs are already in flight; try again in a moment.",
        )
        .into_response();
    }
    let task_root = root.clone();
    let task_path = path.clone();
    let task_spec = *spec;
    let task_content = content;
    let result = tokio::task::spawn_blocking(move || {
        run(&task_root, &task_path, &task_spec, &task_content, &launch)
    })
    .await;
    RUNNING.fetch_sub(1, Ordering::SeqCst);
    match result {
        Ok(Ok(outcome)) => Json(json!({
            "ok": true,
            "path": path,
            "server": spec.program,
            "installed": true,
            "language": spec.language_id,
            "diagnostics": outcome.diagnostics,
            "published": outcome.published,
            "elapsed_ms": outcome.elapsed_ms,
            "note": if outcome.published {
                Value::Null
            } else {
                json!("The language server did not publish diagnostics within 5 seconds.")
            },
        }))
        .into_response(),
        Ok(Err(message)) => json_error(StatusCode::BAD_GATEWAY, message).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The language-server task stopped unexpectedly.",
        )
        .into_response(),
    }
}

/// Which servers this build knows about, and whether each is installed. Reads
/// `PATH` only; it starts nothing.
async fn servers_route(State(state): State<AppState>) -> Json<Value> {
    let saved = crate::language_servers(&state);
    let mut rows: Vec<Value> = Vec::new();
    for spec in SERVERS {
        // One row per language, so a saved command for `cpp` is visible even
        // though it shares `clangd` with `c`.
        if rows
            .iter()
            .any(|row| row.get("language") == Some(&json!(spec.language_id)))
        {
            continue;
        }
        let extensions: Vec<&str> = SERVERS
            .iter()
            .filter(|other| other.language_id == spec.language_id)
            .flat_map(|other| other.extensions.iter().copied())
            .collect();
        // A saved command is what actually runs, so it decides `installed` and
        // is reported verbatim for the settings note.
        let configured = saved
            .iter()
            .find(|(language, _)| language == spec.language_id)
            .map(|(_, command)| command.clone());
        let installed = launch_for(spec, &saved).map(|launch| launch.program).ok();
        rows.push(json!({
            "server": spec.program,
            "language": spec.language_id,
            "extensions": extensions,
            "configured": configured,
            "installed": installed.is_some(),
            "program": installed,
        }));
    }
    Json(json!({ "ok": true, "servers": rows }))
}

/// Merged into `workspace::router`, so both routes inherit the loopback
/// `Host`/`Origin` guard and the request body cap.
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspace/language-diagnostics",
            post(diagnostics_route),
        )
        .route("/api/lsp/servers", get(servers_route))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("sarthi-lsp-{name}-{}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extensions_map_to_one_server_and_unknown_types_have_none() {
        assert_eq!(
            spec_for("src/main.rs").map(|spec| spec.program),
            Some("rust-analyzer")
        );
        assert_eq!(
            spec_for("web/App.tsx").map(|spec| spec.language_id),
            Some("typescript")
        );
        assert_eq!(
            spec_for("scripts\\run.mjs").map(|spec| spec.language_id),
            Some("javascript")
        );
        assert_eq!(
            spec_for("main.py").map(|spec| spec.language_id),
            Some("python")
        );
        assert!(spec_for("README.md").is_none());
        assert!(spec_for("Makefile").is_none());
        assert!(
            spec_for(".gitignore").is_none(),
            "a dotfile has no stem, so it is not a Rust file"
        );
    }

    /// Issue 45: diagnostics used to be PATH-only. A saved command now wins for
    /// its language, and a configured program that is not there says so instead
    /// of being reported as "not on PATH".
    #[test]
    fn a_saved_server_command_wins_over_the_builtin_lookup() {
        let node = crate::find_on_path("node").expect("node is on PATH for the test suite");
        let spec = spec_for("src/main.rs").expect("rust spec");
        let saved = vec![(
            "rust".to_string(),
            format!("\"{}\" --stdio", node.display()),
        )];
        let launch = launch_for(spec, &saved).expect("a saved command resolves");
        assert_eq!(launch.program, node);
        assert_eq!(launch.args, vec!["--stdio".to_string()]);

        let missing = vec![(
            "rust".to_string(),
            "definitely-not-a-real-lsp-binary".to_string(),
        )];
        let error = launch_for(spec, &missing).expect_err("a missing program is refused");
        assert!(error.contains("saved for rust"), "{error}");
        assert!(
            error.contains("definitely-not-a-real-lsp-binary"),
            "{error}"
        );
    }

    #[test]
    fn a_saved_command_splits_on_spaces_and_keeps_a_quoted_path() {
        assert_eq!(
            split_command("  rust-analyzer  "),
            Some(("rust-analyzer".to_string(), Vec::new()))
        );
        assert_eq!(
            split_command("\"/opt/my tools/lsp\" --stdio"),
            Some(("/opt/my tools/lsp".to_string(), vec!["--stdio".to_string()]))
        );
        assert_eq!(split_command("   "), None, "nothing to run");
        assert_eq!(
            split_command("\"unclosed --stdio"),
            None,
            "an unclosed quote"
        );
    }

    #[test]
    fn paths_are_confinement_checked() {
        assert_eq!(clean_path("src/main.rs").unwrap(), "src/main.rs");
        assert_eq!(clean_path("web\\src\\a.ts").unwrap(), "web/src/a.ts");
        assert!(clean_path("../escape.rs").is_err());
        assert!(clean_path("/etc/passwd").is_err());
        assert!(clean_path("C:/Windows/a.rs").is_err());
        assert!(clean_path("  ").is_err());
    }

    #[test]
    fn the_uri_encodes_spaces_and_keeps_a_drive_letter_addressable() {
        let uri = file_uri(Path::new("C:\\Projects\\My App\\main.rs"));
        assert!(uri.starts_with("file:///C:/"), "{uri}");
        assert!(uri.contains("My%20App"), "{uri}");
        assert!(uri.ends_with("/main.rs"), "{uri}");
        assert!(!uri.contains('\\'), "{uri}");
    }

    #[test]
    fn framed_messages_are_read_in_order_and_eof_ends_the_stream() {
        let first = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        let second = r#"{"jsonrpc":"2.0","method":"x","params":{"text":"héllo"}}"#;
        let raw = format!(
            "Content-Length: {}\r\n\r\n{first}Content-Length: {}\r\n\r\n{second}",
            first.len(),
            second.len()
        );
        let mut reader = BufReader::new(std::io::Cursor::new(raw));
        assert_eq!(read_message(&mut reader).unwrap()["id"], json!(1));
        assert_eq!(
            read_message(&mut reader).unwrap()["params"]["text"],
            json!("héllo")
        );
        assert!(read_message(&mut reader).is_none());
    }

    #[test]
    fn diagnostics_are_one_based_with_named_severities() {
        let rows = diagnostic_rows(
            &json!([
                {"range": {"start": {"line": 2, "character": 4}}, "severity": 1, "source": "rustc", "code": "E0425", "message": "cannot find value `x`"},
                {"range": {"start": {"line": 0, "character": 0}}, "severity": 2, "message": "unused import"},
                {"range": {"start": {"line": 1, "character": 0}}, "severity": 3, "message": "note"},
                {"range": {"start": {"line": 1, "character": 0}}, "severity": 99, "message": "hint"},
                {"range": {"start": {"line": 1, "character": 0}}, "severity": 1, "message": "   "}
            ]),
            "src/main.rs",
        );
        assert_eq!(rows.len(), 4, "an empty message is not a problem row");
        assert_eq!(rows[0]["line"], json!(3));
        assert_eq!(rows[0]["column"], json!(5));
        assert_eq!(rows[0]["severity"], json!("error"));
        assert_eq!(rows[0]["code"], json!("E0425"));
        assert_eq!(rows[0]["path"], json!("src/main.rs"));
        assert_eq!(rows[1]["severity"], json!("warning"));
        assert_eq!(rows[2]["severity"], json!("info"));
        assert_eq!(rows[3]["severity"], json!("hint"));
    }

    #[test]
    fn a_long_message_is_capped_rather_than_pasted_whole() {
        let rows = diagnostic_rows(
            &json!([{ "range": {"start": {"line": 0, "character": 0}}, "severity": 1, "message": "x".repeat(2000) }]),
            "a.rs",
        );
        assert_eq!(
            rows[0]["message"].as_str().unwrap().chars().count(),
            MAX_MESSAGE_CHARS
        );
    }

    const FAKE_SERVER: &str = r#"
const send = (message) => {
  const body = JSON.stringify(message);
  process.stdout.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
};
let buffer = Buffer.alloc(0);
process.stdin.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  for (;;) {
    const end = buffer.indexOf("\r\n\r\n");
    if (end < 0) return;
    const header = buffer.slice(0, end).toString();
    const match = /Content-Length: (\d+)/i.exec(header);
    const start = end + 4;
    const length = match ? Number(match[1]) : 0;
    if (buffer.length < start + length) return;
    const message = JSON.parse(buffer.slice(start, start + length).toString());
    buffer = buffer.slice(start + length);
    if (message.method === "initialize") {
      if (process.argv.includes("--fail")) {
        send({ jsonrpc: "2.0", id: message.id, error: { code: -32000, message: "no workspace support" } });
      } else {
        if (process.argv.includes("--noisy-init")) {
          send({ jsonrpc: "2.0", method: "window/logMessage", params: { type: 3, message: "indexing workspace" } });
        }
        send({ jsonrpc: "2.0", id: message.id, result: { capabilities: {} } });
      }
    } else if (message.method === "textDocument/didOpen") {
      if (process.argv.includes("--noisy-publish")) {
        send({ jsonrpc: "2.0", method: "textDocument/publishDiagnostics", params: {
          uri: message.params.textDocument.uri + ".other",
          diagnostics: [
            { range: { start: { line: 0, character: 0 } }, severity: 1, message: "not the open file" }
          ]
        } });
      }
      const rows = [
        { range: { start: { line: 2, character: 4 } }, severity: 1, source: "fake", message: "unused variable" },
        { range: { start: { line: 0, character: 0 } }, severity: 2, message: "line too long" }
      ];
      const publish = (diagnostics) => send({ jsonrpc: "2.0", method: "textDocument/publishDiagnostics", params: {
        uri: message.params.textDocument.uri, diagnostics
      } });
      if (process.argv.includes("--late-publish")) {
        publish([]);
        setTimeout(() => publish(rows), 150);
      } else if (process.argv.includes("--empty-only")) {
        publish([]);
      } else {
        publish(rows);
      }
    } else if (message.method === "shutdown") {
      send({ jsonrpc: "2.0", id: message.id, result: null });
    } else if (message.method === "exit") {
      process.exit(0);
    }
  }
});
"#;

    fn fake_launch(dir: &Path, fail: bool) -> Launch {
        fake_launch_with_flags(dir, if fail { &["--fail"] } else { &[] })
    }

    fn fake_launch_with_flags(dir: &Path, flags: &[&str]) -> Launch {
        let script = dir.join("fake-lsp.mjs");
        std::fs::write(&script, FAKE_SERVER).unwrap();
        let node = crate::find_on_path("node").expect("node is on PATH for the test suite");
        let mut args = vec![script.to_string_lossy().to_string()];
        args.extend(flags.iter().map(|flag| flag.to_string()));
        Launch {
            program: node,
            args,
        }
    }

    #[test]
    fn a_fake_server_publishes_diagnostics_for_the_open_buffer() {
        let dir = temp_dir("publish");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        let spec = spec_for("src/main.rs").unwrap();
        let launch = fake_launch(&dir, false);
        let outcome = run(&dir, "src/main.rs", spec, "fn main() {}\n", &launch).unwrap();
        assert!(outcome.published);
        assert_eq!(outcome.diagnostics.len(), 2);
        assert_eq!(outcome.diagnostics[0]["line"], json!(3));
        assert_eq!(outcome.diagnostics[0]["column"], json!(5));
        assert_eq!(outcome.diagnostics[0]["severity"], json!("error"));
        assert_eq!(outcome.diagnostics[0]["source"], json!("fake"));
        assert_eq!(outcome.diagnostics[1]["severity"], json!("warning"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_initialize_error_keeps_the_servers_own_words() {
        let dir = temp_dir("fail");
        let spec = spec_for("src/main.rs").unwrap();
        let launch = fake_launch(&dir, true);
        let error = run(&dir, "src/main.rs", spec, "fn main() {}\n", &launch).unwrap_err();
        assert!(error.contains("no workspace support"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Real servers log and report progress before they answer `initialize`.
    /// The notification must be kept for later, and the answer still read:
    /// re-examining the notification in a loop would spin forever while the
    /// answer waits unread in the pipe.
    #[test]
    fn a_notification_before_the_initialize_answer_does_not_stall() {
        let dir = temp_dir("noisy-init");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        let spec = spec_for("src/main.rs").unwrap();
        let launch = fake_launch_with_flags(&dir, &["--noisy-init"]);
        let outcome = run(&dir, "src/main.rs", spec, "fn main() {}\n", &launch).unwrap();
        assert!(outcome.published);
        assert_eq!(outcome.diagnostics.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A workspace-wide server publishes for files we never opened. Those
    /// rows are not the answer; the wait must keep reading until the open
    /// buffer's own publish arrives instead of looping on the first one.
    #[test]
    fn a_publish_for_another_file_does_not_stall_the_diagnostics_wait() {
        let dir = temp_dir("noisy-publish");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        let spec = spec_for("src/main.rs").unwrap();
        let launch = fake_launch_with_flags(&dir, &["--noisy-publish"]);
        let outcome = run(&dir, "src/main.rs", spec, "fn main() {}\n", &launch).unwrap();
        assert!(outcome.published);
        assert_eq!(outcome.diagnostics.len(), 2);
        assert_eq!(outcome.diagnostics[0]["message"], json!("unused variable"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// rust-analyzer clears diagnostics first and publishes the real rows
    /// once the crate graph loads (observed live: empty at ~0.6s, E0425 at
    /// ~2.2s). The first empty publish is not the answer: taking it would
    /// report "no problems" for a file that does not compile.
    #[test]
    fn an_empty_first_publish_is_not_the_final_answer() {
        let dir = temp_dir("late-publish");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        let spec = spec_for("src/main.rs").unwrap();
        let launch = fake_launch_with_flags(&dir, &["--late-publish"]);
        let outcome = run(&dir, "src/main.rs", spec, "fn main() {}\n", &launch).unwrap();
        assert!(outcome.published);
        assert_eq!(outcome.diagnostics.len(), 2);
        assert_eq!(outcome.diagnostics[0]["message"], json!("unused variable"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file the server really finds nothing in still reports a publish:
    /// the empty answer stands once the server had its full window to add
    /// rows, and `published` stays true so the pane shows "no problems"
    /// rather than "the server said nothing".
    #[test]
    fn an_empty_publish_that_stays_empty_is_still_reported() {
        let dir = temp_dir("empty-only");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        let spec = spec_for("src/main.rs").unwrap();
        let launch = fake_launch_with_flags(&dir, &["--empty-only"]);
        let outcome = run(&dir, "src/main.rs", spec, "fn main() {}\n", &launch).unwrap();
        assert!(outcome.published);
        assert_eq!(outcome.diagnostics.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_command_that_cannot_start_is_named() {
        let dir = temp_dir("missing");
        let spec = spec_for("src/main.rs").unwrap();
        let launch = Launch {
            program: dir.join("no-such-language-server"),
            args: Vec::new(),
        };
        let error = run(&dir, "src/main.rs", spec, "fn main() {}\n", &launch).unwrap_err();
        assert!(error.contains("Could not start"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
