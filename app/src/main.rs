use axum::{
    body::Body,
    extract::{Path, Path as AxumPath, Query as AxumQuery, State},
    http::{header, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::{Client, Url};
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    env, fs, io,
    path::{Component, Path as FsPath, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::net::TcpListener;
use uuid::Uuid;

mod agent;
mod browser;
mod context;
mod deepsearch;
mod events;
#[cfg(feature = "gui")]
mod gui;
mod host_api;
mod host_auth;
mod host_security;
mod host_transport;
mod media;
mod router;
mod router_console;
mod skills;
mod terminal;
mod workspace;

const APP_NAME: &str = "Saarthi Studio";
const HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8765;
const SYSTEM_PROMPT: &str = "You are a concise, helpful AI coding assistant. Be accurate and safe; never claim to run code unless you did.";

#[derive(Clone)]
struct AppState {
    config_path: PathBuf,
    database_path: PathBuf,
    static_dir: PathBuf,
    workspace: workspace::WorkspaceStore,
    client: Client,
    terminal: terminal::Hub,
    events: events::Hub,
    /// One-minute /models cache keyed by "base|has-key".
    models_cache: Arc<Mutex<HashMap<String, (std::time::Instant, Value)>>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    base_url: String,
    model: String,
    #[serde(default)]
    api_key: String,
    /// Per-request token budget for the packed context.
    #[serde(default = "default_context_budget_tokens")]
    context_budget_tokens: usize,
    /// How many recent messages are considered for each request.
    #[serde(default = "default_history_messages")]
    history_messages: usize,
    /// Send a compact file-tree outline of the open folder with each request.
    #[serde(default = "default_outline_enabled")]
    send_workspace_outline: bool,
    /// Project shown in the UI and used to scope sessions.
    #[serde(default)]
    active_project: String,
    /// Skills the user switched off; everything discovered is on by default.
    #[serde(default)]
    disabled_skills: Vec<String>,
    /// Deep-search backend: "", "exa" or "tavily".
    #[serde(default)]
    search_provider: String,
    /// Search API key for the deep-search loop (server-side only).
    #[serde(default)]
    search_api_key: String,
}

fn default_context_budget_tokens() -> usize {
    context::DEFAULT_BUDGET_TOKENS
}
fn default_history_messages() -> usize {
    context::DEFAULT_HISTORY_MESSAGES
}
fn default_outline_enabled() -> bool {
    true
}

impl Config {
    fn defaults() -> Self {
        Self {
            base_url: env::var("FREE_AI_STUDIO_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:3001/v1".to_string()),
            model: "auto".to_string(),
            api_key: String::new(),
            context_budget_tokens: default_context_budget_tokens(),
            history_messages: default_history_messages(),
            send_workspace_outline: true,
            active_project: String::new(),
            disabled_skills: Vec::new(),
            search_provider: String::new(),
            search_api_key: String::new(),
        }
    }
}

pub(crate) fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn home_dir() -> PathBuf {
    // HOME on Linux/macOS, USERPROFILE on Windows.
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn paths() -> (PathBuf, PathBuf, PathBuf) {
    // One override keeps tests and sandboxed runs off the real user data:
    // both config and database live under this root, like a fake home.
    let home = env::var_os("FREE_AI_STUDIO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(home_dir);
    (
        home.join(".config/free-ai-studio/config.json"),
        home.join(".local/share/free-ai-studio/history.sqlite3"),
        resolve_static_dir(),
    )
}

/// Ordered candidate directories for the static UI. `FREE_AI_STUDIO_STATIC_DIR`
/// (when set) is authoritative; otherwise the installed system layout
/// (`<exe dir>/../share/free-ai-suite/static`, used by the .deb and AppImage)
/// is preferred, then a `static/` directory next to the executable (portable
/// tarball), and finally the source tree as a `cargo run` development
/// fallback. Keeping this runtime-resolved is what lets one binary work from
/// any of those layouts.
fn static_dir_candidates(exe_dir: Option<&FsPath>, env_dir: Option<PathBuf>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = env_dir {
        candidates.push(dir);
    }
    if let Some(dir) = exe_dir {
        candidates.push(dir.join("../share/free-ai-suite/static"));
        candidates.push(dir.join("static"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static"));
    candidates
}

fn resolve_static_dir() -> PathBuf {
    if let Some(dir) = env::var_os("FREE_AI_STUDIO_STATIC_DIR") {
        return PathBuf::from(dir);
    }
    let exe_dir = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from));
    static_dir_candidates(exe_dir.as_deref(), None)
        .into_iter()
        .find(|dir| dir.join("index.html").is_file())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static"))
}

fn set_private_permissions(path: &FsPath, dir: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if dir { 0o700 } else { 0o600 }),
        )?;
    }
    #[cfg(not(unix))]
    let _ = (path, dir);
    Ok(())
}

pub(crate) fn open_db(path: &FsPath) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 10000;")?;
    Ok(conn)
}

fn initialize_database(path: &FsPath) -> rusqlite::Result<()> {
    let conn = open_db(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, folder_path TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL, updated_at REAL NOT NULL, archived_at REAL
        );
        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at REAL NOT NULL,
            updated_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT, conversation_id TEXT NOT NULL,
            role TEXT NOT NULL, content TEXT NOT NULL, created_at REAL NOT NULL,
            FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS request_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT, created_at REAL NOT NULL,
            conversation_id TEXT, model TEXT, provider TEXT, status TEXT NOT NULL,
            latency_ms INTEGER, prompt_tokens INTEGER, completion_tokens INTEGER,
            total_tokens INTEGER, fallback_attempts TEXT, error TEXT
        );",
    )?;
    let _ = migrate_projects(&conn);
    if let Err(error) = agent::initialize(&conn) {
        println!("agent run tables: {error}");
    }
    migrate_message_attachments(&conn)
}

/// Give an existing messages table its `attachments` column (JSON of {kind,ref}
/// entries) so a fresh install and an upgraded install share one schema.
fn migrate_message_attachments(conn: &Connection) -> rusqlite::Result<()> {
    let has_column = {
        let mut stmt = conn.prepare("PRAGMA table_info(messages)")?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names.iter().any(|name| name == "attachments")
    };
    if !has_column {
        conn.execute_batch("ALTER TABLE messages ADD COLUMN attachments TEXT;")?;
    }
    Ok(())
}

/// Give a pre-projects database its `project_id` column, a first project, and
/// backfill every existing conversation into it, so nothing is orphaned by the
/// upgrade and switching projects never shows an empty history.
fn migrate_projects(conn: &Connection) -> rusqlite::Result<()> {
    let has_column = {
        let mut stmt = conn.prepare("PRAGMA table_info(conversations)")?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names.iter().any(|name| name == "project_id")
    };
    if !has_column {
        conn.execute_batch(
            "ALTER TABLE conversations ADD COLUMN project_id TEXT REFERENCES projects(id) ON DELETE CASCADE;",
        )?;
    }
    let now = now_seconds();
    let existing = conn
        .query_row(
            "SELECT id FROM projects ORDER BY created_at LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let fallback = match existing {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO projects(id,name,folder_path,created_at,updated_at) VALUES(?1,'Default','',?2,?2)",
                params![id, now],
            )?;
            id
        }
    };
    conn.execute(
        "UPDATE conversations SET project_id=?1 WHERE project_id IS NULL OR project_id=''",
        params![fallback],
    )?;
    Ok(())
}

