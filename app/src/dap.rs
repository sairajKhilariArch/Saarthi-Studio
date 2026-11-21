//! Phase 9 — the debugger.
//!
//! The IDE could run a program, read its output and kill it, but nothing could
//! stop it, look inside it or step it. This module speaks the Debug Adapter
//! Protocol to a debugger that is already installed on the machine, so a session
//! is a real process being debugged rather than a replayed log:
//!
//!   * `lldb-dap` (falling back to `lldb-vscode`) for Rust, C and C++;
//!   * `debugpy`'s adapter for Python;
//!   * `dlv dap` for Go.
//!
//! Deliberate limits, each reported rather than hidden:
//!
//!   * one session per process. A debugger owns the target it launched, so a
//!     second start is refused until the first is stopped;
//!   * the adapter is chosen from the program's extension and found on `PATH`;
//!     a missing adapter answers with what to install, never with an empty stack;
//!   * the target is a path inside the open folder, and the adapter's own argv is
//!     fixed per language, so nothing from a request reaches a shell;
//!   * frames, scopes, variables and evaluated text are capped, and a nested
//!     value is fetched one level per request;
//!   * nothing is inferred: every frame, variable and console line is what the
//!     adapter sent.
//!
//! The protocol is one HTTP-shaped request/response stream: a `Content-Length`
//! framed JSON body, `type: "request"`, `"response"` or `"event"`. A reader
//! thread owns the adapter's stdout — it answers the waiting request by
//! `request_seq` and folds events (`stopped`, `output`, `terminated`, `exited`)
//! into a small shared state the UI polls.

use crate::{json_error, AppState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CONFIGURATION_TIMEOUT: Duration = Duration::from_millis(2000);
const DISCONNECT_TIMEOUT: Duration = Duration::from_millis(2000);

pub(crate) const MAX_CONSOLE_LINES: usize = 200;
const MAX_CONSOLE_CHARS: usize = 2000;
pub(crate) const MAX_FRAMES: usize = 50;
pub(crate) const MAX_SCOPES: usize = 20;
pub(crate) const MAX_VARIABLES: usize = 100;
const MAX_NAME_CHARS: usize = 200;
const MAX_VALUE_CHARS: usize = 400;
const MAX_EVALUATE_CHARS: usize = 4000;
const MAX_BREAKPOINT_FILES: usize = 20;
const MAX_BREAKPOINTS: usize = 200;
const MAX_CONDITION_CHARS: usize = 200;
const MAX_ARGS: usize = 20;
const MAX_ARG_CHARS: usize = 500;
const MAX_PATH_CHARS: usize = 1000;

/// One debugger Sarthi Studio knows how to speak to.
#[derive(Debug)]
struct AdapterSpec {
    id: &'static str,
    /// The binaries that provide this adapter, first match wins.
    programs: &'static [&'static str],
    args: &'static [&'static str],
    extensions: &'static [&'static str],
    install: &'static str,
}

const ADAPTERS: &[AdapterSpec] = &[
    AdapterSpec {
        id: "lldb-dap",
        programs: &["lldb-dap", "lldb-vscode"],
        args: &[],
        extensions: &["rs", "c", "cc", "cpp", "cxx", "h", "hpp", "m", "mm"],
        install: "Install LLVM (lldb-dap ships with it), or run `rustup component add llvm-tools`",
    },
    AdapterSpec {
        id: "debugpy",
        programs: &["python3", "python"],
        args: &["-m", "debugpy.adapter"],
        extensions: &["py"],
        install: "Run `pip install debugpy` for the Python on PATH",
    },
    AdapterSpec {
        id: "dlv",
        programs: &["dlv"],
        args: &["dap"],
        extensions: &["go"],
        install: "Run `go install github.com/go-delve/delve/cmd/dlv@latest`",
    },
];

fn spec_for(path: &str) -> Option<&'static AdapterSpec> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    let extension = extension.to_ascii_lowercase();
    ADAPTERS
        .iter()
        .find(|spec| spec.extensions.contains(&extension.as_str()))
}

/// A path relative to the open folder, with the rules the editor already uses:
/// no absolute path, no drive prefix, no `..`, no control characters.
fn clean_path(value: &str) -> Result<String, (StatusCode, String)> {
    let path = value.trim().replace('\\', "/");
    if path.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Open a file first.".to_string()));
    }
    if path.chars().count() > MAX_PATH_CHARS {
        return Err((
            StatusCode::BAD_REQUEST,
            "That path is too long.".to_string(),
        ));
    }
    if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        return Err((
            StatusCode::BAD_REQUEST,
            "Use a path relative to the open folder.".to_string(),
        ));
    }
    if path.chars().any(|ch| ch.is_control()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "That path contains a control character.".to_string(),
        ));
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "That path leaves the open folder.".to_string(),
        ));
    }
    Ok(path)
}

fn cap(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

/// What the reader thread and the UI both need to see.
#[derive(Clone, Default)]
struct Shared {
    console: VecDeque<String>,
    stopped: Option<Stopped>,
    /// The adapter is ready for breakpoints and `configurationDone`.
    initialized: bool,
    terminated: bool,
    exited: Option<i64>,
}

#[derive(Clone)]
struct Stopped {
    reason: String,
    thread_id: Option<i64>,
    description: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Breakpoint {
    line: i64,
    condition: String,
}

type Breakpoints = BTreeMap<String, Vec<Breakpoint>>;

struct Session {
    child: Child,
    stdin: ChildStdin,
    responses: mpsc::Receiver<Value>,
    shared: Arc<Mutex<Shared>>,
    seq: i64,
    adapter: String,
    program: String,
    root: PathBuf,
    breakpoints: Breakpoints,
}

/// The one live session, if any. A debugger is a single-user, single-target
/// activity: two sessions would fight over the same workspace folder.
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn session() -> MutexGuard<'static, Option<Session>> {
    SESSION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Breakpoints are stored in the app's own database, per workspace folder, so
/// closing the window or restarting does not silently lose them. A row names a
/// file inside that folder; nothing here writes the workspace.
pub(crate) fn initialize(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS debug_breakpoints (
            root TEXT NOT NULL,
            path TEXT NOT NULL,
            line INTEGER NOT NULL,
            condition TEXT NOT NULL DEFAULT '',
            updated_at REAL NOT NULL,
            PRIMARY KEY (root, path, line)
        );",
    )
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

fn folder_key(root: &Path) -> String {
    root.to_string_lossy().to_string()
}

/// What the folder's breakpoints are, read fresh: a second window (or a restart)
/// sees what the first one stored rather than a copy held in memory.
fn load_stored(db: &Path, root: &Path) -> Result<Breakpoints, String> {
    let conn = crate::open_db(db).map_err(|error| error.to_string())?;
    let mut statement = conn
        .prepare(
            "SELECT path, line, condition FROM debug_breakpoints WHERE root = ?1 \
             ORDER BY path ASC, line ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![folder_key(root)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    let mut map = Breakpoints::new();
    for row in rows {
        let (path, line, condition) = row.map_err(|error| error.to_string())?;
        // A stored row is data, not a location to trust: a path that no longer
        // satisfies the folder rules is ignored instead of mapped elsewhere.
        let Ok(path) = clean_path(&path) else {
            continue;
        };
        map.entry(path)
            .or_default()
            .push(Breakpoint { line, condition });
    }
    Ok(map)
}

/// Replace one file's rows. An empty list clears the file.
fn store_file(db: &Path, root: &Path, path: &str, list: &[Breakpoint]) -> Result<(), String> {
    let mut conn = crate::open_db(db).map_err(|error| error.to_string())?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let folder = folder_key(root);
    tx.execute(
        "DELETE FROM debug_breakpoints WHERE root = ?1 AND path = ?2",
        params![folder, path],
    )
    .map_err(|error| error.to_string())?;
    let updated = now_seconds();
    for breakpoint in list {
        tx.execute(
            "INSERT INTO debug_breakpoints(root, path, line, condition, updated_at) \
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![folder, path, breakpoint.line, breakpoint.condition, updated],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())?;
    Ok(())
}

fn check_caps(breakpoints: &Breakpoints) -> Result<(), (StatusCode, String)> {
    if breakpoints.len() > MAX_BREAKPOINT_FILES {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("At most {MAX_BREAKPOINT_FILES} files can hold breakpoints at once."),
        ));
    }
    let lines: usize = breakpoints.values().map(Vec::len).sum();
    if lines > MAX_BREAKPOINTS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("At most {MAX_BREAKPOINTS} breakpoints can be set at once."),
        ));
    }
    Ok(())
}

