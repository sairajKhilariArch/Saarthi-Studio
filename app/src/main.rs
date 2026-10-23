use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::{Client, Url};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    env, fs,
    io,
    path::{Component, Path as FsPath, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::net::TcpListener;
use uuid::Uuid;

mod host_auth;
mod host_api;
mod host_transport;
mod host_security;

const APP_NAME: &str = "Free AI Studio";
const HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8765;
const SYSTEM_PROMPT: &str = "You are a concise, helpful AI coding assistant. Be accurate and safe; never claim to run code unless you did.";

#[derive(Clone)]
struct AppState {
    config_path: PathBuf,
    database_path: PathBuf,
    static_dir: PathBuf,
    client: Client,
}

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    base_url: String,
    model: String,
    #[serde(default)]
    api_key: String,
}

impl Config {
    fn defaults() -> Self {
        Self {
            base_url: env::var("FREE_AI_STUDIO_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:3001/v1".to_string()),
            model: "auto".to_string(),
            api_key: String::new(),
        }
    }
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn paths() -> (PathBuf, PathBuf, PathBuf) {
    let home = home_dir();
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
        fs::set_permissions(path, fs::Permissions::from_mode(if dir { 0o700 } else { 0o600 }))?;
    }
    #[cfg(not(unix))]
    let _ = (path, dir);
    Ok(())
}

fn open_db(path: &FsPath) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 10000;")?;
    Ok(conn)
}

fn initialize_database(path: &FsPath) -> rusqlite::Result<()> {
    let conn = open_db(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS conversations (
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
    )
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
    Ok(())
}

fn load_config(state: &AppState) -> Config {
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

fn json_error(status: StatusCode, message: impl Into<String>) -> Response<Body> {
    (status, Json(json!({"error": message.into()}))).into_response()
}

fn clean_title(value: &str) -> String {
    let title = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = title.chars().take(64).collect::<String>();
    if title.is_empty() { "New conversation".to_string() } else { title }
}

fn router_url(cfg: &Config, suffix: &str) -> String {
    format!("{}/{}", cfg.base_url.trim_end_matches('/'), suffix.trim_start_matches('/'))
}

fn validate_config(mut cfg: Config) -> Result<Config, String> {
    cfg.base_url = cfg.base_url.trim().trim_end_matches('/').to_string();
    cfg.model = if cfg.model.trim().is_empty() { "auto".to_string() } else { cfg.model.trim().to_string() };
    cfg.api_key = cfg.api_key.trim().to_string();

    let parsed = Url::parse(&cfg.base_url).map_err(|_| "Router URL must start with http:// or https://.".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("Router URL must start with http:// or https://.".into());
    }
    let host = parsed.host_str().unwrap_or_default();
    let is_local = matches!(host, "localhost" | "127.0.0.1" | "::1");
    if parsed.scheme() != "https" && !is_local {
        return Err("For a non-local router, use HTTPS so your API key is encrypted in transit.".into());
    }
    Ok(cfg)
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
        if !joined.is_empty() { return joined; }
    }
    serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string())
}

async fn call_router(state: &AppState, messages: Vec<Value>) -> Result<(String, Value), String> {
    let cfg = load_config(state);
    if cfg.api_key.trim().is_empty() {
        return Err("Add your router's unified API key in Settings first.".into());
    }

    let request_body = json!({
        "model": if cfg.model.trim().is_empty() { "auto" } else { cfg.model.as_str() },
        "messages": messages,
        "stream": false
    });
    let started = Instant::now();
    let response = state.client
        .post(router_url(&cfg, "chat/completions"))
        .bearer_auth(&cfg.api_key)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Could not reach router at {}: {}", cfg.base_url, e))?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.text().await.map_err(|e| format!("Could not read router response: {e}"))?;
    let latency = started.elapsed().as_millis() as i64;

    let parsed: Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| json!({"message": body.chars().take(3000).collect::<String>()}));
    if !status.is_success() {
        let detail = parsed.pointer("/error/message")
            .or_else(|| parsed.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| body.chars().take(3000).collect());
        return Err(format!("Router returned HTTP {}: {}", status.as_u16(), detail));
    }

    let answer = response_content(&parsed);
    let usage = parsed.get("usage").cloned().unwrap_or_else(|| json!({}));
    let provider = headers.get("x-routed-via").and_then(|v| v.to_str().ok())
        .unwrap_or("router / provider not reported");
    let fallback_attempts = headers.get("x-fallback-attempts").and_then(|v| v.to_str().ok()).unwrap_or("");
    let model = parsed.get("model").and_then(Value::as_str).unwrap_or(&cfg.model);
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
    if path.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
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
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    response
}

