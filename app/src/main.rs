use axum::{
    body::Body,
    extract::{Path, Path as AxumPath, Query as AxumQuery, State},
    http::{header, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use reqwest::{Client, Url};
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
mod breaker;
mod workflow;
mod browser;
mod capabilities;
mod context;
mod cost;
mod deepsearch;
mod diagnostics;
mod embeddings;
mod events;
mod extension_run;
mod extensions;
mod filelock;
mod git;
#[cfg(feature = "gui")]
mod gui;
mod health;
mod host_api;
mod host_auth;
mod host_security;
mod host_transport;
mod index;
mod installer;
mod keychain;
mod local;
mod mcp;
mod mcp_oauth;
mod media;
mod memory;
mod model_scope;
mod openai;
mod oplog;
mod permissions;
mod trace;
mod privacy;
mod router;
mod router_console;
mod router_env;
mod router_registry;
mod runner;
mod skills;
mod terminal;
mod thinking;
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
    /// Model used to embed code-index symbols for semantic search. Empty means
    /// the word search is the only one, which is what every install has today.
    #[serde(default)]
    embedding_model: String,
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
    /// Attach a matching skill automatically when a message picks none.
    #[serde(default = "default_auto_skills")]
    auto_select_skills: bool,
    /// Deep-search backend: "", "exa" or "tavily".
    #[serde(default)]
    search_provider: String,
    /// Search API key for the deep-search loop (server-side only).
    #[serde(default)]
    search_api_key: String,
    /// Workspace-relative path prefixes (or exact names) never sent to any
    /// provider, e.g. ".env", "secrets/".
    #[serde(default)]
    privacy_exclusions: Vec<String>,
    /// Opt-in adaptive routing for agent runs: cheap_model decides routine
    /// reading/inspection steps, strong_model decides planning, edits and
    /// post-failure repair. Off (and empty tiers) = unchanged behavior.
    #[serde(default)]
    adaptive_routing: bool,
    #[serde(default)]
    cheap_model: String,
    #[serde(default)]
    strong_model: String,
    /// Local inference endpoint (Ollama): localhost-only, never a remote URL.
    #[serde(default = "default_local_base_url")]
    local_base_url: String,
    /// Local model the chat and agent runs should use when local mode is on.
    #[serde(default)]
    local_model: String,
    /// When on, chat/run requests go ONLY to the local runtime.
    #[serde(default)]
    local_chat: bool,
    /// Native OpenAI Platform key, kept apart from the router key so a
    /// platform pick can never accidentally use the gateway's.
    #[serde(default)]
    openai_api_key: String,
    #[serde(default = "default_openai_base_url")]
    openai_base_url: String,
    /// Thinking default for every turn without a more specific setting.
    #[serde(default)]
    thinking_level: String,
    /// Thinking default for agent runs (agent scope, below message scope).
    #[serde(default)]
    run_thinking_level: String,
    /// Model default for agent runs (agent scope): below a run's own model,
    /// above the project and global defaults.
    #[serde(default)]
    agent_model: String,
    /// Ordered fallback models tried when the chosen one is unavailable
    /// (rate limit, 5xx, timeout, transport failure). Never on a 4xx refusal.
    #[serde(default)]
    fallback_models: Vec<String>,
    /// Money allowed in a rolling 24-hour window, in USD. Zero means no limit,
    /// which is the default: a fresh install must never block its first turn.
    #[serde(default)]
    daily_budget_usd: f64,
    /// How many agent runs may execute at the same time. One by default: runs
    /// share the open workspace, and there is no per-file locking yet, so two
    /// agents editing the same folder is a conflict waiting to happen. Queued
    /// runs start as slots free.
    #[serde(default = "default_max_concurrent_runs")]
    max_concurrent_runs: i64,
}

fn default_max_concurrent_runs() -> i64 {
    // Two runs at once is the default only where each run gets its own checkout;
    // the dispatcher still clamps a non-git folder to one.
    2
}

fn default_auto_skills() -> bool {
    true
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
            embedding_model: String::new(),
            context_budget_tokens: default_context_budget_tokens(),
            history_messages: default_history_messages(),
            send_workspace_outline: true,
            active_project: String::new(),
            disabled_skills: Vec::new(),
            auto_select_skills: true,
            search_provider: String::new(),
            search_api_key: String::new(),
            privacy_exclusions: Vec::new(),
            adaptive_routing: false,
            cheap_model: String::new(),
            strong_model: String::new(),
            local_base_url: default_local_base_url(),
            local_model: String::new(),
            local_chat: false,
            openai_api_key: String::new(),
            openai_base_url: default_openai_base_url(),
            thinking_level: String::new(),
            run_thinking_level: String::new(),
            agent_model: String::new(),
            fallback_models: Vec::new(),
            daily_budget_usd: 0.0,
            max_concurrent_runs: default_max_concurrent_runs(),
        }
    }
}

fn default_local_base_url() -> String {
    "http://127.0.0.1:11434".to_string()
}

fn default_openai_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

