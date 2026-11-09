//! MCP: a registry of local Model Context Protocol servers, a stdio JSON-RPC
//! session, tool discovery with schema validation, and every call bound to the
//! permission store.
//!
//! Scope is stated plainly rather than implied: a server is either a local
//! child process spoken to over stdio, or a remote endpoint reached over
//! streamable HTTP or the older HTTP+SSE transport. A session is reused while
//! it stays warm (five idle minutes), evicted when the server is edited or a
//! request fails, and swept by a supervisor that pings idle sessions so a
//! wedged one is noticed on a timer instead of by the next user call. There is
//! no sandbox beyond what the transport itself gives; that gap is recorded in
//! `docs/FEATURE-STATUS.md`, not faked here.

use std::collections::HashMap;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path as FsPath, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::permissions::{self, Action as PolicyAction, Decision as PolicyDecision};
use crate::AppState;

/// Revision advertised in the handshake. Servers speaking an older revision
/// still answer; the value is a negotiation starting point, not a promise
/// about what comes back.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// No single operation may hold a child process open longer than this.
const OPERATION_TIMEOUT: Duration = Duration::from_secs(20);

/// Tool output becomes model input later; one call must not flood it.
const RESULT_CAP: usize = 20_000;

/// Descriptions are prompt text, so they are capped at the same order as a
/// skill summary.
const DESCRIPTION_CAP: usize = 2_000;

/// A tool description in an agent prompt is a hint, not documentation.
const PROMPT_DESCRIPTION_CAP: usize = 200;

// ---------------------------------------------------------------------------
// Registry: the servers the user has written down

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Server {
    pub name: String,
    /// How this server is reached: `stdio` (a local child process), `http`
    /// (streamable HTTP) or `sse` (the older HTTP+SSE transport). A registry
    /// written before this field existed reads as the stdio server it was.
    #[serde(default = "default_transport")]
    pub transport: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment for the child process. Values never leave the machine and
    /// never appear in an API response — only the names do.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Where an `http`/`sse` server lives. Empty for a stdio server.
    #[serde(default)]
    pub url: String,
    /// Extra request headers for a remote server. Values are secrets in exactly
    /// the way env values are, so the API returns names only.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_transport() -> String {
    "stdio".to_string()
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    servers: Vec<Server>,
}

fn registry_path(state: &AppState) -> PathBuf {
    state.config_path.with_file_name("mcp.json")
}

/// Servers from disk. A file that cannot be parsed reads as an empty registry:
/// no tools is the safe failure, and the panel can rewrite it.
pub(crate) fn read_servers(path: &FsPath) -> Vec<Server> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<RegistryFile>(&bytes).ok())
        .map(|file| file.servers)
        .unwrap_or_default()
}

fn write_servers(path: &FsPath, servers: &[Server]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&RegistryFile {
        servers: servers.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    crate::set_private_permissions(&temp, false).map_err(|e| e.to_string())?;
    fs::rename(&temp, path).map_err(|e| e.to_string())?;
    crate::set_private_permissions(path, false).map_err(|e| e.to_string())
}

/// A server name becomes half of every permission target, so it stays boring:
/// it must survive a round trip through a shell-free field and a rule prefix.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// `server.tool` — the target a permission rule is written against, so one
/// rule can deny a specific tool while another server's tools still work.
pub(crate) fn tool_target(server: &str, tool: &str) -> String {
    format!("{server}.{tool}")
}

fn valid_transport(transport: &str) -> bool {
    matches!(transport, "stdio" | "http" | "sse")
}

/// A shape check, not a URL parser: the scheme must be http(s), the rest must
/// be non-empty, and neither whitespace nor control characters may appear.
fn valid_url(url: &str) -> bool {
    if url.is_empty() || url.len() > 2048 {
        return false;
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or("");
    !rest.is_empty()
}

/// Enabled servers only: a disabled server never runs for a route or a run.
pub(crate) fn enabled_servers(state: &AppState) -> Vec<Server> {
    read_servers(&registry_path(state))
        .into_iter()
        .filter(|server| server.enabled)
        .collect()
}

/// The public view of a server: environment variable names, never values.
fn server_json(server: &Server) -> Value {
    json!({
        "name": server.name,
        "transport": server.transport,
        "command": server.command,
        "args": server.args,
        "url": server.url,
        "environment": server.env.keys().cloned().collect::<Vec<_>>(),
        "headers": server.headers.keys().cloned().collect::<Vec<_>>(),
        "enabled": server.enabled,
    })
}

// ---------------------------------------------------------------------------
// Health and logs: what this process observed, never what is stored on disk

/// Runtime state lives in memory only. A persisted "healthy" would be a lie the
/// moment the machine rebooted, so the registry file stays the only durable
/// thing here; the panel says `unknown` until a server has actually answered.
static MONITOR: OnceLock<Mutex<BTreeMap<String, ServerState>>> = OnceLock::new();

/// Newest lines kept per server. A server that logs in a loop must not grow
/// this process's memory without bound.
const LOG_LINES: usize = 200;
const LOG_LINE_CAP: usize = 400;

/// A fresh session, once, when the first handshake fails.
const RECONNECT_DELAY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize)]
struct LogEntry {
    /// Unix seconds; the panel renders it in local time.
    at: u64,
    /// `server` is the child's own stderr, `client` is what this app did.
    source: &'static str,
    text: String,
}

#[derive(Debug, Default)]
struct ServerState {
    /// `None` means never contacted, which is not the same as unhealthy.
    healthy: Option<bool>,
    error: String,
    version: String,
    protocol: String,
    tools: usize,
    latency_ms: u64,
    checked_at: u64,
    logs: VecDeque<LogEntry>,
}

fn monitor() -> &'static Mutex<BTreeMap<String, ServerState>> {
    MONITOR.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn with_state<T>(name: &str, change: impl FnOnce(&mut ServerState) -> T) -> T {
    let mut all = monitor()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    change(all.entry(name.to_string()).or_default())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

fn record_log(name: &str, source: &'static str, text: impl Into<String>) {
    let text = cap(&text.into(), LOG_LINE_CAP);
    with_state(name, |state| {
        state.logs.push_back(LogEntry {
            at: now_secs(),
            source,
            text,
        });
        while state.logs.len() > LOG_LINES {
            state.logs.pop_front();
        }
    });
}

fn record_handshake(name: &str, protocol: &str, version: &str, tools: usize) {
    with_state(name, |state| {
        state.protocol = protocol.to_string();
        state.version = version.to_string();
        state.tools = tools;
    });
}

fn record_health(name: &str, healthy: bool, error: String, latency_ms: u64) {
    with_state(name, |state| {
        state.healthy = Some(healthy);
        state.error = error;
        state.latency_ms = latency_ms;
        state.checked_at = now_secs();
    });
}

/// The health row the panel reads. A server that was never contacted reports
/// `unknown` — not `healthy`, and not `unhealthy`.
fn report_json(server: &Server, state: &ServerState) -> Value {
    json!({
        "name": server.name,
        "enabled": server.enabled,
        "health": match state.healthy {
            None => "unknown",
            Some(true) => "healthy",
            Some(false) => "unhealthy",
        },
        "error": state.error,
        "version": state.version,
        "protocol": state.protocol,
        "tools": state.tools,
        "latency_ms": state.latency_ms,
        "checked_at": state.checked_at,
        "log_lines": state.logs.len(),
    })
}

/// Every registered server's health row. Public within the crate because the
/// diagnostics timeline projects the same rows the panel shows, rather than
/// deriving a second opinion.
pub(crate) fn health_rows(state: &AppState) -> Vec<Value> {
    let servers = read_servers(&registry_path(state));
    let all = monitor()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let blank = ServerState::default();
    servers
        .iter()
        .map(|server| report_json(server, all.get(&server.name).unwrap_or(&blank)))
        .collect()
}

// ---------------------------------------------------------------------------
// Session: one child per operation, newline-delimited JSON-RPC 2.0

struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

/// One JSON-RPC message per line. MCP's stdio transport is exactly this.
fn encode(message: &Value) -> String {
    let mut line = message.to_string();
    line.push('\n');
    line
}

/// The answer to *our* id, if this line is one. Anything else — a
/// notification, another request, a log line a server wrote to stdout — is not
/// ours and reads as `None`, so the session keeps waiting instead of failing.
fn match_reply(value: Value, id: u64) -> Option<Result<Value, String>> {
    if value.get("id").and_then(Value::as_u64) != Some(id) {
        return None;
    }
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("MCP server returned an error");
        return Some(Err(message.to_string()));
    }
    Some(Ok(value.get("result").cloned().unwrap_or(Value::Null)))
}

fn reply_for(line: &str, id: u64) -> Option<Result<Value, String>> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    match_reply(value, id)
}