/// The breakpoints an idle session reports, with the database's own message when
/// the read fails rather than an empty list that would look like "none set".
fn persisted_breakpoints(db: &Path, root: Option<&Path>) -> (Value, Option<String>) {
    let Some(root) = root else {
        return (json!({}), None);
    };
    match load_stored(db, root) {
        Ok(map) => (breakpoints_json(&map), None),
        Err(message) => (json!({}), Some(message)),
    }
}

/// One `Content-Length` framed JSON message, or `None` at end of stream.
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
    // A header with no usable length, or one asking for an unreasonable body,
    // ends the conversation instead of allocating it.
    if length == 0 || length > 4 * 1024 * 1024 {
        return None;
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn write_message(stdin: &mut ChildStdin, message: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(message).map_err(|error| error.to_string())?;
    stdin
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .and_then(|_| stdin.write_all(&body))
        .and_then(|_| stdin.flush())
        .map_err(|error| format!("Could not write to the debug adapter: {error}"))
}

fn push_console(shared: &Arc<Mutex<Shared>>, line: String) {
    let Ok(mut state) = shared.lock() else {
        return;
    };
    if line.trim().is_empty() {
        return;
    }
    state.console.push_back(line);
    while state.console.len() > MAX_CONSOLE_LINES {
        state.console.pop_front();
    }
}

fn handle_event(shared: &Arc<Mutex<Shared>>, message: &Value) {
    let event = message.get("event").and_then(Value::as_str).unwrap_or("");
    let body = message.get("body").cloned().unwrap_or(Value::Null);
    match event {
        "output" => {
            let category = body.get("category").and_then(Value::as_str).unwrap_or("");
            let text = body.get("output").and_then(Value::as_str).unwrap_or("");
            for line in text.lines().take(MAX_CONSOLE_LINES) {
                let clean = cap(line.trim_end_matches('\r'), MAX_CONSOLE_CHARS);
                if category == "stderr" {
                    push_console(shared, format!("[stderr] {clean}"));
                } else {
                    push_console(shared, clean);
                }
            }
        }
        "stopped" => {
            let reason = body
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("stopped")
                .to_string();
            let description = cap(
                body.get("description")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                MAX_CONSOLE_CHARS,
            );
            if let Ok(mut state) = shared.lock() {
                state.stopped = Some(Stopped {
                    reason,
                    thread_id: body.get("threadId").and_then(Value::as_i64),
                    description,
                });
            }
        }
        "initialized" => {
            if let Ok(mut state) = shared.lock() {
                state.initialized = true;
            }
        }
        "continued" => {
            if let Ok(mut state) = shared.lock() {
                state.stopped = None;
            }
        }
        "terminated" => {
            if let Ok(mut state) = shared.lock() {
                state.terminated = true;
                state.stopped = None;
            }
        }
        "exited" => {
            let code = body.get("exitCode").and_then(Value::as_i64);
            if let Ok(mut state) = shared.lock() {
                state.exited = code;
                state.stopped = None;
            }
        }
        _ => {}
    }
}

/// The adapter's stderr is not the protocol stream; it is kept as console text so
/// a failing debugger's own sentence reaches the person instead of a dead pipe.
fn spawn_stderr_reader(stderr: std::process::ChildStderr, shared: Arc<Mutex<Shared>>) {
    std::thread::spawn(move || {
        let mut buffer = String::new();
        let mut reader = BufReader::new(stderr);
        if reader
            .by_ref()
            .take(64 * 1024)
            .read_to_string(&mut buffer)
            .is_ok()
        {
            for line in buffer.lines().take(MAX_CONSOLE_LINES) {
                push_console(
                    &shared,
                    format!("[adapter] {}", cap(line, MAX_CONSOLE_CHARS)),
                );
            }
        }
    });
}

fn spawn_reader(
    stdout: std::process::ChildStdout,
    shared: Arc<Mutex<Shared>>,
    sender: Sender<Value>,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        while let Some(message) = read_message(&mut reader) {
            if message.get("type").and_then(Value::as_str) == Some("event") {
                handle_event(&shared, &message);
            } else if sender.send(message).is_err() {
                break;
            }
        }
        // The adapter closed its output: nothing else will answer, and the UI
        // must not keep a session that looks alive.
        if let Ok(mut state) = shared.lock() {
            state.terminated = true;
            state.stopped = None;
        }
        push_console(&shared, "The debug adapter closed its output.".to_string());
    });
}

impl Session {
    /// Write one request. A caller that cannot wait for the answer right away
    /// (because the adapter will not answer until a later command) keeps the
    /// sequence number and reads it with `await_response`.
    fn send(&mut self, command: &str, arguments: Value) -> Result<i64, String> {
        self.seq += 1;
        let seq = self.seq;
        write_message(
            &mut self.stdin,
            &json!({"seq": seq, "type": "request", "command": command, "arguments": arguments}),
        )?;
        Ok(seq)
    }

    /// Send one request and wait for its own response. Events are handled by the
    /// reader thread while this waits, so a `stopped` arriving first is kept.
    fn request(
        &mut self,
        command: &str,
        arguments: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let seq = self.send(command, arguments)?;
        self.await_response(seq, command, timeout)
    }

    fn await_response(
        &mut self,
        seq: i64,
        command: &str,
        timeout: Duration,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "The debug adapter did not answer `{command}` within {} seconds.",
                    timeout.as_secs()
                ));
            }
            match self.responses.recv_timeout(remaining) {
                Ok(response) => {
                    if response.get("request_seq").and_then(Value::as_i64) != Some(seq) {
                        // A late answer to a request that already timed out.
                        continue;
                    }
                    let success = response
                        .get("success")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    if success {
                        return Ok(response.get("body").cloned().unwrap_or(Value::Null));
                    }
                    let message = response
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .unwrap_or("the adapter refused the request");
                    return Err(format!("The debug adapter refused `{command}`: {message}"));
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "The debug adapter did not answer `{command}` within {} seconds.",
                        timeout.as_secs()
                    ))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!(
                        "The debug adapter exited before answering `{command}`."
                    ))
                }
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The client owns the process: an aborted handshake or a timed-out
        // request must not leave a debugger running behind it.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
struct LaunchSpec {
    id: String,
    program: PathBuf,
    args: Vec<String>,
}

/// The adapter for a language, as it will be started. The argv is the table's,
/// never the request's.
fn resolve_launch(spec: &AdapterSpec) -> Result<LaunchSpec, (StatusCode, String)> {
    for program in spec.programs {
        if let Some(path) = crate::find_on_path(program) {
            return Ok(LaunchSpec {
                id: spec.id.to_string(),
                program: path,
                args: spec.args.iter().map(|arg| arg.to_string()).collect(),
            });
        }
    }
    Err((
        StatusCode::BAD_REQUEST,
        format!(
            "No debugger for this file type is installed. Sarthi Studio speaks the Debug Adapter \
             Protocol to {}; {} and then try again.",
            spec.id, spec.install
        ),
    ))
}

/// debugpy is a module, so `python3` on PATH is not enough: the import is the
/// check, and its own error is the message when it fails. `None` means the
/// adapter is usable.
fn debugpy_problem(program: &Path) -> Option<String> {
    let output = Command::new(program)
        .args(["-c", "import debugpy.adapter"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    match output {
        Ok(output) if output.status.success() => None,
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr);
            Some(
                detail
                    .lines()
                    .map(str::trim)
                    .rfind(|line| !line.is_empty())
                    .unwrap_or("debugpy is not importable")
                    .to_string(),
            )
        }
        Err(error) => Some(format!("could not run {}: {error}", program.display())),
    }
}

fn preflight(spec: &AdapterSpec, launch: &LaunchSpec) -> Result<(), (StatusCode, String)> {
    if spec.id != "debugpy" {
        return Ok(());
    }
    match debugpy_problem(&launch.program) {
        None => Ok(()),
        Some(problem) => Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Python's debugpy is not installed for {} ({problem}). Run `pip install debugpy`.",
                launch.program.display()
            ),
        )),
    }
}