/// A menu pick of the form "local:<model>" forces local routing for that
/// turn. Pure helper shared by the chat and stream handlers.
fn local_forced_model(chosen: &str) -> Option<String> {
    chosen
        .strip_prefix("local:")
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
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
    let _ = migrate_project_thinking(&conn);
    let _ = migrate_project_model(&conn);
    let _ = migrate_request_log_costs(&conn);
    let _ = migrate_conversation_model(&conn);
    let _ = permissions::initialize(&conn);
    let _ = migrate_permission_request_id(&conn);
    let _ = skills::initialize(&conn);
    let _ = memory::initialize(&conn);
    let _ = embeddings::initialize(&conn);
    let _ = index::initialize(&conn);
    if let Err(error) = agent::initialize(&conn) {
        println!("agent run tables: {error}");
    }
    if let Err(error) = workflow::initialize(&conn) {
        println!("workflow tables: {error}");
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

/// Give projects the per-project Thinking default (`''` = inherit the global
/// setting), so the column exists on fresh and upgraded databases alike.
fn migrate_project_thinking(conn: &Connection) -> rusqlite::Result<()> {
    let has_column = {
        let mut stmt = conn.prepare("PRAGMA table_info(projects)")?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names.iter().any(|name| name == "thinking")
    };
    if !has_column {
        conn.execute_batch("ALTER TABLE projects ADD COLUMN thinking TEXT NOT NULL DEFAULT '';")?;
    }
    Ok(())
}

/// Give conversations their own model, so reopening a thread restores the model
/// it actually ran on instead of falling back to whatever the default is today.
fn migrate_conversation_model(conn: &Connection) -> rusqlite::Result<()> {
    let has_column = {
        let mut stmt = conn.prepare("PRAGMA table_info(conversations)")?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names.iter().any(|name| name == "model")
    };
    if !has_column {
        conn.execute_batch("ALTER TABLE conversations ADD COLUMN model TEXT NOT NULL DEFAULT '';")?;
    }
    Ok(())
}

/// Give `permission_audit` the id of the HTTP call that caused the decision, so
/// a refusal can be matched to the action a person took instead of only to a
/// target and a time. An old row keeps `''`: it was written before ids existed
/// and guessing one would be a fabrication.
fn migrate_permission_request_id(conn: &Connection) -> rusqlite::Result<()> {
    let names = {
        let mut stmt = conn.prepare("PRAGMA table_info(permission_audit)")?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names
    };
    if !names.is_empty() && !names.iter().any(|name| name == "request_id") {
        conn.execute_batch(
            "ALTER TABLE permission_audit ADD COLUMN request_id TEXT NOT NULL DEFAULT '';",
        )?;
    }
    Ok(())
}

/// Give `request_log` its cost columns (`NULL` price / no basis until a provider
/// publishes one), so an upgraded database and a fresh one share a schema.
fn migrate_request_log_costs(conn: &Connection) -> rusqlite::Result<()> {
    let names = {
        let mut stmt = conn.prepare("PRAGMA table_info(request_log)")?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names
    };
    if !names.iter().any(|name| name == "cost_usd") {
        conn.execute_batch("ALTER TABLE request_log ADD COLUMN cost_usd REAL;")?;
    }
    if !names.iter().any(|name| name == "cost_basis") {
        conn.execute_batch("ALTER TABLE request_log ADD COLUMN cost_basis TEXT;")?;
    }
    Ok(())
}

/// Give projects the per-project model default (`''` = inherit the agent or
/// global setting), so one workspace can run on a different model without
/// changing the app-wide choice.
fn migrate_project_model(conn: &Connection) -> rusqlite::Result<()> {
    let has_column = {
        let mut stmt = conn.prepare("PRAGMA table_info(projects)")?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names.iter().any(|name| name == "model")
    };
    if !has_column {
        conn.execute_batch("ALTER TABLE projects ADD COLUMN model TEXT NOT NULL DEFAULT '';")?;
    }
    Ok(())
}

/// A project's model default from a request body: trimmed, capped, and empty to
/// inherit. The id itself is opaque here — whether it can be called is decided
/// when a turn asks for it, with the provider's own answer.
fn project_model_input(input: &Value) -> String {
    input
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .take(200)
        .collect()
}

/// The active project's model default, or empty when it inherits. A read
/// failure means "inherit": a turn is never blocked by this lookup.
fn project_model(state: &AppState, project_id: &str) -> String {
    if project_id.trim().is_empty() {
        return String::new();
    }
    open_db(&state.database_path)
        .and_then(|conn| {
            conn.query_row(
                "SELECT model FROM projects WHERE id=?1",
                [project_id],
                |row| row.get::<_, String>(0),
            )
        })
        .optional()
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// A project's Thinking default from a request body: one of the four levels, or
/// empty to inherit.
fn project_thinking_input(input: &Value) -> String {
    input
        .get("thinking")
        .and_then(Value::as_str)
        .and_then(thinking::ThinkingLevel::parse)
        .map(|level| level.as_str().to_string())
        .unwrap_or_default()
}

/// The active project's Thinking default, or empty when it inherits. A read
/// failure means "inherit": a chat request is never blocked by this.
fn project_thinking_level(state: &AppState, project_id: &str) -> String {
    if project_id.trim().is_empty() {
        return String::new();
    }
    open_db(&state.database_path)
        .and_then(|conn| {
            conn.query_row(
                "SELECT thinking FROM projects WHERE id=?1",
                [project_id],
                |row| row.get::<_, Option<String>>(0),
            )
        })
        .ok()
        .flatten()
        .unwrap_or_default()
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
    // Migration, idempotent: any credential still sitting as plaintext in
    // config.json (pre-keychain stores and upgrades) moves into the OS
    // credential store once, and the empty skipped runs cost one file read.
    migrate_secrets_to_keychain(state);
    Ok(())
}

/// True when a plaintext secret is still sitting in the config file.
fn config_holds_plaintext_secrets(state: &AppState) -> bool {
    // Deliberately NOT `load_config`: that merges the keychain, and a merged
    // value must not look like plaintext on disk.
    let raw = fs::read(&state.config_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Config>(&bytes).ok());
    match raw {
        Some(cfg) => {
            !cfg.api_key.trim().is_empty()
                || !cfg.search_api_key.trim().is_empty()
                || !cfg.openai_api_key.trim().is_empty()
        }
        None => false,
    }
}

/// One-shot migration of plaintext secrets into the keychain: the save path
/// does the split (write to the store, blank the field), so a store failure
/// leaves the existing file untouched and the next startup retries.
fn migrate_secrets_to_keychain(state: &AppState) {
    if !config_holds_plaintext_secrets(state) {
        return;
    }
    let cfg = load_config(state);
    let _ = save_config_file(&state.config_path, &cfg);
}

pub(crate) fn load_config(state: &AppState) -> Config {
    let mut cfg = fs::read(&state.config_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Config>(&bytes).ok())
        .unwrap_or_else(Config::defaults);
    merge_keychain_secrets(state, &mut cfg);
    cfg
}

/// The keychain location identity for this app instance: the directory the
/// config lives in. Mock-mode tests hash it into the service name so parallel
/// fixtures never share credentials; production lands on the stable default
/// service every time.
pub(crate) fn keychain_location(path: &FsPath) -> String {
    path.parent()
        .and_then(|dir| dir.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

/// Move secrets out of the JSON before it is written: each non-empty one
/// goes to the OS credential store and is blanked from the record. When the
/// store rejects the write, the value stays in the JSON — the file is
/// permission-restricted and keeps the pre-keychain behavior — so a broken
/// keychain degrades to plain config.json instead of losing a key.
fn split_keychain_secrets(path: &FsPath, cfg: &mut Config) {
    let location = keychain_location(path);
    for (name, secret) in [
        ("api_key", &mut cfg.api_key),
        ("search_api_key", &mut cfg.search_api_key),
        ("openai_api_key", &mut cfg.openai_api_key),
    ] {
        if secret.is_empty() {
            continue;
        }
        if keychain::write_secret(&location, name, secret).is_ok() {
            secret.clear();
        }
    }
}

/// Stored credentials win over the file, because the file is the legacy
/// carrier: a store read error keeps whatever the file holds rather than
/// dropping the secret.
fn merge_keychain_secrets(state: &AppState, cfg: &mut Config) {
    let location = keychain_location(&state.config_path);
    for name in keychain::SECRET_FIELDS {
        let stored = match keychain::read_secret(&location, name) {
            Ok(Some(value)) => value,
            _ => continue,
        };
        match name {
            "api_key" => cfg.api_key = stored,
            "search_api_key" => cfg.search_api_key = stored,
            "openai_api_key" => cfg.openai_api_key = stored,
            _ => {}
        }
    }
}

pub(crate) fn save_config_file(path: &FsPath, original: &Config) -> io::Result<()> {
    let mut cfg = original.clone();
    split_keychain_secrets(path, &mut cfg);
    let bytes = serde_json::to_vec_pretty(&cfg)
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
    cfg.embedding_model = cfg.embedding_model.trim().to_string();
    cfg.search_provider = match cfg.search_provider.to_lowercase().as_str() {
        "exa" | "tavily" => cfg.search_provider.to_lowercase(),
        _ => String::new(),
    };
    cfg.openai_api_key = cfg.openai_api_key.trim().to_string();
    cfg.openai_base_url = cfg.openai_base_url.trim().trim_end_matches('/').to_string();
    if cfg.openai_base_url.is_empty() {
        cfg.openai_base_url = default_openai_base_url();
    }
    if !cfg.openai_base_url.starts_with("https://") && !cfg.openai_base_url.starts_with("http://") {
        return Err("The OpenAI Platform URL must start with https://.".into());
    }
    // A level nobody can parse is "unset" (provider default), never a guess.
    cfg.thinking_level = thinking::ThinkingLevel::parse(&cfg.thinking_level)
        .map(|level| level.as_str().to_string())
        .unwrap_or_default();
    cfg.run_thinking_level = thinking::ThinkingLevel::parse(&cfg.run_thinking_level)
        .map(|level| level.as_str().to_string())
        .unwrap_or_default();
    // Model ids are opaque here; what is normalised is the shape of the chain:
    // trimmed, deduplicated, capped, and free of local picks a router cannot
    // route.
    cfg.agent_model = cfg.agent_model.trim().to_string();
    cfg.fallback_models = model_scope::normalised(&cfg.fallback_models);
    // A negative budget is meaningless; zero stays "no limit".
    cfg.daily_budget_usd = cfg.daily_budget_usd.max(0.0);
    // Concurrency is a range, not a wish: zero active runs would stall the
    // queue forever, and past four the runs compete for one workspace anyway.
    cfg.max_concurrent_runs = cfg.max_concurrent_runs.clamp(1, 4);

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
        message += "\n\nOmniRoute (port 20128) is not running. Start it in the Integrations view, or install it once from the Router panel in Studio.";
    } else if base_url.contains("localhost:3001") {
        message += "\n\nFreeLLMAPI (port 3001) is not running. Start the Docker Compose router (`cd ~/freellmapi && sudo docker compose up -d`), then try again.";
    }
    message
}

/// Whether the router's catalog says this model accepts reasoning parameters.
/// `None` means the catalog does not say (no metadata, or it could not be
/// read), which is not evidence either way.
/// The catalog row for one model, from the same one-minute cache the model menu
/// fills. `None` when the catalog cannot be read or does not list the id.
async fn catalog_row(state: &AppState, cfg: &Config, id: &str) -> Option<Value> {
    let from_list = |models: &Value| -> Option<Value> {
        models
            .as_array()?
            .iter()
            .find(|model| model.get("id").and_then(Value::as_str) == Some(id))
            .cloned()
    };
    let cache_key = format!(
        "{}|{}",
        cfg.base_url.trim_end_matches('/'),
        !cfg.api_key.is_empty()
    );
    let cached = state.models_cache.lock().ok().and_then(|cache| {
        cache
            .get(&cache_key)
            .map(|(at, models)| (at.elapsed(), models.clone()))
    });
    if let Some((elapsed, models)) = cached {
        if elapsed < Duration::from_secs(60) {
            return from_list(&models);
        }
    }
    let models = json!(fetch_models(state, cfg).await.ok()?);
    let answer = from_list(&models);
    if let Ok(mut cache) = state.models_cache.lock() {
        cache.insert(cache_key, (std::time::Instant::now(), models));
    }
    answer
}

async fn catalog_reasoning(state: &AppState, cfg: &Config, id: &str) -> Option<bool> {
    let row = catalog_row(state, cfg, id).await?;
    row.pointer("/capabilities/reasoning")
        .and_then(Value::as_bool)
}

/// The price the provider publishes for a model, read from that same row. `None`
/// when it publishes none — a cost is never assumed.
async fn catalog_price(state: &AppState, cfg: &Config, id: &str) -> Option<capabilities::Price> {
    let row = catalog_row(state, cfg, id).await?;
    serde_json::from_value(row.pointer("/capabilities/pricing")?.clone()).ok()
}

/// What a recorded request cost: the provider's published price for the model
/// that actually answered, times the tokens it reported. `None` unless both
/// exist. Native platform picks have no router catalog row, so they stay
/// unpriced rather than guessed.
async fn priced_cost(state: &AppState, cfg: &Config, meta: &Value) -> Option<cost::Cost> {
    let model = meta.get("model").and_then(Value::as_str)?;
    if openai::platform_model(model).is_some() {
        return None;
    }
    let price = catalog_price(state, cfg, model).await?;
    cost::estimate(
        &price,
        meta.get("prompt_tokens").and_then(Value::as_i64),
        meta.get("completion_tokens").and_then(Value::as_i64),
    )
}

/// The budget window is a rolling 24 hours: timestamps are stored in UTC and
/// there is no user timezone to cut a calendar day with.
const BUDGET_WINDOW_SECONDS: f64 = 86_400.0;

/// Money spent in the window, plus how many successful requests had no price.
fn spend_window(state: &AppState, seconds: f64) -> cost::Spend {
    let since = now_seconds() - seconds;
    open_db(&state.database_path)
        .and_then(|conn| {
            conn.query_row(
                "SELECT COALESCE(SUM(cost_usd),0), SUM(CASE WHEN cost_usd IS NULL AND status='ok' THEN 1 ELSE 0 END) FROM request_log WHERE created_at>=?1",
                [since],
                |r| Ok((r.get::<_, f64>(0)?, r.get::<_, Option<i64>>(1)?.unwrap_or(0))),
            )
        })
        .map(|(usd, unknown)| cost::Spend { usd, unknown })
        .unwrap_or(cost::Spend { usd: 0.0, unknown: 0 })
}

/// Circuit state is per workspace: two configs in one process must not share a
/// cooldown, and the tests depend on that isolation.
pub(crate) fn breaker_scope(state: &AppState) -> String {
    state.database_path.display().to_string()
}

/// The gate every paid call goes through. Local inference is not a paid provider
/// call, so local mode and `local:` picks never reach it.
fn budget_gate(state: &AppState, cfg: &Config) -> Option<String> {
    if cfg.daily_budget_usd <= 0.0 {
        return None;
    }
    let spend = spend_window(state, BUDGET_WINDOW_SECONDS);
    cost::budget_refusal(cfg.daily_budget_usd, &spend)
}

/// The thinking block a chat response reports: the level in force, where it
/// came from, and what it does. `null` means the provider default applied.
fn thinking_json(resolved: Option<thinking::Resolved>) -> Value {
    match resolved {
        Some(resolved) => json!({
            "level": resolved.level.as_str(),
            "scope": resolved.scope.as_str(),
            "note": resolved.level.describe(),
        }),
        None => Value::Null,
    }
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

/// Opt-in rule-based tier choice for agent steps. Cheap follows routine
/// inspection and summary steps; strong handles planning, edits and repair.
/// Returns None when adaptive routing is off or either tier is unconfigured —
/// the run then uses the base model exactly as before the feature existed.
pub(crate) fn adaptive_tier(
    state: &AppState,
    last_action: &str,
    had_error: bool,
) -> Option<String> {
    let cfg = load_config(state);
    choose_tier(&cfg, last_action, had_error)
}

/// The rule itself, pure so the tier matrix is testable without a live config.
fn choose_tier(cfg: &Config, last_action: &str, had_error: bool) -> Option<String> {
    if !cfg.adaptive_routing
        || cfg.cheap_model.trim().is_empty()
        || cfg.strong_model.trim().is_empty()
    {
        return None;
    }
    // First step and repairs are planning work, never cheap.
    let routine = !had_error
        && matches!(
            last_action,
            "list" | "read" | "think" | "restored" | "failover"
        );
    if routine {
        Some(cfg.cheap_model.trim().to_string())
    } else {
        Some(cfg.strong_model.trim().to_string())
    }
}

/// Snapshot the routing config at run start for the persisted run record,
/// paired with the base model the run starts on. The model is resolved through
/// the same precedence chat uses — task, agent, project, global — so the record
/// says which scope produced it instead of just repeating one setting.
pub(crate) fn run_record_snapshot(state: &AppState, task_model: Option<&str>) -> (String, String) {
    let cfg = load_config(state);
    let project = project_model(state, &cfg.active_project);
    let resolved = model_scope::resolve(
        task_model,
        Some(&cfg.agent_model),
        Some(&project),
        Some(&cfg.model),
    );
    let snapshot = json!({
        "base_url": cfg.base_url,
        "adaptive_routing": cfg.adaptive_routing,
        "cheap_model": cfg.cheap_model,
        "strong_model": cfg.strong_model,
        "local": cfg.local_chat,
        "local_model": cfg.local_model,
        "agent_model": cfg.agent_model,
        "project_model": project,
        "fallback_models": cfg.fallback_models,
        "resolved_model": resolved.model,
        "resolved_scope": resolved.scope.as_str(),
    });
    (snapshot.to_string(), resolved.model)
}

/// The one model seam for chat and agent runs: in Local AI mode every call
/// goes strictly to the configured local runtime (never to the remote
/// router, even when the local call fails); otherwise the remote router is
/// used as before.
pub(crate) async fn call_model(
    state: &AppState,
    messages: Vec<Value>,
    model_override: Option<&str>,
) -> Result<(String, Value), String> {
    let cfg = load_config(state);
    if cfg.local_chat {
        if cfg.local_model.trim().is_empty() {
            return Err(
                "Local AI mode is on but no local model is selected — pick one on the Local AI page.".to_string(),
            );
        }
        let base = local::effective_base_url(&cfg.local_base_url);
        let (answer, tokens) = local::chat_once(&base, &cfg.local_model, &messages).await?;
        let meta = json!({
            "model": cfg.local_model,
            "provider": "local",
            "completion_tokens": if tokens >= 0 { json!(tokens) } else { json!(null) },
        });
        return Ok((answer, meta));
    }
    call_router(state, messages, model_override).await
}

pub(crate) async fn call_router(
    state: &AppState,
    messages: Vec<Value>,
    model_override: Option<&str>,
) -> Result<(String, Value), String> {
    // Agent scope: runs resolve their Thinking default here, below any
    // per-message setting and above the project/global defaults chat uses.
    let level = thinking::ThinkingLevel::parse(&load_config(state).run_thinking_level);
    // A call without an explicit model (a plan or guidance request) takes the
    // agent scope, then the project default, then the global one.
    let model = match model_override {
        Some(model) => model.to_string(),
        None => {
            let cfg = load_config(state);
            let project = project_model(state, &cfg.active_project);
            model_scope::resolve(
                None,
                Some(&cfg.agent_model),
                Some(&project),
                Some(&cfg.model),
            )
            .model
        }
    };
    call_router_with(state, messages, Some(&model), level).await
}

/// The same request with the Thinking level the caller resolved, tried across
/// the configured fallback chain: the chosen model first, then each fallback, but
/// only when the failure was the provider being unavailable (see
/// `model_scope::retryable`). A `fallbacks` array lands in the meta whenever a
/// later model answered.
pub(crate) async fn call_router_with(
    state: &AppState,
    messages: Vec<Value>,
    model_override: Option<&str>,
    level: Option<thinking::ThinkingLevel>,
) -> Result<(String, Value), String> {
    let cfg = load_config(state);
    // Checked once, before the chain: a spent budget must not be circumvented by
    // trying the next model.
    if let Some(refusal) = budget_gate(state, &cfg) {
        return Err(refusal);
    }
    let resolved = model_scope::resolve(model_override, None, None, Some(&cfg.model));
    let chain = model_scope::chain(&resolved.model, &cfg.fallback_models);
    let scope = breaker_scope(state);
    let mut tried: Vec<Value> = Vec::new();
    let mut last_error = String::new();
    let mut skipped = 0usize;
    for (index, candidate) in chain.iter().enumerate() {
        // A model whose circuit is open is not asked at all: the point of the
        // breaker is to stop paying for the same outage on every turn.
        if !breaker::allow(&scope, candidate) {
            skipped += 1;
            tried.push(json!({ "model": candidate, "error": "circuit open: skipped" }));
            continue;
        }
        match attempt_router_once(state, candidate, messages.clone(), level).await {
            Ok((answer, mut meta)) => {
                breaker::success(&scope, candidate);
                if !tried.is_empty() {
                    meta["fallbacks"] = json!(tried);
                }
                return Ok((answer, meta));
            }
            Err(error) => {
                if model_scope::retryable(&error) {
                    breaker::failure(&scope, candidate, &error);
                }
                // A rejected request fails identically on every model; only an
                // unavailable provider is worth the next candidate.
                if index + 1 == chain.len() || !model_scope::retryable(&error) {
                    return Err(error);
                }
                tried.push(json!({ "model": candidate, "error": error }));
                last_error = error;
            }
        }
    }
    if skipped == chain.len() {
        return Err(breaker::all_open_message(&scope, &chain));
    }
    Err(last_error)
}

/// One request to one model. A platform pick goes to the Responses API; a
/// router pick carries `reasoning_effort` only when the router's own catalog
/// says the model accepts it. The fallback loop calls this once per candidate,
/// so it reads nothing about the chain itself.
async fn attempt_router_once(
    state: &AppState,
    chosen: &str,
    messages: Vec<Value>,
    level: Option<thinking::ThinkingLevel>,
) -> Result<(String, Value), String> {
    let cfg = load_config(state);
    let chosen = chosen.trim().to_string();

    if let Some(platform_model) = openai::platform_model(&chosen) {
        if cfg.openai_api_key.trim().is_empty() {
            return Err("Add your OpenAI Platform key in Settings to use this model.".into());
        }
        let started = Instant::now();
        let body = openai::request_body(&platform_model, &messages, level, false);
        let (answer, mut meta) = openai::responses_once(
            &state.client,
            &cfg.openai_base_url,
            &cfg.openai_api_key,
            body,
        )
        .await?;
        meta["latency_ms"] = json!(started.elapsed().as_millis() as i64);
        return Ok((answer, meta));
    }

    if cfg.api_key.trim().is_empty() {
        return Err("Add your router's unified API key in Settings first.".into());
    }
    let mut request_body = json!({
        "model": &chosen,
        "messages": messages,
        "stream": false
    });
    if let Some(level) = level {
        if let Some(effort) = thinking::completions_reasoning(level) {
            match catalog_reasoning(state, &cfg, &chosen).await {
                Some(true) => request_body["reasoning_effort"] = json!(effort),
                Some(false) => {
                    return Err(format!(
                        "{chosen} does not advertise reasoning, so it cannot run with Thinking {}. Choose a reasoning model or set Thinking to Off.",
                        level.as_str()
                    ))
                }
                // Unknown: send nothing rather than guess at a parameter the
                // provider may reject.
                None => {}
            }
        }
    }

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
    // What the provider says it used, or the model this attempt asked for —
    // never the global default, which a fallback attempt did not use.
    let model = parsed
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(chosen.as_str());
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

/// The first upstream stream that accepted the request, plus which models were
/// tried before it. A fallback is only ever tried before a byte of the answer
/// was read: replaying a half-streamed answer on another model would duplicate
/// text the user already saw.
struct StreamStart {
    response: reqwest::Response,
    model: String,
    fallbacks: Vec<Value>,
}

async fn open_stream(
    state: &AppState,
    cfg: &Config,
    chain: &[String],
    messages: &[Value],
    level: Option<thinking::ThinkingLevel>,
) -> Result<StreamStart, (StatusCode, String)> {
    if let Some(refusal) = budget_gate(state, cfg) {
        return Err((StatusCode::TOO_MANY_REQUESTS, refusal));
    }
    let scope = breaker_scope(state);
    let mut fallbacks: Vec<Value> = Vec::new();
    let mut skipped = 0usize;
    for (index, candidate) in chain.iter().enumerate() {
        if !breaker::allow(&scope, candidate) {
            skipped += 1;
            fallbacks.push(json!({ "model": candidate, "error": "circuit open: skipped" }));
            continue;
        }
        let mut request_body = json!({ "model": candidate, "messages": messages, "stream": true });
        if let Some(level) = level {
            if let Some(effort) = thinking::completions_reasoning(level) {
                match catalog_reasoning(state, cfg, candidate).await {
                    Some(true) => request_body["reasoning_effort"] = json!(effort),
                    Some(false) if index == 0 => {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            format!(
                                "{candidate} does not advertise reasoning, so it cannot run with Thinking {}. Choose a reasoning model or set Thinking to Off.",
                                level.as_str()
                            ),
                        ))
                    }
                    // A fallback that does not advertise reasoning, or does not
                    // say, gets no parameter: dropping it for this attempt is
                    // honest, sending one the model may reject is not.
                    Some(false) | None => {}
                }
            }
        }
        let upstream = state
            .client
            .post(router_url(cfg, "chat/completions"))
            .bearer_auth(&cfg.api_key)
            .header("accept", "text/event-stream")
            .json(&request_body)
            .send()
            .await;
        let message = match upstream {
            Err(error) => reach_error(&cfg.base_url, error.to_string()),
            Ok(response) if !response.status().is_success() => {
                let status = response.status();
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
                format!("Router returned HTTP {}: {}", status.as_u16(), detail)
            }
            Ok(response) => {
                breaker::success(&scope, candidate);
                return Ok(StreamStart {
                    response,
                    model: candidate.clone(),
                    fallbacks,
                });
            }
        };
        if model_scope::retryable(&message) {
            breaker::failure(&scope, candidate, &message);
        }
        if index + 1 == chain.len() || !model_scope::retryable(&message) {
            return Err((StatusCode::BAD_GATEWAY, message));
        }
        fallbacks.push(json!({ "model": candidate, "error": message }));
    }
    if skipped == chain.len() {
        return Err((
            StatusCode::BAD_GATEWAY,
            breaker::all_open_message(&scope, chain),
        ));
    }
    Err((
        StatusCode::BAD_GATEWAY,
        "Every configured model was unavailable.".to_string(),
    ))
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
        "svg" => "image/svg+xml",
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
        "keychain": keychain::current_mode().as_str(),
        "base_url": cfg.base_url,
        "model": cfg.model,
        "embedding_model": cfg.embedding_model,
        "has_api_key": !cfg.api_key.is_empty(),
        "context_budget_tokens": cfg.context_budget_tokens,
        "history_messages": cfg.history_messages,
        "send_workspace_outline": cfg.send_workspace_outline,
        "search_provider": cfg.search_provider,
        "has_search_key": !cfg.search_api_key.is_empty(),
        "privacy_exclusions": cfg.privacy_exclusions,
        "adaptive_routing": cfg.adaptive_routing,
        "cheap_model": cfg.cheap_model,
        "strong_model": cfg.strong_model,
        "local_base_url": cfg.local_base_url,
        "local_model": cfg.local_model,
        "local_chat": cfg.local_chat,
        "has_openai_key": !cfg.openai_api_key.is_empty(),
        "openai_base_url": cfg.openai_base_url,
        "thinking_level": cfg.thinking_level,
        "run_thinking_level": cfg.run_thinking_level,
        "agent_model": cfg.agent_model,
        "fallback_models": cfg.fallback_models,
        "daily_budget_usd": cfg.daily_budget_usd,
        "max_concurrent_runs": cfg.max_concurrent_runs,
    }))
}

/// The Thinking levels the composer offers, with what each one actually asks
/// the provider for, so the control never describes an effect it does not have.
async fn thinking_levels() -> Json<Value> {
    let levels = [
        thinking::ThinkingLevel::Off,
        thinking::ThinkingLevel::Low,
        thinking::ThinkingLevel::Medium,
        thinking::ThinkingLevel::High,
    ];
    Json(json!({
        "ok": true,
        "levels": levels
            .iter()
            .map(|level| json!({
                "id": level.as_str(),
                "effort": level.effort(),
                "description": level.describe(),
            }))
            .collect::<Vec<_>>(),
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

/// Every registered router with its measured state. Each field comes from the
/// probe that already backs /api/router, /api/omniroute and /api/local/status.
async fn routers(State(state): State<AppState>) -> Json<Value> {
    let routers = router_registry::views(&state).await;
    // Measured once per list call: package managers, Docker and compose
    // versions are what the install plans depend on, so the panel can explain
    // a blocked install with facts instead of a guess. Blocking subprocesses
    // stay off the async worker.
    let environment = tokio::task::spawn_blocking(router_env::environment)
        .await
        .ok()
        .map(|env| env.to_json());
    Json(json!({
        "platform": {
            "os": router_registry::current_os(),
            "arch": router_registry::current_arch(),
        },
        "environment": environment,
        "routers": routers,
    }))
}

/// The router's editable configuration files, or the reason there is nothing
/// to edit (a container-only router, or a runtime Studio configures elsewhere).
async fn routers_config(Path(id): Path<String>) -> Response<Body> {
    match router_env::config_dir(&id) {
        Ok(dir) => {
            let files = router_env::list_in(&dir);
            Json(json!({
                "ok": true,
                "id": id,
                "directory": dir.to_string_lossy(),
                "files": files,
                "editable": router_env::EDITABLE_FILES,
            }))
            .into_response()
        }
        Err(error) if error.starts_with("Unknown router id") => {
            json_error(StatusCode::NOT_FOUND, error)
        }
        Err(error) => Json(json!({"ok": false, "error": error, "files": []})).into_response(),
    }
}

/// One whitelisted file's text. Anything outside the whitelist is refused by
/// the same validation the write path uses, so neither direction can leave the
/// directory the install recorded.
async fn routers_config_file(
    Path(id): Path<String>,
    AxumQuery(params): AxumQuery<HashMap<String, String>>,
) -> Response<Body> {
    let name = params
        .get("name")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    if name.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "Pass ?name=<config file>.");
    }
    match router_env::config_dir(&id) {
        Ok(dir) => match router_env::read_in(&dir, &name) {
            Ok(content) => Json(json!({
                "ok": true,
                "id": id,
                "name": name,
                "path": dir.join(&name).to_string_lossy(),
                "content": content,
                "bytes": content.len(),
            }))
            .into_response(),
            Err(error) => json_error(StatusCode::BAD_REQUEST, error),
        },
        Err(error) if error.starts_with("Unknown router id") => {
            json_error(StatusCode::NOT_FOUND, error)
        }
        Err(error) => json_error(StatusCode::BAD_REQUEST, error),
    }
}

/// Save one whitelisted file. Gated on `fs_write` for the exact path, so a
/// saved rule can freeze the router's compose file while leaving the rest of
/// the workspace writable.
async fn routers_save_config_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let Some(content) = input.get("content").and_then(Value::as_str) else {
        return json_error(StatusCode::BAD_REQUEST, "Pass the file's new content.");
    };
    let dir = match router_env::config_dir(&id) {
        Ok(dir) => dir,
        Err(error) if error.starts_with("Unknown router id") => {
            return json_error(StatusCode::NOT_FOUND, error)
        }
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    if let Err(error) = router_env::validate_name(&name) {
        return json_error(StatusCode::BAD_REQUEST, error);
    }
    let target = dir.join(&name);
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(response) = permissions::guard_user(
        &state,
        permissions::Action::FsWrite,
        &target.to_string_lossy(),
        "",
        approve,
        "save the router's configuration",
    ) {
        return response;
    }
    match router_env::write_in(&dir, &name, content) {
        Ok(bytes) => Json(json!({
            "ok": true,
            "id": id,
            "name": name,
            "path": target.to_string_lossy(),
            "bytes": bytes,
        }))
        .into_response(),
        Err(error) => json_error(StatusCode::BAD_REQUEST, error),
    }
}

/// Connection tester: a real request to the router's documented health path,
/// reporting latency and status instead of only healthy/unhealthy.
async fn routers_test(State(state): State<AppState>, Path(id): Path<String>) -> Response<Body> {
    match router_registry::test_connection(&state, &id).await {
        Ok(result) => Json(serde_json::to_value(result).unwrap_or(Value::Null)).into_response(),
        Err(error) if error.starts_with("Unknown router id") => {
            json_error(StatusCode::NOT_FOUND, error)
        }
        Err(error) => json_error(StatusCode::BAD_GATEWAY, error),
    }
}

/// What installing a router would run, with measured prerequisites. No side
/// effects: this is the plan an approval names.
async fn routers_install_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    match router_registry::install_plan(&state, &id).await {
        Ok(plan) => Json(serde_json::to_value(plan).unwrap_or(Value::Null)).into_response(),
        Err(error) => json_error(StatusCode::NOT_FOUND, error.message()),
    }
}

/// The last install this app recorded for a router, if any.
async fn routers_install_record(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    match router_registry::install_record(&state, &id) {
        Ok(record) => Json(json!({"id": id, "record": record})).into_response(),
        Err(error) => json_error(StatusCode::NOT_FOUND, error.message()),
    }
}

/// Run an install, but only for the plan the caller was shown. Every refusal is
/// answered before a single command is spawned.
async fn routers_install(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let digest = input.get("digest").and_then(Value::as_str).unwrap_or("");
    // A saved deny rule is checked before the plan is even considered: the
    // approval in the request cannot override a policy the user wrote down.
    let verdict = permissions::check(&state, permissions::Action::RouterInstall, &id, "");
    if verdict.decision == permissions::Decision::Deny {
        permissions::record(
            &state,
            permissions::Action::RouterInstall,
            &id,
            permissions::Decision::Deny,
            verdict.scope,
            "",
            "install refused by a saved rule",
        );
        return json_error(
            StatusCode::FORBIDDEN,
            format!(
                "A saved permission rule denies installing {id}. Revoke it in Settings → Permissions, then retry."
            ),
        );
    }
    let result = router_registry::install(&state, &id, approve, digest).await;
    log_install("install", &id, &result);
    match result {
        Ok(record) => {
            permissions::record(
                &state,
                permissions::Action::RouterInstall,
                &id,
                permissions::Decision::Allow,
                verdict.scope,
                "",
                "install approved and run",
            );
            Json(json!({"record": record})).into_response()
        }
        Err(router_registry::InstallError::Unknown(message)) => {
            json_error(StatusCode::NOT_FOUND, message)
        }
        Err(router_registry::InstallError::Blocked(message)) => {
            json_error(StatusCode::CONFLICT, message)
        }
        Err(router_registry::InstallError::NotApproved(message)) => {
            json_error(StatusCode::BAD_REQUEST, message)
        }
        Err(router_registry::InstallError::Failed(message)) => {
            json_error(StatusCode::BAD_GATEWAY, message)
        }
    }
}

/// Compare an install with what is published now. Read-only, and the plan it
/// carries is the one a subsequent update must be approved against.
async fn routers_update_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    match router_registry::update_status(&state, &id).await {
        Ok(status) => Json(serde_json::to_value(status).unwrap_or(Value::Null)).into_response(),
        Err(error) => json_error(StatusCode::NOT_FOUND, error.message()),
    }
}

/// Record how an install or update ended. This is the one operation that changes
/// the machine, and a failure here is what a user reports, so it goes in the
/// process log before the response is mapped.
fn log_install(
    action: &str,
    id: &str,
    result: &Result<router_registry::InstallRecord, router_registry::InstallError>,
) {
    match result {
        Ok(_) => oplog::record("info", "router", action, format!("{id} succeeded")),
        Err(error) => oplog::record(
            "error",
            "router",
            action,
            format!("{id}: {}", error.message()),
        ),
    }
}

/// One mapping for the install-shaped routes, so each refusal gets the status it
/// deserves instead of every failure looking the same.
fn install_outcome(
    result: Result<router_registry::InstallRecord, router_registry::InstallError>,
) -> Response<Body> {
    match result {
        Ok(record) => Json(json!({"record": record})).into_response(),
        Err(router_registry::InstallError::Unknown(message)) => {
            json_error(StatusCode::NOT_FOUND, message)
        }
        Err(router_registry::InstallError::Blocked(message)) => {
            json_error(StatusCode::CONFLICT, message)
        }
        Err(router_registry::InstallError::NotApproved(message)) => {
            json_error(StatusCode::BAD_REQUEST, message)
        }
        Err(router_registry::InstallError::Failed(message)) => {
            json_error(StatusCode::BAD_GATEWAY, message)
        }
    }
}

/// Replace an existing install with the plan the caller approved.
async fn routers_update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let digest = input.get("digest").and_then(Value::as_str).unwrap_or("");
    let result = router_registry::update(&state, &id, approve, digest).await;
    log_install("update", &id, &result);
    install_outcome(result)
}