impl Session {
    fn start(server: &Server) -> Result<Session, String> {
        let mut command = Command::new(&server.command);
        command
            .args(&server.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, value) in &server.env {
            command.env(key, value);
        }
        let mut child = command.spawn().map_err(|e| {
            format!(
                "Could not start MCP server \"{}\" ({}): {e}",
                server.name, server.command
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "MCP server has no stdin.".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "MCP server has no stdout.".to_string())?;
        // Stderr is captured, not discarded: a server that refuses to start
        // usually says why there, and "it did not answer" is not a diagnosis.
        // The reader ends with the child, and its buffer is bounded.
        if let Some(stderr) = child.stderr.take() {
            let name = server.name.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    record_log(&name, "server", line);
                }
            });
        }
        Ok(Session {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 1,
        })
    }

    async fn send(&mut self, method: &str, params: Value, id: Option<u64>) -> Result<(), String> {
        let mut message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        if let Some(id) = id {
            message["id"] = json!(id);
        }
        let line = encode(&message);
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Writing to the MCP server failed: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("Writing to the MCP server failed: {e}"))
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(method, params, Some(id)).await?;
        let answer = async {
            loop {
                let line = self
                    .lines
                    .next_line()
                    .await
                    .map_err(|e| format!("Reading from the MCP server failed: {e}"))?
                    .ok_or_else(|| {
                        "The MCP server closed its output before answering.".to_string()
                    })?;
                if let Some(answer) = reply_for(&line, id) {
                    return answer;
                }
            }
        };
        match tokio::time::timeout(OPERATION_TIMEOUT, answer).await {
            Err(_) => Err(format!(
                "The MCP server did not answer {method} within {}s.",
                OPERATION_TIMEOUT.as_secs()
            )),
            Ok(answer) => answer,
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(method, params, None).await
    }

    /// A liveness probe: any mapped answer proves the child is speaking.
    /// A JSON-RPC error to `ping` still counts — the question was heard.
    async fn poll(&mut self, within: Duration) -> PingOutcome {
        let id = self.next_id;
        self.next_id += 1;
        if let Err(error) = self.send("ping", json!({}), Some(id)).await {
            return PingOutcome::Failed(error);
        }
        let reply = async {
            loop {
                match self
                    .lines
                    .next_line()
                    .await
                    .map_err(|e| format!("Reading from the MCP server failed: {e}"))?
                {
                    None => {
                        return Err("The MCP server closed its output before answering.".to_string())
                    }
                    Some(line) => {
                        if reply_for(&line, id).is_some() {
                            return Ok(());
                        }
                    }
                }
            }
        };
        match tokio::time::timeout(within, reply).await {
            Err(_) => PingOutcome::TimedOut,
            Ok(Err(error)) => PingOutcome::Failed(error),
            Ok(Ok(())) => PingOutcome::Answered,
        }
    }

    async fn close(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

/// One MCP conversation, however it is carried. The transport changes how a
/// message travels; the handshake, the catalogue and every call are the same.
enum Connection {
    /// Boxed because a live child process plus its two pipes is far larger than
    /// the remote sessions, and every `Connection` would otherwise carry that
    /// size even when it is a URL.
    Stdio(Box<Session>),
    Http(HttpSession),
    Sse(SseSession),
}

impl Connection {
    /// Open the transport the registry names. A remote server is a URL and a
    /// few headers; a stdio server is a child process.
    async fn open(server: &Server) -> Result<Connection, String> {
        match server.transport.as_str() {
            "http" => Ok(Connection::Http(HttpSession::start(server)?)),
            "sse" => Ok(Connection::Sse(SseSession::start(server).await?)),
            _ => Ok(Connection::Stdio(Box::new(Session::start(server)?))),
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        match self {
            Connection::Stdio(session) => session.request(method, params).await,
            Connection::Http(session) => session.request(method, params).await,
            Connection::Sse(session) => session.request(method, params).await,
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        match self {
            Connection::Stdio(session) => session.notify(method, params).await,
            Connection::Http(session) => session.notify(method, params).await,
            Connection::Sse(session) => session.notify(method, params).await,
        }
    }

    async fn close(self) {
        match self {
            Connection::Stdio(session) => (*session).close().await,
            Connection::Http(_) => {}
            Connection::Sse(session) => session.close().await,
        }
    }

    /// A liveness probe the whole transport can answer: any mapped answer
    /// counts, because `ping`'s JSON-RPC body is not the supervisor's concern.
    async fn poll(&mut self, within: Duration) -> PingOutcome {
        match self {
            Connection::Stdio(session) => session.poll(within).await,
            Connection::Http(session) => session.poll(within).await,
            Connection::Sse(session) => session.poll(within).await,
        }
    }
}

/// What a supervisor probe decided about a pooled session. `Answered` covers
/// both a `ping` result and a JSON-RPC error to it — the server is speaking;
/// only silence (`TimedOut`) or a transport failure (`Failed`) is a wedge.
enum PingOutcome {
    Answered,
    TimedOut,
    Failed(String),
}

// ---------------------------------------------------------------------------
// Supervisor: idle sessions are probed before a user call pays for a wedged one
//
// A pooled session may stop working while nobody is calling: a stdio child can
// hang between writes, a remote endpoint can stop answering while keeping the
// TCP stream. Without sweeping, the next user call pays the full operation
// timeout before the fresh-handshake fallback. The supervisor probes each idle
// session on a timer, evicts the wedged ones, and does so only for sessions
// that are in the map — a session in use is checked out and never probed.

/// Streamable HTTP: one POST per message, and the answer comes back either as
/// JSON or as an SSE stream on that same response. The handshake may hand back
/// an `Mcp-Session-Id` that later requests must carry.
struct HttpSession {
    client: reqwest::Client,
    url: String,
    headers: BTreeMap<String, String>,
    session_id: Option<String>,
    next_id: u64,
}

impl HttpSession {
    fn start(server: &Server) -> Result<HttpSession, String> {
        let client = reqwest::Client::builder()
            .timeout(OPERATION_TIMEOUT)
            .build()
            .map_err(|e| format!("Could not build an HTTP client: {e}"))?;
        Ok(HttpSession {
            client,
            url: server.url.clone(),
            headers: server.headers.clone(),
            session_id: None,
            next_id: 1,
        })
    }

    async fn post(&mut self, message: &Value) -> Result<reqwest::Response, String> {
        let mut request = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        for (key, value) in &self.headers {
            request = request.header(key, value);
        }
        if let Some(id) = &self.session_id {
            request = request.header("mcp-session-id", id);
        }
        let response = request
            .body(message.to_string())
            .send()
            .await
            .map_err(|e| format!("Talking to the MCP server failed: {e}"))?;
        if let Some(id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            self.session_id = Some(id.to_string());
        }
        Ok(response)
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let response = self.post(&message).await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!(
                "The MCP server answered {method} with HTTP {}.",
                response.status()
            ))
        }
    }

    /// A liveness probe. A 2xx answer to `ping`, whatever its JSON-RPC body,
    /// proves the endpoint is talking; anything else is a dead transport.
    async fn poll(&mut self, within: Duration) -> PingOutcome {
        let message = json!({ "jsonrpc": "2.0", "id": 0, "method": "ping", "params": {} });
        let probe = self.post(&message);
        match tokio::time::timeout(within, probe).await {
            Err(_) => PingOutcome::TimedOut,
            Ok(Err(error)) => PingOutcome::Failed(error),
            Ok(Ok(response)) => {
                if response.status().is_success() {
                    PingOutcome::Answered
                } else {
                    PingOutcome::Failed(format!(
                        "The MCP server answered ping with HTTP {}.",
                        response.status()
                    ))
                }
            }
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let response = self.post(&message).await?;
        if !response.status().is_success() {
            return Err(format!(
                "The MCP server answered {method} with HTTP {}.",
                response.status()
            ));
        }
        let is_sse = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"));
        let body = response
            .text()
            .await
            .map_err(|e| format!("Reading the MCP server's answer failed: {e}"))?;
        let answers = if is_sse {
            sse_messages(&body)
        } else {
            serde_json::from_str::<Value>(&body)
                .ok()
                .into_iter()
                .collect::<Vec<_>>()
        };
        for value in answers {
            if let Some(answer) = match_reply(value, id) {
                return answer;
            }
        }
        Err(format!(
            "The MCP server answered {method} without a JSON-RPC result."
        ))
    }
}

/// The older HTTP+SSE transport: one GET stream carries every server message,
/// and requests are POSTed to the `endpoint` the server announces on it.
struct SseSession {
    client: reqwest::Client,
    endpoint: String,
    headers: BTreeMap<String, String>,
    incoming: tokio::sync::mpsc::UnboundedReceiver<Value>,
    reader: tokio::task::JoinHandle<()>,
    next_id: u64,
}

impl SseSession {
    async fn start(server: &Server) -> Result<SseSession, String> {
        use futures_util::StreamExt;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| format!("Could not build an HTTP client: {e}"))?;
        let mut request = client
            .get(&server.url)
            .header("accept", "text/event-stream");
        for (key, value) in &server.headers {
            request = request.header(key, value);
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("Connecting to the MCP server failed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "The MCP server's event stream answered HTTP {}.",
                response.status()
            ));
        }
        let base = server.url.clone();
        let (endpoint_tx, endpoint_rx) = tokio::sync::oneshot::channel::<String>();
        let (incoming_tx, incoming) = tokio::sync::mpsc::unbounded_channel::<Value>();
        // The stream stays open for the life of the session; the task ends when
        // the server closes it, and `close` aborts it if the server never does.
        let reader = tokio::spawn(async move {
            let mut endpoint_tx = Some(endpoint_tx);
            let mut buffer = String::new();
            let mut event = String::new();
            let mut data: Vec<String> = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let Ok(bytes) = chunk else { break };
                buffer.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(index) = buffer.find('\n') {
                    let mut line = buffer[..index].to_string();
                    buffer.drain(..=index);
                    if line.ends_with('\r') {
                        line.pop();
                    }
                    if line.is_empty() {
                        if event == "endpoint" && !data.is_empty() {
                            if let Some(tx) = endpoint_tx.take() {
                                let _ = tx.send(resolve_endpoint(&base, &data.join("\n")));
                            }
                        } else if !data.is_empty() {
                            if let Ok(value) = serde_json::from_str::<Value>(&data.join("\n")) {
                                if incoming_tx.send(value).is_err() {
                                    return;
                                }
                            }
                        }
                        event.clear();
                        data.clear();
                        continue;
                    }
                    if let Some(value) = line.strip_prefix("event:") {
                        event = value.trim().to_string();
                    } else if let Some(value) = line.strip_prefix("data:") {
                        data.push(value.trim_start().to_string());
                    }
                }
            }
        });
        let endpoint = tokio::time::timeout(Duration::from_secs(10), endpoint_rx)
            .await
            .map_err(|_| "The MCP server never announced its message endpoint.".to_string())?
            .map_err(|_| {
                "The MCP server closed its event stream before announcing an endpoint.".to_string()
            })?;
        Ok(SseSession {
            client,
            endpoint,
            headers: server.headers.clone(),
            incoming,
            reader,
            next_id: 1,
        })
    }

    async fn send(&self, message: &Value) -> Result<reqwest::Response, String> {
        let mut request = self
            .client
            .post(&self.endpoint)
            .header("content-type", "application/json");
        for (key, value) in &self.headers {
            request = request.header(key, value);
        }
        request
            .body(message.to_string())
            .send()
            .await
            .map_err(|e| format!("Talking to the MCP server failed: {e}"))
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let response = self.send(&message).await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!(
                "The MCP server answered {method} with HTTP {}.",
                response.status()
            ))
        }
    }

    /// A liveness probe for a legacy HTTP+SSE session: the answer arrives on
    /// the open event stream, so `ping` is proven only by a mapped reply.
    async fn poll(&mut self, within: Duration) -> PingOutcome {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": "ping", "params": {} });
        if let Err(error) = self.send(&message).await {
            return PingOutcome::Failed(error);
        }
        let reply = async {
            loop {
                let value = self
                    .incoming
                    .recv()
                    .await
                    .ok_or_else(|| "The MCP server closed its event stream.".to_string())?;
                if match_reply(value, id).is_some() {
                    return Ok(());
                }
            }
        };
        match tokio::time::timeout(within, reply).await {
            Err(_) => PingOutcome::TimedOut,
            Ok(Err(error)) => PingOutcome::Failed(error),
            Ok(Ok(())) => PingOutcome::Answered,
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let response = self.send(&message).await?;
        if !response.status().is_success() {
            return Err(format!(
                "The MCP server answered {method} with HTTP {}.",
                response.status()
            ));
        }
        let answer = async {
            loop {
                let value = self
                    .incoming
                    .recv()
                    .await
                    .ok_or_else(|| "The MCP server closed its event stream.".to_string())?;
                if let Some(answer) = match_reply(value, id) {
                    return answer;
                }
            }
        };
        match tokio::time::timeout(OPERATION_TIMEOUT, answer).await {
            Err(_) => Err(format!(
                "The MCP server did not answer {method} within {}s.",
                OPERATION_TIMEOUT.as_secs()
            )),
            Ok(answer) => answer,
        }
    }

    async fn close(self) {
        self.reader.abort();
    }
}