/// The project new sessions belong to. Repairs an empty or stale pointer
/// (a deleted or archived project) instead of failing the request.
pub(crate) fn active_project_id(state: &AppState) -> String {
    let conn = match open_db(&state.database_path) {
        Ok(conn) => conn,
        Err(_) => return String::new(),
    };
    let mut cfg = load_config(state);
    let current = cfg.active_project.clone();
    let usable = conn
        .query_row(
            "SELECT id FROM projects WHERE id=?1 AND archived_at IS NULL",
            [&current],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten();
    if let Some(id) = usable {
        return id;
    }
    let replacement = conn
        .query_row(
            "SELECT id FROM projects WHERE archived_at IS NULL ORDER BY created_at LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten();
    let id = match replacement {
        Some(id) => id,
        None => {
            let id = Uuid::new_v4().to_string();
            let now = now_seconds();
            let _ = conn.execute(
                "INSERT INTO projects(id,name,folder_path,created_at,updated_at) VALUES(?1,'Default','',?2,?2)",
                params![id, now],
            );
            id
        }
    };
    cfg.active_project = id.clone();
    let _ = save_config_file(&state.config_path, &cfg);
    id
}

/// Remember which folder the active project works in, so switching projects
/// restores its files instead of only relabelling the sidebar.
fn record_project_folder(state: &AppState, folder: &str) {
    let project = active_project_id(state);
    if project.is_empty() {
        return;
    }
    let _ = open_db(&state.database_path).and_then(|conn| {
        conn.execute(
            "UPDATE projects SET folder_path=?1, updated_at=?2 WHERE id=?3",
            params![folder, now_seconds(), project],
        )
    });
}

fn ensure_setup(state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = state.config_path.parent() {
        fs::create_dir_all(parent)?;
        set_private_permissions(parent, true)?;
    }
    if let Some(parent) = state.database_path.parent() {
        fs::create_dir_all(parent)?;
        set_private_permissions(parent, true)?;
    }
    if !state.config_path.exists() {
        save_config_file(&state.config_path, &Config::defaults())?;
    }
    initialize_database(&state.database_path)?;
    // Warm the active-project pointer (also creates the first project).
    active_project_id(state);
    Ok(())
}

pub(crate) fn load_config(state: &AppState) -> Config {
    fs::read(&state.config_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Config>(&bytes).ok())
        .unwrap_or_else(Config::defaults)
}

fn save_config_file(path: &FsPath, cfg: &Config) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(cfg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, bytes)?;
    set_private_permissions(&temp_path, false)?;
    fs::rename(&temp_path, path)?;
    set_private_permissions(path, false)
}

pub(crate) fn json_error(status: StatusCode, message: impl Into<String>) -> Response<Body> {
    (status, Json(json!({"error": message.into()}))).into_response()
}

/// Baseline browser hardening for the local UI. The app has no third-party
/// assets, so the policy can stay tight; inline styles are allowed because the
/// UI uses `element.style` for small dynamic layout details.
async fn local_security_headers(request: Request<Body>, next: Next) -> Response<Body> {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    if embed_framing_allowed() {
        // No X-Frame-Options for an embedded launch: a webview origin is a
        // different origin, so frame-ancestors has to carry the allow-list.
        headers.insert(
            "content-security-policy",
            HeaderValue::from_static(
                "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; \
                 script-src 'self'; connect-src 'self'; base-uri 'none'; \
                 frame-ancestors 'self' vscode-webview:",
            ),
        );
    } else {
        headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
        headers.insert(
            "content-security-policy",
            HeaderValue::from_static(
                "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; \
                 script-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'",
            ),
        );
    }
    response
}

fn clean_title(value: &str) -> String {
    let title = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = title.chars().take(64).collect::<String>();
    if title.is_empty() {
        "New conversation".to_string()
    } else {
        title
    }
}

pub(crate) fn router_url(cfg: &Config, suffix: &str) -> String {
    format!(
        "{}/{}",
        cfg.base_url.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

fn validate_config(mut cfg: Config) -> Result<Config, String> {
    cfg.base_url = cfg.base_url.trim().trim_end_matches('/').to_string();
    cfg.model = if cfg.model.trim().is_empty() {
        "auto".to_string()
    } else {
        cfg.model.trim().to_string()
    };
    cfg.api_key = cfg.api_key.trim().to_string();
    cfg.search_provider = match cfg.search_provider.to_lowercase().as_str() {
        "exa" | "tavily" => cfg.search_provider.to_lowercase(),
        _ => String::new(),
    };

    cfg.context_budget_tokens = cfg
        .context_budget_tokens
        .clamp(context::MIN_BUDGET_TOKENS, context::MAX_BUDGET_TOKENS);
    cfg.history_messages = cfg
        .history_messages
        .clamp(context::MIN_HISTORY_MESSAGES, context::MAX_HISTORY_MESSAGES);

    let parsed = Url::parse(&cfg.base_url)
        .map_err(|_| "Router URL must start with http:// or https://.".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("Router URL must start with http:// or https://.".into());
    }
    let host = parsed.host_str().unwrap_or_default();
    let is_local = matches!(host, "localhost" | "127.0.0.1" | "::1");
    if parsed.scheme() != "https" && !is_local {
        return Err(
            "For a non-local router, use HTTPS so your API key is encrypted in transit.".into(),
        );
    }
    Ok(cfg)
}

/// When the only thing wrong is that a self-hosted gateway is not running,
/// say exactly how to start it instead of a bare "could not reach" line.
pub(crate) fn reach_error(base_url: &str, detail: String) -> String {
    let prefix = "Could not reach router at ".to_string();
    let mut message = format!("{prefix}{base_url}: {detail}");
    if base_url.contains("localhost:20128") {
        message += "\n\nOmniRoute (port 20128) is not running. Start it in the Integrations view, or install it once with `npm i -g omniroute`.";
    } else if base_url.contains("localhost:3001") {
        message += "\n\nFreeLLMAPI (port 3001) is not running. Start the Docker Compose router (`cd ~/freellmapi && sudo docker compose up -d`), then try again.";
    }
    message
}

fn response_content(data: &Value) -> String {
    let content = match data.pointer("/choices/0/message/content") {
        Some(value) => value,
        None => return serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string()),
    };
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(parts) = content.as_array() {
        let joined = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        if !joined.is_empty() {
            return joined;
        }
    }
    serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string())
}

pub(crate) async fn call_router(state: &AppState, messages: Vec<Value>, model_override: Option<&str>) -> Result<(String, Value), String> {
    let cfg = load_config(state);
    if cfg.api_key.trim().is_empty() {
        return Err("Add your router's unified API key in Settings first.".into());
    }

    let chosen = model_override
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(if cfg.model.trim().is_empty() { "auto" } else { cfg.model.as_str() });
    let request_body = json!({
        "model": chosen,
        "messages": messages,
        "stream": false
    });
    let started = Instant::now();
    let response = state
        .client
        .post(router_url(&cfg, "chat/completions"))
        .bearer_auth(&cfg.api_key)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| reach_error(&cfg.base_url, e.to_string()))?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Could not read router response: {e}"))?;
    let latency = started.elapsed().as_millis() as i64;

    let parsed: Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| json!({"message": body.chars().take(3000).collect::<String>()}));
    if !status.is_success() {
        let detail = parsed
            .pointer("/error/message")
            .or_else(|| parsed.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| body.chars().take(3000).collect());
        return Err(format!(
            "Router returned HTTP {}: {}",
            status.as_u16(),
            detail
        ));
    }

    let answer = response_content(&parsed);
    let usage = parsed.get("usage").cloned().unwrap_or_else(|| json!({}));
    let provider = headers
        .get("x-routed-via")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("router / provider not reported");
    let fallback_attempts = headers
        .get("x-fallback-attempts")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let model = parsed
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(&cfg.model);
    let meta = json!({
        "latency_ms": latency,
        "provider": provider,
        "fallback_attempts": fallback_attempts,
        "prompt_tokens": usage.get("prompt_tokens").and_then(Value::as_i64),
        "completion_tokens": usage.get("completion_tokens").and_then(Value::as_i64),
        "total_tokens": usage.get("total_tokens").and_then(Value::as_i64),
        "model": model
    });
    Ok((answer, meta))
}

async fn root(State(state): State<AppState>) -> Response<Body> {
    serve_static(state, "index.html").await
}

async fn static_file(State(state): State<AppState>, Path(file): Path<String>) -> Response<Body> {
    serve_static(state, &file).await
}

async fn serve_static(state: AppState, relative: &str) -> Response<Body> {
    let path = FsPath::new(relative);
    if path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return json_error(StatusCode::BAD_REQUEST, "Invalid path.");
    }
    let full = state.static_dir.join(path);
    let bytes = match fs::read(&full) {
        Ok(bytes) => bytes,
        Err(_) => return json_error(StatusCode::NOT_FOUND, "Not found."),
    };
    let mime = match full.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        _ => "application/octet-stream",
    };
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn get_config(State(state): State<AppState>) -> Json<Value> {
    let cfg = load_config(&state);
    Json(json!({
        "base_url": cfg.base_url,
        "model": cfg.model,
        "has_api_key": !cfg.api_key.is_empty(),
        "context_budget_tokens": cfg.context_budget_tokens,
        "history_messages": cfg.history_messages,
        "send_workspace_outline": cfg.send_workspace_outline,
        "search_provider": cfg.search_provider,
        "has_search_key": !cfg.search_api_key.is_empty(),
    }))
}

async fn router_status(State(_state): State<AppState>) -> Json<Value> {
    Json(router::status().await.to_json())
}

async fn router_start(State(_state): State<AppState>) -> Response<Body> {
    match router::manual_start().await {
        Ok(status) => Json(status.to_json()).into_response(),
        Err(error) => json_error(StatusCode::BAD_GATEWAY, error),
    }
}

async fn router_stop(State(_state): State<AppState>) -> Response<Body> {
    match router::manual_stop().await {
        Ok(status) => Json(status.to_json()).into_response(),
        Err(error) => json_error(StatusCode::BAD_GATEWAY, error),
    }
}

async fn omniroute_status(State(_state): State<AppState>) -> Json<Value> {
    Json(router::omniroute_status().await.to_json())
}

async fn omniroute_start(State(_state): State<AppState>) -> Response<Body> {
    match router::omniroute_start().await {
        Ok(status) => Json(status.to_json()).into_response(),
        Err(error) => json_error(StatusCode::BAD_GATEWAY, error),
    }
}

async fn omniroute_stop(State(_state): State<AppState>) -> Response<Body> {
    match router::omniroute_stop().await {
        Ok(status) => Json(status.to_json()).into_response(),
        Err(error) => json_error(StatusCode::BAD_GATEWAY, error),
    }
}

async fn post_config(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    let mut cfg = load_config(&state);
    let base_url = input
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or(&cfg.base_url)
        .to_string();
    let model = input
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(&cfg.model)
        .to_string();
    if let Some(key) = input.get("api_key").and_then(Value::as_str) {
        if !key.trim().is_empty() {
            cfg.api_key = key.trim().to_string();
        }
    }
    if let Some(budget) = input.get("context_budget_tokens").and_then(Value::as_u64) {
        cfg.context_budget_tokens = budget as usize;
    }
    if let Some(history) = input.get("history_messages").and_then(Value::as_u64) {
        cfg.history_messages = history as usize;
    }
    if let Some(outline) = input.get("send_workspace_outline").and_then(Value::as_bool) {
        cfg.send_workspace_outline = outline;
    }
    if let Some(provider) = input.get("search_provider").and_then(Value::as_str) {
        cfg.search_provider = provider.trim().to_lowercase();
    }
    if let Some(key) = input.get("search_api_key").and_then(Value::as_str) {
        if !key.trim().is_empty() {
            cfg.search_api_key = key.trim().to_string();
        }
    }
    cfg.base_url = base_url;
    cfg.model = model;
    let cfg = match validate_config(cfg) {
        Ok(c) => c,
        Err(e) => return json_error(StatusCode::BAD_REQUEST, e),
    };
    if let Err(e) = save_config_file(&state.config_path, &cfg) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    Json(json!({"ok": true, "base_url": cfg.base_url, "model": cfg.model, "has_api_key": !cfg.api_key.is_empty(), "context_budget_tokens": cfg.context_budget_tokens, "history_messages": cfg.history_messages, "send_workspace_outline": cfg.send_workspace_outline})).into_response()
}

