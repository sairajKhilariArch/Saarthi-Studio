//! MCP: a registry of local Model Context Protocol servers, a stdio JSON-RPC
//! session, tool discovery with schema validation, and every call bound to the
//! permission store.
//!
//! Scope is stated plainly rather than implied: a server is a local child
//! process spoken to over stdio, and one operation = one session. There is no
//! persistent connection pool, no SSE/HTTP transport and no sandbox beyond the
//! command the user wrote into the registry. Those gaps are recorded in
//! `docs/FEATURE-STATUS.md`, not faked here.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path as FsPath, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
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
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment for the child process. Values never leave the machine and
    /// never appear in an API response — only the names do.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
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
        "command": server.command,
        "args": server.args,
        "environment": server.env.keys().cloned().collect::<Vec<_>>(),
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

fn health_rows(state: &AppState) -> Vec<Value> {
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
fn reply_for(line: &str, id: u64) -> Option<Result<Value, String>> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
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

    async fn close(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

/// What a successful handshake taught us about the server.
struct Handshake {
    session: Session,
    tools: Vec<Value>,
    protocol: String,
    version: String,
}

async fn connect_once(server: &Server) -> Result<Handshake, String> {
    let mut session = Session::start(server)?;
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
    Ok(Handshake {
        session,
        tools,
        protocol,
        version,
    })
}

/// Handshake, then discover tools. Every caller gets a fresh session: a server
/// that wedged or exited is replaced by the next call instead of poisoning one
/// long-lived connection. A failed handshake is retried once on a brand-new
/// session — a server can exit while it is still coming up (a lock file, a
/// socket it has not bound yet), and nothing has been called at that point, so
/// the retry cannot duplicate an effect. A `tools/call` is never retried, which
/// is what keeps a side effect from running twice.
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
    let Some(server) = servers
        .into_iter()
        .find(|entry| entry.name == server_name)
    else {
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
    let target = tool_target(&server.name, tool);
    let handshake = connect(&server).await.map_err(CallFailure::Server)?;
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
        .route("/api/mcp/logs", get(server_logs));
    let mutations = Router::new()
        .route("/api/mcp/servers", post(save_server))
        .route("/api/mcp/servers/remove", post(remove_server))
        .route("/api/mcp/test", post(test_server))
        .route("/api/mcp/call", post(call_tool))
        .route("/api/mcp/health/check", post(check_health))
        .route_layer(axum::middleware::from_fn_with_state(
            crate::workspace::LocalOriginGuard::new(port),
            crate::workspace::guard_local_origin,
        ));
    read_only.merge(mutations)
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
    let command = input
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if command.is_empty() {
        return crate::json_error(StatusCode::BAD_REQUEST, "A server needs a command to run.")
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
    let enabled = input
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let server = Server {
        name: name.clone(),
        command,
        args,
        env,
        enabled,
    };
    match servers.iter_mut().find(|entry| entry.name == name) {
        Some(existing) => *existing = server.clone(),
        None => servers.push(server.clone()),
    }
    if let Err(error) = write_servers(&path, &servers) {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error).into_response();
    }
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
    Json(json!({ "ok": true, "removed": removed })).into_response()
}

/// Every route that can start a child process asks the policy first. Testing a
/// server *is* starting it, so it answers to `mcp_tool` on the server name —
/// one rule can keep a server from ever running.
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
            command: "true".to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
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
    fn a_long_log_line_is_capped_and_attributed() {
        let name = "log-cap";
        record_log(name, "server", "x".repeat(LOG_LINE_CAP * 3));
        let all = monitor().lock().expect("monitor");
        let line = all.get(name).expect("state").logs.front().expect("line");
        assert_eq!(line.text.chars().count(), LOG_LINE_CAP);
        assert_eq!(line.source, "server");
    }
}