/// A dropped SSE session must not leave its reader running: the pool replaces
/// entries when a server is edited or goes idle, and those paths drop instead
/// of awaiting `close`.
impl Drop for SseSession {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

/// Every JSON payload in an SSE body. `data:` lines within one event are joined
/// with a newline, which is what the transport specifies.
fn sse_messages(body: &str) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut data: Vec<&str> = Vec::new();
    for line in body.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            flush_sse_data(&mut data, &mut messages);
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data.push(value.trim_start());
        }
    }
    flush_sse_data(&mut data, &mut messages);
    messages
}

fn flush_sse_data(data: &mut Vec<&str>, messages: &mut Vec<Value>) {
    if data.is_empty() {
        return;
    }
    if let Ok(value) = serde_json::from_str::<Value>(&data.join("\n")) {
        messages.push(value);
    }
    data.clear();
}

/// The endpoint event carries either an absolute URL or a path relative to the
/// stream. Both are legal, so both are handled.
fn resolve_endpoint(base: &str, endpoint: &str) -> String {
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        return endpoint.to_string();
    }
    let (scheme, rest) = base.split_once("://").unwrap_or(("http", base));
    let host = rest.split('/').next().unwrap_or(rest);
    if endpoint.starts_with('/') {
        format!("{scheme}://{host}{endpoint}")
    } else {
        format!("{scheme}://{host}/{endpoint}")
    }
}

// ---------------------------------------------------------------------------
// Pool: one live session per server, reused until it goes stale
//
// Without this, every call paid a process start and a handshake. A warm session
// is checked out, asked for its tool list (so validation still reflects the
// server), and handed back by `close`. The pool lives in memory and is bounded
// by an idle window; editing a server evicts its session immediately.

const POOL_IDLE: Duration = Duration::from_secs(300);

/// How often the supervisor looks at idle sessions, and how long a probe waits
/// before it declares silence. The sweep window is then deliberately shorter
/// than the operation ceiling that a user call would pay.
const SUPERVISOR_INTERVAL: Duration = Duration::from_secs(60);
const SUPERVISOR_PING: Duration = Duration::from_secs(5);

struct PooledSession {
    fingerprint: String,
    protocol: String,
    version: String,
    last_used: Instant,
    connection: Connection,
}

fn pool() -> &'static Mutex<BTreeMap<String, PooledSession>> {
    static POOL: OnceLock<Mutex<BTreeMap<String, PooledSession>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Everything that decides what a live connection is talking to. Env and header
/// values are included because a token change is a different identity; the
/// string never leaves this process and is never logged.
fn fingerprint(server: &Server) -> String {
    format!(
        "{}|{}|{:?}|{}|{:?}|{:?}",
        server.transport, server.command, server.args, server.url, server.env, server.headers
    )
}

fn still_warm(idle: Duration) -> bool {
    idle < POOL_IDLE
}

fn take_pooled(name: &str, fingerprint: &str) -> Option<PooledSession> {
    let mut all = pool()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = all.get(name)?;
    // A stale entry is removed (dropping the child) rather than returned: the
    // caller will open a session that matches the settings it just read.
    if entry.fingerprint != fingerprint || !still_warm(entry.last_used.elapsed()) {
        all.remove(name);
        return None;
    }
    all.remove(name)
}

fn put_pooled(
    name: String,
    fingerprint: String,
    protocol: String,
    version: String,
    connection: Connection,
) {
    let mut all = pool()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    all.insert(
        name,
        PooledSession {
            fingerprint,
            protocol,
            version,
            last_used: Instant::now(),
            connection,
        },
    );
}

/// A probe whose session answered is handed back with its original `last_used`:
/// the supervisor works for a user's patience, not for keeping sessions warm,
/// and a re-stamp would keep sessions alive forever.
fn restore_pooled(name: String, entry: PooledSession) {
    let mut all = pool()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // A call that connected while the probe ran already put a fresher session
    // back; keep that one and drop the probed entry.
    all.entry(name).or_insert_with(|| entry);
}

/// Start the supervisor once. Safe to call from every boot path.
pub(crate) fn start_supervisor() {
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    tokio::spawn(supervisor_loop());
}

async fn supervisor_loop() {
    loop {
        supervisor_sweep(SUPERVISOR_PING).await;
        tokio::time::sleep(SUPERVISOR_INTERVAL).await;
    }
}

/// One sweep over the idle sessions. Every entry is checked out by draining
/// (a session in use is never in the map, so the sweep cannot race a call),
/// probed, and either restored or evicted. `within` is the silence budget.
async fn supervisor_sweep(within: Duration) {
    let idle: Vec<(String, PooledSession)> = {
        let mut all = pool()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *all).into_iter().collect()
    };
    for (name, mut entry) in idle {
        match entry.connection.poll(within).await {
            PingOutcome::Answered => restore_pooled(name, entry),
            PingOutcome::TimedOut => {
                entry.connection.close().await;
                record_log(
                    &name,
                    "client",
                    format!(
                        "Supervisor: the idle session stopped answering ping within {}s; it was closed and the next call reconnects.",
                        within.as_secs()
                    ),
                );
            }
            PingOutcome::Failed(reason) => {
                entry.connection.close().await;
                record_log(
                    &name,
                    "client",
                    format!(
                        "Supervisor: the idle session is gone ({reason}); it was closed and the next call reconnects."
                    ),
                );
            }
        }
    }
}

/// A save or a removal changes the entry, so the connection it described is
/// closed instead of being handed to a call that believes the old settings.
pub(crate) fn forget_pooled(name: &str) {
    let mut all = pool()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    all.remove(name);
}

/// One conversation that can outlive its caller: `close` returns a healthy
/// session to the pool, and drops it when an operation failed. A dropped stdio
/// session kills its child (kill_on_drop); a dropped SSE session aborts its
/// reader.
struct LiveSession {
    server: String,
    fingerprint: String,
    protocol: String,
    version: String,
    connection: Option<Connection>,
    reusable: bool,
}