/* Model catalog: fetched live from the configured router's /models so nothing
   is mirrored by hand, cached for a minute (cheap rate limits). Each entry
   carries any context-window metadata the router publishes, which powers the
   composer's context meter. */
async fn models(
    State(state): State<AppState>,
    AxumQuery(params): AxumQuery<HashMap<String, String>>,
) -> Response<Body> {
    let cfg = load_config(&state);
    let cache_key = format!("{}|{}", cfg.base_url.trim_end_matches('/'), !cfg.api_key.is_empty());
    let refresh = params.contains_key("refresh");
    if !refresh {
        if let Some((at, cached)) = state.models_cache.lock().unwrap().get(&cache_key) {
            if at.elapsed() < Duration::from_secs(60) {
                return (
                    StatusCode::OK,
                    [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
                    Json(json!({ "ok": true, "cached": true, "models": cached })),
                )
                    .into_response();
            }
        }
    }
    let catalog = fetch_models(&state, &cfg).await;
    let (ok, error, models) = match catalog {
        Ok(list) => (true, serde_json::Value::Null, json!(list)),
        Err(e) => (false, json!(e), json!([])),
    };
    if ok {
        state.models_cache.lock().unwrap().insert(
            cache_key,
            (std::time::Instant::now(), models.clone()),
        );
    }
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        Json(json!({ "ok": ok, "error": error, "models": models })),
    )
        .into_response()
}

/// One walk of the router's /models. `data[]` is the OpenAI shape; context
/// limits ride under whichever key the router uses.
async fn fetch_models(state: &AppState, cfg: &Config) -> Result<Vec<Value>, String> {
    let url = router_url(cfg, "models");
    let mut request = state.client.get(&url);
    if !cfg.api_key.trim().is_empty() {
        request = request.bearer_auth(cfg.api_key.trim());
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Router not reachable ({url}): {e}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Could not read the router response: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "Router returned HTTP {} for /models. {}",
            status.as_u16(),
            if cfg.api_key.trim().is_empty() {
                "Saving an API key may unlock the catalog."
            } else {
                "The key may lack the models.list permission."
            }
        ));
    }
    let parsed: Value = serde_json::from_str(&body)
        .map_err(|_| "Router answered /models with something that is not JSON.".to_string())?;
    let rows = parsed
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| parsed.as_array().cloned())
        .unwrap_or_default();
    let mut models = Vec::new();
    for row in rows {
        let Some(id) = row.get("id").and_then(Value::as_str) else { continue };
        let context_window = ["context_length", "context_window", "max_input_tokens", "top_provider::context_length"]
            .iter()
            .find_map(|key| row.get(key).and_then(Value::as_i64))
            .or_else(|| {
                // OpenRouter nests limits under top_provider.
                row.pointer("/top_provider/context_length")
                    .and_then(Value::as_i64)
            })
            .unwrap_or(0);
        models.push(json!({
            "id": id,
            "owner": row.get("owned_by").and_then(Value::as_str).unwrap_or(""),
            "context_window": context_window,
        }));
    }
    models.sort_by(|a, b| {
        a.get("id").and_then(Value::as_str).unwrap_or("")
            .cmp(b.get("id").and_then(Value::as_str).unwrap_or(""))
    });
    Ok(models)
}

/* Workflows: saved prompt recipes. They live in a small JSON file next to
   the config — no schema migration, and they are machine-global by nature. */
#[derive(Clone, Serialize, Deserialize)]
struct Workflow {
    id: String,
    name: String,
    prompt: String,
    #[serde(default)]
    skill: String,
}

fn workflows_path(state: &AppState) -> PathBuf {
    state.config_path.with_file_name("workflows.json")
}

fn load_workflows(state: &AppState) -> Vec<Workflow> {
    fs::read(workflows_path(state))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Workflow>>(&bytes).ok())
        .unwrap_or_default()
        // Drop malformed entries instead of failing the whole list.
        .into_iter()
        .filter(|w| !w.id.is_empty() && !w.name.is_empty())
        .collect()
}

fn save_workflows(state: &AppState, workflows: &[Workflow]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(workflows).map_err(|e| e.to_string())?;
    let temp = workflows_path(state).with_extension("tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    set_private_permissions(&temp, false).map_err(|e| e.to_string())?;
    fs::rename(&temp, workflows_path(state)).map_err(|e| e.to_string())
}

async fn list_workflows(State(state): State<AppState>) -> Response<Body> {
    Json(json!({ "ok": true, "workflows": load_workflows(&state) })).into_response()
}

fn clipped_string(value: Option<&str>, max: usize) -> String {
    value.unwrap_or("").chars().take(max).collect::<String>()
}

async fn save_workflow(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    let name = clean_title(input.get("name").and_then(Value::as_str).unwrap_or(""));
    let prompt = clipped_string(input.get("prompt").and_then(Value::as_str), 4000);
    let skill = clipped_string(input.get("skill").and_then(Value::as_str), 160);
    if name.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "A workflow needs a name.");
    }
    if prompt.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "A workflow needs a prompt.");
    }

    let mut workflows = load_workflows(&state);
    // Existing id means update; anything else is a new recipe.
    let requested_id = input.get("id").and_then(Value::as_str).unwrap_or("");
    let known = workflows
        .iter()
        .any(|w| !requested_id.is_empty() && w.id == requested_id);
    let id = if known {
        requested_id.to_string()
    } else {
        Uuid::new_v4().to_string()
    };
    workflows.retain(|w| w.id != id);
    workflows.push(Workflow {
        id: id.clone(),
        name,
        prompt,
        skill,
    });
    workflows.sort_by_key(|workfluff| workfluff.name.to_lowercase());
    if let Err(e) = save_workflows(&state, &workflows) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "id": id })).into_response()
}

async fn delete_workflow(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response<Body> {    let mut workflows = load_workflows(&state);
    let before = workflows.len();
    workflows.retain(|w| w.id != id);
    if workflows.len() == before {
        return json_error(StatusCode::NOT_FOUND, "No such workflow.");
    }
    if let Err(e) = save_workflows(&state, &workflows) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true })).into_response()
}

async fn list_projects(State(state): State<AppState>) -> Response<Body> {
    let active = active_project_id(&state);
    let result = (|| -> rusqlite::Result<Vec<Value>> {
        let conn = open_db(&state.database_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, name, folder_path, created_at, updated_at, archived_at FROM projects
             ORDER BY (archived_at IS NOT NULL), created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "folder": row.get::<_, String>(2)?,
                "created_at": row.get::<_, f64>(3)?,
                "updated_at": row.get::<_, f64>(4)?,
                "archived": row.get::<_, Option<f64>>(5)?.is_some(),
            }))
        })?;
        rows.collect()
    })();
    match result {
        Ok(projects) => Json(json!({ "ok": true, "active": active, "projects": projects })).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn project_name(input: &Value) -> String {
    clean_title(input.get("name").and_then(Value::as_str).unwrap_or(""))
}

async fn create_project(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    let name = project_name(&input);
    if name.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "A project needs a name.");
    }
    let folder = input
        .get("folder")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(4000)
        .collect::<String>();
    let id = Uuid::new_v4().to_string();
    let now = now_seconds();
    match open_db(&state.database_path).and_then(|conn| {
        conn.execute(
            "INSERT INTO projects(id,name,folder_path,created_at,updated_at) VALUES(?1,?2,?3,?4,?4)",
            params![id, name, folder, now],
        )
    }) {
        Ok(_) => (
            StatusCode::CREATED,
            Json(json!({ "ok": true, "id": id, "name": name })),
        )
            .into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let conn = match open_db(&state.database_path) {
        Ok(conn) => conn,
        Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let exists = conn
        .query_row("SELECT id FROM projects WHERE id=?1", [&id], |r| {
            r.get::<_, String>(0)
        })
        .optional();
    match exists {
        Ok(Some(_)) => {}
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "No such project."),
        Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }

    if let Some(name) = input.get("name").and_then(Value::as_str) {
        let name = clean_title(name);
        if !name.is_empty() {
            if let Err(e) = conn.execute(
                "UPDATE projects SET name=?1, updated_at=?2 WHERE id=?3",
                params![name, now_seconds(), id],
            ) {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
            }
        }
    }
    if let Some(folder) = input.get("folder").and_then(Value::as_str) {
        let folder: String = folder.chars().take(4000).collect();
        if let Err(e) = conn.execute(
            "UPDATE projects SET folder_path=?1, updated_at=?2 WHERE id=?3",
            params![folder, now_seconds(), id],
        ) {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        }
    }
    if let Some(archived) = input.get("archived").and_then(Value::as_bool) {
        let stamp: Option<f64> = if archived { Some(now_seconds()) } else { None };
        if let Err(e) = conn.execute(
            "UPDATE projects SET archived_at=?1, updated_at=?2 WHERE id=?3",
            params![stamp, now_seconds(), id],
        ) {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        }
        // Never leave the UI pointed at an archived project.
        if archived {
            let mut cfg = load_config(&state);
            if cfg.active_project == id {
                cfg.active_project.clear();
                let _ = save_config_file(&state.config_path, &cfg);
                active_project_id(&state);
            }
        }
    }
    Json(json!({ "ok": true, "id": id })).into_response()
}