/// Where an attach session connects: a debug server the program already runs.
/// Nothing is launched, so no target file or argv is involved.
#[derive(Debug)]
struct AttachTarget {
    host: String,
    port: u16,
}

struct SessionOptions {
    launch: LaunchSpec,
    program: String,
    program_path: PathBuf,
    root: PathBuf,
    /// Program arguments passed to the adapter at launch. Adapter-specific
    /// launch settings the person configured in the debug pane.
    launch_args: Vec<String>,
    /// Working directory for the launch, relative to the open folder
    /// (default: the open folder).
    launch_cwd: Option<PathBuf>,
    stop_on_entry: bool,
    breakpoints: Breakpoints,
    /// Set for an attach session; `None` launches `program_path`.
    attach: Option<AttachTarget>,
}

fn abort(session: Session, message: String) -> (StatusCode, String) {
    drop(session);
    (StatusCode::BAD_GATEWAY, message)
}

/// Start one session: spawn the adapter, shake hands, launch the target, apply
/// the breakpoints and wait for `configurationDone`. Every failure kills the
/// adapter, so a refused launch leaves nothing behind.
fn begin(options: SessionOptions) -> Result<Value, (StatusCode, String)> {
    if session().is_some() {
        return Err((
            StatusCode::CONFLICT,
            "A debug session is already running. Stop it before starting another.".to_string(),
        ));
    }
    let mut command = Command::new(&options.launch.program);
    command
        .args(&options.launch.args)
        .current_dir(options.launch_cwd.as_deref().unwrap_or(&options.root))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window, and its own process group so a stop reaches only it.
        command.creation_flags(0x0800_0000 | 0x0000_0200);
    }
    let mut child = command.spawn().map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!(
                "Could not start the {} adapter ({}): {error}",
                options.launch.id,
                options.launch.program.display()
            ),
        )
    })?;
    let Some(stdin) = child.stdin.take() else {
        let _ = child.kill();
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "The debug adapter gave us no input stream.".to_string(),
        ));
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "The debug adapter gave us no output stream.".to_string(),
        ));
    };
    let shared = Arc::new(Mutex::new(Shared::default()));
    let (sender, responses) = mpsc::channel();
    spawn_reader(stdout, shared.clone(), sender);
    if let Some(stderr) = child.stderr.take() {
        spawn_stderr_reader(stderr, shared.clone());
    }
    let mut debugger = Session {
        child,
        stdin,
        responses,
        shared,
        seq: 0,
        adapter: options.launch.id.clone(),
        program: options.program.clone(),
        root: options.root.clone(),
        breakpoints: Breakpoints::new(),
    };
    let adapter = debugger.adapter.clone();
    if let Err(message) = debugger.request(
        "initialize",
        json!({
            "clientID": "sarthi-studio",
            "clientName": "Sarthi Studio",
            "adapterID": adapter,
            "pathFormat": "path",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "supportsRunInTerminalRequest": false,
            "supportsVariableType": true,
            "supportsVariablePaging": false,
        }),
        INITIALIZE_TIMEOUT,
    ) {
        return Err(abort(debugger, message));
    }
    // The launch response is not what makes a target run: adapters such as
    // debugpy answer it only after `configurationDone`, and waiting here for an
    // answer that depends on the next request deadlocks the session. Send it,
    // wait for `initialized`, configure, then read the answer.
    // An attach session names a debug server that is already listening; a
    // launch session names the program to start. Either way no argv comes from
    // the request: the adapter's own argv is the table's.
    let (start_command, start_arguments) = match &options.attach {
        Some(target) => (
            "attach",
            json!({
                "host": target.host,
                "port": target.port,
                "cwd": options.root.to_string_lossy(),
                "justMyCode": true,
            }),
        ),
        None => (
            "launch",
            json!({
                "program": options.program_path.to_string_lossy(),
                "cwd": options.root.to_string_lossy(),
                "args": options.launch_args,
                "stopOnEntry": options.stop_on_entry,
                "console": "internalConsole",
                "justMyCode": true,
            }),
        ),
    };
    let launch_seq = match debugger.send(start_command, start_arguments) {
        Ok(seq) => seq,
        Err(message) => return Err(abort(debugger, message)),
    };
    // Adapters that never announce `initialized` still get the breakpoints; the
    // wait is bounded so an old adapter is not mistaken for a hung one.
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < ready_deadline {
        let ready = debugger
            .shared
            .lock()
            .map(|state| state.initialized || state.terminated)
            .unwrap_or(true);
        if ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for (file, list) in &options.breakpoints {
        if let Err((status, message)) = apply_breakpoints(&mut debugger, file, list) {
            return Err(abort(debugger, format!("{message} ({})", status.as_u16())));
        }
        debugger.breakpoints.insert(file.clone(), list.clone());
    }
    // Adapters differ here: most answer, some never do. The launch already
    // succeeded, so a quiet `configurationDone` is not a failure.
    let _ = debugger.request("configurationDone", json!({}), CONFIGURATION_TIMEOUT);
    // Now the launch's own answer is meaningful. A refusal is reported with the
    // adapter's words, and the process it may have started is killed by `abort`.
    if let Err(message) = debugger.await_response(launch_seq, start_command, LAUNCH_TIMEOUT) {
        let what = if options.attach.is_some() {
            "attach to the program"
        } else {
            "launch the target"
        };
        return Err(abort(
            debugger,
            format!("The adapter could not {what}: {message}"),
        ));
    }
    {
        let mut guard = session();
        if guard.is_some() {
            return Err(abort(
                debugger,
                "A debug session is already running.".to_string(),
            ));
        }
        *guard = Some(debugger);
    }
    Ok(state_json())
}

fn apply_breakpoints(
    session: &mut Session,
    file: &str,
    list: &[Breakpoint],
) -> Result<Value, (StatusCode, String)> {
    let absolute = crate::workspace::resolve_inside(&session.root, file).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            format!("{file}: {}", error.message()),
        )
    })?;
    let body = json!({
        "source": { "path": absolute.to_string_lossy() },
        "breakpoints": list
            .iter()
            .map(|breakpoint| {
                let mut item = json!({ "line": breakpoint.line });
                if !breakpoint.condition.is_empty() {
                    item["condition"] = json!(breakpoint.condition);
                }
                item
            })
            .collect::<Vec<_>>(),
    });
    let answer = session
        .request("setBreakpoints", body, REQUEST_TIMEOUT)
        .map_err(|message| (StatusCode::BAD_GATEWAY, message))?;
    let verified = answer
        .get("breakpoints")
        .and_then(Value::as_array)
        .map(|rows| rows.len())
        .unwrap_or(0);
    Ok(json!({ "requested": list.len(), "verified": verified }))
}

fn state_json() -> Value {
    match SESSION.try_lock() {
        Err(TryLockError::WouldBlock) => json!({
            "running": true,
            "busy": true,
            "note": "The debugger is answering the last request.",
        }),
        Ok(guard) => state_of(&guard),
        Err(TryLockError::Poisoned(poisoned)) => state_of(&poisoned.into_inner()),
    }
}

/// Breakpoints as the UI reads them: `{path: [{line, condition}]}`, the same
/// shape in both the running and pending states.
fn breakpoints_json(breakpoints: &Breakpoints) -> Value {
    let mut map = serde_json::Map::new();
    for (path, list) in breakpoints {
        map.insert(
            path.clone(),
            json!(list
                .iter()
                .map(|breakpoint| json!({
                    "line": breakpoint.line,
                    "condition": breakpoint.condition,
                }))
                .collect::<Vec<_>>()),
        );
    }
    Value::Object(map)
}

fn state_of(guard: &Option<Session>) -> Value {
    let Some(session) = guard.as_ref() else {
        return json!({
            "running": false,
            "breakpoints": Value::Object(serde_json::Map::new()),
            "console": [],
        });
    };
    let shared = session
        .shared
        .lock()
        .map(|state| state.clone())
        .unwrap_or_default();
    json!({
        "running": true,
        "busy": false,
        "adapter": session.adapter,
        "program": session.program,
        "root": session.root.to_string_lossy(),
        "breakpoints": breakpoints_json(&session.breakpoints),
        "stopped": shared.stopped.as_ref().map(|stopped| json!({
            "reason": stopped.reason,
            "threadId": stopped.thread_id,
            "description": stopped.description,
        })),
        "terminated": shared.terminated,
        "exited": shared.exited,
        "console": shared.console,
    })
}