impl LiveSession {
    fn new(server: &Server, connection: Connection, protocol: &str, version: &str) -> LiveSession {
        LiveSession {
            server: server.name.clone(),
            fingerprint: fingerprint(server),
            protocol: protocol.to_string(),
            version: version.to_string(),
            connection: Some(connection),
            reusable: true,
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let Some(connection) = self.connection.as_mut() else {
            return Err("The MCP session is already closed.".to_string());
        };
        let answer = connection.request(method, params).await;
        if answer.is_err() {
            // A failed request is not worth reusing. A JSON-RPC error the server
            // sent would be safe to reuse, but discarding both costs only one
            // reconnect and can never hand back a half-broken stream.
            self.reusable = false;
        }
        answer
    }

    async fn close(mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        if self.reusable {
            put_pooled(
                self.server,
                self.fingerprint,
                self.protocol,
                self.version,
                connection,
            );
        } else {
            connection.close().await;
        }
    }
}

/// What a successful handshake taught us about the server.
struct Handshake {
    session: LiveSession,
    tools: Vec<Value>,
    protocol: String,
    version: String,
}

async fn connect_once(server: &Server) -> Result<Handshake, String> {
    let mut session = Connection::open(server).await?;
    let init = session
        .request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "saarthi-studio",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )
        .await?;
    session
        .notify("notifications/initialized", json!({}))
        .await?;
    let listed = session.request("tools/list", json!({})).await?;
    let tools = listed
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // The server names its own revision and version; the panel shows both so an
    // upgrade is visible without reading the command line.
    let protocol = init
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION)
        .to_string();
    let version = init
        .get("serverInfo")
        .and_then(|info| info.get("version"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let session = LiveSession::new(server, session, &protocol, &version);
    Ok(Handshake {
        session,
        tools,
        protocol,
        version,
    })
}

/// Handshake, then discover tools — a *fresh* session, deliberately: Test, the
/// health probe and a run's discovery all exist to ask what the server offers
/// now. A tool call goes through `connect_pooled` instead, which reuses a warm
/// session. A failed handshake is retried once on a brand-new session — a
/// server can exit while it is still coming up (a lock file, a socket it has
/// not bound yet), and nothing has been called at that point, so the retry
/// cannot duplicate an effect. A `tools/call` is never retried, which is what
/// keeps a side effect from running twice.
async fn connect(server: &Server) -> Result<Handshake, String> {
    let started = Instant::now();
    let handshake = match connect_once(server).await {
        Ok(handshake) => handshake,
        Err(error) => {
            record_log(
                &server.name,
                "client",
                format!("handshake failed: {error} — retrying once"),
            );
            tokio::time::sleep(RECONNECT_DELAY).await;
            match connect_once(server).await {
                Ok(handshake) => handshake,
                Err(second) => {
                    let message = format!("{error} (one retry also failed: {second})");
                    record_log(
                        &server.name,
                        "client",
                        format!("handshake failed again: {second}"),
                    );
                    record_health(
                        &server.name,
                        false,
                        message.clone(),
                        started.elapsed().as_millis() as u64,
                    );
                    return Err(message);
                }
            }
        }
    };
    let elapsed = started.elapsed().as_millis() as u64;
    record_handshake(
        &server.name,
        &handshake.protocol,
        &handshake.version,
        handshake.tools.len(),
    );
    record_health(&server.name, true, String::new(), elapsed);
    record_log(
        &server.name,
        "client",
        format!(
            "connected: {} tools, protocol {}, {} ms",
            handshake.tools.len(),
            handshake.protocol,
            elapsed
        ),
    );
    Ok(handshake)
}

/// The pooled path used by a real tool call: reuse the server's warm session
/// when there is one, and re-read its tool list on that same connection so the
/// argument check still describes the server as it is now. A warm session that
/// stopped answering is closed and replaced by a full handshake. Discovery,
/// Test and the health probe deliberately use `connect` — they exist to ask the
/// server what it offers right now.
async fn connect_pooled(server: &Server) -> Result<Handshake, String> {
    let fingerprint = fingerprint(server);
    let Some(entry) = take_pooled(&server.name, &fingerprint) else {
        return connect(server).await;
    };
    let mut connection = entry.connection;
    let started = Instant::now();
    match connection.request("tools/list", json!({})).await {
        Ok(listed) => {
            let tools = listed
                .get("tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let elapsed = started.elapsed().as_millis() as u64;
            record_handshake(&server.name, &entry.protocol, &entry.version, tools.len());
            record_health(&server.name, true, String::new(), elapsed);
            record_log(
                &server.name,
                "client",
                format!(
                    "reused a pooled session: {} tools, protocol {}, {} ms",
                    tools.len(),
                    entry.protocol,
                    elapsed
                ),
            );
            Ok(Handshake {
                session: LiveSession::new(server, connection, &entry.protocol, &entry.version),
                tools,
                protocol: entry.protocol,
                version: entry.version,
            })
        }
        Err(error) => {
            connection.close().await;
            record_log(
                &server.name,
                "client",
                format!("the pooled session stopped answering ({error}) — opening a new one"),
            );
            connect(server).await
        }
    }
}

/// Discovery output shaped for the panel: names, capped descriptions and the
/// real input schema, which is what makes validation possible later.
fn tools_json(tools: &[Value]) -> Value {
    let rows = tools
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(Value::as_str)?;
            Some(json!({
                "name": name,
                "description": cap(
                    tool.get("description").and_then(Value::as_str).unwrap_or(""),
                    DESCRIPTION_CAP,
                ),
                "input_schema": tool.get("inputSchema").cloned().unwrap_or_else(|| json!({})),
            }))
        })
        .collect::<Vec<_>>();
    json!(rows)
}

/// Tool output becomes model input later; one call must not flood it.
const TOOLS_PER_SERVER: usize = 20;

fn cap(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    value.chars().take(limit).collect()
}

// ---------------------------------------------------------------------------
// Schema validation: refuse a call the server itself would reject

fn matches_type(expected: &str, value: &Value) -> bool {
    match expected {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        // A schema type this client does not model is not a reason to refuse:
        // the server is still the authority on its own tool.
        _ => true,
    }
}

/// Check the arguments against the tool's own `inputSchema`. Only `required`
/// presence and declared scalar types are enforced — enough to catch the
/// mistake a model actually makes (dropping a required field, sending a number
/// as a string) without pretending to implement JSON Schema.
fn validate_arguments(schema: &Value, arguments: &Value) -> Result<(), String> {
    let Some(object) = arguments.as_object() else {
        return Err("Tool arguments must be a JSON object.".to_string());
    };
    let Some(required) = schema.get("required").and_then(Value::as_array) else {
        return Ok(());
    };
    for name in required.iter().filter_map(Value::as_str) {
        let Some(value) = object.get(name) else {
            return Err(format!("Missing required argument \"{name}\"."));
        };
        let expected = schema
            .get("properties")
            .and_then(|properties| properties.get(name))
            .and_then(|property| property.get("type"))
            .and_then(Value::as_str);
        if let Some(expected) = expected {
            if !matches_type(expected, value) {
                return Err(format!("Argument \"{name}\" must be {expected}."));
            }
        }
    }
    Ok(())
}