async fn delete_project(State(state): State<AppState>, Path(id): Path<String>) -> Response<Body> {
    // Conversations cascade (FK ON DELETE CASCADE), so threads and messages
    // leave with the project instead of reappearing under another one.
    match open_db(&state.database_path).and_then(|conn| {
        conn.execute("DELETE FROM projects WHERE id=?1", [&id])
    }) {
        Ok(0) => json_error(StatusCode::NOT_FOUND, "No such project."),
        Ok(_) => {
            let mut cfg = load_config(&state);
            if cfg.active_project == id {
                cfg.active_project.clear();
                let _ = save_config_file(&state.config_path, &cfg);
                active_project_id(&state);
            }
            Json(json!({ "ok": true, "active": active_project_id(&state) })).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Point the UI at a project and open the folder it remembers. A folder that
/// no longer exists does not fail the switch; the project still becomes
/// active and the file pane simply reports nothing open.
async fn activate_project(State(state): State<AppState>, Path(id): Path<String>) -> Response<Body> {
    let stored = open_db(&state.database_path).and_then(|conn| {
        conn.query_row(
            "SELECT name, folder_path FROM projects WHERE id=?1",
            [&id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
    });
    let (name, folder) = match stored {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return json_error(StatusCode::NOT_FOUND, "No such project.")
        }
        Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let mut cfg = load_config(&state);
    cfg.active_project = id.clone();
    if let Err(e) = save_config_file(&state.config_path, &cfg) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }

    let workspace = if folder.trim().is_empty() {
        state.workspace.close();
        json!({ "open": false })
    } else {
        match state.workspace.open(&folder) {
            Ok(opened) => json!({
                "open": true,
                "name": opened.name,
                "path": crate::workspace::display_path(&opened.root),
            }),
            Err(error) => json!({ "open": false, "message": error.message() }),
        }
    };
    Json(json!({ "ok": true, "id": id, "name": name, "workspace": workspace })).into_response()
}

async fn list_conversations(
    AxumQuery(params): AxumQuery<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Response<Body> {
    // `scope=all` returns recent threads across every project, each labelled
    // with its project name, so the sidebar can show threads in context.
    // The default keeps the long-standing project-filtered behaviour.
    let all_scope = params.get("scope").map(String::as_str) == Some("all");
    let result = (|| -> rusqlite::Result<Vec<Value>> {
        let conn = open_db(&state.database_path)?;
        let sql = if all_scope {
            "SELECT v.id, v.title, v.created_at, v.updated_at,
             (SELECT content FROM messages m WHERE m.conversation_id=v.id ORDER BY id DESC LIMIT 1) AS last_message,
             COALESCE(p.name, 'No project') AS project
             FROM conversations v LEFT JOIN projects p ON p.id=v.project_id
             ORDER BY v.updated_at DESC LIMIT 100"
        } else {
            "SELECT v.id, v.title, v.created_at, v.updated_at,
             (SELECT content FROM messages m WHERE m.conversation_id=v.id ORDER BY id DESC LIMIT 1) AS last_message,
             (SELECT COALESCE(name, 'No project') FROM projects p WHERE p.id=v.project_id) AS project
             FROM conversations v WHERE v.project_id=?1 ORDER BY v.updated_at DESC"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = if all_scope {
            let rows = stmt.query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?, "title": row.get::<_, String>(1)?,
                    "created_at": row.get::<_, f64>(2)?, "updated_at": row.get::<_, f64>(3)?,
                    "last_message": row.get::<_, Option<String>>(4)?,
                    "project": row.get::<_, String>(5)?
                }))
            })?;
            rows.collect::<rusqlite::Result<Vec<Value>>>()
        } else {
            let project = active_project_id(&state);
            let rows = stmt.query_map([project], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?, "title": row.get::<_, String>(1)?,
                "created_at": row.get::<_, f64>(2)?, "updated_at": row.get::<_, f64>(3)?,
                "last_message": row.get::<_, Option<String>>(4)?
            }))
            })?;
            rows.collect::<rusqlite::Result<Vec<Value>>>()
        };
        Ok(rows?)
    })();
    match result {
        Ok(items) => Json(Value::Array(items)).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_messages(State(state): State<AppState>, Path(id): Path<String>) -> Response<Body> {
    let result = (|| -> rusqlite::Result<Option<Vec<Value>>> {
        let conn = open_db(&state.database_path)?;
        let exists = conn
            .query_row("SELECT id FROM conversations WHERE id=?1", [&id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        if exists.is_none() {
            return Ok(None);
        }
        let mut stmt = conn.prepare("SELECT id, role, content, created_at, attachments FROM messages WHERE conversation_id=?1 ORDER BY id")?;
        let rows = stmt.query_map([id], |row| {
            let attachments: Option<String> = row.get(4)?;
            let attachments = attachments
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                .unwrap_or(Value::Null);
            Ok(json!({
                "id": row.get::<_, i64>(0)?, "role": row.get::<_, String>(1)?,
                "content": row.get::<_, String>(2)?, "created_at": row.get::<_, f64>(3)?,
                "attachments": attachments
            }))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map(Some)
    })();
    match result {
        Ok(Some(items)) => Json(Value::Array(items)).into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "Conversation not found."),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn create_conversation(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let id = Uuid::new_v4().to_string();
    let title = clean_title(
        input
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("New conversation"),
    );
    let now = now_seconds();
    let project = active_project_id(&state);
    match open_db(&state.database_path).and_then(|conn| {
        conn.execute(
            "INSERT INTO conversations(id,title,project_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?4)",
            params![id, title, project, now],
        )
    }) {
        Ok(_) => {
            events::notify(&state.events, &id, "created");
            (StatusCode::CREATED, Json(json!({"id": id, "title": title}))).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn delete_conversation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    match open_db(&state.database_path)
        .and_then(|conn| conn.execute("DELETE FROM conversations WHERE id=?1", [id.as_str()]))
    {
        Ok(count) => {
            if count > 0 {
                events::notify(&state.events, &id, "deleted");
            }
            Json(json!({"ok": true})).into_response()
        }
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn usage(
    AxumQuery(params): AxumQuery<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Response<Body> {
    // Server-side paging, sorting and filtering so the Usage view can page
    // through the full log; the WHERE is shared by every aggregate below.
    let page_size = params
        .get("page_size")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(10)
        .clamp(5, 200);
    let page = params
        .get("page")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(1)
        .max(1);
    let sort_column = match params.get("sort").map(String::as_str) {
        Some("latency") => "COALESCE(latency_ms, -1)",
        Some("tokens") => "COALESCE(total_tokens, -1)",
        Some("status") => "status",
        Some("detail") => "COALESCE(error, '')",
        Some("model") => "COALESCE(model, '')",
        Some("provider") => "COALESCE(provider, '')",
        _ => "created_at",
    };
    let desc = params
        .get("dir")
        .map(String::as_str)
        .map(|v| v != "asc")
        .unwrap_or(true);

    let mut where_sql = String::from(" WHERE 1=1");
    let mut args: Vec<String> = Vec::new();
    for (column, key) in [("model", "model"), ("provider", "provider")] {
        let value = params.get(key).map(|v| v.trim()).filter(|v| !v.is_empty());
        if let Some(value) = value {
            where_sql.push_str(&format!(" AND COALESCE({column},'') LIKE ?"));
            args.push(format!("%{value}%"));
        }
    }
    for (clause, key) in [(" AND created_at >= ?", "from"), (" AND created_at <= ?", "to")] {
        let value = params
            .get(key)
            .and_then(|v| v.parse::<f64>().ok());
        if let Some(value) = value {
            where_sql.push_str(clause);
            args.push(value.to_string());
        }
    }
    let order = format!(" ORDER BY {sort_column} {}, id {}", if desc { "DESC" } else { "ASC" }, if desc { "DESC" } else { "ASC" });

    let result = (|| -> rusqlite::Result<Value> {
        let conn = open_db(&state.database_path)?;
        let summary_sql = format!(
            "SELECT COUNT(*) AS requests,\
             SUM(CASE WHEN status='ok' THEN 1 ELSE 0 END) AS successful,\
             SUM(CASE WHEN status!='ok' THEN 1 ELSE 0 END) AS failed,\
             COALESCE(SUM(total_tokens),0) AS total_tokens,\
             COALESCE(AVG(latency_ms),0) AS avg_latency_ms FROM request_log{where_sql}",
        );
        let summary = conn
            .prepare(&summary_sql)?
            .query_row(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(json!({
                    "requests": r.get::<_, i64>(0)?,
                    "successful": r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    "failed": r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    "total_tokens": r.get::<_, i64>(3)?,
                    "avg_latency_ms": r.get::<_, f64>(4)?
                }))
            })?;
        let total: i64 = summary.get("requests").and_then(Value::as_i64).unwrap_or(0);
        let pages = ((total + page_size - 1) / page_size).max(1);

        let sql = format!(
            "SELECT id, created_at, conversation_id, model, provider, status, latency_ms, prompt_tokens, completion_tokens, total_tokens, fallback_attempts, error \
             FROM request_log{where_sql}{order} LIMIT ? OFFSET ?",
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut row_args = args.clone();
        row_args.push((page_size).to_string());
        row_args.push(((page - 1) * page_size).to_string());
        let requests = stmt
            .query_map(
                rusqlite::params_from_iter(row_args.iter()),
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?, "created_at": r.get::<_, f64>(1)?,
                        "conversation_id": r.get::<_, Option<String>>(2)?, "model": r.get::<_, Option<String>>(3)?,
                        "provider": r.get::<_, Option<String>>(4)?, "status": r.get::<_, String>(5)?,
                        "latency_ms": r.get::<_, Option<i64>>(6)?, "prompt_tokens": r.get::<_, Option<i64>>(7)?,
                        "completion_tokens": r.get::<_, Option<i64>>(8)?, "total_tokens": r.get::<_, Option<i64>>(9)?,
                        "fallback_attempts": r.get::<_, Option<String>>(10)?, "error": r.get::<_, Option<String>>(11)?
                    }))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Daily buckets (requests / failures / tokens / latency) from the
        // filtered set, oldest first — the chart's whole data source.
        let series_sql = format!(
            "SELECT CAST(CAST(created_at AS INTEGER) / 86400 AS INTEGER) * 86400 AS day,\
             COUNT(*), SUM(CASE WHEN status='ok' THEN 1 ELSE 0 END),\
             COALESCE(SUM(total_tokens),0), COALESCE(AVG(latency_ms),0)\
             FROM request_log{where_sql} GROUP BY day ORDER BY day ASC LIMIT 60",
        );
        let mut series_stmt = conn.prepare(&series_sql)?;
        let series = series_stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(json!({
                    "day": r.get::<_, i64>(0)?, "requests": r.get::<_, i64>(1)?,
                    "ok": r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    "tokens": r.get::<_, i64>(3)?, "avg_latency_ms": r.get::<_, f64>(4)?
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let by_sql = format!(
            "SELECT COALESCE(model,'(unknown)') AS model, COALESCE(MAX(provider),'(unknown)') AS provider,\
             COUNT(*), SUM(CASE WHEN status='ok' THEN 1 ELSE 0 END),\
             COALESCE(SUM(total_tokens),0), COALESCE(AVG(latency_ms),0)\
             FROM request_log{where_sql} GROUP BY model, provider ORDER BY COUNT(*) DESC LIMIT 20",
        );
        let mut by_stmt = conn.prepare(&by_sql)?;
        let by_model = by_stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(json!({
                    "model": r.get::<_, String>(0)?, "provider": r.get::<_, String>(1)?,
                    "requests": r.get::<_, i64>(2)?, "ok": r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    "tokens": r.get::<_, i64>(4)?, "avg_latency_ms": r.get::<_, f64>(5)?
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(json!({
            "summary": summary,
            "total": total,
            "page": page,
            "page_size": page_size,
            "pages": pages,
            "requests": requests,
            "series": series,
            "by_model": by_model,
        }))
    })();
    match result {
        Ok(data) => Json(data).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Ask the server process to open a URL in the OS default browser. Only the
/// loopback origin UI may call this (same LocalOriginGuard family as the other
/// session routes), and only http(s) URLs are accepted.
async fn open_url(Json(input): Json<Value>) -> Response<Body> {
    let Some(url) = input.get("url").and_then(Value::as_str) else {
        return json_error(StatusCode::BAD_REQUEST, "Provide a url.");
    };
    if !(url.starts_with("http://127.0.0.1:")
        || url.starts_with("http://localhost:")
        || url.starts_with("https://"))
    {
        return json_error(StatusCode::BAD_REQUEST, "Only http(s) URLs can be opened.");
    }
    browser::open_url(url);
    Json(json!({"ok": true})).into_response()
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let cfg = load_config(&state);
    Json(json!({
        "ok": true,
        "app": APP_NAME,
        "runtime": "rust",
        "router": cfg.base_url,
        // True only when launched by an embedding host (the VS Code
        // extension); the extension uses it to decide whether it may frame
        // this UI or should open it externally.
        "embeddable": embed_framing_allowed(),
    }))
}

/// Set when the app is launched by an embedding host. Framing stays disabled
/// by default and, when enabled, only a VS Code webview may frame the UI.
fn embed_framing_allowed() -> bool {
    std::env::var("FREE_AI_STUDIO_EMBED")
        .map(|value| value == "vscode")
        .unwrap_or(false)
}

async fn test_router(State(state): State<AppState>) -> Response<Body> {
    let cfg = load_config(&state);
    if cfg.api_key.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "Add the router API key in Settings first."})),
        )
            .into_response();
    }
    let response = state
        .client
        .get(router_url(&cfg, "models"))
        .bearer_auth(&cfg.api_key)
        .send()
        .await;
    match response {
        Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
            Ok(result) => {
                let count = result
                    .get("data")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                Json(json!({"ok": true, "message": format!("Connected. Router returned {count} model(s).")})).into_response()
            }
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                Json(json!({"ok": false, "error": format!("Invalid router response: {e}")})),
            )
                .into_response(),
        },
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            (StatusCode::BAD_GATEWAY, Json(json!({"ok": false, "error": format!("HTTP {}: {}", status.as_u16(), body.chars().take(1500).collect::<String>())}))).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Everything both chat handlers need before the router is called: the
/// validated conversation, the loaded config, and the packed request. Shared
/// so the buffered and streaming paths can never drift apart.
struct ChatPrep {
    cid: String,
    cfg: Config,
    packed: context::PackedRequest,
    /// Per-message model override from the composer's model picker.
    model: Option<String>,
    /// (kind, mime, file bytes) for media referenced by this message.
    attachments: Vec<(String, String, Vec<u8>)>,
}

/// Validate a chat payload, persist the user's message, and pack the context
/// under the token budget. Returns a ready-to-send error response on failure.
/// The error response is boxed to keep the `Result` small (clippy
/// result_large_err); errors never occur in bulk on hot paths.
fn prepare_chat(state: &AppState, input: &Value) -> Result<ChatPrep, Box<Response>> {
    let cid = input
        .get("conversation_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let message = input
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let code: String = input
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(50_000)
        .collect();
    let language: String = input
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("text")
        .chars()
        .take(40)
        .collect();
    if cid.is_empty() || message.is_empty() {
        return Err(Box::new(json_error(
            StatusCode::BAD_REQUEST,
            "Choose a conversation and enter a message.",
        )));
    }
    if message.chars().count() > 50_000 {
        return Err(Box::new(json_error(
            StatusCode::BAD_REQUEST,
            "Message is too long (50,000 characters maximum).",
        )));
    }
    // Media listed with this message: validated against the on-disk store now,
    // so a missing file fails the request instead of degrading silently.
    let attachments = match media::resolve_attachments(state, input) {
        Ok(attachments) => attachments,
        Err(error) => return Err(Box::new(json_error(StatusCode::BAD_REQUEST, error))),
    };
    let attachment_records = input
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut prompt = if code.trim().is_empty() {
        message.clone()
    } else {
        format!("{message}\n\nHere is the code from my editor (language: {language}):\n```{language}\n{code}\n```")
    };

    // Skills: the system prompt carries one line per enabled skill, and the
    // picked skill's instructions ride with this message only.
    let workspace_root = state.workspace.current_root();
    let available = skills::discover(&skills::roots(&state.static_dir, workspace_root.as_deref()));
    let cfg = load_config(state);
    if let Some(skill_id) = input.get("skill").and_then(Value::as_str) {
        if let Some(skill) = skills::instructions_for(skill_id, &available) {
            if skill.error.is_none() {
                prompt = format!(
                    "Follow the skill \"{}\" for this message:\n\n{}\n\n---\n\n{}",
                    skill.name, skill.body, prompt
                );
            }
        }
    }
    // Mode rides with the message too: plan first, build second.
    if input.get("mode").and_then(Value::as_str) == Some("plan") {
        prompt = format!(
            "Plan mode: do not make changes this reply. Answer with a short,\nordered implementation plan (files, steps, risks) the user can approve\nbefore anything is built.\n\n---\n\n{}",
            prompt
        );
    }
    // Decision mode: the reply must end in an actionable recommendation.
    if input.get("mode").and_then(Value::as_str) == Some("decide") {
        prompt = format!(
            "Decision mode: compare the candidate options against the stated\ngoal. Show a short pros/cons table, pick one option, and end with\na single clear recommendation line.\n\n---\n\n{}",
            prompt
        );
    }
    let model = input
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string);

    let history_limit =
        cfg.history_messages
            .clamp(context::MIN_HISTORY_MESSAGES, context::MAX_HISTORY_MESSAGES) as i64;
    let history_result = (|| -> rusqlite::Result<Option<Vec<(String, String)>>> {
        let mut conn = open_db(&state.database_path)?;
        let tx = conn.transaction()?;
        let title = tx
            .query_row("SELECT title FROM conversations WHERE id=?1", [&cid], |r| {
                r.get::<_, String>(0)
            })
            .optional()?;
        if title.is_none() {
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO messages(conversation_id,role,content,created_at,attachments) VALUES(?1,'user',?2,?3,?4)",
            params![cid, prompt, now_seconds(),            serde_json::to_string(&attachment_records)
                .ok()
                .filter(|_| !attachment_records.is_empty())],
        )?;
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM messages WHERE conversation_id=?1",
            [&cid],
            |r| r.get(0),
        )?;
        if count == 1 {
            tx.execute(
                "UPDATE conversations SET title=?1, updated_at=?2 WHERE id=?3",
                params![clean_title(&message), now_seconds(), cid],
            )?;
        } else {
            tx.execute(
                "UPDATE conversations SET updated_at=?1 WHERE id=?2",
                params![now_seconds(), cid],
            )?;
        }
        let mut stmt = tx.prepare(
            "SELECT role,content FROM messages WHERE conversation_id=?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![cid, history_limit], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        tx.commit()?;
        Ok(Some(rows))
    })();

    let rows = match history_result {
        Ok(Some(items)) => items,
        Ok(None) => {
            return Err(Box::new(json_error(
                StatusCode::NOT_FOUND,
                "Conversation not found.",
            )))
        }
        Err(e) => {
            return Err(Box::new(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            )))
        }
    };
    let history: Vec<Value> = rows
        .into_iter()
        .rev()
        .map(|(role, content)| json!({"role": role, "content": content}))
        .collect();
    let outline = if cfg.send_workspace_outline {
        state.workspace.outline()
    } else {
        None
    };
    let mut system = SYSTEM_PROMPT.to_string();
    if let Some(index) = skills::index_text(&cfg.disabled_skills, &available) {
        system.push_str("\n\n");
        system.push_str(&index);
    }
    let packed = context::pack(
        &system,
        outline.as_deref(),
        &history,
        cfg.context_budget_tokens,
        cfg.history_messages,
    );
    Ok(ChatPrep {
        cid,
        cfg,
        packed,
        model,
        attachments,
    })
}

/// Record a failed router call so the usage page reflects it.
fn log_router_error(state: &AppState, cid: &str, error: &str, latency: i64, model: &str) {
    let _ = open_db(&state.database_path).and_then(|conn| conn.execute(
        "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,error) VALUES(?1,?2,?3,'router / unknown','error',?4,?5)",
        params![now_seconds(), cid, model, latency, error.chars().take(2500).collect::<String>()],
    ));
}

/// Deep-search pre-flight, shared by `chat` and `chat_stream`. When the
/// request asked for it: runs provider tools or the bounded search loop,
/// pushes step/source events into `events`, and either returns a finished
/// native answer or rewrites `input.message` with the evidence block.
async fn deepsearch_phase(
    state: &AppState,
    input: &mut Value,
    events: &mut Vec<Value>,
) -> Result<(Option<String>, Vec<Value>), String> {
    if !input.get("deepsearch").and_then(Value::as_bool).unwrap_or(false) {
        return Ok((None, Vec::new()));
    }
    let question = input
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if question.is_empty() {
        return Ok((None, Vec::new()));
    }
    let cfg = load_config(state);
    let outcome = deepsearch::run(
        &state.client,
        &deepsearch::Config {
            base_url: &cfg.base_url,
            api_key: &cfg.api_key,
            model: &cfg.model,
            search_provider: &cfg.search_provider,
            search_api_key: &cfg.search_api_key,
        },
        &question,
        &mut |event| events.push(event),
    )
    .await?;
    let sources = outcome.sources;
    if let Some(answer) = outcome.answer {
        return Ok((Some(answer), sources));
    }
    if let Some(augmented) = outcome.augmented {
        input["message"] = json!(augmented);
    }
    Ok((None, sources))
}

async fn chat(State(state): State<AppState>, Json(mut input): Json<Value>) -> Response<Body> {
    let mut ds_events = Vec::new();
    let ds_answer = match deepsearch_phase(&state, &mut input, &mut ds_events).await {
        Ok((answer, _sources)) => answer,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    let ChatPrep { cid, cfg, mut packed, model, attachments, .. } = match prepare_chat(&state, &input) {
        Ok(prep) => prep,
        Err(response) => return *response,
    };
    // Mirroring: the user's message is saved inside prepare_chat, so every
    // open view of this thread should refresh now that it landed.
    events::notify(&state.events, &cid, "user");
    let context_tokens = packed.estimated_tokens;
    let context_trimmed = packed.trimmed_messages;
    media::apply_media(&mut packed.messages, &attachments);
    // The composer's model wins when present; config's model is the default.
    let chosen_model = model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| if cfg.model.trim().is_empty() { "auto".to_string() } else { cfg.model.clone() });

    // A native provider already answered during the deep-search phase.
    if let Some(answer) = ds_answer {
        let now = now_seconds();
        let log_result = (|| -> rusqlite::Result<()> {
            let conn = open_db(&state.database_path)?;
            conn.execute("INSERT INTO messages(conversation_id,role,content,created_at) VALUES(?1,'assistant',?2,?3)", params![cid, answer, now])?;
            conn.execute(
                "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,prompt_tokens,completion_tokens,total_tokens,fallback_attempts,error) VALUES(?1,?2,?3,'deepsearch','ok',?4,NULL,NULL,NULL,'','')",
                params![now, cid, cfg.model],
            )?;
            conn.execute("UPDATE conversations SET updated_at=?1 WHERE id=?2", params![now, cid])?;
            Ok(())
        })();
        if let Err(e) = log_result {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("AI replied, but saving the response failed: {e}"),
            );
        }
        events::notify(&state.events, &cid, "message");
        let mut meta = json!({ "model": cfg.model, "deepsearch": true });
        meta["context_tokens"] = json!(context_tokens);
        meta["context_trimmed"] = json!(context_trimmed);
        return Json(json!({
            "ok": true,
            "answer": answer,
            "meta": meta,
            "sources": ds_events.into_iter().filter(|e| e["type"] == "source").collect::<Vec<_>>(),
        }))
        .into_response();
    }

    let started = Instant::now();
    match call_router(&state, packed.messages, Some(&chosen_model)).await {
        Ok((answer, mut meta)) => {
            let now = now_seconds();
            let log_result = (|| -> rusqlite::Result<()> {
                let conn = open_db(&state.database_path)?;
                conn.execute("INSERT INTO messages(conversation_id,role,content,created_at) VALUES(?1,'assistant',?2,?3)", params![cid, answer, now])?;
                conn.execute(
                    "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,prompt_tokens,completion_tokens,total_tokens,fallback_attempts,error) VALUES(?1,?2,?3,?4,'ok',?5,?6,?7,?8,?9,'')",
                    params![now, cid, meta.get("model").and_then(Value::as_str).unwrap_or("auto"), meta.get("provider").and_then(Value::as_str).unwrap_or("unknown"), meta.get("latency_ms").and_then(Value::as_i64), meta.get("prompt_tokens").and_then(Value::as_i64), meta.get("completion_tokens").and_then(Value::as_i64), meta.get("total_tokens").and_then(Value::as_i64), meta.get("fallback_attempts").and_then(Value::as_str).unwrap_or("")],
                )?;
                conn.execute(
                    "UPDATE conversations SET updated_at=?1 WHERE id=?2",
                    params![now, cid],
                )?;
                Ok(())
            })();
            if let Err(e) = log_result {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("AI replied, but saving the response failed: {e}"),
                );
            }
            events::notify(&state.events, &cid, "message");
            meta["context_tokens"] = json!(context_tokens);
            meta["context_trimmed"] = json!(context_trimmed);
            Json(json!({"ok": true, "answer": answer, "meta": meta})).into_response()
        }
        Err(error) => {
            let latency = started.elapsed().as_millis() as i64;
            log_router_error(&state, &cid, &error, latency, &cfg.model);
            events::notify(&state.events, &cid, "failed");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"ok": false, "error": error, "meta": {"latency_ms": latency}})),
            )
                .into_response()
        }
    }
}

/// One parsed `data:` payload from an OpenAI-style completion stream.
#[derive(Debug, Default, PartialEq)]
struct StreamEvent {
    delta: Option<String>,
    usage: Option<Value>,
    done: bool,
    error: Option<String>,
}

/// Parse one SSE payload from the router: text deltas, the final usage block,
/// `[DONE]`, or an inline error. Kept pure so it can be tested directly.
fn parse_stream_payload(payload: &str) -> StreamEvent {
    let payload = payload.trim();
    if payload.is_empty() {
        return StreamEvent::default();
    }
    if payload == "[DONE]" {
        return StreamEvent {
            done: true,
            ..StreamEvent::default()
        };
    }
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return StreamEvent::default();
    };
    if let Some(error) = value.pointer("/error/message").and_then(Value::as_str) {
        return StreamEvent {
            error: Some(error.to_string()),
            done: true,
            ..StreamEvent::default()
        };
    }
    let delta = value
        .pointer("/choices/0/delta/content")
        .or_else(|| value.pointer("/choices/0/message/content"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.is_empty());
    let usage = value.get("usage").filter(|u| !u.is_null()).cloned();
    StreamEvent {
        delta,
        usage,
        done: false,
        error: None,
    }
}

/// Saves the streamed reply exactly once — when the stream ends, errors, or
/// the client walks away mid-answer — so a stopped request is never lost.
struct ReplyRecorder {
    database_path: PathBuf,
    conversation_id: String,
    text: Arc<Mutex<String>>,
    meta: Arc<Mutex<Value>>,
    started: Instant,
    saved: Arc<AtomicBool>,
    events: events::Hub,
}

impl ReplyRecorder {
    fn new(
        database_path: PathBuf,
        conversation_id: String,
        context_tokens: usize,
        events: events::Hub,
    ) -> Self {
        Self {
            database_path,
            conversation_id,
            text: Arc::new(Mutex::new(String::new())),
            meta: Arc::new(Mutex::new(json!({ "context_tokens": context_tokens }))),
            started: Instant::now(),
            saved: Arc::new(AtomicBool::new(false)),
            events,
        }
    }

    fn save(&self) {
        if self.saved.swap(true, Ordering::SeqCst) {
            return;
        }
        let text = self.text.lock().map(|guard| guard.clone()).unwrap_or_default();
        let meta = self
            .meta
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| json!({}));
        let latency = self.started.elapsed().as_millis() as i64;
        let _ = open_db(&self.database_path).and_then(|conn| {
            let now = now_seconds();
            if !text.trim().is_empty() {
                conn.execute("INSERT INTO messages(conversation_id,role,content,created_at) VALUES(?1,'assistant',?2,?3)", params![self.conversation_id, text, now])?;
            }
            conn.execute(
                "UPDATE conversations SET updated_at=?1 WHERE id=?2",
                params![now, self.conversation_id],
            )?;
            conn.execute(
                "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,prompt_tokens,completion_tokens,total_tokens,fallback_attempts,error) VALUES(?1,?2,?3,?4,'ok',?5,?6,?7,?8,?9,'')",
                params![now, self.conversation_id,
                    meta.get("model").and_then(Value::as_str).unwrap_or("auto"),
                    meta.get("provider").and_then(Value::as_str).unwrap_or("unknown"),
                    latency,
                    meta.get("prompt_tokens").and_then(Value::as_i64),
                    meta.get("completion_tokens").and_then(Value::as_i64),
                    meta.get("total_tokens").and_then(Value::as_i64),
                    meta.get("fallback_attempts").and_then(Value::as_str).unwrap_or("")],
            )?;
            Ok(())
        });
        events::notify(
            &self.events,
            &self.conversation_id,
            if text.trim().is_empty() { "updated" } else { "message" },
        );
    }
}

impl Drop for ReplyRecorder {
    fn drop(&mut self) {
        self.save();
    }
}

fn sse_frame(value: &Value) -> String {
    format!("data: {value}\n\n")
}

struct StreamState {
    bytes: BoxStream<'static, Result<bytes::Bytes, reqwest::Error>>,
    leftover: String,
    pending: VecDeque<String>,
    recorder: ReplyRecorder,
    finished: bool,
}

/// Wrap a state machine in the SSE response the browser reads.
fn sse_response(state: StreamState) -> Response<Body> {
    let body = futures_util::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(frame) = st.pending.pop_front() {
                return Some((Ok::<String, std::io::Error>(frame), st));
            }
            if st.finished {
                return None; // dropping the state runs the recorder's Drop
            }
            match st.bytes.next().await {
                Some(Ok(chunk)) => st.ingest(&String::from_utf8_lossy(&chunk)),
                Some(Err(error)) => {
                    st.push(json!({ "error": format!("The stream from the router failed: {error}") }));
                    st.finished = true;
                }
                None => st.finish(),
            }
        }
    });
    let mut response = Response::new(Body::from_stream(body));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

impl StreamState {
    fn push(&mut self, value: Value) {
        self.pending.push_back(sse_frame(&value));
    }

    /// Feed one router chunk: split complete SSE events off the buffer and
    /// turn them into client frames.
    fn ingest(&mut self, chunk: &str) {
        self.leftover.push_str(chunk);
        while let Some(index) = self.leftover.find("\n\n") {
            let event: String = self.leftover.drain(..index + 2).collect();
            for line in event.lines() {
                let Some(payload) = line.strip_prefix("data:") else { continue };
                let parsed = parse_stream_payload(payload);
                if let Some(text) = parsed.delta {
                    if let Ok(mut held) = self.recorder.text.lock() {
                        held.push_str(&text);
                    }
                    self.push(json!({ "delta": text }));
                }
                if let Some(usage) = parsed.usage {
                    if let Ok(mut held) = self.recorder.meta.lock() {
                        held["prompt_tokens"] = usage.get("prompt_tokens").cloned().unwrap_or(Value::Null);
                        held["completion_tokens"] = usage.get("completion_tokens").cloned().unwrap_or(Value::Null);
                        held["total_tokens"] = usage.get("total_tokens").cloned().unwrap_or(Value::Null);
                    }
                }
                if let Some(error) = parsed.error {
                    self.push(json!({ "error": error }));
                    self.finished = true;
                    return;
                }
                if parsed.done {
                    self.finish();
                    return;
                }
            }
        }
    }

    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        let meta = self
            .recorder
            .meta
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| json!({}));
        self.push(json!({ "done": true, "meta": meta }));
    }
}