/// Remove an install. `purge` is the explicit "and delete its data" switch, so
/// the default removes the container and keeps the volume.
async fn routers_uninstall(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let purge = input.get("purge").and_then(Value::as_bool).unwrap_or(false);
    if !approve {
        return json_error(
            StatusCode::BAD_REQUEST,
            "The uninstall was not approved. Send {\"approve\": true} to remove an install.",
        );
    }
    let result = router_registry::uninstall(&state, &id, purge).await;
    match &result {
        Ok(_) => oplog::record(
            "info",
            "router",
            "uninstall",
            format!("{id} removed{}", if purge { " with its data" } else { "" }),
        ),
        Err(error) => oplog::record(
            "error",
            "router",
            "uninstall",
            format!("{id}: {}", error.message()),
        ),
    }
    match result {
        Ok(outcome) => Json(serde_json::to_value(outcome).unwrap_or(Value::Null)).into_response(),
        Err(router_registry::InstallError::Unknown(message)) => {
            json_error(StatusCode::NOT_FOUND, message)
        }
        Err(error) => json_error(StatusCode::BAD_GATEWAY, error.message()),
    }
}

/// Store a router's launch preference. Only ids the registry knows are stored.
async fn routers_auto_start(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let Some(enabled) = input.get("enabled").and_then(Value::as_bool) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Send {\"enabled\": true} or {\"enabled\": false}.",
        );
    };
    match router_registry::set_auto_start(&state, &id, enabled) {
        Ok(value) => Json(json!({"id": id, "auto_start": value})).into_response(),
        Err(error) if error.starts_with("Unknown router id") => {
            json_error(StatusCode::NOT_FOUND, error)
        }
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

/// Recent output of one router's container, tail-limited by the query and by the
/// registry's own cap.
async fn routers_logs(
    Path(id): Path<String>,
    AxumQuery(params): AxumQuery<HashMap<String, String>>,
) -> Response<Body> {
    let tail = params
        .get("tail")
        .and_then(|value| value.trim().parse::<usize>().ok());
    match router_registry::logs(&id, tail).await {
        Ok(logs) => Json(serde_json::to_value(logs).unwrap_or(Value::Null)).into_response(),
        Err(error) if error.starts_with("Unknown router id") => {
            json_error(StatusCode::NOT_FOUND, error)
        }
        Err(error) => json_error(StatusCode::BAD_GATEWAY, error),
    }
}

/// Restart through the existing stop/start paths, so the ownership rules apply
/// unchanged (an OmniRoute Studio did not start is still not stopped by Studio).
async fn routers_restart(Path(id): Path<String>) -> Response<Body> {
    if router_registry::find(&id).is_none() {
        return json_error(StatusCode::NOT_FOUND, format!("Unknown router id: {id}"));
    }
    match router_registry::restart(&id).await {
        Ok(message) => {
            oplog::record("info", "router", "restart", format!("{id}: {message}"));
            Json(json!({"id": id, "restarted": true, "message": message})).into_response()
        }
        Err(error) => {
            oplog::record("error", "router", "restart", format!("{id}: {error}"));
            json_error(StatusCode::BAD_GATEWAY, error)
        }
    }
}

async fn post_config(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    // Keys travel through the same endpoint as ordinary settings, so they get
    // their own target: a rule can deny `config.key` without freezing the
    // rest of the form.
    let key_change = ["api_key", "search_api_key", "openai_api_key"]
        .iter()
        .any(|field| {
            input
                .get(*field)
                .and_then(Value::as_str)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        });
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let target = if key_change { "config.key" } else { "config" };
    if let Some(response) = permissions::guard_user(
        &state,
        permissions::Action::Settings,
        target,
        "",
        approve,
        if key_change {
            "change a saved key"
        } else {
            "save settings"
        },
    ) {
        return response;
    }
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
    if let Some(model) = input.get("embedding_model").and_then(Value::as_str) {
        cfg.embedding_model = model.trim().to_string();
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
    // Exclusions replace wholesale: the UI sends the full list, so removing a
    // path takes effect on the next send.
    if let Some(list) = input.get("privacy_exclusions").and_then(Value::as_array) {
        cfg.privacy_exclusions = list
            .iter()
            .filter_map(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(key) = input.get("search_api_key").and_then(Value::as_str) {
        if !key.trim().is_empty() {
            cfg.search_api_key = key.trim().to_string();
        }
    }
    // Adaptive routing for agent runs: replace wholesale like the other
    // scalar settings so unchecking the box turns it back off.
    if let Some(flag) = input.get("adaptive_routing").and_then(Value::as_bool) {
        cfg.adaptive_routing = flag;
    }
    if let Some(m) = input.get("cheap_model").and_then(Value::as_str) {
        cfg.cheap_model = m.trim().to_string();
    }
    if let Some(m) = input.get("strong_model").and_then(Value::as_str) {
        cfg.strong_model = m.trim().to_string();
    }
    if let Some(key) = input.get("openai_api_key").and_then(Value::as_str) {
        if !key.trim().is_empty() {
            cfg.openai_api_key = key.trim().to_string();
        }
    }
    if let Some(url) = input.get("openai_base_url").and_then(Value::as_str) {
        cfg.openai_base_url = url.trim().to_string();
    }
    // Thinking defaults replace wholesale, so clearing a select clears the
    // setting instead of leaving the old level behind.
    for (key, target) in [
        ("thinking_level", &mut cfg.thinking_level),
        ("run_thinking_level", &mut cfg.run_thinking_level),
    ] {
        if let Some(level) = input.get(key).and_then(Value::as_str) {
            *target = level.trim().to_string();
        }
    }
    if let Some(m) = input.get("agent_model").and_then(Value::as_str) {
        cfg.agent_model = m.trim().to_string();
    }
    // Fallback models replace wholesale, like the exclusion list: the UI sends
    // the whole chain, so removing one takes effect on the next turn.
    if let Some(list) = input.get("fallback_models").and_then(Value::as_array) {
        cfg.fallback_models = list
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
    }
    if let Some(budget) = input.get("daily_budget_usd").and_then(Value::as_f64) {
        cfg.daily_budget_usd = budget;
    }
    if let Some(limit) = input.get("max_concurrent_runs").and_then(Value::as_i64) {
        cfg.max_concurrent_runs = limit;
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
    Json(json!({"ok": true, "base_url": cfg.base_url, "model": cfg.model, "has_api_key": !cfg.api_key.is_empty(), "context_budget_tokens": cfg.context_budget_tokens, "history_messages": cfg.history_messages, "send_workspace_outline": cfg.send_workspace_outline, "adaptive_routing": cfg.adaptive_routing, "cheap_model": cfg.cheap_model, "strong_model": cfg.strong_model, "has_openai_key": !cfg.openai_api_key.is_empty(), "openai_base_url": cfg.openai_base_url, "thinking_level": cfg.thinking_level, "run_thinking_level": cfg.run_thinking_level, "agent_model": cfg.agent_model, "fallback_models": cfg.fallback_models, "daily_budget_usd": cfg.daily_budget_usd, "max_concurrent_runs": cfg.max_concurrent_runs})).into_response()
}

/// Remove one saved credential entirely — keychain entry first, then the
/// config field. A posted key that stays empty always means "keep it", so a
/// deliberate removal must name the field here.
async fn forget_saved_key(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let field = input
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !keychain::SECRET_FIELDS.contains(&field.as_str()) {
        let names = keychain::SECRET_FIELDS.join(", ");
        return json_error(
            StatusCode::BAD_REQUEST,
            format!("Name one of these fields: {names}."),
        );
    }
    if let Some(response) = permissions::guard_user(
        &state,
        permissions::Action::Settings,
        "config.key",
        "",
        input
            .get("approve")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "forget a saved key",
    ) {
        return response;
    }
    if let Err(error) = keychain::delete_secret(&keychain_location(&state.config_path), &field) {
        // The store refusing to delete means the key is not gone: refuse the
        // request rather than half-remove it.
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, error);
    }
    let mut cfg = load_config(&state);
    match field.as_str() {
        "api_key" => cfg.api_key = String::new(),
        "search_api_key" => cfg.search_api_key = String::new(),
        "openai_api_key" => cfg.openai_api_key = String::new(),
        _ => unreachable!(),
    }
    if let Err(e) = save_config_file(&state.config_path, &cfg) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    Json(json!({"ok": true, "field": field})).into_response()
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
    let cache_key = format!(
        "{}|{}",
        cfg.base_url.trim_end_matches('/'),
        !cfg.api_key.is_empty()
    );
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
        state
            .models_cache
            .lock()
            .unwrap()
            .insert(cache_key, (std::time::Instant::now(), models.clone()));
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
        let Some(id) = row.get("id").and_then(Value::as_str) else {
            continue;
        };
        // Capabilities come from this row's own metadata; a provider that
        // publishes none leaves them null rather than guessed (capabilities.rs).
        let caps = capabilities::from_catalog_row(&row);
        models.push(json!({
            "id": id,
            "owner": row.get("owned_by").and_then(Value::as_str).unwrap_or(""),
            "context_window": caps.context_limit,
            "capabilities": caps,
        }));
    }
    models.sort_by(|a, b| {
        a.get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(b.get("id").and_then(Value::as_str).unwrap_or(""))
    });
    Ok(models)
}

/* Workflows: saved prompt recipes. They live in a small JSON file next to
the config — no schema migration, and they are machine-global by nature. */
/* Crew templates: saved stage rosters for crew runs. Same pattern as
workflows — a small JSON file next to the config, no schema migration;
keyed by name, because that is what people type in the composer. */
#[derive(Clone, Serialize, Deserialize)]
struct CrewTemplate {
    name: String,
    crew: Vec<String>,
}

fn crew_templates_path(state: &AppState) -> PathBuf {
    state.config_path.with_file_name("crew-templates.json")
}

fn load_crew_templates(state: &AppState) -> Vec<CrewTemplate> {
    fs::read(crew_templates_path(state))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<CrewTemplate>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        // Malformed entries do not poison the whole list.
        .filter(|t| !t.name.is_empty() && !t.crew.is_empty())
        .collect()
}

fn save_crew_templates(state: &AppState, templates: &[CrewTemplate]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(templates).map_err(|e| e.to_string())?;
    let temp = crew_templates_path(state).with_extension("tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    set_private_permissions(&temp, false).map_err(|e| e.to_string())?;
    fs::rename(&temp, crew_templates_path(state)).map_err(|e| e.to_string())
}

async fn list_crew_templates(State(state): State<AppState>) -> Response<Body> {
    Json(json!({ "ok": true, "templates": load_crew_templates(&state) })).into_response()
}

async fn save_crew_template(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let name = clean_title(input.get("name").and_then(Value::as_str).unwrap_or(""));
    if name.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "A crew template needs a name.");
    }
    let roles = match agent::validate_crew(&input.get("crew").cloned().unwrap_or(Value::Null)) {
        Ok(roles) => roles,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };

    let mut templates = load_crew_templates(&state);
    templates.retain(|t| t.name != name);
    templates.push(CrewTemplate {
        name: name.clone(),
        crew: roles,
    });
    templates.sort_by_key(|t| t.name.to_lowercase());
    if let Err(e) = save_crew_templates(&state, &templates) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true, "name": name })).into_response()
}