/// Flatten a `tools/call` result into the text a caller can read, keeping the
/// structured content alongside it for anything richer than text.
fn tool_text(result: &Value) -> String {
    let parts = result
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .filter_map(|part| {
                    if part.get("type").and_then(Value::as_str) == Some("text") {
                        part.get("text").and_then(Value::as_str).map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if parts.is_empty() {
        result.to_string()
    } else {
        parts.join("\n")
    }
}

// ---------------------------------------------------------------------------
// One call path for the HTTP route and the agent loop

/// Why a tool call did not happen. The route maps these onto HTTP statuses; an
/// agent step turns them into text the model can read and repair.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CallFailure {
    NotFound(String),
    Disabled(String),
    BadRequest(String),
    Server(String),
}

impl CallFailure {
    pub(crate) fn message(&self) -> &str {
        match self {
            Self::NotFound(message)
            | Self::Disabled(message)
            | Self::BadRequest(message)
            | Self::Server(message) => message,
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Disabled(_) => StatusCode::CONFLICT,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Server(_) => StatusCode::BAD_GATEWAY,
        }
    }
}

/// Call one tool on one registry server: find the server, open a session,
/// check the arguments against the tool's own schema, call it, and record the
/// outcome. Callers ask the permission store first — this function decides
/// whether a call *worked*, never whether it *may happen*, so both the route
/// and an agent step run through exactly the same execution path.
pub(crate) async fn invoke(
    state: &AppState,
    server_name: &str,
    tool: &str,
    arguments: &Value,
) -> Result<Value, CallFailure> {
    if tool.trim().is_empty() {
        return Err(CallFailure::BadRequest("Name a tool to call.".to_string()));
    }
    let servers = read_servers(&registry_path(state));
    let Some(server) = servers.into_iter().find(|entry| entry.name == server_name) else {
        return Err(CallFailure::NotFound(format!(
            "No MCP server named \"{server_name}\"."
        )));
    };
    if !server.enabled {
        return Err(CallFailure::Disabled(format!(
            "MCP server \"{}\" is disabled.",
            server.name
        )));
    }
    // OAuth: a saved token (refreshed first when past its expiry) rides on the
    // connection unless the user has written their own authorization header.
    // A stdio server needs none, which attachable_header refuses directly.
    let mut server = server;
    if let Some(value) = crate::mcp_oauth::attachable_header(state, &server).await {
        server.headers.insert("authorization".to_string(), value);
    }
    let target = tool_target(&server.name, tool);
    let handshake = connect_pooled(&server).await.map_err(CallFailure::Server)?;
    record_log(&server.name, "client", format!("tools/call {target}"));
    let mut session = handshake.session;
    let tools = handshake.tools;
    let Some(discovered) = tools
        .iter()
        .find(|entry| entry.get("name").and_then(Value::as_str) == Some(tool))
    else {
        session.close().await;
        let known = tools
            .iter()
            .filter_map(|entry| entry.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(CallFailure::NotFound(format!(
            "{} exposes no tool named \"{tool}\". It lists: {}.",
            server.name,
            if known.is_empty() {
                "nothing"
            } else {
                known.as_str()
            }
        )));
    };
    let schema = discovered
        .get("inputSchema")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Err(error) = validate_arguments(&schema, arguments) {
        session.close().await;
        return Err(CallFailure::BadRequest(error));
    }
    let outcome = session
        .request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )
        .await;
    session.close().await;
    match outcome {
        Ok(result) => {
            permissions::record(
                state,
                PolicyAction::McpTool,
                &target,
                PolicyDecision::Allow,
                permissions::Scope::Always,
                "",
                "MCP tool called",
            );
            let text = tool_text(&result);
            Ok(json!({
                "ok": true,
                "server": server.name,
                "tool": tool,
                "is_error": result.get("isError").and_then(Value::as_bool).unwrap_or(false),
                "text": cap(&text, RESULT_CAP),
                "content": result.get("content").cloned().unwrap_or(Value::Null),
            }))
        }
        Err(error) => {
            permissions::record(
                state,
                PolicyAction::McpTool,
                &target,
                PolicyDecision::Allow,
                permissions::Scope::Always,
                "",
                "MCP tool call failed",
            );
            Err(CallFailure::Server(error))
        }
    }
}

/// The catalogue an agent prompt can carry: enabled servers only, one entry
/// per tool with just what a model needs to choose it. Discovery starts each
/// server once, so an agent run takes this snapshot at run start instead of on
/// every step. A server that cannot be reached is reported, never silently
/// dropped — the model should know a tool it heard about is unavailable.
pub(crate) async fn discover_for_prompt(state: &AppState) -> (Vec<Value>, Vec<String>) {
    let mut tools = Vec::new();
    let mut errors = Vec::new();
    for server in enabled_servers(state) {
        let mut server = server;
        if let Some(value) = crate::mcp_oauth::attachable_header(state, &server).await {
            server.headers.insert("authorization".to_string(), value);
        }
        match connect(&server).await {
            Ok(handshake) => {
                handshake.session.close().await;
                for tool in handshake.tools.iter().take(TOOLS_PER_SERVER) {
                    let Some(name) = tool.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let required = tool
                        .get("inputSchema")
                        .and_then(|schema| schema.get("required"))
                        .and_then(Value::as_array)
                        .map(|list| {
                            list.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    tools.push(json!({
                        "server": server.name,
                        "name": name,
                        "description": cap(
                            tool.get("description").and_then(Value::as_str).unwrap_or(""),
                            PROMPT_DESCRIPTION_CAP,
                        ),
                        "required": required,
                    }));
                }
            }
            Err(error) => errors.push(format!("{}: {}", server.name, cap(&error, 200))),
        }
    }
    (tools, errors)
}

// ---------------------------------------------------------------------------
// Routes

pub(crate) fn router(port: u16) -> Router<AppState> {
    let read_only = Router::new()
        .route("/api/mcp", get(list_servers))
        .route("/api/mcp/health", get(health_report))
        .route("/api/mcp/logs", get(server_logs))
        // The browser arrives here from the authorization server's redirect,
        // so it must not be origin-guarded and it must answer HTML.
        .route("/api/mcp/oauth/callback", get(oauth_callback))
        .route("/api/mcp/:name/oauth/status", get(oauth_status));
    let mutations = Router::new()
        .route("/api/mcp/servers", post(save_server))
        .route("/api/mcp/servers/remove", post(remove_server))
        .route("/api/mcp/test", post(test_server))
        .route("/api/mcp/call", post(call_tool))
        .route("/api/mcp/health/check", post(check_health))
        .route("/api/mcp/:name/oauth/start", post(oauth_start))
        .route("/api/mcp/:name/oauth/forget", post(oauth_forget))
        .route_layer(axum::middleware::from_fn_with_state(
            crate::workspace::LocalOriginGuard::new(port),
            crate::workspace::guard_local_origin,
        ));
    read_only
        .merge(mutations)
        // The redirect URI names this app's port, so the flow handlers carry it.
        .layer(axum::Extension(port))
}

// ---------------------------------------------------------------------------
// OAuth for remote servers
//
// The sign-in spans two endpoints: /start opens the browser and waits, and
// /oauth/callback (read-only, browser-driven) hands the waiting side its code.
// Tokens never appear here — only booleans and the expiry — because the OS
// credential store keeps the values.

async fn oauth_status(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    let server = read_servers(&registry_path(&state))
        .into_iter()
        .find(|entry| entry.name == name);
    let Some(server) = server else {
        return crate::json_error(StatusCode::NOT_FOUND, format!("Unknown MCP server: {name}"))
            .into_response();
    };
    let remote = !server.url.trim().is_empty();
    let session = crate::mcp_oauth::credentials(&state, &name);
    let (registered, authorized, expires_at, has_refresh) = match &session {
        Some(creds) => (
            creds.registered(),
            creds.authorized(),
            creds.expires_at,
            !creds.refresh_token.trim().is_empty(),
        ),
        None => (false, false, 0.0, false),
    };
    Json(json!({
        "ok": true,
        "name": name,
        "transport": server.transport,
        "url": if remote { server.url.clone() } else { String::new() },
        // A hand-written authorization header overrides anything OAuth does.
        "manual_header": server.headers.contains_key("authorization"),
        "registered": registered,
        "authorized": authorized,
        "expires_at": expires_at,
        "has_refresh": has_refresh,
    }))
    .into_response()
}

async fn oauth_start(
    State(state): State<AppState>,
    axum::Extension(port): axum::Extension<u16>,
    Path(name): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    let server = read_servers(&registry_path(&state))
        .into_iter()
        .find(|entry| entry.name == name);
    let Some(server) = server else {
        return crate::json_error(StatusCode::NOT_FOUND, format!("Unknown MCP server: {name}"))
            .into_response();
    };
    if server.url.trim().is_empty() {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "A stdio server is a local program and needs no OAuth sign-in.",
        )
        .into_response();
    }
    if server.headers.contains_key("authorization") {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "This server carries a hand-written authorization header; remove it before signing in.",
        )
        .into_response();
    }
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(response) = crate::permissions::guard_user(
        &state,
        PolicyAction::Network,
        &server.url,
        "",
        approve,
        "sign in to the MCP server",
    ) {
        return response.into_response();
    }
    let flow = match crate::mcp_oauth::prepare(&state, &name, server.url.trim(), port).await {
        Ok(flow) => flow,
        Err(error) => return crate::json_error(StatusCode::BAD_GATEWAY, error).into_response(),
    };
    crate::browser::open_url(&flow.url);
    match crate::mcp_oauth::wait_for_browser(&state, flow.receiver, port).await {
        Ok(renewed) => Json(json!({
            "ok": true,
            "name": name,
            "authorized": renewed.authorized(),
            "expires_at": renewed.expires_at,
            "has_refresh": !renewed.refresh_token.trim().is_empty(),
            "message": "Signed in. The connection carries your OAuth token now.",
        }))
        .into_response(),
        Err(error) => crate::json_error(StatusCode::BAD_GATEWAY, error).into_response(),
    }
}

/// Browser landing page for an OAuth redirect. It only redeems the query
/// against the armed flow — it holds no authority of its own.
async fn oauth_callback(Query(params): Query<HashMap<String, String>>) -> Response {
    let page = crate::mcp_oauth::browser_callback(
        params.get("code").map(String::as_str).unwrap_or(""),
        params.get("state").map(String::as_str).unwrap_or(""),
        params.get("error").map(String::as_str).unwrap_or(""),
    );
    let (status, body) = match page {
        Ok(html) => (StatusCode::OK, html.to_string()),
        Err(message) => (StatusCode::NOT_FOUND, format!("<div>{}</div>", message)),
    };
    Response::builder()
        .status(status)
        .header("content-type", "text/html; charset=utf-8")
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|error| {
            axum::http::Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::from(format!(
                    "callback page failed: {error}"
                )))
                .unwrap()
        })
        .into_response()
}

async fn oauth_forget(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    let exists = read_servers(&registry_path(&state))
        .into_iter()
        .any(|entry| entry.name == name);
    if !exists {
        return crate::json_error(StatusCode::NOT_FOUND, format!("Unknown MCP server: {name}"))
            .into_response();
    }
    crate::mcp_oauth::forget(&state, &name);
    crate::permissions::record(
        &state,
        PolicyAction::Network,
        &format!("mcp:{name}"),
        PolicyDecision::Deny,
        crate::permissions::Scope::Session,
        "",
        "OAuth session for the MCP server removed",
    );
    Json(json!({ "ok": true, "name": name, "message": "OAuth session removed." })).into_response()
}

async fn list_servers(State(state): State<AppState>) -> Response {
    let servers = read_servers(&registry_path(&state));
    Json(json!({
        "ok": true,
        "servers": servers.iter().map(server_json).collect::<Vec<_>>(),
        "path": registry_path(&state).to_string_lossy(),
    }))
    .into_response()
}

fn clean_args(input: Option<&Value>) -> Result<Vec<String>, String> {
    match input {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "Every argument must be a string.".to_string())
            })
            .collect(),
        Some(_) => Err("Arguments must be a list of strings.".to_string()),
    }
}