/// Stream the router's answer token by token. The client aborting the fetch
/// drops this body, which drops the byte stream and runs the recorder's
/// `Drop`, so whatever was answered so far is still stored.
async fn chat_stream(State(state): State<AppState>, Json(mut input): Json<Value>) -> Response<Body> {
    let mut ds_frames: VecDeque<String> = VecDeque::new();
    let mut ds_events: Vec<Value> = Vec::new();
    let ds_answer = match deepsearch_phase(&state, &mut input, &mut ds_events).await {
        Ok((answer, _sources)) => answer,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    for event in ds_events {
        ds_frames.push_back(sse_frame(&event));
    }

    let ChatPrep { cid, cfg, mut packed, model, attachments, .. } = match prepare_chat(&state, &input) {
        Ok(prep) => prep,
        Err(response) => return *response,
    };
    // Mirroring: the user's message is saved inside prepare_chat, so every
    // open view of this thread should refresh now that it landed.
    events::notify(&state.events, &cid, "user");
    let context_tokens = packed.estimated_tokens;
    media::apply_media(&mut packed.messages, &attachments);

    if ds_answer.is_none() && cfg.api_key.trim().is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Add your router's unified API key in Settings first.",
        );
    }
    let chosen_model = model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| if cfg.model.trim().is_empty() { "auto".to_string() } else { cfg.model.clone() });

    // Deep search already answered through a native provider: stream the
    // finished text as one frame set instead of calling the router.
    if let Some(answer) = ds_answer {
        let recorder = ReplyRecorder::new(state.database_path.clone(), cid, context_tokens, state.events.clone());
        if let Ok(mut held) = recorder.meta.lock() {
            held["model"] = json!(&chosen_model);
            held["deepsearch"] = json!(true);
        }
        if let Ok(mut held) = recorder.text.lock() {
            held.push_str(&answer);
        }
        let mut pending = ds_frames;
        pending.push_back(sse_frame(&json!({ "ready": true })));
        pending.push_back(sse_frame(&json!({ "delta": answer })));
        let meta = recorder
            .meta
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| json!({}));
        pending.push_back(sse_frame(&json!({ "done": true, "meta": meta })));
        return sse_response(StreamState {
            bytes: futures_util::stream::empty().boxed(),
            leftover: String::new(),
            pending,
            recorder,
            finished: false,
        });
    }

    let request_body = json!({
        "model": &chosen_model,
        "messages": packed.messages,
        "stream": true
    });
    let upstream = state
        .client
        .post(router_url(&cfg, "chat/completions"))
        .bearer_auth(&cfg.api_key)
        .header("accept", "text/event-stream")
        .json(&request_body)
        .send()
        .await;

    let response = match upstream {
        Ok(response) => response,
        Err(error) => {
            let message = reach_error(&cfg.base_url, error.to_string());
            log_router_error(&state, &cid, &message, 0, &cfg.model);
            return json_error(StatusCode::BAD_GATEWAY, message);
        }
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| body.chars().take(3000).collect());
        let message = format!("Router returned HTTP {}: {}", status.as_u16(), detail);
        log_router_error(&state, &cid, &message, 0, &cfg.model);
        return json_error(StatusCode::BAD_GATEWAY, message);
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let recorder = ReplyRecorder::new(state.database_path.clone(), cid, context_tokens, state.events.clone());
    if let Ok(mut held) = recorder.meta.lock() {
        held["model"] = json!(&chosen_model);
    }
    let mut pending: VecDeque<String> = ds_frames;
    pending.push_back(sse_frame(&json!({ "ready": true })));

    // A router that ignores `stream` and answers with plain JSON still gets a
    // one-shot stream, so the UI's stop/retry path works either way.
    if !content_type.contains("event-stream") {
        let body = response.text().await.unwrap_or_default();
        let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let answer = response_content(&parsed);
        let usage = parsed.get("usage").cloned().unwrap_or_else(|| json!({}));
        if let Ok(mut held) = recorder.meta.lock() {
            for key in ["prompt_tokens", "completion_tokens", "total_tokens"] {
                held[key] = usage.get(key).cloned().unwrap_or(Value::Null);
            }
            if let Some(model) = parsed.get("model").and_then(Value::as_str) {
                held["model"] = json!(model);
            }
        }
        if let Ok(mut held) = recorder.text.lock() {
            held.push_str(&answer);
        }
        pending.push_back(sse_frame(&json!({ "delta": answer })));
        let meta = recorder
            .meta
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| json!({}));
        pending.push_back(sse_frame(&json!({ "done": true, "meta": meta })));
        return sse_response(StreamState {
            bytes: futures_util::stream::empty().boxed(),
            leftover: String::new(),
            pending,
            recorder,
            finished: false,
        });
    }

    sse_response(StreamState {
        bytes: response.bytes_stream().boxed(),
        leftover: String::new(),
        pending,
        recorder,
        finished: false,
    })
}