fn stop_session() -> Value {
    let mut guard = session();
    let Some(mut session) = guard.take() else {
        return json!({"stopped": false, "reason": "No debug session is running."});
    };
    let _ = session.request(
        "disconnect",
        json!({"terminateDebuggee": true}),
        DISCONNECT_TIMEOUT,
    );
    let adapter = session.adapter.clone();
    let program = session.program.clone();
    drop(session);
    json!({"stopped": true, "adapter": adapter, "program": program})
}

fn unknown_session() -> (StatusCode, String) {
    (
        StatusCode::CONFLICT,
        "No debug session is running. Start one first.".to_string(),
    )
}

/// The thread to act on: the one that stopped, else the adapter's first thread.
fn session_thread(
    session: &mut Session,
    requested: Option<i64>,
) -> Result<i64, (StatusCode, String)> {
    if let Some(thread) = requested {
        return Ok(thread);
    }
    if let Some(thread) = session
        .shared
        .lock()
        .ok()
        .and_then(|state| state.stopped.as_ref().and_then(|stopped| stopped.thread_id))
    {
        return Ok(thread);
    }
    let body = session
        .request("threads", json!({}), REQUEST_TIMEOUT)
        .map_err(|message| (StatusCode::BAD_GATEWAY, message))?;
    body.get("threads")
        .and_then(Value::as_array)
        .and_then(|threads| threads.first())
        .and_then(|thread| thread.get("id").and_then(Value::as_i64))
        .ok_or((
            StatusCode::CONFLICT,
            "The debug adapter has not reported a thread to act on.".to_string(),
        ))
}

fn step(command: &str, thread: Option<i64>) -> Result<Value, (StatusCode, String)> {
    let mut guard = session();
    let Some(session) = guard.as_mut() else {
        return Err(unknown_session());
    };
    let thread = session_thread(session, thread)?;
    // The target is about to move; the old stop is over.
    if let Ok(mut state) = session.shared.lock() {
        state.stopped = None;
    }
    session
        .request(command, json!({"threadId": thread}), REQUEST_TIMEOUT)
        .map_err(|message| (StatusCode::BAD_GATEWAY, message))?;
    Ok(json!({"ok": true, "command": command, "threadId": thread}))
}

fn stack(thread: Option<i64>) -> Result<Value, (StatusCode, String)> {
    let mut guard = session();
    let Some(session) = guard.as_mut() else {
        return Err(unknown_session());
    };
    let thread = session_thread(session, thread)?;
    let body = session
        .request(
            "stackTrace",
            json!({"threadId": thread, "startFrame": 0, "levels": MAX_FRAMES}),
            REQUEST_TIMEOUT,
        )
        .map_err(|message| (StatusCode::BAD_GATEWAY, message))?;
    let total = body
        .get("totalFrames")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| {
            body.get("stackFrames")
                .and_then(Value::as_array)
                .map(|frames| frames.len() as i64)
                .unwrap_or(0)
        });
    let frames = body
        .get("stackFrames")
        .and_then(Value::as_array)
        .map(|frames| {
            frames
                .iter()
                .take(MAX_FRAMES)
                .map(|frame| {
                    json!({
                        "id": frame.get("id").and_then(Value::as_i64),
                        "name": cap(frame.get("name").and_then(Value::as_str).unwrap_or(""), MAX_NAME_CHARS),
                        "path": frame.pointer("/source/path").and_then(Value::as_str).map(|path| cap(path, MAX_PATH_CHARS)),
                        "line": frame.get("line").and_then(Value::as_i64).unwrap_or(0),
                        "column": frame.get("column").and_then(Value::as_i64).unwrap_or(0),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(json!({
        "threadId": thread,
        "frames": frames,
        "total": total,
        "truncated": total > MAX_FRAMES as i64,
    }))
}

fn scopes(frame: i64) -> Result<Value, (StatusCode, String)> {
    let mut guard = session();
    let Some(session) = guard.as_mut() else {
        return Err(unknown_session());
    };
    let body = session
        .request("scopes", json!({"frameId": frame}), REQUEST_TIMEOUT)
        .map_err(|message| (StatusCode::BAD_GATEWAY, message))?;
    let scopes = body
        .get("scopes")
        .and_then(Value::as_array)
        .map(|scopes| {
            scopes
                .iter()
                .take(MAX_SCOPES)
                .map(|scope| {
                    json!({
                        "name": cap(scope.get("name").and_then(Value::as_str).unwrap_or(""), MAX_NAME_CHARS),
                        "reference": scope.get("variablesReference").and_then(Value::as_i64).unwrap_or(0),
                        "expensive": scope.get("expensive").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(json!({"frameId": frame, "scopes": scopes}))
}

fn variables(reference: i64) -> Result<Value, (StatusCode, String)> {
    let mut guard = session();
    let Some(session) = guard.as_mut() else {
        return Err(unknown_session());
    };
    let body = session
        .request(
            "variables",
            json!({"variablesReference": reference}),
            REQUEST_TIMEOUT,
        )
        .map_err(|message| (StatusCode::BAD_GATEWAY, message))?;
    let variables = body
        .get("variables")
        .and_then(Value::as_array)
        .map(|variables| {
            variables
                .iter()
                .take(MAX_VARIABLES)
                .map(|variable| {
                    json!({
                        "name": cap(variable.get("name").and_then(Value::as_str).unwrap_or(""), MAX_NAME_CHARS),
                        "value": cap(variable.get("value").and_then(Value::as_str).unwrap_or(""), MAX_VALUE_CHARS),
                        "type": cap(variable.get("type").and_then(Value::as_str).unwrap_or(""), MAX_NAME_CHARS),
                        "reference": variable.get("variablesReference").and_then(Value::as_i64).unwrap_or(0),
                        "namedVariables": variable.get("namedVariables").and_then(Value::as_i64),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let total = body
        .get("variables")
        .and_then(Value::as_array)
        .map(|variables| variables.len())
        .unwrap_or(0);
    Ok(json!({
        "reference": reference,
        "variables": variables,
        "truncated": total > MAX_VARIABLES,
    }))
}

fn evaluate(expression: &str, frame: Option<i64>) -> Result<Value, (StatusCode, String)> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Provide an expression to evaluate.".to_string(),
        ));
    }
    if expression.chars().count() > MAX_CONDITION_CHARS * 10 {
        return Err((
            StatusCode::BAD_REQUEST,
            "That expression is too long.".to_string(),
        ));
    }
    let mut guard = session();
    let Some(session) = guard.as_mut() else {
        return Err(unknown_session());
    };
    let mut arguments = json!({"expression": expression, "context": "repl"});
    if let Some(frame) = frame {
        arguments["frameId"] = json!(frame);
    }
    let body = session
        .request("evaluate", arguments, REQUEST_TIMEOUT)
        .map_err(|message| {
            // debugpy answers a failed evaluation with a traceback; the last line
            // is the sentence a person needs, not the frames that led there.
            let cause = message
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .unwrap_or("the adapter refused the expression");
            (
                StatusCode::BAD_GATEWAY,
                format!("The debug adapter refused the expression: {cause}"),
            )
        })?;
    let result = body
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(json!({
        "result": cap(&result, MAX_EVALUATE_CHARS),
        "truncated": result.chars().count() > MAX_EVALUATE_CHARS,
        "type": cap(body.get("type").and_then(Value::as_str).unwrap_or(""), MAX_NAME_CHARS),
        "reference": body.get("variablesReference").and_then(Value::as_i64).unwrap_or(0),
    }))
}

fn parse_breakpoints(value: &Value) -> Result<Vec<Breakpoint>, (StatusCode, String)> {
    let Some(items) = value.as_array() else {
        return Err((
            StatusCode::BAD_REQUEST,
            "Provide a `lines` array of breakpoints.".to_string(),
        ));
    };
    if items.len() > MAX_BREAKPOINTS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("At most {MAX_BREAKPOINTS} breakpoints can be set at once."),
        ));
    }
    let mut list: Vec<Breakpoint> = Vec::new();
    for item in items {
        let (line, condition) = match item {
            Value::Number(number) => (number.as_i64(), String::new()),
            Value::Object(map) => (
                map.get("line").and_then(Value::as_i64),
                map.get("condition")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            ),
            _ => (None, String::new()),
        };
        let Some(line) = line else {
            return Err((
                StatusCode::BAD_REQUEST,
                "Every breakpoint needs a numeric line.".to_string(),
            ));
        };
        if !(1..=1_000_000).contains(&line) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Line {line} is not a line in a file."),
            ));
        }
        if condition.chars().count() > MAX_CONDITION_CHARS {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("A breakpoint condition is limited to {MAX_CONDITION_CHARS} characters."),
            ));
        }
        if let Some(existing) = list.iter_mut().find(|breakpoint| breakpoint.line == line) {
            // A duplicate without a condition must not erase the one already set.
            if !condition.is_empty() {
                existing.condition = condition;
            }
        } else {
            list.push(Breakpoint { line, condition });
        }
    }
    list.sort_by_key(|breakpoint| breakpoint.line);
    Ok(list)
}

/// Set (or clear) the breakpoints of one file. They are stored first, so a
/// refusal from a live adapter never loses the person's intent; a session then
/// gets them immediately, and the next start reads the same rows.
fn set_breakpoints(state: &AppState, input: &Value) -> Result<Value, (StatusCode, String)> {
    let root = state
        .workspace
        .root()
        .map_err(|error| (StatusCode::BAD_REQUEST, error.message().to_string()))?;
    let path = clean_path(
        input
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )?;
    let list = parse_breakpoints(input.get("lines").unwrap_or(&Value::Null))?;
    let mut next = load_stored(&state.database_path, &root)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    next.remove(&path);
    if !list.is_empty() {
        next.insert(path.clone(), list.clone());
    }
    check_caps(&next)?;
    store_file(&state.database_path, &root, &path, &list)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    let count = list.len();
    let verified = apply_and_record(&path, &list, next)?;
    Ok(json!({
        "path": path,
        "applied": verified.is_some(),
        "persisted": true,
        "lines": count,
        "verified": verified.as_ref().map(|answer| answer["verified"].clone()),
    }))
}

/// Give one file's breakpoints to the live session, if there is one, and keep the
/// folder's full set on it for the state view. No session is not an error: the
/// rows are already stored for the next start.
fn apply_and_record(
    path: &str,
    list: &[Breakpoint],
    all: Breakpoints,
) -> Result<Option<Value>, (StatusCode, String)> {
    let mut guard = session();
    let Some(session) = guard.as_mut() else {
        return Ok(None);
    };
    let answer = apply_breakpoints(session, path, list)?;
    session.breakpoints = all;
    Ok(Some(answer))
}

fn adapters_json() -> Value {
    let rows = ADAPTERS
        .iter()
        .map(|spec| {
            let program = spec
                .programs
                .iter()
                .find_map(|program| crate::find_on_path(program));
            // A Python interpreter on PATH is not a Python debugger; the list
            // must not claim an adapter is usable when Start would refuse it.
            let usable = match (&program, spec.id) {
                (Some(path), "debugpy") => debugpy_problem(path).is_none(),
                (Some(_), _) => true,
                (None, _) => false,
            };
            json!({
                "id": spec.id,
                "extensions": spec.extensions,
                "install": spec.install,
                "installed": usable,
                "program": program.map(|path| path.to_string_lossy().to_string()),
            })
        })
        .collect::<Vec<_>>();
    json!({"adapters": rows})
}

/// The request's optional launch settings, validated before anything spawns.
fn parse_args(value: Option<&Value>) -> Result<Vec<String>, (StatusCode, String)> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err((
            StatusCode::BAD_REQUEST,
            "`args` must be an array of strings.".to_string(),
        ));
    };
    if items.len() > MAX_ARGS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("At most {MAX_ARGS} program arguments are supported."),
        ));
    }
    let mut args = Vec::new();
    for item in items {
        let Some(text) = item.as_str() else {
            return Err((
                StatusCode::BAD_REQUEST,
                "Every program argument must be a string.".to_string(),
            ));
        };
        if text.chars().count() > MAX_ARG_CHARS {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("An argument is limited to {MAX_ARG_CHARS} characters."),
            ));
        }
        if text.chars().any(char::is_control) {
            return Err((
                StatusCode::BAD_REQUEST,
                "An argument cannot contain a control character.".to_string(),
            ));
        }
        args.push(text.to_string());
    }
    Ok(args)
}