/// Environment handling for a save. An absent (or null) `env` keeps whatever
/// the server already had: the API never returns values, so a client that only
/// edits the command must not silently wipe the saved secrets.
fn merge_env(
    input: Option<&Value>,
    existing: Option<&Server>,
) -> Result<BTreeMap<String, String>, String> {
    match input {
        None | Some(Value::Null) => Ok(existing.map(|entry| entry.env.clone()).unwrap_or_default()),
        Some(Value::Object(values)) => {
            let mut env = BTreeMap::new();
            for (key, value) in values {
                let Some(value) = value.as_str() else {
                    return Err(format!("Environment value for {key} must be a string."));
                };
                if !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                {
                    return Err(format!(
                        "Environment name {key} must be letters, digits, underscore or dot."
                    ));
                }
                env.insert(key.clone(), value.to_string());
            }
            Ok(env)
        }
        Some(_) => Err("Environment must be an object of KEY: value pairs.".to_string()),
    }
}

/// Header values are secrets in exactly the way env values are: an absent
/// `headers` keeps what was saved, so editing a URL cannot silently wipe an
/// authorization token.
fn merge_headers(
    input: Option<&Value>,
    existing: Option<&Server>,
) -> Result<BTreeMap<String, String>, String> {
    match input {
        None | Some(Value::Null) => Ok(existing
            .map(|entry| entry.headers.clone())
            .unwrap_or_default()),
        Some(Value::Object(values)) => {
            let mut headers = BTreeMap::new();
            for (key, value) in values {
                let Some(value) = value.as_str() else {
                    return Err(format!("Header value for {key} must be a string."));
                };
                if key.is_empty()
                    || !key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
                {
                    return Err(format!(
                        "Header name {key} must be letters, digits, dash, underscore or dot."
                    ));
                }
                if value.contains('\r') || value.contains('\n') {
                    return Err(format!("Header value for {key} must be one line."));
                }
                headers.insert(key.clone(), value.to_string());
            }
            Ok(headers)
        }
        Some(_) => Err("Headers must be an object of NAME: value pairs.".to_string()),
    }
}

async fn save_server(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !valid_name(&name) {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "A server name is 1–64 characters: letters, digits, dot, dash or underscore.",
        )
        .into_response();
    }
    let transport = input
        .get("transport")
        .and_then(Value::as_str)
        .unwrap_or("stdio")
        .trim()
        .to_string();
    if !valid_transport(&transport) {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "Transport must be stdio, http or sse.",
        )
        .into_response();
    }
    let command = input
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if transport == "stdio" && command.is_empty() {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "A stdio server needs a command to run.",
        )
        .into_response();
    }
    let url = input
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if transport != "stdio" && !valid_url(&url) {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "A remote server needs an http:// or https:// URL.",
        )
        .into_response();
    }
    let args = match clean_args(input.get("args")) {
        Ok(args) => args,
        Err(error) => return crate::json_error(StatusCode::BAD_REQUEST, error).into_response(),
    };
    let path = registry_path(&state);
    let mut servers = read_servers(&path);
    let existing = servers.iter().find(|entry| entry.name == name).cloned();
    let env = match merge_env(input.get("env"), existing.as_ref()) {
        Ok(env) => env,
        Err(error) => return crate::json_error(StatusCode::BAD_REQUEST, error).into_response(),
    };
    let headers = match merge_headers(input.get("headers"), existing.as_ref()) {
        Ok(headers) => headers,
        Err(error) => return crate::json_error(StatusCode::BAD_REQUEST, error).into_response(),
    };
    let enabled = input
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let stdio = transport == "stdio";
    let server = Server {
        name: name.clone(),
        transport,
        command: if stdio { command } else { String::new() },
        args,
        env,
        url: if stdio { String::new() } else { url },
        headers,
        enabled,
    };
    match servers.iter_mut().find(|entry| entry.name == name) {
        Some(existing) => *existing = server.clone(),
        None => servers.push(server.clone()),
    }
    if let Err(error) = write_servers(&path, &servers) {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
    }
    // The saved command or URL is what a live session was opened with; drop it
    // so the next call starts from the entry that was just written.
    forget_pooled(&name);
    Json(json!({ "ok": true, "server": server_json(&server) })).into_response()
}

async fn remove_server(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let name = input.get("name").and_then(Value::as_str).unwrap_or("");
    let path = registry_path(&state);
    let mut servers = read_servers(&path);
    let before = servers.len();
    servers.retain(|server| server.name != name);
    let removed = before - servers.len();
    if let Err(error) = write_servers(&path, &servers) {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
    }
    forget_pooled(name);
    Json(json!({ "ok": true, "removed": removed })).into_response()
}

/// Every route that can start a child process or reach a remote server asks
/// the policy first. Testing a server *is* using it, so it answers to
/// `mcp_tool` on the server name — one rule can keep a server from ever being
/// contacted.
fn gate(state: &AppState, target: &str, approve: bool, detail: &str) -> Option<Response> {
    let verdict = permissions::check(state, PolicyAction::McpTool, target, "");
    match verdict.decision {
        PolicyDecision::Deny => {
            permissions::record(
                state,
                PolicyAction::McpTool,
                target,
                PolicyDecision::Deny,
                verdict.scope,
                "",
                detail,
            );
            Some(
                crate::json_error(
                    StatusCode::FORBIDDEN,
                    format!(
                        "A saved permission rule denies {target}. Change it in Settings → Permissions."
                    ),
                )
                .into_response(),
            )
        }
        PolicyDecision::Ask if !approve => {
            permissions::record(
                state,
                PolicyAction::McpTool,
                target,
                PolicyDecision::Ask,
                verdict.scope,
                "",
                detail,
            );
            Some(
                (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": format!("{target} asks for approval before it runs."),
                        "needs_approval": true,
                    })),
                )
                    .into_response(),
            )
        }
        _ => None,
    }
}

async fn test_server(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let name = input.get("name").and_then(Value::as_str).unwrap_or("");
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let servers = read_servers(&registry_path(&state));
    let Some(server) = servers.iter().find(|server| server.name == name).cloned() else {
        return crate::json_error(
            StatusCode::NOT_FOUND,
            format!("No MCP server named \"{name}\"."),
        )
        .into_response();
    };
    if let Some(response) = gate(&state, &server.name, approve, "MCP server test") {
        return response;
    }
    let handshake = match connect(&server).await {
        Ok(handshake) => handshake,
        Err(error) => {
            return crate::json_error(StatusCode::BAD_GATEWAY, error).into_response();
        }
    };
    let tools = tools_json(&handshake.tools);
    handshake.session.close().await;
    Json(json!({
        "ok": true,
        "server": server.name,
        "tools": tools,
        "protocol": handshake.protocol,
        "version": handshake.version,
    }))
    .into_response()
}

async fn call_tool(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let name = input.get("server").and_then(Value::as_str).unwrap_or("");
    let tool = input.get("tool").and_then(Value::as_str).unwrap_or("");
    let arguments = input.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if tool.trim().is_empty() {
        return crate::json_error(StatusCode::BAD_REQUEST, "Name a tool to call.").into_response();
    }
    let servers = read_servers(&registry_path(&state));
    let Some(server) = servers.iter().find(|server| server.name == name).cloned() else {
        return crate::json_error(
            StatusCode::NOT_FOUND,
            format!("No MCP server named \"{name}\"."),
        )
        .into_response();
    };
    if !server.enabled {
        return crate::json_error(
            StatusCode::CONFLICT,
            format!("MCP server \"{}\" is disabled.", server.name),
        )
        .into_response();
    }
    let target = tool_target(&server.name, tool);
    if let Some(response) = gate(&state, &target, approve, "MCP tool call") {
        return response;
    }
    match invoke(&state, &server.name, tool, &arguments).await {
        Ok(payload) => Json(payload).into_response(),
        Err(failure) => {
            crate::json_error(failure.status(), failure.message().to_string()).into_response()
        }
    }
}

/// The state as observed so far. Read-only: it never starts a process, so the
/// panel can poll it freely.
async fn health_report(State(state): State<AppState>) -> Response {
    Json(json!({ "ok": true, "servers": health_rows(&state) })).into_response()
}