struct Boot {
    port: u16,
    server: tokio::task::JoinHandle<Result<(), io::Error>>,
    remote_task: Option<tokio::task::JoinHandle<Result<(), String>>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
}

/// Bind listeners and start the local UI server plus the optional desktop
/// host API. Runs before the GUI takes over the main thread.
async fn bootstrap() -> Result<Boot, Box<dyn std::error::Error>> {
    let (config_path, database_path, static_dir) = paths();
    let workspace_state_path = config_path.with_file_name("workspace.json");
    let state = AppState {
        workspace: workspace::WorkspaceStore::load(workspace_state_path),
        config_path,
        database_path,
        static_dir,
        client: Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?,
        terminal: terminal::hub(),
        events: events::Hub::new(),
        models_cache: Arc::new(Mutex::new(HashMap::new())),
    };
    ensure_setup(&state)?;
    agent::mark_interrupted(&state);
    let port: u16 = env::var("FREE_AI_STUDIO_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let address = format!("{HOST}:{port}");
    let listener = TcpListener::bind(&address).await?;
    let router_api = Router::new()
        .route("/api/router", get(router_status))
        .route("/api/router/start", post(router_start))
        .route("/api/router/stop", post(router_stop))
        .route("/api/omniroute", get(omniroute_status))
        .route("/api/omniroute/start", post(omniroute_start))
        .route("/api/omniroute/stop", post(omniroute_stop))
        .route_layer(middleware::from_fn_with_state(
            workspace::LocalOriginGuard::new(port),
            workspace::guard_local_origin,
        ))
        .with_state(state.clone());
    let app = Router::new()
        .route("/", get(root))
        .route("/static/*file", get(static_file))
        .route("/api/config", get(get_config).post(post_config))
        .route(
            "/api/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route("/api/conversations/:id/messages", get(get_messages))
        .route("/api/conversations/:id/delete", post(delete_conversation))
        .route("/api/usage", get(usage))
        .route("/api/open", post(open_url))
        .route("/api/health", get(health))
        .route("/api/test", post(test_router))
        .route("/api/models", get(models))
        .route("/api/chat", post(chat))
        .route("/api/chat/stream", post(chat_stream))
        .route("/api/events", get(crate::events::events))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/:id/update", post(update_project))
        .route("/api/projects/:id/delete", post(delete_project))
        .route("/api/projects/:id/activate", post(activate_project))
        .route("/api/workflows", get(list_workflows).post(save_workflow))
        .route("/api/workflows/:id/delete", post(delete_workflow))
        .merge(
            skills::router().route_layer(middleware::from_fn_with_state(
                workspace::LocalOriginGuard::new(port),
                workspace::guard_local_origin,
            )),
        )
        .merge(workspace::router(port))
        .merge(terminal::router(port))
        .merge(media::router(port))
        .merge(agent::router().route_layer(middleware::from_fn_with_state(
            workspace::LocalOriginGuard::new(port),
            workspace::guard_local_origin,
        )))
        .merge(router_console::router(port))
        .merge(router_api)
        .layer(middleware::from_fn(local_security_headers))
        .with_state(state.clone());
    let remote_config = host_transport::RemoteListenerConfig::from_env()?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let remote_task = if let Some(remote) = remote_config {
        let token_path = state.config_path.with_file_name("host-auth.json");
        let token_store = host_auth::HostTokenStore::new(token_path);
        if token_store.status()?.is_none() {
            return Err("Desktop Host API is enabled but no host token exists. Run `free-ai-studio host-token create` locally first.".into());
        }

        let tls_config = remote.load_tls_config().await?;
        let remote_listener = TcpListener::bind(remote.socket_addr()).await?;
        let remote_std_listener = remote_listener.into_std()?;
        let remote_router =
            host_security::harden_router(host_api::build_router(state.clone(), token_store));
        let address = remote.socket_addr();
        let server_handle = axum_server::Handle::new();
        let handle_for_shutdown = server_handle.clone();
        let mut remote_shutdown_rx = shutdown_rx.clone();
        let server = axum_server::from_tcp_rustls(remote_std_listener, tls_config)
            .handle(server_handle)
            .serve(remote_router.into_make_service());

        println!("Desktop Host API enabled over HTTPS at https://{address}/api/host/v1 (bearer authentication required).");
        Some(tokio::spawn(async move {
            tokio::pin!(server);
            tokio::select! {
                result = &mut server => result.map_err(|error| format!("Desktop Host API server failed: {error}")),
                changed = remote_shutdown_rx.changed() => {
                    if changed.is_ok() && *remote_shutdown_rx.borrow() {
                        handle_for_shutdown.graceful_shutdown(Some(Duration::from_secs(10)));
                        server.await.map_err(|error| format!("Desktop Host API shutdown failed: {error}"))
                    } else {
                        Ok(())
                    }
                }
            }
        }))
    } else {
        println!("Desktop Host API disabled (default). Set FREE_AI_STUDIO_HOST_API_ENABLED=true only after configuring TLS and a host token.");
        None
    };

    println!("{APP_NAME} (Rust) is running at http://{address}");
    println!("Settings: {}", state.config_path.display());
    println!("Conversation database: {}", state.database_path.display());
    println!("Static UI directory: {}", state.static_dir.display());

    let ctrl_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = ctrl_tx.send(true);
    });
    let mut server_shutdown_rx = shutdown_rx.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = server_shutdown_rx.changed().await;
                println!("Stopping {APP_NAME}.");
            })
            .await
    });
    Ok(Boot {
        port,
        server,
        remote_task,
        shutdown_tx,
    })
}