fn parse_breakpoint_files(input: &Value) -> Result<Breakpoints, (StatusCode, String)> {
    let mut map = Breakpoints::new();
    let Some(items) = input.as_array() else {
        return Err((
            StatusCode::BAD_REQUEST,
            "`breakpoints` must be an array of {path, lines}.".to_string(),
        ));
    };
    if items.len() > MAX_BREAKPOINT_FILES {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("At most {MAX_BREAKPOINT_FILES} files can hold breakpoints at once."),
        ));
    }
    for item in items {
        let path = clean_path(item.get("path").and_then(Value::as_str).unwrap_or_default())?;
        let list = parse_breakpoints(item.get("lines").unwrap_or(&Value::Null))?;
        if !list.is_empty() {
            map.insert(path, list);
        }
    }
    Ok(map)
}

/// An attach request: the adapter, the loopback debug server and the label the
/// session reports. Split out so every refusal is unit-tested without spawning
/// anything.
fn parse_attach(
    input: &Value,
) -> Result<(AttachTarget, &'static AdapterSpec, String), (StatusCode, String)> {
    let port = input
        .get("port")
        .and_then(Value::as_u64)
        .filter(|port| (1..=65535).contains(port))
        .ok_or((
            StatusCode::BAD_REQUEST,
            "Name the port the program's debug server listens on (1-65535).".to_string(),
        ))? as u16;
    let host = input
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let host = if host.is_empty() {
        "127.0.0.1".to_string()
    } else {
        host
    };
    if !matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost") {
        return Err((
            StatusCode::BAD_REQUEST,
            "Attach only reaches a debug server on this machine (127.0.0.1, ::1 or localhost)."
                .to_string(),
        ));
    }
    let adapter = input
        .get("adapter")
        .and_then(Value::as_str)
        .map(|id| id.trim().to_ascii_lowercase())
        .filter(|id| !id.is_empty());
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty());
    let label: String;
    let spec = match (&adapter, path) {
        (Some(id), _) => {
            label = id.clone();
            ADAPTERS.iter().find(|spec| spec.id == *id).ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    format!(
                        "No debug adapter is called \"{id}\". Use one of: {}.",
                        ADAPTERS
                            .iter()
                            .map(|spec| spec.id)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
            })?
        }
        (None, Some(path)) => {
            let path = clean_path(path)?;
            let spec = spec_for(&path).ok_or((
                StatusCode::BAD_REQUEST,
                "This build debugs Rust, C, C++, Python and Go. Name one of those files, or an \
                 adapter id."
                    .to_string(),
            ))?;
            label = path;
            spec
        }
        (None, None) => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Name the file whose language picks the debugger, or an adapter id (lldb-dap, \
                 debugpy, dlv)."
                    .to_string(),
            ))
        }
    };
    Ok((AttachTarget { host, port }, spec, label))
}

/// Attach to a debug server the program already runs: the adapter is started and
/// asked to attach instead of launch, so no target file is required.
fn start_attach(state: &AppState, input: &Value) -> Result<Value, (StatusCode, String)> {
    let root = state
        .workspace
        .root()
        .map_err(|error| (StatusCode::BAD_REQUEST, error.message().to_string()))?;
    let (target, spec, label) = parse_attach(input)?;
    let launch = resolve_launch(spec)?;
    preflight(spec, &launch)?;
    let breakpoints = match input.get("breakpoints") {
        Some(value) => parse_breakpoint_files(value)?,
        None => load_stored(&state.database_path, &root)
            .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?,
    };
    begin(SessionOptions {
        launch,
        program: label,
        program_path: PathBuf::new(),
        root,
        launch_args: Vec::new(),
        launch_cwd: None,
        stop_on_entry: false,
        breakpoints,
        attach: Some(target),
    })
}