async fn delete_crew_template(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Response<Body> {
    let mut templates = load_crew_templates(&state);
    let before = templates.len();
    templates.retain(|t| t.name != name);
    if templates.len() == before {
        return json_error(StatusCode::NOT_FOUND, "No such crew template.");
    }
    if let Err(e) = save_crew_templates(&state, &templates) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    Json(json!({ "ok": true })).into_response()
}

async fn list_projects(State(state): State<AppState>) -> Response<Body> {
    let active = active_project_id(&state);
    let result = (|| -> rusqlite::Result<Vec<Value>> {
        let conn = open_db(&state.database_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, name, folder_path, created_at, updated_at, archived_at, thinking, model FROM projects
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
                "thinking": row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                "model": row.get::<_, Option<String>>(7)?.unwrap_or_default(),
            }))
        })?;
        rows.collect()
    })();
    match result {
        Ok(projects) => {
            Json(json!({ "ok": true, "active": active, "projects": projects })).into_response()
        }
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
    let thinking = project_thinking_input(&input);
    let model = project_model_input(&input);
    match open_db(&state.database_path).and_then(|conn| {
        conn.execute(
            "INSERT INTO projects(id,name,folder_path,created_at,updated_at,thinking,model) VALUES(?1,?2,?3,?4,?4,?5,?6)",
            params![id, name, folder, now, thinking, model],
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
    if input.get("model").is_some() {
        let model = project_model_input(&input);
        if let Err(e) = conn.execute(
            "UPDATE projects SET model=?1, updated_at=?2 WHERE id=?3",
            params![model, now_seconds(), id],
        ) {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        }
    }
    if input.get("thinking").is_some() {
        let thinking = project_thinking_input(&input);
        if let Err(e) = conn.execute(
            "UPDATE projects SET thinking=?1, updated_at=?2 WHERE id=?3",
            params![thinking, now_seconds(), id],
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
    match open_db(&state.database_path)
        .and_then(|conn| conn.execute("DELETE FROM projects WHERE id=?1", [&id]))
    {
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
        rows
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
    for (clause, key) in [
        (" AND created_at >= ?", "from"),
        (" AND created_at <= ?", "to"),
    ] {
        let value = params.get(key).and_then(|v| v.parse::<f64>().ok());
        if let Some(value) = value {
            where_sql.push_str(clause);
            args.push(value.to_string());
        }
    }
    let order = format!(
        " ORDER BY {sort_column} {}, id {}",
        if desc { "DESC" } else { "ASC" },
        if desc { "DESC" } else { "ASC" }
    );

    let result = (|| -> rusqlite::Result<Value> {
        let conn = open_db(&state.database_path)?;
        let summary_sql = format!(
            "SELECT COUNT(*) AS requests,\
             SUM(CASE WHEN status='ok' THEN 1 ELSE 0 END) AS successful,\
             SUM(CASE WHEN status!='ok' THEN 1 ELSE 0 END) AS failed,\
             COALESCE(SUM(total_tokens),0) AS total_tokens,\
             COALESCE(AVG(latency_ms),0) AS avg_latency_ms FROM request_log{where_sql}",
        );
        let summary = conn.prepare(&summary_sql)?.query_row(
            rusqlite::params_from_iter(args.iter()),
            |r| {
                Ok(json!({
                    "requests": r.get::<_, i64>(0)?,
                    "successful": r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    "failed": r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    "total_tokens": r.get::<_, i64>(3)?,
                    "avg_latency_ms": r.get::<_, f64>(4)?
                }))
            },
        )?;
        let total: i64 = summary.get("requests").and_then(Value::as_i64).unwrap_or(0);
        let pages = ((total + page_size - 1) / page_size).max(1);

        let sql = format!(
            "SELECT id, created_at, conversation_id, model, provider, status, latency_ms, prompt_tokens, completion_tokens, total_tokens, fallback_attempts, error, cost_usd, cost_basis \
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
                        "fallback_attempts": r.get::<_, Option<String>>(10)?, "error": r.get::<_, Option<String>>(11)?,
                        "cost_usd": r.get::<_, Option<f64>>(12)?, "cost_basis": r.get::<_, Option<String>>(13)?.unwrap_or_default()
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
             COALESCE(SUM(total_tokens),0), COALESCE(AVG(latency_ms),0),\
             COALESCE(SUM(cost_usd),0), SUM(CASE WHEN cost_usd IS NULL AND status='ok' THEN 1 ELSE 0 END)\
             FROM request_log{where_sql} GROUP BY model, provider ORDER BY COUNT(*) DESC LIMIT 20",
        );
        let mut by_stmt = conn.prepare(&by_sql)?;
        let by_model = by_stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(json!({
                    "model": r.get::<_, String>(0)?, "provider": r.get::<_, String>(1)?,
                    "requests": r.get::<_, i64>(2)?, "ok": r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    "tokens": r.get::<_, i64>(4)?, "avg_latency_ms": r.get::<_, f64>(5)?,
                    "usd": r.get::<_, f64>(6)?, "unpriced": r.get::<_, Option<i64>>(7)?.unwrap_or(0)
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Cost is summed, never derived here: a row without a price stays out of
        // the total and is counted instead, so the figure is a labelled floor.
        let window_start = now_seconds() - BUDGET_WINDOW_SECONDS;
        let spend_24h = spend_window(&state, BUDGET_WINDOW_SECONDS);
        let cost_sql = format!(
            "SELECT COALESCE(SUM(cost_usd),0), SUM(CASE WHEN cost_usd IS NULL AND status='ok' THEN 1 ELSE 0 END)\
             FROM request_log{where_sql}",
        );
        let (filtered_usd, unpriced_filtered) =
            conn.prepare(&cost_sql)?
                .query_row(rusqlite::params_from_iter(args.iter()), |r| {
                    Ok((
                        r.get::<_, f64>(0)?,
                        r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    ))
                })?;
        let by_project = {
            let mut stmt = conn.prepare(
                "SELECT COALESCE(p.name,'(no project)') AS name, COUNT(*), COALESCE(SUM(r.cost_usd),0),\
                 SUM(CASE WHEN r.cost_usd IS NULL AND r.status='ok' THEN 1 ELSE 0 END)\
                 FROM request_log r LEFT JOIN conversations c ON c.id=r.conversation_id\
                 LEFT JOIN projects p ON p.id=c.project_id\
                 WHERE r.created_at>=?1 GROUP BY p.name ORDER BY COUNT(*) DESC LIMIT 20",
            )?;
            let rows = stmt.query_map([window_start], |r| {
                Ok(json!({
                    "project": r.get::<_, String>(0)?, "requests": r.get::<_, i64>(1)?,
                    "usd": r.get::<_, f64>(2)?, "unpriced": r.get::<_, Option<i64>>(3)?.unwrap_or(0)
                }))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        Ok(json!({
            "summary": summary,
            "total": total,
            "page": page,
            "page_size": page_size,
            "pages": pages,
            "requests": requests,
            "series": series,
            "by_model": by_model,
            "by_project": by_project,
            "cost": {
                "window_hours": 24,
                "usd_24h": spend_24h.usd,
                "unpriced_24h": spend_24h.unknown,
                "filtered_usd": filtered_usd,
                "filtered_unpriced": unpriced_filtered,
                "budget_usd": load_config(&state).daily_budget_usd,
                // What the numbers are, said plainly: no provider bills this app
                // per request, so every figure is an estimate from published prices.
                "basis": "estimated from the provider's published price and its own reported token counts",
            },
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
    /// The Thinking level this turn will ask for, and which scope set it.
    thinking: Option<thinking::Resolved>,
    /// The active project's model default, or empty when it inherits.
    project_model: String,
    /// Skills attached by matching, not by the composer; their ids ride in the
    /// reply meta so the user sees what the turn was told to follow.
    auto_skills: Vec<String>,
    /// Memory records recalled for this message, as "kind: title", so the reply
    /// can say what background it was given.
    memory_used: Vec<String>,
    /// Code the index matched, as "path:line", so the reply can point at where
    /// its context came from.
    retrieved: Vec<String>,
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
    // `@Thinking` is a command only at the very start of a message; `\@Thinking`
    // writes the token literally. The command is stripped before the message is
    // stored, so switching Thinking on never leaves the token in the history.
    let directive = thinking::take_directive(&message);
    let message = if directive.command {
        directive.text.clone()
    } else {
        message
    };
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
        // Outbound privacy policy: an editor attachment that looks like it
        // carries credentials is refused outright — masking shapes could still
        // leak enough to matter in prose. The reply names the route to fix it.
        let (_, kinds) = privacy::sanitize(&code);
        if !kinds.is_empty() {
            return Err(Box::new(json_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!(
                    "Your editor attachment was not sent: it looks like it contains credentials ({}).\nRemove the secrets (or send only the parts you need) and retry.",
                    kinds.join(", ")
                ),
            )));
        }
        format!("{message}\n\nHere is the code from my editor (language: {language}):\n```{language}\n{code}\n```")
    };

    // The transcript keeps what the user actually sent. Skill and mode
    // preambles are prompt engineering for this request only: stored, they
    // would be replayed as the user's words on every later turn.
    let stored = prompt.clone();
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
    // Automatic selection is the fallback for a message that names no skill:
    // the composer's pick above means the user already decided.
    let mut auto_skills: Vec<String> = Vec::new();
    if input
        .get("skill")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
        && cfg.auto_select_skills
    {
        let chosen = skills::select_for(&message, &cfg.disabled_skills, &available);
        if !chosen.is_empty() {
            let names: Vec<String> = chosen
                .iter()
                .map(|skill| format!("\"{}\"", skill.name))
                .collect();
            let bodies: Vec<String> = chosen
                .iter()
                .map(|skill| format!("## {}\n{}", skill.name, skill.body))
                .collect();
            prompt = format!(
                "Follow the skill{} {} for this message:\n\n{}\n\n---\n\n{}",
                if chosen.len() == 1 { "" } else { "s" },
                names.join(", "),
                bodies.join("\n\n"),
                prompt
            );
            auto_skills = chosen.iter().map(|skill| skill.id.clone()).collect();
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
            params![cid, stored, now_seconds(),            serde_json::to_string(&attachment_records)
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
    let mut history: Vec<Value> = rows
        .into_iter()
        .rev()
        .map(|(role, content)| json!({"role": role, "content": content}))
        .collect();
    // The turn being sent is `prompt` — the user's words plus any preamble that
    // belongs to this request only (skills, plan/decide mode). The transcript
    // keeps `stored`, so the newest row is replaced here instead of the
    // preamble being written into history and replayed as the user's words.
    if let Some(last) = history.last_mut() {
        if last.get("role").and_then(Value::as_str) == Some("user") {
            last["content"] = json!(prompt);
        }
    }
    // One project pointer for the whole turn: it scopes recall, the model
    // default and the Thinking default, so a stale pointer cannot disagree with
    // itself halfway through.
    let active_project = active_project_id(state);
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
    // Recall runs on the user's own words, before any skill or mode preamble
    // decorates them, so a preamble cannot select its own memory.
    let recalled = memory::recall(&state.database_path, &message, &active_project);
    let memory_used: Vec<String> = recalled
        .iter()
        .map(|record| format!("{}: {}", record.kind, record.title))
        .collect();
    if let Some(block) = memory::prompt_block(&recalled) {
        system.push_str("\n\n");
        system.push_str(&block);
    }
    // Traceable retrieval: the index says where, the disk says what. Excerpts are
    // read now, so the block can never quote a file that has since changed.
    let snippets = match state.workspace.current_root() {
        Some(root) => index::retrieve(&root, &state.database_path, &message),
        None => Vec::new(),
    };
    let retrieved: Vec<String> = snippets
        .iter()
        .map(|snippet| format!("{}:{}", snippet.path, snippet.line))
        .collect();
    if let Some(block) = index::prompt_block(&snippets) {
        system.push_str("\n\n");
        system.push_str(&block);
    }
    let packed = context::pack(
        &system,
        outline.as_deref(),
        &history,
        cfg.context_budget_tokens,
        cfg.history_messages,
    );
    // Precedence is message > agent > project > global > provider default.
    // Chat has no agent scope: agent runs resolve their own level.
    // Both project-scoped defaults come from the same repaired pointer, so a
    // stale active_project in config never scopes a turn to a deleted project.
    let project_level = project_thinking_level(state, &active_project);
    let message_level = input
        .get("thinking")
        .and_then(Value::as_str)
        .and_then(thinking::ThinkingLevel::parse)
        .or(directive.level);
    let resolved = thinking::resolve(
        message_level,
        None,
        Some(&project_level),
        Some(&cfg.thinking_level),
    );
    Ok(ChatPrep {
        cid,
        cfg,
        packed,
        model,
        attachments,
        thinking: resolved,
        project_model: project_model(state, &active_project),
        auto_skills,
        memory_used,
        retrieved,
    })
}

/// Record a failed router call so the usage page reflects it.
fn log_router_error(state: &AppState, cid: &str, error: &str, latency: i64, model: &str) {
    oplog::record(
        "error",
        "provider",
        "request-failed",
        format!("{model} after {latency} ms: {error}"),
    );
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
    if !input
        .get("deepsearch")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
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
    let ChatPrep {
        cid,
        cfg,
        mut packed,
        model,
        attachments,
        thinking,
        project_model,
        auto_skills,
        memory_used,
        retrieved,
    } = match prepare_chat(&state, &input) {
        Ok(prep) => prep,
        Err(response) => return *response,
    };
    // Mirroring: the user's message is saved inside prepare_chat, so every
    // open view of this thread should refresh now that it landed.
    events::notify(&state.events, &cid, "user");
    // Usage history for the picked skill: recorded here, where the conversation
    // id exists, so a brand-new thread is attributed too.
    if let Some(skill_id) = input.get("skill").and_then(Value::as_str) {
        if !skill_id.is_empty() {
            skills::record_pick(&state, skill_id, &cid);
        }
    }
    // Matching counts as use too, otherwise the skills panel would claim a
    // skill that answered a dozen turns was never used.
    for skill_id in &auto_skills {
        skills::record_pick(&state, skill_id, &cid);
    }
    let context_tokens = packed.estimated_tokens;
    let context_trimmed = packed.trimmed_messages;
    let context_dropped = packed.dropped_messages;
    let context_saved = packed.tokens_saved;
    media::apply_media(&mut packed.messages, &attachments);
    // The composer's model wins when present; config's model is the default.
    let resolved_model = model_scope::resolve(
        model.as_deref(),
        None,
        Some(project_model.as_str()),
        Some(cfg.model.as_str()),
    );
    let model_scope_label = resolved_model.scope.as_str();
    let chosen_model = resolved_model.model;

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
        let mut meta = json!({ "model": cfg.model, "deepsearch": true });
        meta["context_tokens"] = json!(context_tokens);
        meta["context_trimmed"] = json!(context_trimmed);
        meta["context_dropped"] = json!(context_dropped);
        meta["context_saved_tokens"] = json!(context_saved);
        if !auto_skills.is_empty() {
            meta["auto_skills"] = json!(&auto_skills);
        }
        if !memory_used.is_empty() {
            meta["memory_used"] = json!(&memory_used);
        }
        if !retrieved.is_empty() {
            meta["retrieved"] = json!(&retrieved);
        }
        return Json(json!({
            "ok": true,
            "answer": answer,
            "meta": meta,
            "sources": ds_events.into_iter().filter(|e| e["type"] == "source").collect::<Vec<_>>(),
        }))
        .into_response();
    }

    // Local AI mode: this turn is answered strictly by the configured local
    // runtime. It is never rerouted to the remote router, so a dead runtime
    // stays a visible, actionable error.
    // Local AI mode: this turn is answered strictly by the configured local
    // runtime. It is never rerouted to the remote router, so a dead runtime
    // stays a visible, actionable error. A "local:<model>" manifest pick
    // forces local routing exactly the same way.
    let explicit_local = local_forced_model(&chosen_model);
    if cfg.local_chat || explicit_local.is_some() {
        let started = Instant::now();
        let base = local::effective_base_url(&cfg.local_base_url);
        let model_name = explicit_local.unwrap_or_else(|| cfg.local_model.clone());
        return match local::chat_once(&base, &model_name, &packed.messages).await {
            Ok((answer, tokens)) => {
                let now = now_seconds();
                let completion = if tokens >= 0 { Some(tokens) } else { None };
                let log_result = (|| -> rusqlite::Result<()> {
                    let conn = open_db(&state.database_path)?;
                    conn.execute("INSERT INTO messages(conversation_id,role,content,created_at) VALUES(?1,'assistant',?2,?3)", params![cid, answer, now])?;
                    conn.execute(
                        "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,prompt_tokens,completion_tokens,total_tokens,fallback_attempts,error) VALUES(?1,?2,?3,'local','ok',?4,NULL,?5,NULL,'','')",
                        params![now, cid, model_name, started.elapsed().as_millis() as i64, completion],
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
                Json(json!({
                    "ok": true,
                    "answer": answer,
                    "meta": {
                        "model": model_name,
                        "provider": "local",
                        "completion_tokens": completion,
                        "context_tokens": context_tokens,
                        "context_trimmed": context_trimmed,
                        "context_dropped": context_dropped,
                        "context_saved_tokens": context_saved,
                    }
                }))
                .into_response()
            }
            Err(error) => {
                let latency = started.elapsed().as_millis() as i64;
                log_router_error(&state, &cid, &error, latency, &model_name);
                events::notify(&state.events, &cid, "failed");
                (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"ok": false, "error": error, "meta": {"provider": "local", "latency_ms": latency}})),
                )
                    .into_response()
            }
        };
    }

    let started = Instant::now();
    match call_router_with(
        &state,
        packed.messages,
        Some(&chosen_model),
        thinking.map(|resolved| resolved.level),
    )
    .await
    {
        Ok((answer, mut meta)) => {
            meta["thinking"] = thinking_json(thinking);
            meta["model_scope"] = json!(model_scope_label);
            meta["context_budget"] = json!(cfg.context_budget_tokens);
            // Recorded only when the provider published a price for the model
            // that answered; the token counts are the ones it reported.
            let (cost_usd, cost_basis) = match priced_cost(&state, &cfg, &meta).await {
                Some(cost) => (Some(cost.usd), cost.basis.to_string()),
                None => (None, String::new()),
            };
            let now = now_seconds();
            let log_result = (|| -> rusqlite::Result<()> {
                let conn = open_db(&state.database_path)?;
                conn.execute("INSERT INTO messages(conversation_id,role,content,created_at) VALUES(?1,'assistant',?2,?3)", params![cid, answer, now])?;
                conn.execute(
                    "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,prompt_tokens,completion_tokens,total_tokens,fallback_attempts,error,cost_usd,cost_basis) VALUES(?1,?2,?3,?4,'ok',?5,?6,?7,?8,?9,'',?10,?11)",
                    params![now, cid, meta.get("model").and_then(Value::as_str).unwrap_or("auto"), meta.get("provider").and_then(Value::as_str).unwrap_or("unknown"), meta.get("latency_ms").and_then(Value::as_i64), meta.get("prompt_tokens").and_then(Value::as_i64), meta.get("completion_tokens").and_then(Value::as_i64), meta.get("total_tokens").and_then(Value::as_i64), meta.get("fallback_attempts").and_then(Value::as_str).unwrap_or(""), cost_usd, cost_basis],
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
            meta["context_dropped"] = json!(context_dropped);
            meta["context_saved_tokens"] = json!(context_saved);
            if !auto_skills.is_empty() {
                meta["auto_skills"] = json!(&auto_skills);
            }
            if !memory_used.is_empty() {
                meta["memory_used"] = json!(&memory_used);
            }
            if !retrieved.is_empty() {
                meta["retrieved"] = json!(&retrieved);
            }
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
    /// A concise reasoning summary from a provider that publishes one. Never
    /// presented as hidden chain-of-thought.
    reasoning: Option<String>,
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
    let reasoning = value
        .pointer("/reasoning/summary")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.is_empty());
    StreamEvent {
        delta,
        reasoning,
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
    /// The provider's published price for the model that answered, read while
    /// the catalog was warm. The tokens only arrive with the stream, so the
    /// cost is computed when the reply is saved; `None` means unpriced.
    price: Option<capabilities::Price>,
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
            price: None,
        }
    }

    fn with_price(mut self, price: Option<capabilities::Price>) -> Self {
        self.price = price;
        self
    }

    fn save(&self) {
        if self.saved.swap(true, Ordering::SeqCst) {
            return;
        }
        let text = self
            .text
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let meta = self
            .meta
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| json!({}));
        let latency = self.started.elapsed().as_millis() as i64;
        let cost = self.price.as_ref().and_then(|price| {
            cost::estimate(
                price,
                meta.get("prompt_tokens").and_then(Value::as_i64),
                meta.get("completion_tokens").and_then(Value::as_i64),
            )
        });
        let _ = open_db(&self.database_path).and_then(|conn| {
            let now = now_seconds();
            if !text.trim().is_empty() {
                conn.execute("INSERT INTO messages(conversation_id,role,content,created_at) VALUES(?1,'assistant',?2,?3)", params![self.conversation_id, text, now])?;
            }
            conn.execute(
                "UPDATE conversations SET updated_at=?1 WHERE id=?2",
                params![now, self.conversation_id],
            )?;
            // The thread keeps the model that answered it, so reopening it
            // restores that choice instead of today's default.
            conn.execute(
                "UPDATE conversations SET model=?1 WHERE id=?2",
                params![
                    meta.get("model").and_then(Value::as_str).unwrap_or(""),
                    self.conversation_id
                ],
            )?;
            conn.execute(
                "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,prompt_tokens,completion_tokens,total_tokens,fallback_attempts,error,cost_usd,cost_basis) VALUES(?1,?2,?3,?4,'ok',?5,?6,?7,?8,?9,'',?10,?11)",
                params![now, self.conversation_id,
                    meta.get("model").and_then(Value::as_str).unwrap_or("auto"),
                    meta.get("provider").and_then(Value::as_str).unwrap_or("unknown"),
                    latency,
                    meta.get("prompt_tokens").and_then(Value::as_i64),
                    meta.get("completion_tokens").and_then(Value::as_i64),
                    meta.get("total_tokens").and_then(Value::as_i64),
                    meta.get("fallback_attempts").and_then(Value::as_str).unwrap_or(""),
                    cost.as_ref().map(|cost| cost.usd),
                    cost.as_ref().map(|cost| cost.basis).unwrap_or("")],
            )?;
            Ok(())
        });
        events::notify(
            &self.events,
            &self.conversation_id,
            if text.trim().is_empty() {
                "updated"
            } else {
                "message"
            },
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
                    st.push(
                        json!({ "error": format!("The stream from the router failed: {error}") }),
                    );
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
                let Some(payload) = line.strip_prefix("data:") else {
                    continue;
                };
                let parsed = parse_stream_payload(payload);
                if let Some(text) = parsed.reasoning {
                    if let Ok(mut held) = self.recorder.meta.lock() {
                        held["reasoning"] = json!(true);
                    }
                    self.push(json!({ "type": "reasoning", "text": text }));
                }
                if let Some(text) = parsed.delta {
                    if let Ok(mut held) = self.recorder.text.lock() {
                        held.push_str(&text);
                    }
                    self.push(json!({ "delta": text }));
                }
                if let Some(usage) = parsed.usage {
                    if let Ok(mut held) = self.recorder.meta.lock() {
                        held["prompt_tokens"] =
                            usage.get("prompt_tokens").cloned().unwrap_or(Value::Null);
                        held["completion_tokens"] = usage
                            .get("completion_tokens")
                            .cloned()
                            .unwrap_or(Value::Null);
                        held["total_tokens"] =
                            usage.get("total_tokens").cloned().unwrap_or(Value::Null);
                        held["reasoning_tokens"] = usage
                            .get("reasoning_tokens")
                            .cloned()
                            .unwrap_or(Value::Null);
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
async fn chat_stream(
    State(state): State<AppState>,
    Json(mut input): Json<Value>,
) -> Response<Body> {
    let mut ds_frames: VecDeque<String> = VecDeque::new();
    let mut ds_events: Vec<Value> = Vec::new();
    let ds_answer = match deepsearch_phase(&state, &mut input, &mut ds_events).await {
        Ok((answer, _sources)) => answer,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    for event in ds_events {
        ds_frames.push_back(sse_frame(&event));
    }

    let ChatPrep {
        cid,
        cfg,
        mut packed,
        model,
        attachments,
        thinking,
        project_model,
        auto_skills,
        memory_used,
        retrieved,
    } = match prepare_chat(&state, &input) {
        Ok(prep) => prep,
        Err(response) => return *response,
    };
    // Mirroring: the user's message is saved inside prepare_chat, so every
    // open view of this thread should refresh now that it landed.
    events::notify(&state.events, &cid, "user");
    // Usage history for the picked skill: recorded here, where the conversation
    // id exists, so a brand-new thread is attributed too.
    if let Some(skill_id) = input.get("skill").and_then(Value::as_str) {
        if !skill_id.is_empty() {
            skills::record_pick(&state, skill_id, &cid);
        }
    }
    // Matching counts as use too, otherwise the skills panel would claim a
    // skill that answered a dozen turns was never used.
    for skill_id in &auto_skills {
        skills::record_pick(&state, skill_id, &cid);
    }
    let context_tokens = packed.estimated_tokens;
    let context_trimmed = packed.trimmed_messages;
    let context_dropped = packed.dropped_messages;
    let context_saved = packed.tokens_saved;
    media::apply_media(&mut packed.messages, &attachments);

    if ds_answer.is_none() && !cfg.local_chat && cfg.api_key.trim().is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Add your router's unified API key in Settings first.",
        );
    }
    let resolved_model = model_scope::resolve(
        model.as_deref(),
        None,
        Some(project_model.as_str()),
        Some(cfg.model.as_str()),
    );
    let model_scope_label = resolved_model.scope.as_str();
    let chosen_model = resolved_model.model;

    // Deep search already answered through a native provider: stream the
    // finished text as one frame set instead of calling the router.
    if let Some(answer) = ds_answer {
        let recorder = ReplyRecorder::new(
            state.database_path.clone(),
            cid,
            context_tokens,
            state.events.clone(),
        );
        if let Ok(mut held) = recorder.meta.lock() {
            held["model"] = json!(&chosen_model);
            if !auto_skills.is_empty() {
                held["auto_skills"] = json!(&auto_skills);
                if !memory_used.is_empty() {
                    held["memory_used"] = json!(&memory_used);
                    if !retrieved.is_empty() {
                        held["retrieved"] = json!(&retrieved);
                    }
                }
            }
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

    // Native OpenAI Platform pick: stream the Responses API. Its events are
    // translated into the same upstream frames the shared stream machine
    // reads, so recording, stopping and usage accounting stay shared.
    if let Some(platform_model) = openai::platform_model(&chosen_model) {
        if cfg.openai_api_key.trim().is_empty() {
            return json_error(
                StatusCode::BAD_REQUEST,
                "Add your OpenAI Platform key in Settings to use this model.",
            );
        }
        let body = openai::request_body(
            &platform_model,
            &packed.messages,
            thinking.map(|resolved| resolved.level),
            true,
        );
        return match openai::responses_stream_bytes(
            &state.client,
            &cfg.openai_base_url,
            &cfg.openai_api_key,
            body,
        )
        .await
        {
            Ok(stream) => {
                let recorder = ReplyRecorder::new(
                    state.database_path.clone(),
                    cid,
                    context_tokens,
                    state.events.clone(),
                );
                if let Ok(mut held) = recorder.meta.lock() {
                    held["model"] = json!(&platform_model);
                    held["provider"] = json!("openai-platform");
                    held["thinking"] = thinking_json(thinking);
                }
                let mut pending: VecDeque<String> = ds_frames;
                pending.push_back(sse_frame(&json!({ "ready": true })));
                sse_response(StreamState {
                    bytes: stream,
                    leftover: String::new(),
                    pending,
                    recorder,
                    finished: false,
                })
            }
            Err(error) => {
                log_router_error(&state, &cid, &error, 0, &platform_model);
                events::notify(&state.events, &cid, "failed");
                json_error(StatusCode::BAD_GATEWAY, error)
            }
        };
    }

    // Local AI mode: stream strictly from the configured local runtime —
    // never from the remote router. Ollama's NDJSON deltas are folded into
    // the same SSE frame shape so the browser and recorder stay unchanged.
    let explicit_local_stream_model = local_forced_model(&chosen_model);
    if cfg.local_chat || explicit_local_stream_model.is_some() {
        let base = local::effective_base_url(&cfg.local_base_url);
        let local_name = explicit_local_stream_model.unwrap_or_else(|| cfg.local_model.clone());
        return match local::chat_stream_bytes(&base, &local_name, &packed.messages).await {
            Ok(stream) => {
                let recorder = ReplyRecorder::new(
                    state.database_path.clone(),
                    cid,
                    context_tokens,
                    state.events.clone(),
                );
                if let Ok(mut held) = recorder.meta.lock() {
                    held["model"] = json!(&local_name);
                    held["provider"] = json!("local");
                }
                let mut pending: VecDeque<String> = ds_frames;
                pending.push_back(sse_frame(&json!({ "ready": true })));
                sse_response(StreamState {
                    bytes: stream,
                    leftover: String::new(),
                    pending,
                    recorder,
                    finished: false,
                })
            }
            Err(error) => {
                log_router_error(&state, &cid, &error, 0, &local_name);
                events::notify(&state.events, &cid, "failed");
                json_error(StatusCode::BAD_GATEWAY, error)
            }
        };
    }

    // The same precedence as chat: task, then project, then global. Each
    // configured fallback is tried only while nothing has been streamed yet, so
    // a half-read answer is never replayed on another model.
    let chain = model_scope::chain(&chosen_model, &cfg.fallback_models);
    let start = match open_stream(
        &state,
        &cfg,
        &chain,
        &packed.messages,
        thinking.map(|resolved| resolved.level),
    )
    .await
    {
        Ok(start) => start,
        Err((status, message)) => {
            log_router_error(&state, &cid, &message, 0, &chosen_model);
            events::notify(&state.events, &cid, "failed");
            return json_error(status, message);
        }
    };
    let fallbacks = start.fallbacks;
    let chosen_model = start.model;
    let response = start.response;

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    // The price is read now, while the catalog is warm; the tokens arrive with
    // the stream, so the cost itself is computed when the reply is saved.
    let recorder = ReplyRecorder::new(
        state.database_path.clone(),
        cid,
        context_tokens,
        state.events.clone(),
    )
    .with_price(catalog_price(&state, &cfg, &chosen_model).await);
    if let Ok(mut held) = recorder.meta.lock() {
        held["model"] = json!(&chosen_model);
        if !auto_skills.is_empty() {
            held["auto_skills"] = json!(&auto_skills);
            if !memory_used.is_empty() {
                held["memory_used"] = json!(&memory_used);
                if !retrieved.is_empty() {
                    held["retrieved"] = json!(&retrieved);
                }
            }
        }
        held["thinking"] = thinking_json(thinking);
        held["model_scope"] = json!(model_scope_label);
        held["context_budget"] = json!(cfg.context_budget_tokens);
        held["context_trimmed"] = json!(context_trimmed);
        held["context_dropped"] = json!(context_dropped);
        held["context_saved_tokens"] = json!(context_saved);
        if !fallbacks.is_empty() {
            held["fallbacks"] = json!(fallbacks);
        }
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
            for key in [
                "prompt_tokens",
                "completion_tokens",
                "total_tokens",
                "reasoning_tokens",
            ] {
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
    // Scheduled workflows wake on a timer; the first tick is one interval after
    // boot, so starting the app never runs a schedule early.
    workflow::start_scheduler(state.clone());
    // The process log lives beside the database: it is the one record that
    // survives a crash before the row was committed, so it starts before
    // anything that can fail. A no-op until this point by design.
    if let Some(dir) = state.database_path.parent() {
        oplog::init(dir);
    }
    oplog::record(
        "info",
        "app",
        "started",
        format!(
            "v{} on {}/{}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    );
    // Auto-start runs once the state exists, because it applies the same measured
    // state the Router view shows: only a router that is enabled, installed here
    // and not already answering is started, and one that answers is never adopted.
    if env::var_os("FREE_AI_STUDIO_SKIP_ROUTER").is_none() {
        for outcome in router_registry::startup_sweep(&state).await {
            println!("{}", outcome.message);
        }
    }
    // The MCP supervisor keeps pooled sessions honest: a session that stopped
    // answering while idle is evicted on the timer instead of the next user
    // call paying the full operation timeout to discover it.
    mcp::start_supervisor();
    agent::mark_interrupted(&state);
    workflow::mark_interrupted(&state);
    // Queued runs outlive a restart on purpose: the interrupted sweep above has
    // freed the slots they were waiting for, so admit them now instead of
    // leaving them parked until something else happens to dispatch.
    agent::start_queued(&state).await;
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
        .route("/api/routers", get(routers))
        .route("/api/routers/:id/config", get(routers_config))
        .route(
            "/api/routers/:id/config/file",
            get(routers_config_file).post(routers_save_config_file),
        )
        .route("/api/routers/:id/test", post(routers_test))
        .route("/api/routers/:id/restart", post(routers_restart))
        .route("/api/routers/:id/logs", get(routers_logs))
        .route("/api/routers/:id/auto-start", post(routers_auto_start))
        .route("/api/routers/:id/install-plan", get(routers_install_plan))
        .route(
            "/api/routers/:id/install",
            get(routers_install_record).post(routers_install),
        )
        .route(
            "/api/routers/:id/update",
            get(routers_update_status).post(routers_update),
        )
        .route("/api/routers/:id/uninstall", post(routers_uninstall))
        .route_layer(middleware::from_fn_with_state(
            workspace::LocalOriginGuard::new(port),
            workspace::guard_local_origin,
        ))
        .with_state(state.clone());
    let app = Router::new()
        .route("/", get(root))
        .route("/static/*file", get(static_file))
        .route("/api/config", get(get_config).post(post_config))
        .route("/api/config/forget", post(forget_saved_key))
        .route(
            "/api/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route("/api/conversations/:id/messages", get(get_messages))
        .route("/api/conversations/:id/delete", post(delete_conversation))
        .route("/api/usage", get(usage))
        .route("/api/open", post(open_url))
        .route("/api/health", get(health))
        .route("/api/health/providers", get(health::providers))
        .route("/api/test", post(test_router))
        .route("/api/models", get(models))
        .route("/api/thinking/levels", get(thinking_levels))
        .route("/api/chat", post(chat))
        .route("/api/chat/stream", post(chat_stream))
        .route("/api/events", get(crate::events::events))
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/:id/update", post(update_project))
        .route("/api/projects/:id/delete", post(delete_project))
        .route("/api/projects/:id/activate", post(activate_project))
        .route("/api/workflows", get(workflow::list).post(workflow::save))
        .route("/api/extensions", get(extensions::list))
        .route("/api/extensions/install", post(extensions::install))
        .route("/api/extensions/:id/remove", post(extensions::remove))
        .route("/api/extensions/:id/run", post(extensions::run))
        .route("/api/extensions/:id/cancel", post(extensions::cancel))
        .route("/api/workflows/import", post(workflow::import_graph))
        .route("/api/workflows/:id/export", get(workflow::export))
        .route("/api/workflows/:id/delete", post(workflow::delete))
        .route("/api/workflows/:id/run", post(workflow::run))
        .route("/api/workflow-runs", get(workflow::runs))
        .route("/api/workflow-runs/:id", get(workflow::run_detail))
        .route("/api/workflow-runs/:id/approve", post(workflow::approve))
        .route("/api/workflow-runs/:id/resume", post(workflow::resume))
        .route("/api/workflow-runs/:id/cancel", post(workflow::cancel))
        .route(
            "/api/crew-templates",
            get(list_crew_templates).post(save_crew_template),
        )
        .route(
            "/api/crew-templates/:name/delete",
            post(delete_crew_template),
        )
        .merge(skills::router().route_layer(middleware::from_fn_with_state(
            workspace::LocalOriginGuard::new(port),
            workspace::guard_local_origin,
        )))
        .merge(memory::router().route_layer(middleware::from_fn_with_state(
            workspace::LocalOriginGuard::new(port),
            workspace::guard_local_origin,
        )))
        .merge(index::router().route_layer(middleware::from_fn_with_state(
            workspace::LocalOriginGuard::new(port),
            workspace::guard_local_origin,
        )))
        .merge(workspace::router(port))
        .merge(terminal::router(port))
        .merge(media::router(port))
        .merge(local::read_routes())
        .merge(openai::read_routes())
        .merge(diagnostics::router(port))
        .merge(permissions::router(port))
        .merge(mcp::router(port))
        .merge(local::mutate_routes(port))
        .merge(agent::router().route_layer(middleware::from_fn_with_state(
            workspace::LocalOriginGuard::new(port),
            workspace::guard_local_origin,
        )))
        .merge(router_console::router(port))
        .merge(router_api)
        .layer(middleware::from_fn(local_security_headers))
        // Outermost layer: every route, including the guards inside it, runs
        // with the request's id in scope.
        .layer(middleware::from_fn(trace::assign))
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
        // Only a real Ctrl+C stops the server. `ctrl_c()` fails outright when
        // the process has no console (headless launch, service, CI), and
        // treating that failure as a signal made such a launch exit at once.
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = ctrl_tx.send(true);
        }
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
    // A build without the `gui` feature has no window to wait on, so it takes
    // the headless path even when nothing asked for one. Treating it as a
    // windowed launch sent the shutdown signal immediately after startup, which
    // is why a server-only binary could not serve at all.
    let gui_mode = cfg!(feature = "gui")
        && !wants_browser
        && !args
            .iter()
            .any(|arg| arg == "--no-gui" || arg == "--headless")
        && env::var_os("FREE_AI_STUDIO_NO_GUI").is_none();

    let runtime = tokio::runtime::Runtime::new()?;
    let boot = runtime.block_on(bootstrap())?;

    if gui_mode {
        println!("Application window: http://127.0.0.1:{}", boot.port);
        #[cfg(feature = "gui")]
        {
            if let Err(error) = gui::run(boot.port, boot.shutdown_tx.clone(), APP_NAME) {
                println!("Could not open the application window ({error}).");
                println!("Continuing headless: falling back to your browser.");
                browser::open(boot.port);
                println!("Or open http://127.0.0.1:{} manually.", boot.port);
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
    fn permission_audit_gains_a_request_id_column_once() {
        let conn = Connection::open_in_memory().expect("memory database");
        // The pre-2.4.0 shape: no request_id column.
        conn.execute_batch(
            "CREATE TABLE permission_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT, created_at REAL NOT NULL,
                action TEXT NOT NULL, target TEXT NOT NULL, decision TEXT NOT NULL,
                scope TEXT NOT NULL, project TEXT NOT NULL DEFAULT '',
                detail TEXT NOT NULL DEFAULT ''
            );",
        )
        .expect("old schema");
        conn.execute(
            "INSERT INTO permission_audit(created_at,action,target,decision,scope,project,detail)
             VALUES(1.0,'git','stage','deny','project','','old row')",
            [],
        )
        .expect("old row");

        migrate_permission_request_id(&conn).expect("migrate");
        // Idempotent: a second boot must not try to add it again.
        migrate_permission_request_id(&conn).expect("migrate twice");

        let columns: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(permission_audit)").expect("pragma");
            stmt.query_map([], |row| row.get::<_, String>(1))
                .expect("columns")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("collect")
        };
        assert!(columns.iter().any(|name| name == "request_id"));
        // The row written before the column existed keeps an empty id rather
        // than being given one it never had.
        let old: String = conn
            .query_row(
                "SELECT request_id FROM permission_audit WHERE target='stage'",
                [],
                |row| row.get(0),
            )
            .expect("old row reads");
        assert_eq!(old, "");
        conn.execute(
            "INSERT INTO permission_audit(created_at,action,target,decision,scope,project,detail,request_id)
             VALUES(2.0,'git','commit','deny','project','','new row','req-1')",
            [],
        )
        .expect("a new row carries an id");
    }

    #[test]
    fn adaptive_tier_matrix_off_or_unconfigured_means_no_override() {
        let mut cfg = Config::defaults();
        cfg.adaptive_routing = true;
        cfg.cheap_model = "cheap-x".into();
        cfg.strong_model = "strong-x".into();
        // Routine follow-ups ride the cheap tier.
        for action in ["list", "read", "think", "restored", "failover"] {
            assert_eq!(
                choose_tier(&cfg, action, false).as_deref(),
                Some("cheap-x"),
                "{action}"
            );
        }
        // First step, work, and repair all go strong.
        assert_eq!(choose_tier(&cfg, "", false).as_deref(), Some("strong-x"));
        assert_eq!(
            choose_tier(&cfg, "write", false).as_deref(),
            Some("strong-x")
        );
        assert_eq!(
            choose_tier(&cfg, "command", false).as_deref(),
            Some("strong-x")
        );
        assert_eq!(choose_tier(&cfg, "read", true).as_deref(), Some("strong-x"));
        // Every off-switch returns None: model_override absent = old behavior.
        cfg.adaptive_routing = false;
        assert_eq!(choose_tier(&cfg, "read", false), None);
        cfg.adaptive_routing = true;
        cfg.cheap_model = "".into();
        assert_eq!(choose_tier(&cfg, "read", false), None);
    }

    fn fixture(name: &str) -> AppState {
        let dir = scratch(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("fixture dir");
        let state = AppState {
            config_path: dir.join("config.json"),
            database_path: dir.join("studio.db"),
            static_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static"),
            workspace: workspace::WorkspaceStore::load(dir.join("workspace.json")),
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            terminal: terminal::hub(),
            events: events::Hub::new(),
            models_cache: Arc::new(Mutex::new(HashMap::new())),
        };
        ensure_setup(&state).expect("ensure_setup");
        state
    }

    /// A router that answers with the scripted statuses in order (then 200 with
    /// `answer`), recording each request's model so a test can prove which
    /// candidates were actually asked.
    async fn scripted_router(statuses: &[u16], answer: &str) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let queue: Arc<Mutex<Vec<u16>>> = Arc::new(Mutex::new(statuses.to_vec()));
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_for_route = seen.clone();
        let answer = answer.to_string();
        let app = Router::new()
            // A catalog route so the cost tests can prove a published price is
            // read rather than assumed.
            .route(
                "/models",
                axum::routing::get(|| async {
                    Json(json!({ "data": [
                        { "id": "primary-model", "pricing": { "prompt": "0.000002", "completion": "0.000010" } },
                        { "id": "backup-model", "pricing": { "prompt": "0.000003", "completion": "0.000015" } },
                        { "id": "unpriced-model" }
                    ]}))
                }),
            )
            .route(
            "/chat/completions",
            axum::routing::post(move |Json(body): Json<Value>| {
                let queue = queue.clone();
                let seen = seen_for_route.clone();
                let answer = answer.clone();
                async move {
                    seen.lock()
                        .unwrap()
                        .push(body["model"].as_str().unwrap_or("").to_string());
                    let status = {
                        let mut queue = queue.lock().unwrap();
                        if queue.is_empty() {
                            200
                        } else {
                            queue.remove(0)
                        }
                    };
                    if status == 200 {
                        (
                            StatusCode::OK,
                            Json(json!({
                                "choices": [{ "message": { "role": "assistant", "content": answer } }],
                                "usage": { "prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5 }
                            })),
                        )
                            .into_response()
                    } else {
                        (
                            StatusCode::from_u16(status).unwrap(),
                            Json(json!({ "error": { "message": "provider unavailable" } })),
                        )
                            .into_response()
                    }
                }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}"), seen)
    }

    #[tokio::test]
    async fn a_rate_limited_model_falls_through_to_the_configured_fallback() {
        let (base, seen) = scripted_router(&[429, 200], "answered by the fallback").await;
        let state = fixture("model-fallback");
        let mut cfg = load_config(&state);
        cfg.base_url = base;
        cfg.api_key = "test-key".to_string();
        cfg.model = "primary-model".to_string();
        cfg.fallback_models = vec!["backup-model".to_string()];
        save_config_file(&state.config_path, &cfg).expect("save config");

        let (answer, meta) = call_router(
            &state,
            vec![json!({ "role": "user", "content": "hi" })],
            None,
        )
        .await
        .expect("the fallback answered");
        assert_eq!(answer, "answered by the fallback");
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            ["primary-model", "backup-model"],
            "both candidates were asked, in order"
        );
        assert_eq!(
            meta["fallbacks"][0]["model"],
            json!("primary-model"),
            "{meta}"
        );
        assert_eq!(meta["model"], json!("backup-model"), "{meta}");
        let _ = std::fs::remove_dir_all(state.config_path.parent().unwrap());
    }

    #[test]
    fn a_run_record_snapshot_reports_the_scope_that_set_the_model() {
        let state = fixture("snapshot-scope");
        let mut cfg = load_config(&state);
        cfg.model = "global-model".to_string();
        cfg.agent_model = "agent-model".to_string();
        save_config_file(&state.config_path, &cfg).expect("save config");

        // Agent scope beats the global default.
        let (snapshot, model) = run_record_snapshot(&state, None);
        assert_eq!(model, "agent-model");
        let parsed: Value = serde_json::from_str(&snapshot).expect("snapshot json");
        assert_eq!(parsed["resolved_scope"], json!("agent"));

        // The run's own model (task scope) beats everything.
        let (_, model) = run_record_snapshot(&state, Some("run-model"));
        assert_eq!(model, "run-model");

        // A project default beats the agent scope. The test activates the
        // project itself rather than trusting a side effect of another helper.
        let project = active_project_id(&state);
        open_db(&state.database_path)
            .expect("db")
            .execute(
                "UPDATE projects SET model='project-model' WHERE id=?1",
                [&project],
            )
            .expect("project model");
        assert_eq!(
            project_model(&state, &project),
            "project-model",
            "the project default must be readable before it is resolved"
        );
        let mut cfg = load_config(&state);
        cfg.active_project = project.clone();
        save_config_file(&state.config_path, &cfg).expect("activate the project");
        let (_, model) = run_record_snapshot(&state, None);
        assert_eq!(model, "agent-model", "agent scope is above project scope");

        // With the agent scope cleared, the project default takes over.
        let mut cfg = load_config(&state);
        cfg.agent_model.clear();
        save_config_file(&state.config_path, &cfg).expect("clear the agent model");
        let (snapshot, model) = run_record_snapshot(&state, None);
        assert_eq!(model, "project-model");
        let parsed: Value = serde_json::from_str(&snapshot).expect("snapshot json");
        assert_eq!(parsed["resolved_scope"], json!("project"));
        let _ = std::fs::remove_dir_all(state.config_path.parent().unwrap());
    }

    #[tokio::test]
    async fn a_published_price_is_read_from_the_catalog_and_never_invented() {
        let (base, _seen) = scripted_router(&[], "unused").await;
        let state = fixture("catalog-price");
        let mut cfg = load_config(&state);
        cfg.base_url = base;
        cfg.api_key = "test-key".to_string();
        save_config_file(&state.config_path, &cfg).expect("save config");

        let price = catalog_price(&state, &cfg, "primary-model")
            .await
            .expect("a published price");
        assert_eq!(price.input_per_million, 2.0);
        assert_eq!(price.output_per_million, 10.0);
        assert_eq!(
            catalog_price(&state, &cfg, "unpriced-model").await,
            None,
            "a model with no published price must not be given one"
        );

        // What the chat path records becomes money only through that price.
        let cost = priced_cost(
            &state,
            &cfg,
            &json!({
                "model": "primary-model", "prompt_tokens": 1_000_000, "completion_tokens": 0
            }),
        )
        .await
        .expect("a cost");
        assert_eq!(cost.usd, 2.0);
        assert_eq!(cost.basis, "estimated");

        // No reported tokens means no cost, even with a price in hand.
        assert!(
            priced_cost(&state, &cfg, &json!({ "model": "primary-model" }))
                .await
                .is_none()
        );
        let _ = std::fs::remove_dir_all(state.config_path.parent().unwrap());
    }

    #[tokio::test]
    async fn a_spent_daily_budget_refuses_the_call_before_the_router_is_asked() {
        let (base, seen) = scripted_router(&[], "must not answer").await;
        let state = fixture("daily-budget");
        let mut cfg = load_config(&state);
        cfg.base_url = base;
        cfg.api_key = "test-key".to_string();
        cfg.model = "primary-model".to_string();
        cfg.daily_budget_usd = 0.05;
        save_config_file(&state.config_path, &cfg).expect("save config");

        let seed = |usd: f64| {
            open_db(&state.database_path)
                .expect("db")
                .execute(
                    "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,cost_usd,cost_basis) VALUES(?1,'','primary-model','router / test','ok',10,?2,'estimated')",
                    params![now_seconds(), usd],
                )
                .expect("seed spend");
        };

        // Under the budget: the call goes through.
        seed(0.02);
        assert!(call_router(
            &state,
            vec![json!({ "role": "user", "content": "hi" })],
            None
        )
        .await
        .is_ok());
        assert_eq!(seen.lock().unwrap().len(), 1, "the first turn was allowed");

        // A second recorded spend takes the window to the limit.
        seed(0.04);
        let error = call_router(
            &state,
            vec![json!({ "role": "user", "content": "hi" })],
            None,
        )
        .await
        .expect_err("the budget must refuse the call");
        assert!(error.contains("budget"), "{error}");
        assert!(error.contains("0.0600"), "{error}");
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "the refused call never reached the router"
        );
        let _ = std::fs::remove_dir_all(state.config_path.parent().unwrap());
    }

    #[tokio::test]
    async fn an_open_circuit_skips_the_router_entirely() {
        let (base, seen) = scripted_router(&[], "must not answer").await;
        let state = fixture("circuit-open");
        let mut cfg = load_config(&state);
        cfg.base_url = base;
        cfg.api_key = "test-key".to_string();
        cfg.model = "primary-model".to_string();
        save_config_file(&state.config_path, &cfg).expect("save config");

        let scope = breaker_scope(&state);
        for _ in 0..3 {
            breaker::failure(&scope, "primary-model", "HTTP 503");
        }
        let error = call_router(
            &state,
            vec![json!({ "role": "user", "content": "hi" })],
            None,
        )
        .await
        .expect_err("an open circuit must refuse the call");
        assert!(error.contains("cooling down"), "{error}");
        assert!(error.contains("primary-model"), "{error}");
        assert!(
            seen.lock().unwrap().is_empty(),
            "the router was never asked while the circuit was open"
        );
        let _ = std::fs::remove_dir_all(state.config_path.parent().unwrap());
    }

    #[tokio::test]
    async fn a_rejected_request_never_burns_the_fallback_chain() {
        let (base, seen) = scripted_router(&[400, 200], "must not answer").await;
        let state = fixture("model-no-fallback");
        let mut cfg = load_config(&state);
        cfg.base_url = base;
        cfg.api_key = "test-key".to_string();
        cfg.model = "primary-model".to_string();
        cfg.fallback_models = vec!["backup-model".to_string()];
        save_config_file(&state.config_path, &cfg).expect("save config");

        let error = call_router(
            &state,
            vec![json!({ "role": "user", "content": "hi" })],
            None,
        )
        .await
        .expect_err("a 400 is returned as-is");
        assert!(error.contains("HTTP 400"), "{error}");
        assert_eq!(seen.lock().unwrap().len(), 1, "the fallback was not asked");
        let _ = std::fs::remove_dir_all(state.config_path.parent().unwrap());
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

        let usage = parse_stream_payload(
            r#"{"usage":{"prompt_tokens":12,"completion_tokens":4,"total_tokens":16}}"#,
        );
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