async fn wait_for_shutdown(mut rx: tokio::sync::watch::Receiver<bool>) {
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Choose a process-wide Rustls provider before any TLS client/server builder:
    // reqwest and axum-server can enable both aws-lc-rs and ring through Cargo
    // feature unification, and Rustls panics when it cannot infer a provider.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("host-token") => {
            let (config_path, _, _) = paths();
            let token_path = config_path.with_file_name("host-auth.json");
            let store = host_auth::HostTokenStore::new(token_path);
            host_auth::run_cli(&args[1..], &store)?;
            return Ok(());
        }
        Some("router") => {
            router::run_cli(&args[1..])?;
            return Ok(());
        }
        Some("omniroute") => {
            router::run_omniroute_cli(&args[1..])?;
            return Ok(());
        }
        _ => {}
    }

    let wants_browser = args.iter().any(|arg| arg == "--browser");
    let gui_mode = !wants_browser
        && !args
            .iter()
            .any(|arg| arg == "--no-gui" || arg == "--headless")
        && env::var_os("FREE_AI_STUDIO_NO_GUI").is_none();

    let runtime = tokio::runtime::Runtime::new()?;
    let boot = runtime.block_on(async {
        if env::var_os("FREE_AI_STUDIO_SKIP_ROUTER").is_none() {
            let status = router::ensure_started().await;
            if status.reachable {
                println!(
                    "FreeLLMAPI router is responding at http://127.0.0.1:{}.",
                    status.port
                );
            }
        }
        bootstrap().await
    })?;

    if gui_mode {
        println!("Application window: http://127.0.0.1:{}", boot.port);
        #[cfg(feature = "gui")]
        {
            if let Err(error) = gui::run(boot.port, boot.shutdown_tx.clone(), APP_NAME) {
                println!(
                    "Could not open the application window ({error})."
                );
                println!(
                    "Continuing headless: falling back to your browser."
                );
                browser::open(boot.port);
                println!(
                    "Or open http://127.0.0.1:{} manually.",
                    boot.port
                );
            }
        }
        #[cfg(not(feature = "gui"))]
        println!("This build has no window support. Open http://127.0.0.1:{} in a browser, or rebuild with the default gui feature.", boot.port);
    } else if wants_browser {
        println!("Opening the interface in your browser. Stop with Ctrl+C.");
        browser::open(boot.port);
    } else {
        println!(
            "Headless mode. Open http://127.0.0.1:{} or stop with Ctrl+C.",
            boot.port
        );
    }

    if gui_mode {
        // The window closed: run the shared shutdown path now.
        let _ = boot.shutdown_tx.send(true);
    } else {
        runtime.block_on(wait_for_shutdown(boot.shutdown_tx.subscribe()));
    }
    runtime.block_on(async move {
        match boot.server.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => println!("Server stopped with an error: {error}"),
            Err(error) => println!("Server task failed: {error}"),
        }
        if let Some(task) = boot.remote_task {
            match task.await {
                Ok(Ok(())) => {}
                Ok(Err(message)) => println!("Desktop Host API stopped with an error: {message}"),
                Err(error) => println!("Desktop Host API task failed: {error}"),
            }
        }
        router::shutdown_cleanup().await;
        router::omniroute_shutdown_cleanup().await;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("free-ai-studio-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn static_dir_candidates_prefer_env_then_installed_then_adjacent_then_source() {
        let exe_dir = scratch("bin");
        let env_dir = scratch("override");
        let candidates = static_dir_candidates(Some(&exe_dir), Some(env_dir.clone()));
        assert_eq!(candidates[0], env_dir);
        assert_eq!(candidates[1], exe_dir.join("../share/free-ai-suite/static"));
        assert_eq!(candidates[2], exe_dir.join("static"));
        assert_eq!(
            candidates[3],
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static")
        );
    }

    #[test]
    fn static_dir_candidates_fall_back_to_source_tree() {
        let candidates = static_dir_candidates(None, None);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].join("index.html").is_file());
    }

    #[test]
    fn parses_router_stream_payloads() {
        let delta = parse_stream_payload(r#"{"choices":[{"delta":{"content":"Hello"}}]}"#);
        assert_eq!(delta.delta.as_deref(), Some("Hello"));
        assert!(!delta.done);

        let done = parse_stream_payload("[DONE]");
        assert!(done.done);
        assert!(done.delta.is_none());

        let usage =
            parse_stream_payload(r#"{"usage":{"prompt_tokens":12,"completion_tokens":4,"total_tokens":16}}"#);
        assert_eq!(
            usage
                .usage
                .as_ref()
                .and_then(|value| value.get("total_tokens"))
                .and_then(Value::as_i64),
            Some(16)
        );

        let failure = parse_stream_payload(r#"{"error":{"message":"quota exceeded"}}"#);
        assert_eq!(failure.error.as_deref(), Some("quota exceeded"));
        assert!(failure.done);

        assert_eq!(parse_stream_payload(""), StreamEvent::default());
        assert_eq!(parse_stream_payload("not json"), StreamEvent::default());
    }

    #[test]
    fn migrates_a_pre_projects_database() {
        let path = scratch("migrate.sqlite3");
        let _ = fs::remove_file(&path);
        {
            // The schema as it shipped before projects existed.
            let conn = open_db(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE conversations (
                    id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at REAL NOT NULL,
                    updated_at REAL NOT NULL
                );
                CREATE TABLE messages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT, conversation_id TEXT NOT NULL,
                    role TEXT NOT NULL, content TEXT NOT NULL, created_at REAL NOT NULL
                );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO conversations(id,title,created_at,updated_at) VALUES('c1','Old thread',1,1)",
                [],
            )
            .unwrap();
        }

        initialize_database(&path).unwrap();

        let conn = open_db(&path).unwrap();
        let project: String = conn
            .query_row(
                "SELECT project_id FROM conversations WHERE id='c1'",
                [],
                |row| row.get(0),
            )
            .expect("existing conversation backfilled into a project");
        assert!(!project.is_empty());
        let projects: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(projects, 1);

        // Running the migration again must be a no-op.
        initialize_database(&path).unwrap();
        let again: i64 = conn
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(again, 1);
    }
}