fn start_session(state: &AppState, input: &Value) -> Result<Value, (StatusCode, String)> {
    let request = input
        .get("request")
        .and_then(Value::as_str)
        .unwrap_or("launch")
        .trim()
        .to_ascii_lowercase();
    if request == "attach" {
        return start_attach(state, input);
    }
    if request != "launch" {
        return Err((
            StatusCode::BAD_REQUEST,
            "`request` must be \"launch\" or \"attach\".".to_string(),
        ));
    }
    let root = state
        .workspace
        .root()
        .map_err(|error| (StatusCode::BAD_REQUEST, error.message().to_string()))?;
    let path = clean_path(
        input
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )?;
    let program = match input
        .get("program")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(program) => clean_path(program)?,
        None => path.clone(),
    };
    let program_path = crate::workspace::resolve_inside(&root, &program)
        .map_err(|error| (StatusCode::BAD_REQUEST, error.message().to_string()))?;
    if !program_path.is_file() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "{program} is not a file in the open folder. Build it first, or name the binary with \
                 `program`."
            ),
        ));
    }
    let spec = spec_for(&program).ok_or((
        StatusCode::BAD_REQUEST,
        "This build debugs Rust, C, C++, Python and Go. Open one of those files, or name the \
         program explicitly."
            .to_string(),
    ))?;
    let launch = resolve_launch(spec)?;
    preflight(spec, &launch)?;
    let launch_args = parse_args(input.get("args"))?;
    let launch_cwd = input
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| crate::workspace::resolve_inside(&root, value))
        .transpose()
        .map_err(|error| (StatusCode::BAD_REQUEST, error.message().to_string()))?;
    let stop_on_entry = input
        .get("stopOnEntry")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let breakpoints = match input.get("breakpoints") {
        Some(value) => parse_breakpoint_files(value)?,
        None => load_stored(&state.database_path, &root)
            .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?,
    };
    begin(SessionOptions {
        launch,
        program: program.clone(),
        program_path,
        root,
        launch_args,
        launch_cwd,
        stop_on_entry,
        breakpoints,
        attach: None,
    })
}

// ---------------------------------------------------------------------------
// HTTP surface

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/debug/adapters", get(adapters_route))
        .route("/api/debug/state", get(state_route))
        .route("/api/debug/start", post(start_route))
        .route("/api/debug/stop", post(stop_route))
        .route("/api/debug/breakpoints", post(breakpoints_route))
        .route("/api/debug/continue", post(continue_route))
        .route("/api/debug/pause", post(pause_route))
        .route("/api/debug/next", post(next_route))
        .route("/api/debug/step-in", post(step_in_route))
        .route("/api/debug/step-out", post(step_out_route))
        .route("/api/debug/stack", get(stack_route))
        .route("/api/debug/scopes", get(scopes_route))
        .route("/api/debug/variables", get(variables_route))
        .route("/api/debug/evaluate", post(evaluate_route))
}

async fn adapters_route() -> Json<Value> {
    Json(adapters_json())
}

async fn state_route(State(state): State<AppState>) -> Json<Value> {
    let mut value = state_json();
    if value.get("running").and_then(Value::as_bool) == Some(false) {
        let root = state.workspace.root().ok();
        let (breakpoints, error) = persisted_breakpoints(&state.database_path, root.as_deref());
        value["breakpoints"] = breakpoints;
        if let Some(error) = error {
            value["breakpointsError"] = json!(error);
        }
    }
    Json(value)
}

async fn start_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let target = input
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // Starting a debugger runs a program: the same permission a command needs.
    if let Some(response) = crate::permissions::guard_user(
        &state,
        crate::permissions::Action::Command,
        &target,
        "",
        approve,
        "start a debug session",
    ) {
        return response;
    }
    match tokio::task::spawn_blocking(move || start_session(&state, &input)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err((status, message))) => json_error(status, message),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The debugger task failed.",
        ),
    }
}

async fn stop_route() -> Response {
    match tokio::task::spawn_blocking(stop_session).await {
        Ok(value) => Json(value).into_response(),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The debugger task failed.",
        ),
    }
}

async fn breakpoints_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    match tokio::task::spawn_blocking(move || set_breakpoints(&state, &input)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err((status, message))) => json_error(status, message),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The debugger task failed.",
        ),
    }
}

async fn step_command(command: &'static str, thread: Option<i64>) -> Response {
    match tokio::task::spawn_blocking(move || step(command, thread)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err((status, message))) => json_error(status, message),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The debugger task failed.",
        ),
    }
}

fn body_thread(input: &Value) -> Option<i64> {
    input.get("threadId").and_then(Value::as_i64)
}

async fn continue_route(Json(input): Json<Value>) -> Response {
    step_command("continue", body_thread(&input)).await
}

async fn pause_route(Json(input): Json<Value>) -> Response {
    step_command("pause", body_thread(&input)).await
}

async fn next_route(Json(input): Json<Value>) -> Response {
    step_command("next", body_thread(&input)).await
}

async fn step_in_route(Json(input): Json<Value>) -> Response {
    step_command("stepIn", body_thread(&input)).await
}

async fn step_out_route(Json(input): Json<Value>) -> Response {
    step_command("stepOut", body_thread(&input)).await
}

#[derive(Deserialize)]
struct ThreadQuery {
    thread: Option<i64>,
}

#[derive(Deserialize)]
struct FrameQuery {
    frame: Option<i64>,
}

#[derive(Deserialize)]
struct VariablesQuery {
    reference: Option<i64>,
}

async fn stack_route(Query(query): Query<ThreadQuery>) -> Response {
    match tokio::task::spawn_blocking(move || stack(query.thread)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err((status, message))) => json_error(status, message),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The debugger task failed.",
        ),
    }
}

async fn scopes_route(Query(query): Query<FrameQuery>) -> Response {
    let Some(frame) = query.frame else {
        return json_error(StatusCode::BAD_REQUEST, "Provide the frame to read.");
    };
    match tokio::task::spawn_blocking(move || scopes(frame)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err((status, message))) => json_error(status, message),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The debugger task failed.",
        ),
    }
}

async fn variables_route(Query(query): Query<VariablesQuery>) -> Response {
    let Some(reference) = query.reference else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Provide the variable reference to expand.",
        );
    };
    match tokio::task::spawn_blocking(move || variables(reference)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err((status, message))) => json_error(status, message),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The debugger task failed.",
        ),
    }
}

async fn evaluate_route(Json(input): Json<Value>) -> Response {
    let expression = input
        .get("expression")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let frame = input.get("frameId").and_then(Value::as_i64);
    match tokio::task::spawn_blocking(move || evaluate(&expression, frame)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err((status, message))) => json_error(status, message),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The debugger task failed.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One session per process: these tests share the static, so they take turns.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    const FAKE_ADAPTER: &str = r#"
import json
import os
import sys

def read_message():
    length = 0
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":", 1)[1])
    body = sys.stdin.buffer.read(length)
    return json.loads(body)

def send(message):
    body = json.dumps(message).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    sys.stdout.buffer.flush()

refuse_initialize = os.environ.get("FAKE_DAP_REFUSE_INIT") == "1"
# Like debugpy: the launch answer waits for configurationDone, because the
# target is not configured until the breakpoints are in place.
pending_start = None