/// Captured output for one server. A removed server keeps its last words until
/// the process exits — that is the log a user reads after something went wrong.
async fn server_logs(Query(params): Query<BTreeMap<String, String>>) -> Response {
    let name = params.get("name").cloned().unwrap_or_default();
    if name.is_empty() {
        return crate::json_error(StatusCode::BAD_REQUEST, "Name a server to read its log.")
            .into_response();
    }
    let all = monitor()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entries = all
        .get(&name)
        .map(|state| state.logs.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    Json(json!({ "ok": true, "name": name, "entries": entries })).into_response()
}

async fn probe(server: &Server) -> Value {
    match connect(server).await {
        Ok(handshake) => {
            handshake.session.close().await;
            json!({ "name": server.name, "ok": true })
        }
        Err(error) => json!({ "name": server.name, "ok": false, "error": error }),
    }
}

/// A real handshake per server, on request. The click that sends this is the
/// approval, exactly like Test: checking a server *is* starting it, so the
/// policy store is asked first and a saved `deny` skips that server instead of
/// failing the whole sweep.
async fn check_health(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let wanted = input.get("name").and_then(Value::as_str).unwrap_or("");
    let servers = if wanted.is_empty() {
        enabled_servers(&state)
    } else {
        read_servers(&registry_path(&state))
            .into_iter()
            .filter(|server| server.name == wanted && server.enabled)
            .collect()
    };
    if servers.is_empty() {
        return Json(json!({
            "ok": true,
            "checked": health_rows(&state),
            "results": [],
            "skipped": [],
            "message": if wanted.is_empty() {
                "No enabled MCP servers to check."
            } else {
                "No enabled MCP server by that name."
            },
        }))
        .into_response();
    }
    let mut skipped = Vec::new();
    let mut allowed = Vec::new();
    for server in servers {
        if let Some(response) = gate(&state, &server.name, approve, "MCP health check") {
            if response.status() == StatusCode::FORBIDDEN {
                skipped.push(json!({
                    "name": server.name,
                    "reason": "a saved permission rule denies it",
                }));
                continue;
            }
            return response;
        }
        allowed.push(server);
    }
    let results = join_all(allowed.iter().map(probe)).await;
    Json(json!({
        "ok": true,
        "checked": health_rows(&state),
        "results": results,
        "skipped": skipped,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(name: &str) -> Server {
        Server {
            name: name.to_string(),
            transport: "stdio".to_string(),
            command: "true".to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
            url: String::new(),
            headers: BTreeMap::new(),
            enabled: true,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("saarthi-mcp-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir.join("mcp.json")
    }

    #[test]
    fn a_message_is_exactly_one_line() {
        let line = encode(&json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list" }));
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        let parsed: Value = serde_json::from_str(line.trim()).expect("round trip");
        assert_eq!(parsed["id"], json!(7));
    }

    #[test]
    fn replies_are_matched_by_id_and_other_traffic_is_ignored() {
        let result = json!({ "jsonrpc": "2.0", "id": 4, "result": {"tools": []} });
        assert!(reply_for(&result.to_string(), 5).is_none());
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/progress" });
        assert!(reply_for(&notification.to_string(), 4).is_none());
        // A server that logs to stdout: not JSON, so not an answer.
        assert!(reply_for("starting up...", 4).is_none());
        match reply_for(&result.to_string(), 4) {
            Some(Ok(value)) => assert_eq!(value["tools"], json!([])),
            other => panic!("expected our result, got {other:?}"),
        }
    }

    #[test]
    fn a_jsonrpc_error_becomes_a_readable_message() {
        let failure = json!({
            "jsonrpc": "2.0", "id": 2, "error": {"code": -32601, "message": "no such tool"}
        });
        match reply_for(&failure.to_string(), 2) {
            Some(Err(message)) => assert!(message.contains("no such tool")),
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn tool_targets_are_namespaced_by_server() {
        assert_eq!(tool_target("files", "read_file"), "files.read_file");
        assert_eq!(tool_target("a", "b.c"), "a.b.c");
    }

    #[test]
    fn server_names_reject_anything_a_rule_cannot_address() {
        assert!(valid_name("files"));
        assert!(valid_name("my-server_2.local"));
        assert!(!valid_name(""));
        assert!(!valid_name("has space"));
        assert!(!valid_name("slash/name"));
        assert!(!valid_name(&"x".repeat(65)));
    }

    #[test]
    fn registry_round_trips_through_disk() {
        let path = scratch("roundtrip");
        let mut entry = server("files");
        entry.args = vec!["--root".to_string(), ".".to_string()];
        entry.env.insert("TOKEN".to_string(), "secret".to_string());
        entry.enabled = false;
        write_servers(&path, &[entry.clone()]).expect("write");
        assert_eq!(read_servers(&path), vec![entry]);
        let _ = fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn a_broken_registry_reads_as_empty() {
        let path = scratch("broken");
        fs::write(&path, b"{ not json").expect("write");
        assert!(read_servers(&path).is_empty());
        let _ = fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn env_values_never_reach_an_api_response() {
        let mut entry = server("files");
        entry
            .env
            .insert("API_TOKEN".to_string(), "super-secret".to_string());
        let rendered = server_json(&entry).to_string();
        assert!(rendered.contains("API_TOKEN"));
        assert!(!rendered.contains("super-secret"));
    }

    #[test]
    fn saving_without_env_keeps_the_saved_values() {
        let mut existing = server("files");
        existing
            .env
            .insert("TOKEN".to_string(), "secret".to_string());
        assert_eq!(merge_env(None, Some(&existing)), Ok(existing.env.clone()));
        assert_eq!(
            merge_env(Some(&Value::Null), Some(&existing)),
            Ok(existing.env.clone())
        );
        let replaced =
            merge_env(Some(&json!({ "LOG": "debug" })), Some(&existing)).expect("object");
        assert_eq!(replaced.len(), 1);
        assert!(replaced.contains_key("LOG"));
        assert!(merge_env(Some(&json!(["nope"])), None).is_err());
    }

    #[test]
    fn missing_required_arguments_are_named() {
        let schema = json!({
            "type": "object",
            "required": ["path"],
            "properties": { "path": { "type": "string" } }
        });
        assert_eq!(
            validate_arguments(&schema, &json!({})),
            Err("Missing required argument \"path\".".to_string())
        );
        assert_eq!(
            validate_arguments(&schema, &json!({ "path": 3 })),
            Err("Argument \"path\" must be string.".to_string())
        );
        assert!(validate_arguments(&schema, &json!({ "path": "src/main.rs" })).is_ok());
        // Extra arguments are the server's business, not this client's.
        assert!(validate_arguments(&schema, &json!({ "path": "a", "extra": true })).is_ok());
    }

    #[test]
    fn a_tool_without_required_fields_accepts_an_empty_object() {
        let schema = json!({ "type": "object", "properties": {} });
        assert!(validate_arguments(&schema, &json!({})).is_ok());
        assert!(validate_arguments(&schema, &json!([])).is_err());
    }

    #[test]
    fn tool_text_joins_text_parts_and_falls_back_to_the_raw_result() {
        let with_text = json!({
            "content": [
                { "type": "text", "text": "first" },
                { "type": "image", "data": "..." },
                { "type": "text", "text": "second" }
            ]
        });
        assert_eq!(tool_text(&with_text), "first\nsecond");
        let bare = json!({ "content": [] });
        assert!(tool_text(&bare).contains("content"));
    }

    #[test]
    fn discovery_keeps_schemas_and_caps_descriptions() {
        let tools = vec![json!({
            "name": "read_file",
            "description": "x".repeat(5_000),
            "inputSchema": { "type": "object", "required": ["path"] }
        })];
        let rendered = tools_json(&tools);
        assert_eq!(rendered[0]["name"], json!("read_file"));
        assert_eq!(
            rendered[0]["description"].as_str().unwrap().chars().count(),
            DESCRIPTION_CAP
        );
        assert_eq!(rendered[0]["input_schema"]["required"], json!(["path"]));
    }

    #[test]
    fn health_is_unknown_until_something_answers() {
        let rendered = report_json(&server("never-contacted"), &ServerState::default());
        assert_eq!(rendered["health"], json!("unknown"));
        assert_eq!(rendered["tools"], json!(0));
        assert_eq!(rendered["checked_at"], json!(0));
    }

    #[test]
    fn a_failed_check_keeps_the_reason_it_failed() {
        let name = "health-failed";
        record_health(name, false, "server closed its output".to_string(), 12);
        let all = monitor().lock().expect("monitor");
        let rendered = report_json(&server(name), all.get(name).expect("state"));
        assert_eq!(rendered["health"], json!("unhealthy"));
        assert_eq!(rendered["error"], json!("server closed its output"));
        assert_eq!(rendered["latency_ms"], json!(12));
    }

    #[test]
    fn a_handshake_records_the_version_it_was_given() {
        let name = "versioned";
        record_handshake(name, "2024-11-05", "0.4.1", 3);
        let all = monitor().lock().expect("monitor");
        let rendered = report_json(&server(name), all.get(name).expect("state"));
        assert_eq!(rendered["protocol"], json!("2024-11-05"));
        assert_eq!(rendered["version"], json!("0.4.1"));
        assert_eq!(rendered["tools"], json!(3));
    }

    #[test]
    fn logs_keep_only_the_newest_lines() {
        let name = "log-ring";
        for index in 0..(LOG_LINES + 25) {
            record_log(name, "server", format!("line {index}"));
        }
        let all = monitor().lock().expect("monitor");
        let state = all.get(name).expect("state");
        assert_eq!(state.logs.len(), LOG_LINES);
        assert_eq!(state.logs.front().expect("first").text, "line 25");
        assert_eq!(
            state.logs.back().expect("last").text,
            format!("line {}", LOG_LINES + 24)
        );
    }

    #[test]
    fn an_old_registry_entry_reads_as_a_stdio_server() {
        let parsed: Server = serde_json::from_str(
            r#"{"name":"files","command":"mcp-files","args":[],"enabled":true}"#,
        )
        .expect("old entry");
        assert_eq!(parsed.transport, "stdio");
        assert!(parsed.url.is_empty());
        assert!(parsed.headers.is_empty());
    }

    #[test]
    fn transport_names_are_a_closed_set() {
        assert!(valid_transport("stdio"));
        assert!(valid_transport("http"));
        assert!(valid_transport("sse"));
        assert!(!valid_transport("websocket"));
        assert!(!valid_transport(""));
    }

    #[test]
    fn remote_urls_must_be_http_and_free_of_whitespace() {
        assert!(valid_url("http://127.0.0.1:8821/mcp"));
        assert!(valid_url("https://example.com/mcp"));
        assert!(!valid_url("ftp://example.com/mcp"));
        assert!(!valid_url("example.com/mcp"));
        assert!(!valid_url("https://"));
        assert!(!valid_url("https://example.com/a b"));
        assert!(!valid_url(&format!(
            "https://example.com/{}",
            "x".repeat(2048)
        )));
    }

    #[test]
    fn sse_bodies_yield_their_json_payloads() {
        let body = "event: endpoint\ndata: /messages?sessionId=1\n\n\
                    event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n\
                    : keep-alive\n\n\
                    data: not json\n\n";
        let messages = sse_messages(body);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["id"], json!(1));
    }

    #[test]
    fn sse_data_lines_in_one_event_are_joined() {
        let body = "data: {\"jsonrpc\":\"2.0\",\ndata: \"id\":7,\"result\":[]}\n\n";
        let messages = sse_messages(body);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["id"], json!(7));
    }

    #[test]
    fn an_endpoint_is_resolved_against_the_stream_it_came_from() {
        assert_eq!(
            resolve_endpoint("http://127.0.0.1:8822/sse", "/messages?sessionId=9"),
            "http://127.0.0.1:8822/messages?sessionId=9"
        );
        assert_eq!(
            resolve_endpoint("https://example.com/mcp", "messages"),
            "https://example.com/messages"
        );
        assert_eq!(
            resolve_endpoint("https://example.com/mcp", "https://other.test/x"),
            "https://other.test/x"
        );
    }

    #[test]
    fn header_values_never_reach_an_api_response() {
        let mut entry = server("remote");
        entry.transport = "http".to_string();
        entry.url = "https://example.com/mcp".to_string();
        entry
            .headers
            .insert("Authorization".to_string(), "Bearer secret".to_string());
        let rendered = server_json(&entry).to_string();
        assert!(rendered.contains("Authorization"));
        assert!(!rendered.contains("Bearer secret"));
        assert!(rendered.contains("https://example.com/mcp"));
    }

    #[test]
    fn saving_without_headers_keeps_the_saved_values() {
        let mut existing = server("remote");
        existing
            .headers
            .insert("Authorization".to_string(), "Bearer secret".to_string());
        assert_eq!(
            merge_headers(None, Some(&existing)),
            Ok(existing.headers.clone())
        );
        let replaced =
            merge_headers(Some(&json!({ "X-Trace": "1" })), Some(&existing)).expect("object");
        assert_eq!(replaced.len(), 1);
        assert!(merge_headers(Some(&json!({ "Bad Name": "x" })), None).is_err());
        assert!(merge_headers(Some(&json!(["nope"])), None).is_err());
    }

    #[test]
    fn a_fingerprint_follows_the_settings_it_describes() {
        let base = server("fingerprint");
        let first = fingerprint(&base);
        assert_eq!(first, fingerprint(&base));
        let mut edited = base.clone();
        edited.transport = "http".to_string();
        edited.url = "https://example.com/mcp".to_string();
        assert_ne!(first, fingerprint(&edited));
        let mut new_token = base.clone();
        new_token
            .headers
            .insert("Authorization".to_string(), "Bearer other".to_string());
        assert_ne!(first, fingerprint(&new_token));
    }

    #[test]
    fn a_pooled_session_is_warm_only_inside_its_idle_window() {
        assert!(still_warm(Duration::ZERO));
        assert!(still_warm(POOL_IDLE - Duration::from_secs(1)));
        assert!(!still_warm(POOL_IDLE));
        assert!(!still_warm(POOL_IDLE + Duration::from_secs(60)));
    }

    #[test]
    fn forgetting_an_unpooled_server_is_a_no_op() {
        forget_pooled("never-pooled");
        forget_pooled("never-pooled");
    }

    #[test]
    fn a_long_log_line_is_capped_and_attributed() {
        let name = "log-cap";
        record_log(name, "server", "x".repeat(LOG_LINE_CAP * 3));
        let all = monitor().lock().expect("monitor");
        let line = all.get(name).expect("state").logs.front().expect("line");
        assert_eq!(line.text.chars().count(), LOG_LINE_CAP);
        assert_eq!(line.source, "server");
    }

    // -----------------------------------------------------------------------
    // Supervisor
    // -----------------------------------------------------------------------

    #[derive(Clone, Copy)]
    enum Probe {
        Answers,
        Silent,
        Broken,
    }

    /// A real streamable-HTTP MCP endpoint: handshake, an empty catalogue and a
    /// `ping` whose behaviour the caller chooses. The supervisor is exercised
    /// over a transport it will actually probe, not a stand-in for one.
    async fn mock_mcp(name: &str, probe: Probe) -> Server {
        let app = Router::new().route(
            "/",
            post(move |Json(body): Json<Value>| async move {
                let method = body.get("method").and_then(Value::as_str).unwrap_or("");
                let id = body.get("id").cloned().unwrap_or(Value::Null);
                match method {
                    "ping" => match probe {
                        Probe::Answers => Json(json!({ "jsonrpc": "2.0", "id": id, "result": {} }))
                            .into_response(),
                        Probe::Silent => {
                            tokio::time::sleep(Duration::from_secs(10)).await;
                            Json(json!({ "jsonrpc": "2.0", "id": id, "result": {} }))
                                .into_response()
                        }
                        Probe::Broken => {
                            (StatusCode::INTERNAL_SERVER_ERROR, "this endpoint is down")
                                .into_response()
                        }
                    },
                    "initialize" => Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": "2024-11-05",
                            "serverInfo": { "version": "9.9.9" }
                        }
                    }))
                    .into_response(),
                    "tools/list" => Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "tools": [] }
                    }))
                    .into_response(),
                    _ => StatusCode::ACCEPTED.into_response(),
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let mut entry = server(name);
        entry.transport = "http".to_string();
        entry.command = String::new();
        entry.url = format!("http://{addr}");
        entry
    }

    /// The pooled entry a finished call leaves behind, with an explicit stamp
    /// so the sweep sees a given amount of idle time without waiting for it.
    fn parked(entry: &Server, handshake: Handshake, last_used: Instant) -> PooledSession {
        PooledSession {
            fingerprint: fingerprint(entry),
            protocol: handshake.protocol,
            version: handshake.version,
            last_used,
            connection: handshake.session.connection.expect("live connection"),
        }
    }

    fn park(entry: &Server, handshake: Handshake, last_used: Instant) {
        pool()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(entry.name.clone(), parked(entry, handshake, last_used));
    }

    fn pooled_stamp(name: &str) -> Option<Instant> {
        pool()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(name)
            .map(|entry| entry.last_used)
    }

    fn last_log(name: &str) -> String {
        let all = monitor().lock().expect("monitor");
        all.get(name)
            .and_then(|state| state.logs.back())
            .map(|line| line.text.clone())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn the_supervisor_keeps_answering_sessions_and_evicts_wedged_ones() {
        // A 200 ms silence budget: the code path is the boot path, only the
        // clock is shortened so the test does not wait minutes.
        let within = Duration::from_millis(200);

        // 1. An idle session that still answers is handed back untouched,
        //    including its original stamp — a probe must not keep it alive.
        let alive = mock_mcp("supervisor-alive", Probe::Answers).await;
        let handshake = connect_once(&alive).await.expect("handshake");
        assert_eq!(handshake.version, "9.9.9");
        let stamped = Instant::now() - Duration::from_secs(120);
        park(&alive, handshake, stamped);
        supervisor_sweep(within).await;
        assert_eq!(pooled_stamp(&alive.name), Some(stamped), "kept its stamp");

        // 2. A session that stops answering while idle is closed on the timer,
        //    so the next call connects instead of paying the timeout to find out.
        let wedged = mock_mcp("supervisor-wedged", Probe::Silent).await;
        park(
            &wedged,
            connect_once(&wedged).await.expect("handshake"),
            Instant::now() - Duration::from_secs(60),
        );
        supervisor_sweep(within).await;
        assert_eq!(
            pooled_stamp(&wedged.name),
            None,
            "timed-out session evicted"
        );
        let logged = last_log(&wedged.name);
        assert!(logged.contains("stopped answering ping"), "{logged}");

        // 3. A transport answering with an error is gone, and the log says why
        //    rather than pretending the session was merely idle.
        let broken = mock_mcp("supervisor-broken", Probe::Broken).await;
        park(
            &broken,
            connect_once(&broken).await.expect("handshake"),
            Instant::now() - Duration::from_secs(60),
        );
        supervisor_sweep(within).await;
        assert_eq!(pooled_stamp(&broken.name), None, "broken session evicted");
        let logged = last_log(&broken.name);
        assert!(logged.contains("is gone"), "{logged}");
        assert!(logged.contains("500"), "{logged}");

        // A sweep with nothing parked is a no-op, not a panic.
        supervisor_sweep(within).await;
    }

    #[tokio::test]
    async fn a_probe_does_not_replace_a_session_that_reconnected_meanwhile() {
        let entry = mock_mcp("supervisor-race", Probe::Answers).await;
        let probed = parked(
            &entry,
            connect_once(&entry).await.expect("handshake"),
            Instant::now() - Duration::from_secs(120),
        );
        // A call reconnected while the probe was in flight and put a fresher
        // session back; the probe's older copy must lose.
        let fresh = Instant::now();
        park(
            &entry,
            connect_once(&entry).await.expect("handshake"),
            fresh,
        );
        restore_pooled(entry.name.clone(), probed);
        assert_eq!(pooled_stamp(&entry.name), Some(fresh));
        forget_pooled(&entry.name);
    }
}
