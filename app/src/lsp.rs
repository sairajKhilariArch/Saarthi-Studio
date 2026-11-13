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

/// `PATH` lookup with `PATHEXT` on Windows, so the check matches what `spawn`
/// would do. A name with a separator is used as given.
fn find_on_path(program: &str) -> Option<PathBuf> {
    if program.contains('/') || program.contains('\\') {
        let candidate = PathBuf::from(program);
        return candidate.is_file().then_some(candidate);
    }
    let search = std::env::var_os("PATH")?;
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string())
            .split(';')
            .map(|entry| entry.trim().to_ascii_lowercase())
            .filter(|entry| !entry.is_empty())
            .collect()
    } else {
        vec![String::new()]
    };
    for directory in std::env::split_paths(&search) {
        for extension in &extensions {
            let name = if extension.is_empty() {
                program.to_string()
            } else {
                format!("{program}{extension}")
            };
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn cap(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

struct Launch {
    program: PathBuf,
    args: Vec<String>,
}

fn launch_for(spec: &ServerSpec) -> Option<Launch> {
    Some(Launch {
        program: find_on_path(spec.program)?,
        args: spec.args.iter().map(|arg| arg.to_string()).collect(),
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
        loop {
            match self.next_message(deadline) {
                Ok(message) => {
                    if message.get("id").and_then(Value::as_u64) == Some(id) {
                        if let Some(error) = message.get("error") {
                            let text = error
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("the server returned an error");
                            return Err(format!("{method}: {text}"));
                        }
                        return Ok(message.get("result").cloned().unwrap_or(Value::Null));
                    }
                    self.buffered.push_back(message);
                }
                Err(ReadFailure::Closed) => {
                    return Err(format!("The language server stopped during {method}."))
                }
                Err(ReadFailure::Timeout) => {
                    return Err(format!("{method} did not answer in time."))
                }
            }
        }
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
    loop {
        match session.next_message(deadline) {
            Ok(message) => {
                let method = message.get("method").and_then(Value::as_str).unwrap_or("");
                if method == "textDocument/publishDiagnostics" {
                    let params = message.get("params").cloned().unwrap_or(Value::Null);
                    // Only the file we opened: a workspace-wide publish for
                    // other files is not an answer to this request.
                    if params.get("uri").and_then(Value::as_str) == Some(document.as_str()) {
                        published = Some(
                            params
                                .get("diagnostics")
                                .cloned()
                                .unwrap_or_else(|| json!([])),
                        );
                        break;
                    }
                }
                session.buffered.push_back(message);
            }
            Err(ReadFailure::Timeout) => break,
            Err(ReadFailure::Closed) => {
                return Err("The language server stopped before publishing diagnostics.".to_string())
            }
        }
    }

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
    let Some(launch) = launch_for(spec) else {
        return Json(json!({
            "ok": true,
            "path": path,
            "server": spec.program,
            "installed": false,
            "diagnostics": [],
            "note": format!(
                "{} is not on PATH, so no language-server diagnostics were read.",
                spec.program
            ),
        }))
        .into_response();
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
async fn servers_route() -> Json<Value> {
    let mut rows: Vec<Value> = Vec::new();
    for spec in SERVERS {
        if rows
            .iter()
            .any(|row| row.get("server") == Some(&json!(spec.program)))
        {
            continue;
        }
        let extensions: Vec<&str> = SERVERS
            .iter()
            .filter(|other| other.program == spec.program)
            .flat_map(|other| other.extensions.iter().copied())
            .collect();
        rows.push(json!({
            "server": spec.program,
            "extensions": extensions,
            "installed": find_on_path(spec.program).is_some(),
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
        send({ jsonrpc: "2.0", id: message.id, result: { capabilities: {} } });
      }
    } else if (message.method === "textDocument/didOpen") {
      send({ jsonrpc: "2.0", method: "textDocument/publishDiagnostics", params: {
        uri: message.params.textDocument.uri,
        diagnostics: [
          { range: { start: { line: 2, character: 4 } }, severity: 1, source: "fake", message: "unused variable" },
          { range: { start: { line: 0, character: 0 } }, severity: 2, message: "line too long" }
        ]
      } });
    } else if (message.method === "shutdown") {
      send({ jsonrpc: "2.0", id: message.id, result: null });
    } else if (message.method === "exit") {
      process.exit(0);
    }
  }
});
"#;

    fn fake_launch(dir: &Path, fail: bool) -> Launch {
        let script = dir.join("fake-lsp.mjs");
        std::fs::write(&script, FAKE_SERVER).unwrap();
        let node = find_on_path("node").expect("node is on PATH for the test suite");
        let mut args = vec![script.to_string_lossy().to_string()];
        if fail {
            args.push("--fail".to_string());
        }
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