while True:
    message = read_message()
    if message is None:
        break
    seq = message.get("seq", 0)
    command = message.get("command", "")

    def answer(body=None, success=True, text=None):
        reply = {"seq": seq, "type": "response", "request_seq": seq, "command": command, "success": success}
        if body is not None:
            reply["body"] = body
        if text:
            reply["message"] = text
        send(reply)

    if command == "initialize":
        if refuse_initialize:
            answer(success=False, text="this adapter refuses to initialize")
        else:
            answer({"supportsConfigurationDoneRequest": True})
    elif command in ("launch", "attach"):
        # An attach carries the debug server's address and no program; a launch
        # carries the program. Both answer only after configurationDone.
        arguments = message.get("arguments", {})
        if command == "attach" and not (arguments.get("port") and arguments.get("host")):
            answer(success=False, text="attach needs a host and a port")
        else:
            pending_start = (seq, command)
            send({"seq": 100 + seq, "type": "event", "event": "initialized"})
    elif command == "setBreakpoints":
        lines = [item.get("line") for item in message.get("arguments", {}).get("breakpoints", [])]
        answer({"breakpoints": [{"verified": True, "line": line} for line in lines]})
    elif command == "configurationDone":
        answer({})
        if pending_start is not None:
            started_seq, started_command = pending_start
            send({"seq": 600 + seq, "type": "response", "request_seq": started_seq,
                  "command": started_command, "success": True, "body": {}})
            pending_start = None
        send({"seq": 700 + seq, "type": "event", "event": "stopped",
              "body": {"reason": "entry", "threadId": 1, "description": "paused at entry"}})
    elif command in ("continue", "next", "stepIn", "stepOut", "pause"):
        answer({"allThreadsContinued": True})
        send({"seq": 200 + seq, "type": "event", "event": "continued", "body": {"threadId": 1}})
        send({"seq": 300 + seq, "type": "event", "event": "stopped",
              "body": {"reason": "breakpoint", "threadId": 1, "description": "paused at main"}})
    elif command == "threads":
        answer({"threads": [{"id": 1, "name": "main"}]})
    elif command == "stackTrace":
        answer({"stackFrames": [{"id": 7, "name": "main", "source": {"path": "/tmp/main.py"},
                                 "line": 3, "column": 5}], "totalFrames": 1})
    elif command == "scopes":
        answer({"scopes": [{"name": "Locals", "variablesReference": 42, "expensive": False}]})
    elif command == "variables":
        answer({"variables": [{"name": "count", "value": "3", "type": "int",
                               "variablesReference": 0}]})
    elif command == "evaluate":
        answer({"result": "42", "type": "int", "variablesReference": 0})
    elif command == "disconnect":
        answer({})
        send({"seq": 400 + seq, "type": "event", "event": "terminated"})
        break
    else:
        answer(success=False, text="unknown command " + command)

    send({"seq": 500 + seq, "type": "event", "event": "output",
          "body": {"category": "stdout", "output": "hello from the target\n"}})