async fn get_config(State(state): State<AppState>) -> Json<Value> {
    let cfg = load_config(&state);
    Json(json!({ "base_url": cfg.base_url, "model": cfg.model, "has_api_key": !cfg.api_key.is_empty() }))
}

async fn post_config(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    let mut cfg = load_config(&state);
    let base_url = input.get("base_url").and_then(Value::as_str).unwrap_or(&cfg.base_url).to_string();
    let model = input.get("model").and_then(Value::as_str).unwrap_or(&cfg.model).to_string();
    if let Some(key) = input.get("api_key").and_then(Value::as_str) {
        if !key.trim().is_empty() { cfg.api_key = key.trim().to_string(); }
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
    Json(json!({"ok": true, "base_url": cfg.base_url, "model": cfg.model, "has_api_key": !cfg.api_key.is_empty()})).into_response()
}

async fn list_conversations(State(state): State<AppState>) -> Response<Body> {
    let result = (|| -> rusqlite::Result<Vec<Value>> {
        let conn = open_db(&state.database_path)?;
        let mut stmt = conn.prepare(
            "SELECT v.id, v.title, v.created_at, v.updated_at,
             (SELECT content FROM messages m WHERE m.conversation_id=v.id ORDER BY id DESC LIMIT 1) AS last_message
             FROM conversations v ORDER BY v.updated_at DESC"
        )?;
        let rows = stmt.query_map([], |row| Ok(json!({
            "id": row.get::<_, String>(0)?, "title": row.get::<_, String>(1)?,
            "created_at": row.get::<_, f64>(2)?, "updated_at": row.get::<_, f64>(3)?,
            "last_message": row.get::<_, Option<String>>(4)?
        })))?;
        rows.collect()
    })();
    match result {
        Ok(items) => Json(Value::Array(items)).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_messages(State(state): State<AppState>, Path(id): Path<String>) -> Response<Body> {
    let result = (|| -> rusqlite::Result<Option<Vec<Value>>> {
        let conn = open_db(&state.database_path)?;
        let exists = conn.query_row("SELECT id FROM conversations WHERE id=?1", [&id], |r| r.get::<_, String>(0)).optional()?;
        if exists.is_none() { return Ok(None); }
        let mut stmt = conn.prepare("SELECT id, role, content, created_at FROM messages WHERE conversation_id=?1 ORDER BY id")?;
        let rows = stmt.query_map([id], |row| Ok(json!({
            "id": row.get::<_, i64>(0)?, "role": row.get::<_, String>(1)?,
            "content": row.get::<_, String>(2)?, "created_at": row.get::<_, f64>(3)?
        })))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map(Some)
    })();
    match result {
        Ok(Some(items)) => Json(Value::Array(items)).into_response(),
        Ok(None) => json_error(StatusCode::NOT_FOUND, "Conversation not found."),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn create_conversation(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    let id = Uuid::new_v4().to_string();
    let title = clean_title(input.get("title").and_then(Value::as_str).unwrap_or("New conversation"));
    let now = now_seconds();
    match open_db(&state.database_path).and_then(|conn| conn.execute(
        "INSERT INTO conversations(id,title,created_at,updated_at) VALUES(?1,?2,?3,?4)",
        params![id, title, now, now],
    )) {
        Ok(_) => (StatusCode::CREATED, Json(json!({"id": id, "title": title}))).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn delete_conversation(State(state): State<AppState>, Path(id): Path<String>) -> Response<Body> {
    match open_db(&state.database_path).and_then(|conn| conn.execute("DELETE FROM conversations WHERE id=?1", [id])) {
        Ok(_) => Json(json!({"ok": true})).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn usage(State(state): State<AppState>) -> Response<Body> {
    let result = (|| -> rusqlite::Result<Value> {
        let conn = open_db(&state.database_path)?;
        let summary = conn.query_row(
            "SELECT COUNT(*) AS requests,
             SUM(CASE WHEN status='ok' THEN 1 ELSE 0 END) AS successful,
             SUM(CASE WHEN status!='ok' THEN 1 ELSE 0 END) AS failed,
             COALESCE(SUM(total_tokens),0) AS total_tokens,
             COALESCE(AVG(latency_ms),0) AS avg_latency_ms FROM request_log",
            [],
            |r| Ok(json!({
                "requests": r.get::<_, i64>(0)?, "successful": r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                "failed": r.get::<_, Option<i64>>(2)?.unwrap_or(0), "total_tokens": r.get::<_, i64>(3)?,
                "avg_latency_ms": r.get::<_, f64>(4)?
            })),
        )?;
        let mut stmt = conn.prepare("SELECT id, created_at, conversation_id, model, provider, status, latency_ms, prompt_tokens, completion_tokens, total_tokens, fallback_attempts, error FROM request_log ORDER BY id DESC LIMIT 50")?;
        let requests = stmt.query_map([], |r| Ok(json!({
            "id": r.get::<_, i64>(0)?, "created_at": r.get::<_, f64>(1)?,
            "conversation_id": r.get::<_, Option<String>>(2)?, "model": r.get::<_, Option<String>>(3)?,
            "provider": r.get::<_, Option<String>>(4)?, "status": r.get::<_, String>(5)?,
            "latency_ms": r.get::<_, Option<i64>>(6)?, "prompt_tokens": r.get::<_, Option<i64>>(7)?,
            "completion_tokens": r.get::<_, Option<i64>>(8)?, "total_tokens": r.get::<_, Option<i64>>(9)?,
            "fallback_attempts": r.get::<_, Option<String>>(10)?, "error": r.get::<_, Option<String>>(11)?
        })))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"summary": summary, "requests": requests}))
    })();
    match result {
        Ok(data) => Json(data).into_response(),
        Err(e) => json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let cfg = load_config(&state);
    Json(json!({"ok": true, "app": APP_NAME, "runtime": "rust", "router": cfg.base_url}))
}

async fn test_router(State(state): State<AppState>) -> Response<Body> {
    let cfg = load_config(&state);
    if cfg.api_key.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": "Add the router API key in Settings first."}))).into_response();
    }
    let response = state.client.get(router_url(&cfg, "models")).bearer_auth(&cfg.api_key).send().await;
    match response {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<Value>().await {
                Ok(result) => {
                    let count = result.get("data").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
                    Json(json!({"ok": true, "message": format!("Connected. Router returned {count} model(s).")})).into_response()
                }
                Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({"ok": false, "error": format!("Invalid router response: {e}")}))).into_response(),
            }
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            (StatusCode::BAD_GATEWAY, Json(json!({"ok": false, "error": format!("HTTP {}: {}", status.as_u16(), body.chars().take(1500).collect::<String>())}))).into_response()
        }
        Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({"ok": false, "error": e.to_string()}))).into_response(),
    }
}

async fn chat(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    let cid = input.get("conversation_id").and_then(Value::as_str).unwrap_or("").to_string();
    let message = input.get("message").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let code: String = input.get("code").and_then(Value::as_str).unwrap_or("").chars().take(50_000).collect();
    let language: String = input.get("language").and_then(Value::as_str).unwrap_or("text").chars().take(40).collect();
    if cid.is_empty() || message.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "Choose a conversation and enter a message.");
    }
    if message.chars().count() > 50_000 {
        return json_error(StatusCode::BAD_REQUEST, "Message is too long (50,000 characters maximum).");
    }

    let prompt = if code.trim().is_empty() {
        message.clone()
    } else {
        format!("{message}\n\nHere is the code from my editor (language: {language}):\n```{language}\n{code}\n```")
    };
    let history_result = (|| -> rusqlite::Result<Option<Vec<Value>>> {
        let mut conn = open_db(&state.database_path)?;
        let tx = conn.transaction()?;
        let title = tx.query_row("SELECT title FROM conversations WHERE id=?1", [&cid], |r| r.get::<_, String>(0)).optional()?;
        if title.is_none() { return Ok(None); }
        tx.execute("INSERT INTO messages(conversation_id,role,content,created_at) VALUES(?1,'user',?2,?3)", params![cid, prompt, now_seconds()])?;
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM messages WHERE conversation_id=?1", [&cid], |r| r.get(0))?;
        if count == 1 {
            tx.execute("UPDATE conversations SET title=?1, updated_at=?2 WHERE id=?3", params![clean_title(&message), now_seconds(), cid])?;
        } else {
            tx.execute("UPDATE conversations SET updated_at=?1 WHERE id=?2", params![now_seconds(), cid])?;
        }
        let mut stmt = tx.prepare("SELECT role,content FROM messages WHERE conversation_id=?1 ORDER BY id DESC LIMIT 4")?;
        let rows = stmt.query_map([&cid], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut messages = vec![json!({"role": "system", "content": SYSTEM_PROMPT})];
        for (role, content) in rows.into_iter().rev() {
            messages.push(json!({"role": role, "content": content}));
        }
        drop(stmt);
        tx.commit()?;
        Ok(Some(messages))
    })();

    let messages = match history_result {
        Ok(Some(items)) => items,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "Conversation not found."),
        Err(e) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let started = Instant::now();
    match call_router(&state, messages).await {
        Ok((answer, meta)) => {
            let now = now_seconds();
            let log_result = (|| -> rusqlite::Result<()> {
                let conn = open_db(&state.database_path)?;
                conn.execute("INSERT INTO messages(conversation_id,role,content,created_at) VALUES(?1,'assistant',?2,?3)", params![cid, answer, now])?;
                conn.execute(
                    "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,prompt_tokens,completion_tokens,total_tokens,fallback_attempts,error) VALUES(?1,?2,?3,?4,'ok',?5,?6,?7,?8,?9,'')",
                    params![now, cid, meta.get("model").and_then(Value::as_str).unwrap_or("auto"), meta.get("provider").and_then(Value::as_str).unwrap_or("unknown"), meta.get("latency_ms").and_then(Value::as_i64), meta.get("prompt_tokens").and_then(Value::as_i64), meta.get("completion_tokens").and_then(Value::as_i64), meta.get("total_tokens").and_then(Value::as_i64), meta.get("fallback_attempts").and_then(Value::as_str).unwrap_or("")],
                )?;
                conn.execute("UPDATE conversations SET updated_at=?1 WHERE id=?2", params![now, cid])?;
                Ok(())
            })();
            if let Err(e) = log_result {
                return json_error(StatusCode::INTERNAL_SERVER_ERROR, format!("AI replied, but saving the response failed: {e}"));
            }
            Json(json!({"ok": true, "answer": answer, "meta": meta})).into_response()
        }
        Err(error) => {
            let latency = started.elapsed().as_millis() as i64;
            let cfg = load_config(&state);
            let _ = open_db(&state.database_path).and_then(|conn| conn.execute(
                "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,error) VALUES(?1,?2,?3,'router / unknown','error',?4,?5)",
                params![now_seconds(), cid, cfg.model, latency, error.chars().take(2500).collect::<String>()],
            ));
            (StatusCode::BAD_GATEWAY, Json(json!({"ok": false, "error": error, "meta": {"latency_ms": latency}}))).into_response()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("host-token") {
        let (config_path, _, _) = paths();
        let token_path = config_path.with_file_name("host-auth.json");
        let store = host_auth::HostTokenStore::new(token_path);
        host_auth::run_cli(&args[1..], &store)?;
        return Ok(());
    }

    let (config_path, database_path, static_dir) = paths();
    let state = AppState {
        config_path,
        database_path,
        static_dir,
        client: Client::builder().timeout(Duration::from_secs(120)).build()?,
    };
    ensure_setup(&state)?;
    let port: u16 = env::var("FREE_AI_STUDIO_PORT")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_PORT);
    let address = format!("{HOST}:{port}");
    let listener = TcpListener::bind(&address).await?;
    let app = Router::new()
        .route("/", get(root))
        .route("/static/*file", get(static_file))
        .route("/api/config", get(get_config).post(post_config))
        .route("/api/conversations", get(list_conversations).post(create_conversation))
        .route("/api/conversations/:id/messages", get(get_messages))
        .route("/api/conversations/:id/delete", post(delete_conversation))
        .route("/api/usage", get(usage))
        .route("/api/health", get(health))
        .route("/api/test", post(test_router))
        .route("/api/chat", post(chat))
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
        let remote_router = host_security::harden_router(host_api::build_router(state.clone(), token_store));
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

    let signal_tx = shutdown_tx.clone();
    let local_result = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = signal_tx.send(true);
        println!("Stopping Free AI Studio.");
    }).await;
    let _ = shutdown_tx.send(true);

    if let Some(task) = remote_task {
        match task.await? {
            Ok(()) => {},
            Err(message) => return Err(io::Error::new(io::ErrorKind::Other, message).into()),
        }
    }
    local_result?;
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
        assert_eq!(candidates[3], PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static"));
    }

    #[test]
    fn static_dir_candidates_fall_back_to_source_tree() {
        let candidates = static_dir_candidates(None, None);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].join("index.html").is_file());
    }
}