"#;

    fn python() -> Option<PathBuf> {
        ["python3", "python"]
            .iter()
            .find_map(|program| crate::find_on_path(program))
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sarthi-dap-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn wait_for(predicate: impl Fn(&Value) -> bool) -> bool {
        for _ in 0..80 {
            if predicate(&state_json()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn adapters_answer_for_their_languages_only() {
        assert_eq!(
            spec_for("src/main.rs").map(|spec| spec.id),
            Some("lldb-dap")
        );
        assert_eq!(spec_for("tool.py").map(|spec| spec.id), Some("debugpy"));
        assert_eq!(spec_for("cmd/server.go").map(|spec| spec.id), Some("dlv"));
        assert_eq!(spec_for("web/app.ts").map(|spec| spec.id), None);
        assert_eq!(spec_for("Makefile").map(|spec| spec.id), None);
        assert_eq!(spec_for("").map(|spec| spec.id), None);
    }

    #[test]
    fn paths_stay_inside_the_folder() {
        assert_eq!(clean_path("src/main.rs").unwrap(), "src/main.rs");
        assert!(clean_path("").is_err());
        assert!(clean_path("/etc/passwd").is_err());
        assert!(clean_path("C:/Windows/win.ini").is_err());
        assert!(clean_path("../escape.py").is_err());
        assert!(clean_path("src/../../escape.py").is_err());
        assert!(clean_path("src//main.rs").is_err());
        assert!(clean_path("src/a\u{0}.rs").is_err());
    }

    #[test]
    fn a_missing_adapter_names_what_to_install() {
        let spec = AdapterSpec {
            id: "missing",
            programs: &["sarthi-no-such-debug-adapter"],
            args: &[],
            extensions: &["zz"],
            install: "Install the missing adapter",
        };
        let error = resolve_launch(&spec).unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert!(error.1.contains("missing"), "{}", error.1);
        assert!(
            error.1.contains("Install the missing adapter"),
            "{}",
            error.1
        );
    }

    #[test]
    fn breakpoints_are_stored_per_folder() {
        let dir = scratch("breakpoint-store");
        let db = dir.join("history.sqlite3");
        let root = dir.join("workspace");
        let elsewhere = dir.join("elsewhere");
        std::fs::create_dir_all(&root).expect("workspace");
        std::fs::create_dir_all(&elsewhere).expect("other folder");
        let conn = crate::open_db(&db).expect("database");
        initialize(&conn).expect("schema");
        drop(conn);

        store_file(
            &db,
            &root,
            "main.py",
            &[Breakpoint {
                line: 3,
                condition: "count > 1".to_string(),
            }],
        )
        .expect("store one");
        store_file(
            &db,
            &root,
            "src/lib.rs",
            &[Breakpoint {
                line: 9,
                condition: String::new(),
            }],
        )
        .expect("store two");
        let stored = load_stored(&db, &root).expect("load");
        assert_eq!(stored.len(), 2);
        assert_eq!(stored["main.py"][0].condition, "count > 1");
        assert!(load_stored(&db, &elsewhere)
            .expect("another folder")
            .is_empty());

        // Clearing one file leaves the other, and the idle state reads the rows.
        store_file(&db, &root, "main.py", &[]).expect("clear");
        assert_eq!(load_stored(&db, &root).expect("reload").len(), 1);
        let (value, error) = persisted_breakpoints(&db, Some(&root));
        assert!(error.is_none());
        assert_eq!(value["src/lib.rs"][0]["line"], json!(9));
        assert_eq!(persisted_breakpoints(&db, None).0, json!({}));

        let mut full = Breakpoints::new();
        for index in 0..MAX_BREAKPOINT_FILES {
            full.insert(
                format!("file{index}.py"),
                vec![Breakpoint {
                    line: 1,
                    condition: String::new(),
                }],
            );
        }
        assert!(check_caps(&full).is_ok());
        full.insert(
            "one-more.py".to_string(),
            vec![Breakpoint {
                line: 1,
                condition: String::new(),
            }],
        );
        assert!(check_caps(&full).is_err());
        let mut many = Breakpoints::new();
        many.insert(
            "big.py".to_string(),
            (1..=MAX_BREAKPOINTS as i64)
                .map(|line| Breakpoint {
                    line,
                    condition: String::new(),
                })
                .collect(),
        );
        assert!(check_caps(&many).is_ok());
        many.get_mut("big.py").expect("file").push(Breakpoint {
            line: MAX_BREAKPOINTS as i64 + 1,
            condition: String::new(),
        });
        assert!(check_caps(&many).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn breakpoints_are_parsed_ordered_and_bounded() {
        let list = parse_breakpoints(&json!([{"line": 9}, {"line": 2, "condition": "x > 1"}, 2]))
            .expect("parse");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].line, 2);
        assert_eq!(list[0].condition, "x > 1");
        assert_eq!(list[1].line, 9);
        assert!(parse_breakpoints(&json!([{"line": 0}])).is_err());
        assert!(parse_breakpoints(&json!(["nope"])).is_err());
        assert!(parse_breakpoints(&json!({"line": 1})).is_err());
    }

    /// An attach names a debug server the program already runs, so it needs a
    /// loopback port and an adapter rather than a target file. Every refusal
    /// happens before anything spawns.
    #[test]
    fn an_attach_request_is_checked_before_anything_spawns() {
        let missing_port = parse_attach(&json!({ "path": "main.py" })).unwrap_err();
        assert_eq!(missing_port.0, StatusCode::BAD_REQUEST);
        assert!(missing_port.1.contains("port"), "{}", missing_port.1);
        assert!(parse_attach(&json!({ "port": 0, "path": "main.py" })).is_err());
        assert!(parse_attach(&json!({ "port": 70000, "path": "main.py" })).is_err());
        let remote = parse_attach(&json!({ "port": 5678, "host": "10.0.0.5", "path": "main.py" }))
            .unwrap_err();
        assert!(remote.1.contains("this machine"), "{}", remote.1);
        let unknown = parse_attach(&json!({ "port": 5678, "adapter": "gdb" })).unwrap_err();
        assert!(unknown.1.contains("lldb-dap"), "{}", unknown.1);
        let unnamed = parse_attach(&json!({ "port": 5678 })).unwrap_err();
        assert!(unnamed.1.contains("adapter id"), "{}", unnamed.1);

        // The file's language picks the adapter; an adapter id works on its own.
        let (target, spec, label) =
            parse_attach(&json!({ "port": 5678, "path": "main.py" })).expect("python attach");
        assert_eq!(target.port, 5678);
        assert_eq!(target.host, "127.0.0.1");
        assert_eq!(spec.id, "debugpy");
        assert_eq!(label, "main.py");
        let (target, spec, label) =
            parse_attach(&json!({ "port": 2345, "adapter": "dlv" })).expect("adapter attach");
        assert_eq!(target.port, 2345);
        assert_eq!(spec.id, "dlv");
        assert_eq!(label, "dlv");
        // A path outside the folder is refused like every other path.
        assert!(parse_attach(&json!({ "port": 5678, "path": "../evil.py" })).is_err());
    }

    /// The session sends `attach` with the address it was given — not `launch`
    /// with a program — and the fake adapter refuses an attach that lacks them.
    #[test]
    fn an_attach_session_asks_the_adapter_to_attach() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(python) = python() else {
            eprintln!("skipping: python3 is not on PATH");
            return;
        };
        let root = scratch("attach");
        std::fs::write(root.join("main.py"), "print('hi')\n").expect("target");
        let adapter = root.join("fake_adapter.py");
        std::fs::write(&adapter, FAKE_ADAPTER).expect("adapter");
        let started = begin(SessionOptions {
            launch: LaunchSpec {
                id: "fake".to_string(),
                program: python,
                args: vec![adapter.to_string_lossy().to_string()],
            },
            program: "main.py".to_string(),
            program_path: root.join("main.py"),
            root: root.clone(),
            launch_args: Vec::new(),
            launch_cwd: None,
            stop_on_entry: false,
            breakpoints: Breakpoints::new(),
            attach: Some(AttachTarget {
                host: "127.0.0.1".to_string(),
                port: 5678,
            }),
        })
        .expect("an attach session starts");
        assert_eq!(started["running"], json!(true));
        // The fake adapter answers `attach` only when the host and port arrived,
        // so a refusal here would have failed the session.
        assert_eq!(started["adapter"], json!("fake"));
        let stopped = stop_session();
        assert_eq!(stopped["stopped"], json!(true));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_session_can_start_step_inspect_and_stop() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(python) = python() else {
            eprintln!("skipping: python3 is not on PATH");
            return;
        };
        let root = scratch("session");
        std::fs::write(root.join("main.py"), "print('hi')\n").expect("target");
        let adapter = root.join("fake_adapter.py");
        std::fs::write(&adapter, FAKE_ADAPTER).expect("adapter");
        let started = begin(SessionOptions {
            launch: LaunchSpec {
                id: "fake".to_string(),
                program: python,
                args: vec![adapter.to_string_lossy().to_string()],
            },
            program: "main.py".to_string(),
            program_path: root.join("main.py"),
            root: root.clone(),
            launch_args: Vec::new(),
            launch_cwd: None,
            stop_on_entry: true,
            breakpoints: [(
                "main.py".to_string(),
                vec![Breakpoint {
                    line: 3,
                    condition: String::new(),
                }],
            )]
            .into_iter()
            .collect(),
            attach: None,
        })
        .expect("a fake session starts");
        assert_eq!(started["running"], json!(true));
        assert_eq!(started["adapter"], json!("fake"));
        assert_eq!(started["breakpoints"]["main.py"][0]["line"], json!(3));
        // The failure mode this guards: a session that looks alive but never
        // reported a stop. The fake adapter sends `stopped` with the launch.
        assert!(
            wait_for(|state| state["stopped"]["reason"] == json!("entry")),
            "the entry stop arrives: {}",
            state_json()
        );
        assert!(
            wait_for(|state| state["console"]
                .as_array()
                .map(|lines| lines
                    .iter()
                    .any(|line| line.as_str() == Some("hello from the target")))
                .unwrap_or(false)),
            "the target's output reaches the console: {}",
            state_json()
        );

        let whole_file = vec![Breakpoint {
            line: 2,
            condition: "count > 1".to_string(),
        }];
        let applied = apply_and_record(
            "main.py",
            &whole_file,
            [("main.py".to_string(), whole_file.clone())]
                .into_iter()
                .collect(),
        )
        .expect("breakpoints apply")
        .expect("a live session");
        assert_eq!(applied["verified"], json!(1));
        assert_eq!(
            state_json()["breakpoints"]["main.py"][0]["condition"],
            json!("count > 1")
        );

        step("next", None).expect("step over");
        assert!(
            wait_for(|state| state["stopped"]["reason"] == json!("breakpoint")),
            "the step stops again: {}",
            state_json()
        );

        let frames = stack(None).expect("stack");
        assert_eq!(frames["frames"][0]["name"], json!("main"));
        assert_eq!(frames["frames"][0]["line"], json!(3));
        assert_eq!(frames["total"], json!(1));

        let scopes = scopes(7).expect("scopes");
        assert_eq!(scopes["scopes"][0]["name"], json!("Locals"));
        assert_eq!(scopes["scopes"][0]["reference"], json!(42));

        let variables = variables(42).expect("variables");
        assert_eq!(variables["variables"][0]["name"], json!("count"));
        assert_eq!(variables["variables"][0]["value"], json!("3"));

        let evaluated = evaluate("count + 39", Some(7)).expect("evaluate");
        assert_eq!(evaluated["result"], json!("42"));

        let stopped = stop_session();
        assert_eq!(stopped["stopped"], json!(true));
        assert_eq!(state_json()["running"], json!(false));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_refused_initialize_leaves_no_session_behind() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(python) = python() else {
            eprintln!("skipping: python3 is not on PATH");
            return;
        };
        let root = scratch("refused");
        std::fs::write(root.join("main.py"), "print('hi')\n").expect("target");
        let adapter = root.join("fake_adapter.py");
        std::fs::write(&adapter, FAKE_ADAPTER).expect("adapter");
        let error = begin_with_refusal(python, adapter, &root).unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_GATEWAY);
        assert!(
            error.1.contains("refuses to initialize"),
            "the adapter's own words are kept: {}",
            error.1
        );
        assert_eq!(state_json()["running"], json!(false));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_session_starts_with_adapter_args_and_cwd() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(python) = python() else {
            eprintln!("skipping: python3 is not on PATH");
            return;
        };
        let root = scratch("args-cwd");
        std::fs::write(root.join("main.py"), "print('hi')\n").expect("target");
        let adapter = root.join("fake_adapter.py");
        std::fs::write(&adapter, FAKE_ADAPTER).expect("adapter");
        let started = begin(SessionOptions {
            launch: LaunchSpec {
                id: "fake".to_string(),
                program: python,
                args: vec![adapter.to_string_lossy().to_string()],
            },
            program: "main.py".to_string(),
            program_path: root.join("main.py"),
            root: root.clone(),
            launch_args: vec!["--flag".to_string(), "x".to_string()],
            launch_cwd: Some(root.clone()),
            stop_on_entry: false,
            breakpoints: Breakpoints::new(),
            attach: None,
        })
        .expect("session starts");
        assert_eq!(started["running"], json!(true));
        assert_eq!(started["adapter"], json!("fake"));
        let stopped = stop_session();
        assert_eq!(stopped["stopped"], json!(true));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Same handshake as `begin`, with the adapter told to refuse `initialize`.
    fn begin_with_refusal(
        python: PathBuf,
        adapter: PathBuf,
        root: &Path,
    ) -> Result<Value, (StatusCode, String)> {
        let mut command = Command::new(python);
        command
            .args([adapter.to_string_lossy().to_string()])
            .env("FAKE_DAP_REFUSE_INIT", "1")
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().expect("spawn");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (sender, responses) = mpsc::channel();
        spawn_reader(stdout, shared.clone(), sender);
        let mut session = Session {
            child,
            stdin,
            responses,
            shared,
            seq: 0,
            adapter: "fake".to_string(),
            program: "main.py".to_string(),
            root: root.to_path_buf(),
            breakpoints: Breakpoints::new(),
        };
        session
            .request(
                "initialize",
                json!({"clientID": "test", "adapterID": "fake"}),
                INITIALIZE_TIMEOUT,
            )
            .map_err(|message| abort(session, message))
    }
}
