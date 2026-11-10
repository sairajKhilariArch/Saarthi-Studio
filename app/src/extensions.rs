//! Extensions: installed folders with a `manifest.json`.
//!
//! A manifest is discovered, validated and reported here; execution goes through
//! [`crate::extension_run`], a wasmi sandbox with no host functions (ADR 0001).
//! This module only decides whether a manifest may run at all, and refuses by name
//! any capability the runner does not provide.
//!
//! Layout, under the app's data directory:
//!
//! ```text
//! extensions/
//!   hello-tools/
//!     manifest.json
//!     main.js
//! ```
//!
//! ```json
//! {
//!   "id": "hello-tools",
//!   "name": "Hello Tools",
//!   "version": "0.1.0",
//!   "description": "What it does, in one line.",
//!   "author": "Someone",
//!   "entry": "main.js",
//!   "permissions": ["workspace.read"]
//! }
//! ```
//!
//! The folder name is the id, which makes a copy-and-rename mistake visible
//! instead of silently loading the wrong extension. `entry` is optional (a
//! future UI-only extension has no script) but when present it must be a
//! relative path inside the folder and the file must exist. Permissions are a
//! closed list: an unknown permission fails validation rather than being
//! ignored, because silently dropping a capability request is how a user ends up
//! believing an extension cannot do something that it can.
//!
//! Compatibility is declared, not guessed. `apiVersion` names the manifest
//! schema (missing means 1, the only version this build reads, so an older
//! folder keeps installing); a schema version from the future is refused at
//! parse instead of being read partially. `requiresStudio` is the oldest app
//! version the extension works on: a well-formed manifest that names a newer
//! build stays `valid` — it is a real extension — but is `compatible: false` and
//! is neither installed nor run. `dependencies` lists other extension ids that
//! must already be installed; a missing one is reported and blocks a run, but
//! does not block an install, because the dependency can be added afterwards.

use crate::AppState;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Closed permission vocabulary. Adding one is a product decision, not a typo.
const ALLOWED_PERMISSIONS: [&str; 5] = [
    "workspace.read",
    "workspace.write",
    "terminal.run",
    "network",
    "mcp.call",
];
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// Install limits: an extension is copied into the app's data directory, so an
/// unbounded copy could fill the disk. These are ceilings, not quotas.
const MAX_FILES: usize = 500;
const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
/// The capabilities the runner actually provides (ADR 0001): the two workspace
/// host functions. Everything else in the closed vocabulary installs but is
/// refused at run by name, instead of running under a narrower sandbox than the
/// manifest asked for.
const V1_PERMISSIONS: [&str; 2] = ["workspace.read", "workspace.write"];
/// The manifest schema this build understands. Only 1 exists, so any other
/// declared number is a folder written for a different build.
const SUPPORTED_API_VERSION: u32 = 1;
const MAX_PERMISSIONS: usize = 16;
const MAX_DEPENDENCIES: usize = 16;
const MAX_TOOLS: usize = 16;
const MAX_TOOL_DESCRIPTION_CHARS: usize = 400;
const MAX_TOOL_SCHEMA_BYTES: usize = 8 * 1024;
/// A tool call carries its arguments as JSON text; this bounds what the host
/// hands the module so a call cannot become a bulk copy into the sandbox.
const MAX_TOOL_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_DESCRIPTION_CHARS: usize = 400;
const MAX_AUTHOR_CHARS: usize = 80;
const MAX_ENTRY_CHARS: usize = 160;

#[derive(Clone, Debug, PartialEq)]
struct Manifest {
    id: String,
    name: String,
    version: String,
    description: String,
    author: String,
    entry: Option<String>,
    /// Only `wasm` today. Missing means the extension is installable but has no
    /// runtime this app can execute.
    runtime: Option<String>,
    /// Manifest schema version. Missing means 1; a higher number is refused at
    /// parse, because reading part of a schema we do not know is worse than
    /// saying we cannot read it.
    api_version: u32,
    /// Oldest app version this extension works on, `1.2.3`.
    requires_studio: Option<String>,
    /// Other extension ids that must be installed first.
    dependencies: Vec<String>,
    /// Tools the agent may call. Each one runs as its own sandboxed module, so a
    /// manifest declaring tools must declare the wasm runtime.
    tools: Vec<Tool>,
    permissions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct Tool {
    name: String,
    /// What the tool does, in one line. The model chooses by it, so it is
    /// required rather than optional.
    description: String,
    /// Module to run; `None` means the manifest's own `entry`.
    entry: Option<String>,
    /// Declared for the model; the sandbox does not enforce it (see
    /// `docs/FEATURE-STATUS.md`), so an argument the module mishandles is the
    /// module's problem, not a refusal.
    input_schema: Option<Value>,
}

/// History of runs, newest last. One row per run request that reached the
/// sandbox, including the ones that failed to load or stopped early: a run that
/// left no trace would make "it did nothing" indistinguishable from "it
/// failed".
pub(crate) fn initialize(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS extension_runs (
            id TEXT PRIMARY KEY, extension TEXT NOT NULL, project TEXT NOT NULL DEFAULT '',
            started_at REAL NOT NULL, duration_ms INTEGER NOT NULL,
            exit_code INTEGER, fuel_used INTEGER NOT NULL DEFAULT 0,
            host_calls INTEGER NOT NULL DEFAULT 0, error TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS extension_runs_started
            ON extension_runs(started_at DESC);",
    )
}

struct RunRecord<'a> {
    extension: &'a str,
    project: &'a str,
    started_at: f64,
    duration_ms: u128,
    exit_code: Option<i32>,
    fuel_used: u64,
    host_calls: usize,
    error: &'a str,
}

/// A failed write must never fail the run it describes — the run already
/// happened — so this ignores storage errors, like the permission audit does.
fn record_run(db: &std::path::Path, run: &RunRecord<'_>) {
    let _ = crate::open_db(db).and_then(|conn| {
        initialize(&conn)?;
        conn.execute(
            "INSERT INTO extension_runs(id,extension,project,started_at,duration_ms,exit_code,fuel_used,host_calls,error) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                run.extension,
                run.project,
                run.started_at,
                run.duration_ms as i64,
                run.exit_code,
                run.fuel_used as i64,
                run.host_calls as i64,
                run.error.chars().take(500).collect::<String>(),
            ],
        )
    });
}

pub(crate) fn runs_at(db: &std::path::Path, limit: usize) -> Vec<Value> {
    let limit = limit.clamp(1, 500) as i64;
    let Ok(conn) = crate::open_db(db) else {
        return Vec::new();
    };
    let _ = initialize(&conn);
    let Ok(mut stmt) = conn.prepare(
        "SELECT extension, project, started_at, duration_ms, exit_code, fuel_used, host_calls, error \
         FROM extension_runs ORDER BY started_at DESC, rowid DESC LIMIT ?1",
    ) else {
        return Vec::new();
    };
    stmt.query_map([limit], |row| {
        Ok(json!({
            "extension": row.get::<_, String>(0)?,
            "project": row.get::<_, String>(1)?,
            "started_at": row.get::<_, f64>(2)?,
            "duration_ms": row.get::<_, i64>(3)?,
            "exit_code": row.get::<_, Option<i32>>(4)?,
            "fuel_used": row.get::<_, i64>(5)?,
            "host_calls": row.get::<_, i64>(6)?,
            "error": row.get::<_, String>(7)?,
        }))
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

fn extensions_dir(state: &AppState) -> PathBuf {
    state
        .database_path
        .parent()
        .map(|dir| dir.join("extensions"))
        .unwrap_or_else(|| PathBuf::from("extensions"))
}

fn clean_text(value: Option<&str>, max: usize) -> String {
    value.unwrap_or("").trim().chars().take(max).collect()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Three numeric parts. A range prefix (`^1.2.0`) belongs in a future dependency
/// section, not in the version the installed folder actually is.
fn valid_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 4 && part.chars().all(|c| c.is_ascii_digit())
        })
}

/// A relative path that cannot escape the extension folder on either platform.
fn valid_entry(entry: &str) -> bool {
    !entry.is_empty()
        && entry.chars().count() <= MAX_ENTRY_CHARS
        && !entry.starts_with('/')
        && !entry.contains('\\')
        && !entry.contains(':')
        && entry
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn parse_manifest(folder: &str, raw: &Value) -> Result<Manifest, String> {
    if !raw.is_object() {
        return Err("manifest.json must be a JSON object.".to_string());
    }
    let id = clean_text(raw.get("id").and_then(Value::as_str), 40);
    if !valid_id(&id) {
        return Err("id must be 1-40 lowercase letters, digits, - or _.".to_string());
    }
    if id != folder {
        return Err(format!(
            "id \"{id}\" does not match its folder \"{folder}\"."
        ));
    }
    let name = clean_text(raw.get("name").and_then(Value::as_str), 80);
    if name.is_empty() {
        return Err("name is required.".to_string());
    }
    let version = clean_text(raw.get("version").and_then(Value::as_str), 20);
    if !valid_version(&version) {
        return Err("version must look like 1.2.3.".to_string());
    }
    let description = clean_text(
        raw.get("description").and_then(Value::as_str),
        MAX_DESCRIPTION_CHARS,
    );
    let author = clean_text(raw.get("author").and_then(Value::as_str), MAX_AUTHOR_CHARS);
    let entry = match raw.get("entry").and_then(Value::as_str) {
        None => None,
        Some(value) => {
            let value = value.trim();
            if value.is_empty() {
                None
            } else if valid_entry(value) {
                Some(value.to_string())
            } else {
                return Err(
                    "entry must be a relative path inside the extension folder.".to_string()
                );
            }
        }
    };
    let runtime = match raw.get("runtime").and_then(Value::as_str) {
        None => None,
        Some("wasm") => Some("wasm".to_string()),
        Some(other) => {
            return Err(format!(
                "runtime \"{other}\" is not supported; the v1 runtime is \"wasm\"."
            ))
        }
    };
    if runtime.as_deref() == Some("wasm") {
        match entry.as_deref() {
            None => return Err("A wasm extension needs a .wasm entry file.".to_string()),
            Some(path) if !path.ends_with(".wasm") => {
                return Err("A wasm extension's entry must end in .wasm.".to_string())
            }
            Some(_) => {}
        }
    }
    let api_version = match raw.get("apiVersion") {
        None => SUPPORTED_API_VERSION,
        Some(value) => match value.as_u64() {
            Some(version) if version >= 1 && version <= u32::MAX as u64 => version as u32,
            _ => return Err("apiVersion must be a positive integer.".to_string()),
        },
    };
    if api_version > SUPPORTED_API_VERSION {
        return Err(format!(
            "This extension declares manifest apiVersion {api_version}; this build supports {SUPPORTED_API_VERSION}."
        ));
    }
    let requires_studio = match raw.get("requiresStudio") {
        None => None,
        Some(Value::String(value)) => {
            let value = value.trim();
            if !valid_version(value) {
                return Err("requiresStudio must look like 1.2.3.".to_string());
            }
            Some(value.to_string())
        }
        Some(_) => return Err("requiresStudio must be a version string like 1.2.3.".to_string()),
    };
    let dependencies = match raw.get("dependencies") {
        None => Vec::new(),
        Some(Value::Array(list)) => {
            if list.len() > MAX_DEPENDENCIES {
                return Err(format!(
                    "dependencies: at most {MAX_DEPENDENCIES} entries are allowed."
                ));
            }
            let mut dependencies: Vec<String> = Vec::new();
            for entry in list {
                let dependency = entry.as_str().unwrap_or("");
                if !valid_id(dependency) {
                    return Err(format!(
                        "dependency \"{dependency}\" is not a valid extension id."
                    ));
                }
                if dependency == id {
                    return Err("An extension cannot depend on itself.".to_string());
                }
                if !dependencies.iter().any(|seen| seen == dependency) {
                    dependencies.push(dependency.to_string());
                }
            }
            dependencies
        }
        Some(_) => return Err("dependencies must be a list of extension ids.".to_string()),
    };
    let tools = match raw.get("tools") {
        None => Vec::new(),
        Some(Value::Array(list)) => {
            if list.is_empty() {
                return Err("tools must not be empty when present.".to_string());
            }
            if list.len() > MAX_TOOLS {
                return Err(format!("tools: at most {MAX_TOOLS} entries are allowed."));
            }
            let mut tools: Vec<Tool> = Vec::new();
            for entry in list {
                if !entry.is_object() {
                    return Err("Each tool must be an object.".to_string());
                }
                let name = clean_text(entry.get("name").and_then(Value::as_str), 40);
                if !valid_id(&name) {
                    return Err(format!(
                        "tool name \"{name}\" must be 1-40 lowercase letters, digits, - or _."
                    ));
                }
                if tools.iter().any(|tool| tool.name == name) {
                    return Err(format!("tool \"{name}\" is declared twice."));
                }
                let description = clean_text(
                    entry.get("description").and_then(Value::as_str),
                    MAX_TOOL_DESCRIPTION_CHARS,
                );
                if description.is_empty() {
                    return Err(format!(
                        "tool \"{name}\" needs a description: the model picks tools by it."
                    ));
                }
                let tool_entry = match entry.get("entry").and_then(Value::as_str) {
                    None => None,
                    Some(value) => {
                        let value = value.trim();
                        if !valid_entry(value) || !value.ends_with(".wasm") {
                            return Err(format!(
                                "tool \"{name}\" entry must be a relative .wasm path inside the extension folder."
                            ));
                        }
                        Some(value.to_string())
                    }
                };
                let input_schema = match entry.get("inputSchema") {
                    None => None,
                    Some(schema @ (Value::Object(_) | Value::Bool(_))) => {
                        let too_big = serde_json::to_string(schema)
                            .map(|text| text.len() > MAX_TOOL_SCHEMA_BYTES)
                            .unwrap_or(true);
                        if too_big {
                            return Err(format!(
                                "tool \"{name}\" inputSchema is larger than {MAX_TOOL_SCHEMA_BYTES} bytes."
                            ));
                        }
                        Some(schema.clone())
                    }
                    Some(_) => {
                        return Err(format!(
                            "tool \"{name}\" inputSchema must be a JSON object or boolean."
                        ))
                    }
                };
                tools.push(Tool {
                    name,
                    description,
                    entry: tool_entry,
                    input_schema,
                });
            }
            tools
        }
        Some(_) => return Err("tools must be a list of objects.".to_string()),
    };
    // Only the sandbox can run a tool, so declaring one without a runtime would
    // be a promise nothing keeps.
    if !tools.is_empty() && runtime.as_deref() != Some("wasm") {
        return Err("Tools need runtime: \"wasm\" — only the sandbox can run them.".to_string());
    }
    let permissions = match raw.get("permissions") {
        None => Vec::new(),
        Some(Value::Array(list)) => {
            if list.len() > MAX_PERMISSIONS {
                return Err(format!(
                    "permissions: at most {MAX_PERMISSIONS} entries are allowed."
                ));
            }
            let mut permissions: Vec<String> = Vec::new();
            for entry in list {
                let permission = entry.as_str().unwrap_or("");
                if !ALLOWED_PERMISSIONS.contains(&permission) {
                    return Err(format!(
                        "unknown permission \"{permission}\" — use {}.",
                        ALLOWED_PERMISSIONS.join(", ")
                    ));
                }
                if !permissions.iter().any(|seen| seen == permission) {
                    permissions.push(permission.to_string());
                }
            }
            permissions
        }
        Some(_) => return Err("permissions must be a list of strings.".to_string()),
    };
    Ok(Manifest {
        id,
        name,
        version,
        description,
        author,
        entry,
        runtime,
        api_version,
        requires_studio,
        dependencies,
        tools,
        permissions,
    })
}

/// Numeric triple comparison. Both sides are validated as `1.2.3` before they
/// reach here, so a short or odd part can only come from a hand-written call.
fn version_at_least(version: &str, required: &str) -> bool {
    let parts = |text: &str| -> Vec<u64> {
        text.split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (have, want) = (parts(version), parts(required));
    for index in 0..3 {
        let have = have.get(index).copied().unwrap_or(0);
        let want = want.get(index).copied().unwrap_or(0);
        if have != want {
            return have > want;
        }
    }
    true
}

/// Whether this build is too old for the extension. A compatibility fact, not a
/// validation error: the manifest is well formed, it just names a newer Studio.
fn compatibility_block(manifest: &Manifest, app_version: &str) -> Option<String> {
    match manifest.requires_studio.as_deref() {
        None => None,
        Some(required) if version_at_least(app_version, required) => None,
        Some(required) => Some(format!(
            "Requires Sarthi Studio {required} or newer; this build is {app_version}."
        )),
    }
}

fn missing_dependencies(manifest: &Manifest, installed: &[String]) -> Vec<String> {
    manifest
        .dependencies
        .iter()
        .filter(|dependency| !installed.iter().any(|id| id == *dependency))
        .cloned()
        .collect()
}

/// Why the runner cannot run this manifest, if it cannot. A wasm extension is
/// runnable when every permission it declares is one the runner provides (ADR
/// 0001); a capability with no host function behind it is refused here.
fn run_block(manifest: &Manifest) -> Option<String> {
    match manifest.runtime.as_deref() {
        None => Some("No runtime declared, so this extension cannot run.".to_string()),
        Some("wasm") => {
            for permission in &manifest.permissions {
                if !V1_PERMISSIONS.contains(&permission.as_str()) {
                    return Some(format!(
                        "Requires \"{permission}\", which the v1 runner does not provide."
                    ));
                }
            }
            None
        }
        Some(other) => Some(format!("Runtime \"{other}\" is not supported.")),
    }
}

fn invalid(folder: &str, error: String) -> Value {
    json!({
        "id": folder,
        "name": "",
        "version": "",
        "description": "",
        "author": "",
        "entry": Value::Null,
        "runtime": Value::Null,
        "apiVersion": Value::Null,
        "requiresStudio": Value::Null,
        "dependencies": [],
        "missingDependencies": [],
        "tools": [],
        "permissions": [],
        "valid": false,
        "compatible": false,
        "runnable": false,
        "blockedReason": error.clone(),
        "error": error,
    })
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
}

/// Copy a folder into the extensions directory. Refuses symlinks outright: a
/// link could point anywhere on the machine, and the manifest's canonical check
/// only covers `entry`. The copy is staged and renamed, so a failure leaves no
/// half-installed folder behind.
fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    let mut files = 0usize;
    let mut bytes = 0u64;
    while let Some((from, to)) = pending.pop() {
        fs::create_dir_all(&to).map_err(|error| format!("Could not create a folder: {error}"))?;
        for entry in fs::read_dir(&from).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            let target = to.join(entry.file_name());
            if file_type.is_symlink() {
                return Err(format!(
                    "{} is a symlink; extensions must be plain folders.",
                    entry.path().display()
                ));
            }
            if file_type.is_dir() {
                pending.push((entry.path(), target));
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            files += 1;
            bytes = bytes.saturating_add(entry.metadata().map(|meta| meta.len()).unwrap_or(0));
            if files > MAX_FILES || bytes > MAX_TOTAL_BYTES {
                return Err(format!(
                    "The extension is larger than the install limit ({MAX_FILES} files, {} MiB).",
                    MAX_TOTAL_BYTES / (1024 * 1024)
                ));
            }
            fs::copy(entry.path(), &target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn install_into(source: &Path, root: &Path) -> Result<String, String> {
    let folder = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if !valid_id(folder) {
        return Err(
            "The folder name must be the extension id: 1-40 lowercase letters, digits, - or _."
                .to_string(),
        );
    }
    let manifest_path = source.join("manifest.json");
    let meta = fs::metadata(&manifest_path)
        .map_err(|_| "The folder has no readable manifest.json.".to_string())?;
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "manifest.json is larger than {MAX_MANIFEST_BYTES} bytes."
        ));
    }
    let text = fs::read_to_string(&manifest_path).map_err(|error| error.to_string())?;
    let raw: Value = serde_json::from_str(&text)
        .map_err(|error| format!("manifest.json is not valid JSON: {error}"))?;
    let manifest = parse_manifest(folder, &raw)?;
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let destination = root.join(&manifest.id);
    if destination.exists() {
        return Err(format!(
            "Extension \"{}\" is already installed. Remove it first.",
            manifest.id
        ));
    }
    let staging = root.join(format!(".install-{}", uuid::Uuid::new_v4().simple()));
    let staged: Result<(), String> = (|| {
        copy_tree(source, &staging)?;
        fs::rename(&staging, &destination).map_err(|error| error.to_string())
    })();
    if let Err(error) = staged {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    // Re-read the copy: this is what checks the entry file and canonical containment.
    let entries = inspect(root);
    let installed = entries
        .iter()
        .find(|entry| entry["id"] == manifest.id.as_str());
    match installed {
        Some(entry)
            if entry["valid"] == Value::Bool(true) && entry["compatible"] == Value::Bool(true) =>
        {
            Ok(manifest.id)
        }
        Some(entry) if entry["valid"] == Value::Bool(true) => {
            // Valid but not compatible: the manifest is readable and names a
            // newer build, so it is refused with that sentence and nothing is
            // left behind. Missing dependencies are not a reason to refuse the
            // install — they can be added afterwards.
            let error = entry["blockedReason"]
                .as_str()
                .filter(|reason| !reason.is_empty())
                .unwrap_or("This extension is not compatible with this build.")
                .to_string();
            let _ = fs::remove_dir_all(&destination);
            Err(error)
        }
        Some(entry) => {
            let error = entry["error"]
                .as_str()
                .unwrap_or("The installed manifest is not valid.")
                .to_string();
            let _ = fs::remove_dir_all(&destination);
            Err(error)
        }
        None => {
            let _ = fs::remove_dir_all(&destination);
            Err("The installed extension could not be read back.".to_string())
        }
    }
}

/// Removing moves the folder into `.removed/` instead of deleting it, matching
/// skills: a mistaken removal is recoverable, and an install can then proceed.
fn remove_extension(root: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("Invalid extension id.".to_string());
    }
    let target = root.join(id);
    if !target.is_dir() {
        return Err("No such extension.".to_string());
    }
    let base = root.canonicalize().map_err(|error| error.to_string())?;
    let canonical = target.canonicalize().map_err(|error| error.to_string())?;
    if !canonical.starts_with(&base) {
        return Err("Refusing to remove a folder outside the extensions directory.".to_string());
    }
    let removed = root.join(".removed");
    fs::create_dir_all(&removed).map_err(|error| error.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    fs::rename(&canonical, removed.join(format!("{id}-{stamp}"))).map_err(|error| error.to_string())
}

fn bad(message: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "ok": false, "error": message.to_string() })),
    )
        .into_response()
}

/// At most one run per extension, so a cancel request names exactly one run and
/// a stuck extension cannot pile up blocking threads. The entry is removed when
/// the ticket drops, which is when the route returns — including on error.
static RUNNING: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn running() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    RUNNING.get_or_init(|| Mutex::new(HashMap::new()))
}

struct RunTicket {
    id: String,
    cancel: Arc<AtomicBool>,
}

impl Drop for RunTicket {
    fn drop(&mut self) {
        if let Ok(mut map) = running().lock() {
            map.remove(&self.id);
        }
    }
}

fn begin_run(id: &str) -> Option<RunTicket> {
    let mut map = running().lock().ok()?;
    if map.contains_key(id) {
        return None;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    map.insert(id.to_string(), cancel.clone());
    Some(RunTicket {
        id: id.to_string(),
        cancel,
    })
}

fn cancel_run(id: &str) -> bool {
    let flag = running().lock().ok().and_then(|map| map.get(id).cloned());
    match flag {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

fn server_error(message: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "ok": false, "error": message.to_string() })),
    )
        .into_response()
}

/// Read one folder's manifest and check the files it names. Split out of
/// [`inspect`] because dependencies make a folder's verdict depend on every
/// other folder, which needs two passes.
fn read_manifest(folder: &Path, name: &str) -> Result<Manifest, String> {
    let manifest_path = folder.join("manifest.json");
    let too_big = fs::metadata(&manifest_path)
        .map(|meta| meta.len() > MAX_MANIFEST_BYTES)
        .unwrap_or(false);
    if too_big {
        return Err(format!(
            "manifest.json is larger than {MAX_MANIFEST_BYTES} bytes."
        ));
    }
    let parsed = match fs::read_to_string(&manifest_path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => parse_manifest(name, &value),
            Err(error) => Err(format!("manifest.json is not valid JSON: {error}")),
        },
        Err(_) => Err("manifest.json is missing or unreadable.".to_string()),
    }?;
    /** Every module the manifest names must exist and stay inside the folder. */
    fn check_entry(folder: &Path, entry: &str, label: &str) -> Result<(), String> {
        let path = folder.join(entry);
        if !path.is_file() {
            return Err(format!("{label} \"{entry}\" is missing."));
        }
        let base = folder.canonicalize().map_err(|error| error.to_string())?;
        let canonical = path.canonicalize().map_err(|error| error.to_string())?;
        if !canonical.starts_with(&base) {
            return Err(format!("{label} must stay inside the extension folder."));
        }
        Ok(())
    }
    if let Some(entry) = parsed.entry.as_deref() {
        check_entry(folder, entry, "entry file")?;
    }
    for tool in &parsed.tools {
        if let Some(entry) = tool.entry.as_deref() {
            check_entry(folder, entry, &format!("tool \"{}\" entry", tool.name))?;
        }
    }
    Ok(parsed)
}

fn inspect(dir: &Path) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && !is_hidden(path))
        .collect();
    folders.sort();
    let app_version = env!("CARGO_PKG_VERSION");
    let mut listed: Vec<(String, Result<Manifest, String>)> = folders
        .into_iter()
        .map(|folder| {
            let name = folder
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string();
            let manifest = read_manifest(&folder, &name);
            (name, manifest)
        })
        .collect();
    // Every readable manifest is known before any one of them is judged, so a
    // dependency is checked against the folders that really are installed.
    let installed: Vec<String> = listed
        .iter()
        .filter_map(|(_, manifest)| manifest.as_ref().ok().map(|manifest| manifest.id.clone()))
        .collect();
    listed
        .drain(..)
        .map(|(name, parsed)| match parsed {
            Ok(manifest) => {
                let incompatible = compatibility_block(&manifest, app_version);
                let missing = missing_dependencies(&manifest, &installed);
                let blocked = incompatible.clone().or_else(|| {
                    if missing.is_empty() {
                        None
                    } else {
                        Some(format!("Needs {} installed first.", missing.join(", ")))
                    }
                });
                let blocked = blocked.or_else(|| run_block(&manifest));
                let tools: Vec<Value> = manifest
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "entry": tool.entry,
                            "inputSchema": tool.input_schema,
                        })
                    })
                    .collect();
                json!({
                    "id": manifest.id,
                    "name": manifest.name,
                    "version": manifest.version,
                    "description": manifest.description,
                    "author": manifest.author,
                    "entry": manifest.entry,
                    "runtime": manifest.runtime,
                    "apiVersion": manifest.api_version,
                    "requiresStudio": manifest.requires_studio,
                    "dependencies": manifest.dependencies,
                    "missingDependencies": missing,
                    "tools": tools,
                    "permissions": manifest.permissions,
                    "valid": true,
                    "compatible": incompatible.is_none(),
                    "runnable": blocked.is_none(),
                    "blockedReason": blocked.unwrap_or_default(),
                    "error": "",
                })
            }
            Err(error) => invalid(&name, error),
        })
        .collect()
}

pub(crate) async fn list(State(state): State<AppState>) -> Response {
    let dir = extensions_dir(&state);
    Json(json!({
        "ok": true,
        "directory": dir.display().to_string(),
        "extensions": inspect(&dir),
    }))
    .into_response()
}

pub(crate) async fn install(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let Some(path) = input
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        return bad("Provide the folder path to install.");
    };
    match install_into(Path::new(path), &extensions_dir(&state)) {
        Ok(id) => Json(json!({ "ok": true, "id": id })).into_response(),
        Err(error) => bad(error),
    }
}

pub(crate) async fn remove(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    // Gate 7 of ADR 0001: an uninstall stops a run in flight before moving the
    // folder. The module bytes are already loaded, so the run ends at its next
    // host call rather than reading from a folder that just moved.
    if cancel_run(&id) {
        crate::oplog::record(
            "info",
            "extensions",
            "cancel",
            format!("{id}: cancelled by uninstall"),
        );
    }
    match remove_extension(&extensions_dir(&state), &id) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(error) => bad(error),
    }
}

/// Gate 2 of ADR 0001: a run request must fail with a reason before any code is
/// loaded. Only then is the module handed to the sandboxed runner. Execution is
/// blocking interpreter work, so it runs on a blocking thread instead of the
/// async runtime's worker.
pub(crate) async fn runs(State(state): State<AppState>) -> Response {
    Json(json!({
        "ok": true,
        "runs": runs_at(&state.database_path, 50),
    }))
    .into_response()
}

pub(crate) async fn cancel(AxumPath(id): AxumPath<String>) -> Response {
    if !valid_id(&id) {
        return bad("Invalid extension id.");
    }
    let cancelled = cancel_run(&id);
    if cancelled {
        crate::oplog::record(
            "info",
            "extensions",
            "cancel",
            format!("{id}: stop requested"),
        );
    }
    Json(json!({
        "ok": true,
        "cancelled": cancelled,
        "detail": if cancelled {
            "The run stops at its next host call."
        } else {
            "No run of this extension is in progress."
        }
    }))
    .into_response()
}

pub(crate) async fn run(State(state): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
    if !valid_id(&id) {
        return bad("Invalid extension id.");
    }
    let dir = extensions_dir(&state);
    let entries = inspect(&dir);
    let Some(entry) = entries.iter().find(|entry| entry["id"] == id.as_str()) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "No such extension." })),
        )
            .into_response();
    };
    if entry["valid"] != Value::Bool(true) {
        return bad(entry["error"]
            .as_str()
            .unwrap_or("The extension is not valid."));
    }
    let reason = entry["blockedReason"].as_str().unwrap_or("");
    if !reason.is_empty() {
        return bad(reason);
    }
    let Some(entry_file) = entry["entry"].as_str() else {
        return bad("This extension has no entry file to run.");
    };
    let module_path = dir.join(&id).join(entry_file);
    // The grant is a snapshot: the rule list and the open folder are read once,
    // here, so a rule added mid-run cannot widen it.
    let declared = declared_permissions(entry);
    let project = crate::active_project_id(&state);
    let grant = host_grant(&state, &dir, &id, &declared, &project);
    let Some(ticket) = begin_run(&id) else {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "ok": false,
                "error": "This extension is already running. Cancel that run first."
            })),
        )
            .into_response();
    };
    let cancel = ticket.cancel.clone();
    let _ticket = ticket;
    let options = crate::extension_run::RunOptions {
        cancel: Some(cancel),
        wall: None,
        arguments: None,
    };
    let started_at = crate::now_seconds();
    let started = std::time::Instant::now();
    let outcome = tokio::task::spawn_blocking(move || {
        crate::extension_run::run_file_with(&module_path, Some(grant), options)
    })
    .await;
    let outcome = match outcome {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            record_run(
                &state.database_path,
                &RunRecord {
                    extension: &id,
                    project: &project,
                    started_at,
                    duration_ms: started.elapsed().as_millis(),
                    exit_code: None,
                    fuel_used: 0,
                    host_calls: 0,
                    error: &error,
                },
            );
            crate::oplog::record("warn", "extensions", "run", format!("{id}: {error}"));
            return bad(error);
        }
        Err(error) => {
            let message = format!("The extension stopped unexpectedly: {error}");
            record_run(
                &state.database_path,
                &RunRecord {
                    extension: &id,
                    project: &project,
                    started_at,
                    duration_ms: started.elapsed().as_millis(),
                    exit_code: None,
                    fuel_used: 0,
                    host_calls: 0,
                    error: &message,
                },
            );
            crate::oplog::record("warn", "extensions", "run", format!("{id}: {message}"));
            return server_error(message);
        }
    };
    record_run(
        &state.database_path,
        &RunRecord {
            extension: &id,
            project: &project,
            started_at,
            duration_ms: outcome.elapsed_ms,
            exit_code: Some(outcome.exit_code),
            fuel_used: outcome.fuel_used,
            host_calls: outcome.audit.len(),
            error: "",
        },
    );
    crate::oplog::record(
        "info",
        "extensions",
        "run",
        format!(
            "{id} exited with {} after {} ms ({} fuel, {} host calls)",
            outcome.exit_code,
            outcome.elapsed_ms,
            outcome.fuel_used,
            outcome.audit.len()
        ),
    );
    // Every host call is audited with the extension id. Refusals and asks also
    // reach the permission audit table, where the agent's own refusals live, so
    // Settings → Permissions shows why an extension was stopped.
    audit_calls(&state, &id, &project, &outcome);
    Json(json!({
        "ok": true,
        "id": id,
        "run": {
            "exitCode": outcome.exit_code,
            "fuelUsed": outcome.fuel_used,
            "elapsedMs": outcome.elapsed_ms,
            "hostCalls": outcome.audit.len(),
        }
    }))
    .into_response()
}

/// Every tool a run may call: only folders that are valid and runnable, so the
/// model is never told about a tool that would be refused. Read from disk each
/// time because a manifest is a few small files — unlike MCP discovery, this
/// spawns nothing and needs no snapshot.
pub(crate) fn catalog(state: &AppState) -> Vec<Value> {
    let dir = extensions_dir(state);
    let mut tools = Vec::new();
    for entry in inspect(&dir) {
        if entry["valid"] != Value::Bool(true) {
            continue;
        }
        if !entry["blockedReason"].as_str().unwrap_or("").is_empty() {
            continue;
        }
        let id = entry["id"].as_str().unwrap_or("");
        let Some(list) = entry["tools"].as_array() else {
            continue;
        };
        for definition in list {
            let Some(name) = definition["name"].as_str() else {
                continue;
            };
            tools.push(json!({
                "extension": id,
                "tool": name,
                "description": definition["description"].clone(),
                "inputSchema": definition["inputSchema"].clone(),
            }));
        }
    }
    tools
}

/// Run one declared tool for an agent step. The step has already applied the
/// saved rules — an `ask` was answered by the approval flow and a `deny` never
/// reaches here — so this runs the module and records the run, and decides
/// nothing itself.
pub(crate) async fn invoke_for_run(
    state: &AppState,
    extension: &str,
    tool: &str,
    arguments: &Value,
) -> Result<Value, String> {
    if !valid_id(extension) || !valid_id(tool) {
        return Err("Invalid extension id or tool name.".to_string());
    }
    let dir = extensions_dir(state);
    let entries = inspect(&dir);
    let Some(entry) = entries.iter().find(|entry| entry["id"] == extension) else {
        return Err(format!("No extension \"{extension}\" is installed."));
    };
    if entry["valid"] != Value::Bool(true) {
        return Err(entry["error"]
            .as_str()
            .unwrap_or("The extension is not valid.")
            .to_string());
    }
    let reason = entry["blockedReason"].as_str().unwrap_or("");
    if !reason.is_empty() {
        return Err(reason.to_string());
    }
    let definition = entry["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|candidate| candidate["name"] == tool));
    let Some(definition) = definition else {
        return Err(format!(
            "Extension \"{extension}\" declares no tool \"{tool}\"."
        ));
    };
    check_tool_arguments(definition, tool, arguments)?;
    let module_entry = definition["entry"]
        .as_str()
        .or_else(|| entry["entry"].as_str())
        .unwrap_or("");
    if module_entry.is_empty() {
        return Err("This tool has no module to run.".to_string());
    }
    let arguments_text = serde_json::to_string(arguments)
        .map_err(|error| format!("The arguments could not be serialized: {error}"))?;
    if arguments_text.len() > MAX_TOOL_ARGUMENT_BYTES {
        return Err(format!(
            "The arguments are larger than {MAX_TOOL_ARGUMENT_BYTES} bytes."
        ));
    }
    let project = crate::active_project_id(state);
    let declared = declared_permissions(entry);
    let module_path = dir.join(extension).join(module_entry);
    let grant = host_grant(state, &dir, extension, &declared, &project);
    let target = format!("{extension}/{tool}");
    let Some(ticket) = begin_run(extension) else {
        return Err("This extension is already running; wait for it to finish.".to_string());
    };
    let cancel = ticket.cancel.clone();
    let _ticket = ticket;
    let options = crate::extension_run::RunOptions {
        cancel: Some(cancel),
        wall: None,
        arguments: Some(arguments_text),
    };
    let started_at = crate::now_seconds();
    let started = std::time::Instant::now();
    let outcome = tokio::task::spawn_blocking(move || {
        crate::extension_run::run_file_with(&module_path, Some(grant), options)
    })
    .await;
    let outcome = match outcome {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            record_run(
                &state.database_path,
                &RunRecord {
                    extension,
                    project: &project,
                    started_at,
                    duration_ms: started.elapsed().as_millis(),
                    exit_code: None,
                    fuel_used: 0,
                    host_calls: 0,
                    error: &error,
                },
            );
            return Err(error);
        }
        Err(error) => {
            let message = format!("The tool stopped unexpectedly: {error}");
            record_run(
                &state.database_path,
                &RunRecord {
                    extension,
                    project: &project,
                    started_at,
                    duration_ms: started.elapsed().as_millis(),
                    exit_code: None,
                    fuel_used: 0,
                    host_calls: 0,
                    error: &message,
                },
            );
            return Err(message);
        }
    };
    record_run(
        &state.database_path,
        &RunRecord {
            extension,
            project: &project,
            started_at,
            duration_ms: outcome.elapsed_ms,
            exit_code: Some(outcome.exit_code),
            fuel_used: outcome.fuel_used,
            host_calls: outcome.audit.len(),
            error: "",
        },
    );
    audit_calls(state, extension, &project, &outcome);
    crate::oplog::record(
        "info",
        "extensions",
        "tool",
        format!(
            "agent called {target}: exit {} after {} ms ({} fuel)",
            outcome.exit_code, outcome.elapsed_ms, outcome.fuel_used
        ),
    );
    match outcome.result.as_deref() {
        Some(text) => Ok(serde_json::from_str::<Value>(text)
            .unwrap_or_else(|_| Value::String(text.to_string()))),
        None => Ok(Value::Null),
    }
}

fn declared_permissions(entry: &Value) -> Vec<String> {
    entry["permissions"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// What one run may reach, built once per run. A missing folder leaves an empty
/// root, and every host call then reports that nothing is open.
fn host_grant(
    state: &AppState,
    dir: &Path,
    id: &str,
    declared: &[String],
    project: &str,
) -> crate::extension_run::HostGrant {
    crate::extension_run::HostGrant {
        root: state.workspace.root().unwrap_or_default(),
        // Its own folder is the only place an extension can persist anything;
        // the code beside it stays read-only to the module.
        data_root: Some(dir.join(id).join("data")),
        project: project.to_string(),
        rules: crate::permissions::rules(state),
        read: declared
            .iter()
            .any(|permission| permission == "workspace.read"),
        write: declared
            .iter()
            .any(|permission| permission == "workspace.write"),
    }
}

/// Every host call is audited with the extension id. Refusals and asks also
/// reach the permission audit table, where the agent's own refusals live.
fn audit_calls(state: &AppState, id: &str, project: &str, outcome: &crate::extension_run::Outcome) {
    for call in &outcome.audit {
        crate::oplog::record(
            if call.decision == crate::permissions::Decision::Allow {
                "info"
            } else {
                "warn"
            },
            "extensions",
            "host",
            format!(
                "{id} {} \"{}\": {}",
                call.capability, call.path, call.detail
            ),
        );
        if call.decision != crate::permissions::Decision::Allow {
            let action = if call.capability == "workspace.write" {
                crate::permissions::Action::FsWrite
            } else {
                crate::permissions::Action::FsRead
            };
            crate::permissions::record(
                state,
                action,
                &call.path,
                call.decision,
                call.scope,
                project,
                &format!("extension {id} host call"),
            );
        }
    }
}

/// Refuse arguments the tool's own `inputSchema` rejects, before the module is
/// loaded. The keyword subset is MCP's, so an extension tool and an MCP tool
/// behave the same way for the model; a tool that declares no schema takes any
/// argument object.
fn check_tool_arguments(definition: &Value, tool: &str, arguments: &Value) -> Result<(), String> {
    // `inspect` writes a JSON null for a tool that declares no schema.
    let schema = match definition.get("inputSchema") {
        None | Some(Value::Null) => json!({}),
        Some(schema) => schema.clone(),
    };
    if !schema.is_object() && !schema.is_boolean() {
        return Err(format!(
            "Tool \"{tool}\" declares an input schema this build cannot read."
        ));
    }
    crate::mcp::validate_arguments(&schema, arguments)
        .map_err(|error| format!("Tool \"{tool}\" rejected the arguments: {error}"))
}

/// A tool call is decided before the module is read. `deny` refuses, and so does
/// `ask`: this route has no approval channel, the same reason an extension's own
/// host calls cannot prompt.
fn gate_tool_call(state: &AppState, project: &str, target: &str) -> Option<Response> {
    let action = crate::permissions::Action::ExtensionTool;
    let rules = crate::permissions::rules(state);
    let effective = crate::permissions::resolve(rules.iter(), action, target, project);
    let (decision, scope) = match effective {
        Some(effective) => (effective.decision, effective.scope),
        None => (action.default_decision(), crate::permissions::Scope::Always),
    };
    if decision == crate::permissions::Decision::Allow {
        return None;
    }
    crate::permissions::record(
        state,
        action,
        target,
        decision,
        scope,
        project,
        "extension tool call",
    );
    let reason = match decision {
        crate::permissions::Decision::Deny => {
            "A saved permission rule denies this tool.".to_string()
        }
        _ => "Calling an extension tool asks first, and this route cannot prompt: save an allow rule for extension_tool in Settings → Permissions.".to_string(),
    };
    crate::oplog::record("warn", "extensions", "tool", format!("{target}: {reason}"));
    Some(
        (
            StatusCode::FORBIDDEN,
            Json(json!({ "ok": false, "error": reason })),
        )
            .into_response(),
    )
}

/// A tool a manifest declares, called on demand. It runs in the same sandbox as
/// a plain run — same budgets, same workspace grants, same one-run-per-extension
/// rule — but takes JSON arguments and answers with the module's own result.
/// A call that never loads is still recorded, like a failed run.
pub(crate) async fn call_tool(
    State(state): State<AppState>,
    AxumPath((id, tool)): AxumPath<(String, String)>,
    Json(input): Json<Value>,
) -> Response {
    if !valid_id(&id) || !valid_id(&tool) {
        return bad("Invalid extension id or tool name.");
    }
    let dir = extensions_dir(&state);
    let entries = inspect(&dir);
    let Some(entry) = entries.iter().find(|entry| entry["id"] == id.as_str()) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "No such extension." })),
        )
            .into_response();
    };
    if entry["valid"] != Value::Bool(true) {
        return bad(entry["error"]
            .as_str()
            .unwrap_or("The extension is not valid."));
    }
    let reason = entry["blockedReason"].as_str().unwrap_or("");
    if !reason.is_empty() {
        return bad(reason);
    }
    let definition = entry["tools"].as_array().and_then(|tools| {
        tools
            .iter()
            .find(|candidate| candidate["name"] == tool.as_str())
    });
    let Some(definition) = definition else {
        return bad(format!("This extension declares no tool \"{tool}\"."));
    };
    let module_entry = definition["entry"]
        .as_str()
        .or_else(|| entry["entry"].as_str())
        .unwrap_or("");
    if module_entry.is_empty() {
        return bad("This tool has no module to run.");
    }
    let arguments = match input.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(value) => value.clone(),
    };
    let arguments_text = match serde_json::to_string(&arguments) {
        Ok(text) if text.len() <= MAX_TOOL_ARGUMENT_BYTES => text,
        Ok(_) => {
            return bad(format!(
                "The arguments are larger than {MAX_TOOL_ARGUMENT_BYTES} bytes."
            ))
        }
        Err(error) => return bad(format!("The arguments could not be serialized: {error}")),
    };
    let project = crate::active_project_id(&state);
    let target = format!("{id}/{tool}");
    if let Some(response) = gate_tool_call(&state, &project, &target) {
        return response;
    }
    // Permission first, then content: a denied call must read as denied, not as a
    // broken argument list.
    if let Err(error) = check_tool_arguments(definition, &tool, &arguments) {
        return bad(error);
    }
    let declared = declared_permissions(entry);
    let module_path = dir.join(&id).join(module_entry);
    let grant = host_grant(&state, &dir, &id, &declared, &project);
    let Some(ticket) = begin_run(&id) else {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "ok": false,
                "error": "This extension is already running. Cancel that run first."
            })),
        )
            .into_response();
    };
    let cancel = ticket.cancel.clone();
    let _ticket = ticket;
    let options = crate::extension_run::RunOptions {
        cancel: Some(cancel),
        wall: None,
        arguments: Some(arguments_text),
    };
    let started_at = crate::now_seconds();
    let started = std::time::Instant::now();
    let outcome = tokio::task::spawn_blocking(move || {
        crate::extension_run::run_file_with(&module_path, Some(grant), options)
    })
    .await;
    let outcome = match outcome {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            record_run(
                &state.database_path,
                &RunRecord {
                    extension: &id,
                    project: &project,
                    started_at,
                    duration_ms: started.elapsed().as_millis(),
                    exit_code: None,
                    fuel_used: 0,
                    host_calls: 0,
                    error: &error,
                },
            );
            crate::oplog::record("warn", "extensions", "tool", format!("{target}: {error}"));
            return bad(error);
        }
        Err(error) => {
            let message = format!("The tool stopped unexpectedly: {error}");
            record_run(
                &state.database_path,
                &RunRecord {
                    extension: &id,
                    project: &project,
                    started_at,
                    duration_ms: started.elapsed().as_millis(),
                    exit_code: None,
                    fuel_used: 0,
                    host_calls: 0,
                    error: &message,
                },
            );
            crate::oplog::record("warn", "extensions", "tool", format!("{target}: {message}"));
            return server_error(message);
        }
    };
    record_run(
        &state.database_path,
        &RunRecord {
            extension: &id,
            project: &project,
            started_at,
            duration_ms: outcome.elapsed_ms,
            exit_code: Some(outcome.exit_code),
            fuel_used: outcome.fuel_used,
            host_calls: outcome.audit.len(),
            error: "",
        },
    );
    audit_calls(&state, &id, &project, &outcome);
    let (result, reported) = match outcome.result.as_deref() {
        Some(text) => (
            serde_json::from_str::<Value>(text).unwrap_or_else(|_| Value::String(text.to_string())),
            true,
        ),
        None => (Value::Null, false),
    };
    crate::oplog::record(
        "info",
        "extensions",
        "tool",
        format!(
            "{target} exited with {} after {} ms ({} result, {} host calls)",
            outcome.exit_code,
            outcome.elapsed_ms,
            if reported { "a" } else { "no" },
            outcome.audit.len()
        ),
    );
    Json(json!({
        "ok": true,
        "id": id,
        "tool": tool,
        "result": result,
        "reported": reported,
        "run": {
            "exitCode": outcome.exit_code,
            "fuelUsed": outcome.fuel_used,
            "elapsedMs": outcome.elapsed_ms,
            "hostCalls": outcome.audit.len(),
        }
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "free-ai-studio-extensions-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    #[test]
    fn installing_copies_a_valid_extension_and_refuses_a_second_copy() {
        let root = scratch("install");
        let source = root.join("src").join("hello-tools");
        fs::create_dir_all(&source).expect("source folder");
        fs::write(
            source.join("manifest.json"),
            r#"{"id":"hello-tools","name":"Hello Tools","version":"1.0.0","entry":"main.js"}"#,
        )
        .expect("manifest");
        fs::write(source.join("main.js"), "export default {};\n").expect("entry");

        let target = root.join("extensions");
        assert_eq!(
            install_into(&source, &target).expect("install"),
            "hello-tools"
        );
        assert!(target.join("hello-tools").join("main.js").is_file());
        assert_eq!(inspect(&target)[0]["valid"], true);

        let error = install_into(&source, &target).expect_err("second copy");
        assert!(error.contains("already installed"), "{error}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_install_leaves_nothing_and_a_missing_entry_fails() {
        let root = scratch("install-bad");
        let bad = root.join("src").join("bad-tools");
        fs::create_dir_all(&bad).expect("bad folder");
        fs::write(
            bad.join("manifest.json"),
            r#"{"id":"bad-tools","name":"Bad","version":"1.0.0","permissions":["root"]}"#,
        )
        .expect("bad manifest");
        let target = root.join("extensions");
        let error = install_into(&bad, &target).expect_err("bad manifest");
        assert!(error.contains("unknown permission"), "{error}");
        assert!(!target.join("bad-tools").exists());

        let no_entry = root.join("src").join("no-entry");
        fs::create_dir_all(&no_entry).expect("folder");
        fs::write(
            no_entry.join("manifest.json"),
            r#"{"id":"no-entry","name":"No Entry","version":"1.0.0","entry":"missing.js"}"#,
        )
        .expect("manifest");
        let error = install_into(&no_entry, &target).expect_err("missing entry");
        assert!(error.contains("missing"), "{error}");
        assert!(!target.join("no-entry").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstalling_moves_the_folder_aside_and_validates_the_id() {
        let root = scratch("remove");
        let folder = root.join("hello-tools");
        fs::create_dir_all(&folder).expect("extension folder");
        fs::write(
            folder.join("manifest.json"),
            r#"{"id":"hello-tools","name":"Hello Tools","version":"1.0.0"}"#,
        )
        .expect("manifest");

        remove_extension(&root, "hello-tools").expect("remove");
        assert!(!root.join("hello-tools").exists());
        assert!(root.join(".removed").is_dir());
        // The moved folder is hidden from discovery.
        assert!(inspect(&root).is_empty());

        assert!(remove_extension(&root, "missing").is_err());
        assert!(remove_extension(&root, "../escape").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_history_round_trips_newest_first() {
        let dir = scratch("runs");
        let db = dir.join("studio.db");
        record_run(
            &db,
            &RunRecord {
                extension: "seven-tools",
                project: "p1",
                started_at: 10.0,
                duration_ms: 5,
                exit_code: Some(7),
                fuel_used: 3,
                host_calls: 1,
                error: "",
            },
        );
        record_run(
            &db,
            &RunRecord {
                extension: "broken-tools",
                project: "p1",
                started_at: 20.0,
                duration_ms: 9,
                exit_code: None,
                fuel_used: 0,
                host_calls: 0,
                error: "Not valid WebAssembly",
            },
        );
        let runs = runs_at(&db, 10);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0]["extension"], "broken-tools");
        assert_eq!(runs[0]["exit_code"], Value::Null);
        assert_eq!(runs[0]["error"], "Not valid WebAssembly");
        assert_eq!(runs[1]["exit_code"], 7);
        assert_eq!(runs[1]["fuel_used"], 3);
        assert_eq!(runs[1]["host_calls"], 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wasm_manifest_needs_a_wasm_entry() {
        let base = json!({
            "id": "wasm-tools",
            "name": "Wasm Tools",
            "version": "1.0.0"
        });
        let with = |key: &str, value: Value| {
            let mut copy = base.clone();
            copy[key] = value;
            copy
        };
        let parsed = parse_manifest(
            "wasm-tools",
            &json!({
                "id": "wasm-tools", "name": "Wasm Tools", "version": "1.0.0",
                "runtime": "wasm", "entry": "tool.wasm"
            }),
        )
        .expect("wasm manifest");
        assert_eq!(parsed.runtime.as_deref(), Some("wasm"));
        assert!(
            parse_manifest("wasm-tools", &with("runtime", json!("wasm")))
                .unwrap_err()
                .contains(".wasm entry")
        );
        let mut js_entry = with("runtime", json!("wasm"));
        js_entry["entry"] = json!("main.js");
        assert!(parse_manifest("wasm-tools", &js_entry)
            .unwrap_err()
            .contains("end in .wasm"));
        assert!(parse_manifest("wasm-tools", &with("runtime", json!("js")))
            .unwrap_err()
            .contains("not supported"));
        // No runtime is still installable: a saved manifest predates the field.
        assert!(parse_manifest("wasm-tools", &base).is_ok());
    }

    #[test]
    fn runtime_gating_names_the_reason() {
        let manifest = |runtime: Option<&str>, permissions: Vec<&str>| Manifest {
            id: "x".to_string(),
            name: "X".to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            author: String::new(),
            entry: runtime.map(|_| "tool.wasm".to_string()),
            runtime: runtime.map(str::to_string),
            api_version: SUPPORTED_API_VERSION,
            requires_studio: None,
            dependencies: Vec::new(),
            tools: Vec::new(),
            permissions: permissions.into_iter().map(str::to_string).collect(),
        };
        assert!(run_block(&manifest(None, vec![]))
            .unwrap_or_default()
            .contains("No runtime"));
        assert!(
            run_block(&manifest(Some("wasm"), vec!["workspace.read", "network"]))
                .unwrap_or_default()
                .contains("network")
        );
        assert!(run_block(&manifest(Some("wasm"), vec!["network"]))
            .unwrap_or_default()
            .contains("network"));
        assert!(run_block(&manifest(Some("wasm"), vec!["workspace.read"])).is_none());
        assert!(run_block(&manifest(
            Some("wasm"),
            vec!["workspace.read", "workspace.write"]
        ))
        .is_none());
        assert!(run_block(&manifest(Some("wasm"), vec!["terminal.run"]))
            .unwrap_or_default()
            .contains("terminal.run"));
        assert!(run_block(&manifest(Some("wasm"), vec![])).is_none());
    }

    #[test]
    fn inspection_exposes_the_runtime_and_the_run_block_reason() {
        let root = scratch("wasm-inspect");
        let plain = root.join("plain-tools");
        fs::create_dir_all(&plain).expect("folder");
        fs::write(
            plain.join("manifest.json"),
            r#"{"id":"plain-tools","name":"Plain Tools","version":"1.0.0","runtime":"wasm","entry":"tool.wasm"}"#,
        )
        .expect("manifest");
        fs::write(plain.join("tool.wasm"), b"\0asm").expect("entry");

        let asking = root.join("wasm-tools");
        fs::create_dir_all(&asking).expect("folder");
        fs::write(
            asking.join("manifest.json"),
            r#"{"id":"wasm-tools","name":"Wasm Tools","version":"1.0.0","runtime":"wasm","entry":"tool.wasm","permissions":["workspace.read"]}"#,
        )
        .expect("manifest");
        fs::write(asking.join("tool.wasm"), b"\0asm").expect("entry");

        let entries = inspect(&root);
        let by_id = |id: &str| {
            entries
                .iter()
                .find(|entry| entry["id"] == id)
                .unwrap_or_else(|| panic!("entry {id}"))
        };
        assert_eq!(by_id("plain-tools")["runtime"], "wasm");
        assert_eq!(by_id("plain-tools")["runnable"], true);
        assert_eq!(by_id("plain-tools")["blockedReason"], "");
        // The workspace host functions exist as of 2.21.0, so a read grant is
        // runnable; the reason column stays empty.
        assert_eq!(by_id("wasm-tools")["runnable"], true);
        assert_eq!(by_id("wasm-tools")["blockedReason"], "");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_valid_manifest_is_normalised_and_permissions_are_deduped() {
        let parsed = parse_manifest(
            "hello-tools",
            &json!({
                "id": "hello-tools",
                "name": "  Hello Tools  ",
                "version": "0.1.0",
                "description": "Does one thing.",
                "author": "Someone",
                "entry": "main.js",
                "permissions": ["workspace.read", "workspace.read", "network"]
            }),
        )
        .expect("valid manifest");
        assert_eq!(parsed.name, "Hello Tools");
        assert_eq!(parsed.entry.as_deref(), Some("main.js"));
        assert_eq!(parsed.permissions, vec!["workspace.read", "network"]);
    }

    #[test]
    fn a_manifest_that_breaks_a_rule_is_refused_with_a_reason() {
        let base = json!({
            "id": "hello-tools",
            "name": "Hello Tools",
            "version": "0.1.0"
        });
        let with = |key: &str, value: Value| {
            let mut copy = base.clone();
            copy[key] = value;
            copy
        };
        assert!(parse_manifest("other-folder", &base)
            .unwrap_err()
            .contains("does not match"));
        assert!(
            parse_manifest("hello-tools", &with("version", json!("1.2")))
                .unwrap_err()
                .contains("1.2.3")
        );
        assert!(
            parse_manifest("hello-tools", &with("entry", json!("../escape.js")))
                .unwrap_err()
                .contains("relative path")
        );
        assert!(
            parse_manifest("hello-tools", &with("entry", json!("/abs.js")))
                .unwrap_err()
                .contains("relative path")
        );
        assert!(parse_manifest(
            "hello-tools",
            &with("permissions", json!(["workspace.destroy"]))
        )
        .unwrap_err()
        .contains("unknown permission"));
        assert!(parse_manifest("hello-tools", &with("name", json!("   ")))
            .unwrap_err()
            .contains("name"));
        assert!(parse_manifest("hello-tools", &json!([1, 2])).is_err());
    }

    #[test]
    fn inspection_reports_each_folder_and_keeps_the_valid_ones() {
        let root = scratch("inspect");
        let good = root.join("hello-tools");
        fs::create_dir_all(&good).expect("good folder");
        fs::write(
            good.join("manifest.json"),
            r#"{"id":"hello-tools","name":"Hello Tools","version":"1.0.0","entry":"main.js","permissions":["workspace.read"]}"#,
        )
        .expect("good manifest");
        fs::write(good.join("main.js"), "export default {};\n").expect("entry file");

        let bad = root.join("bad-tools");
        fs::create_dir_all(&bad).expect("bad folder");
        fs::write(
            bad.join("manifest.json"),
            r#"{"id":"bad-tools","name":"Bad","version":"1.0.0","permissions":["root"]}"#,
        )
        .expect("bad manifest");

        let empty = root.join("empty");
        fs::create_dir_all(&empty).expect("empty folder");

        let entries = inspect(&root);
        assert_eq!(entries.len(), 3);
        let by_id = |id: &str| {
            entries
                .iter()
                .find(|entry| entry["id"] == id)
                .unwrap_or_else(|| panic!("entry {id}"))
        };
        assert_eq!(by_id("hello-tools")["valid"], true);
        assert_eq!(by_id("hello-tools")["permissions"][0], "workspace.read");
        assert_eq!(by_id("bad-tools")["valid"], false);
        assert!(by_id("bad-tools")["error"]
            .as_str()
            .unwrap_or("")
            .contains("unknown permission"));
        assert_eq!(by_id("empty")["valid"], false);
        assert!(by_id("empty")["error"]
            .as_str()
            .unwrap_or("")
            .contains("missing"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_newer_studio_requirement_is_valid_but_incompatible() {
        let parsed = parse_manifest(
            "future-tools",
            &json!({
                "id": "future-tools", "name": "Future Tools", "version": "1.0.0",
                "runtime": "wasm", "entry": "tool.wasm", "requiresStudio": "9.0.0"
            }),
        )
        .expect("manifest");
        assert_eq!(parsed.requires_studio.as_deref(), Some("9.0.0"));
        let reason = compatibility_block(&parsed, "2.25.0").expect("incompatible");
        assert!(
            reason.contains("9.0.0") && reason.contains("2.25.0"),
            "{reason}"
        );
        // The requirement is a floor, and equality satisfies it.
        assert!(compatibility_block(&parsed, "9.0.0").is_none());
        assert!(compatibility_block(&parsed, "10.1.2").is_none());
        assert!(compatibility_block(&parsed, "8.99.99").is_some());

        // A folder that declares nothing keeps working everywhere, and its
        // manifest schema defaults to the one this build reads.
        let plain = parse_manifest(
            "plain-tools",
            &json!({ "id": "plain-tools", "name": "Plain", "version": "1.0.0" }),
        )
        .expect("manifest");
        assert_eq!(plain.api_version, SUPPORTED_API_VERSION);
        assert!(plain.requires_studio.is_none());
        assert!(compatibility_block(&plain, "1.0.0").is_none());
    }

    #[test]
    fn versions_compare_numerically_not_as_text() {
        assert!(version_at_least("2.25.0", "2.9.0"));
        assert!(!version_at_least("2.9.0", "2.25.0"));
        assert!(version_at_least("2.25.0", "2.25.0"));
        assert!(version_at_least("3.0.0", "2.99.99"));
        assert!(!version_at_least("2.25.0", "2.25.1"));
    }

    #[test]
    fn a_manifest_schema_from_the_future_is_refused() {
        let base = json!({ "id": "x", "name": "X", "version": "1.0.0" });
        let with = |key: &str, value: Value| {
            let mut copy = base.clone();
            copy[key] = value;
            copy
        };
        assert!(parse_manifest("x", &with("apiVersion", json!(2)))
            .unwrap_err()
            .contains("apiVersion 2"));
        assert!(parse_manifest("x", &with("apiVersion", json!(0)))
            .unwrap_err()
            .contains("positive integer"));
        assert!(parse_manifest("x", &with("apiVersion", json!("1")))
            .unwrap_err()
            .contains("positive integer"));
        assert!(parse_manifest("x", &with("apiVersion", json!(1))).is_ok());
        assert!(parse_manifest("x", &with("requiresStudio", json!("next")))
            .unwrap_err()
            .contains("1.2.3"));
        assert!(parse_manifest("x", &with("requiresStudio", json!(3)))
            .unwrap_err()
            .contains("version string"));
    }

    #[test]
    fn dependencies_are_validated_and_deduped() {
        let base = json!({ "id": "x", "name": "X", "version": "1.0.0" });
        let with = |value: Value| {
            let mut copy = base.clone();
            copy["dependencies"] = value;
            copy
        };
        let parsed = parse_manifest(
            "x",
            &with(json!(["base-tools", "base-tools", "other_tools"])),
        )
        .expect("manifest");
        assert_eq!(parsed.dependencies, vec!["base-tools", "other_tools"]);
        assert!(parse_manifest("x", &with(json!(["x"])))
            .unwrap_err()
            .contains("itself"));
        assert!(parse_manifest("x", &with(json!(["Bad Id"])))
            .unwrap_err()
            .contains("not a valid extension id"));
        assert!(parse_manifest("x", &with(json!("base-tools")))
            .unwrap_err()
            .contains("list of extension ids"));
        let many: Vec<String> = (0..MAX_DEPENDENCIES + 1)
            .map(|index| format!("dep-{index}"))
            .collect();
        assert!(parse_manifest("x", &with(json!(many)))
            .unwrap_err()
            .contains("at most"));
    }

    #[test]
    fn a_missing_dependency_blocks_a_run_until_it_is_installed() {
        let root = scratch("dependencies");
        let base = root.join("base-tools");
        fs::create_dir_all(&base).expect("base folder");
        fs::write(
            base.join("manifest.json"),
            r#"{"id":"base-tools","name":"Base","version":"1.0.0","runtime":"wasm","entry":"tool.wasm"}"#,
        )
        .expect("base manifest");
        fs::write(base.join("tool.wasm"), b"\0asm").expect("base entry");

        let addon = root.join("addon-tools");
        fs::create_dir_all(&addon).expect("addon folder");
        fs::write(
            addon.join("manifest.json"),
            r#"{"id":"addon-tools","name":"Addon","version":"1.0.0","runtime":"wasm","entry":"tool.wasm","dependencies":["base-tools"]}"#,
        )
        .expect("addon manifest");
        fs::write(addon.join("tool.wasm"), b"\0asm").expect("addon entry");

        let entries = inspect(&root);
        let by_id = |id: &str| {
            entries
                .iter()
                .find(|entry| entry["id"] == id)
                .unwrap_or_else(|| panic!("entry {id}"))
        };
        assert_eq!(by_id("addon-tools")["valid"], true);
        assert_eq!(by_id("addon-tools")["missingDependencies"], json!([]));
        assert_eq!(by_id("addon-tools")["runnable"], true);
        assert_eq!(by_id("addon-tools")["dependencies"], json!(["base-tools"]));

        // The dependency disappears: the addon stays valid, names what is
        // missing and cannot run.
        remove_extension(&root, "base-tools").expect("remove base");
        let entries = inspect(&root);
        let addon = entries
            .iter()
            .find(|entry| entry["id"] == "addon-tools")
            .expect("addon entry");
        assert_eq!(addon["valid"], true);
        assert_eq!(addon["missingDependencies"], json!(["base-tools"]));
        assert_eq!(addon["runnable"], false);
        assert!(addon["blockedReason"]
            .as_str()
            .unwrap_or("")
            .contains("base-tools"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_incompatible_folder_is_not_installed() {
        let root = scratch("install-incompatible");
        let source = root.join("src").join("future-tools");
        fs::create_dir_all(&source).expect("source folder");
        fs::write(
            source.join("manifest.json"),
            r#"{"id":"future-tools","name":"Future","version":"1.0.0","runtime":"wasm","entry":"tool.wasm","requiresStudio":"99.0.0"}"#,
        )
        .expect("manifest");
        fs::write(source.join("tool.wasm"), b"\0asm").expect("entry");

        let target = root.join("extensions");
        let error = install_into(&source, &target).expect_err("incompatible");
        assert!(error.contains("99.0.0"), "{error}");
        assert!(!target.join("future-tools").exists(), "nothing left behind");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_arguments_are_checked_against_the_declared_schema() {
        let definition = json!({
            "name": "echo", "description": "Echoes its text.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }
        });
        assert!(check_tool_arguments(&definition, "echo", &json!({ "text": "hi" })).is_ok());
        let missing = check_tool_arguments(&definition, "echo", &json!({})).unwrap_err();
        assert!(missing.contains("echo"), "{missing}");
        assert!(missing.contains("text"), "{missing}");
        assert!(check_tool_arguments(&definition, "echo", &json!({ "text": 3 })).is_err());
        // Arguments are an object, exactly as for an MCP call.
        assert!(check_tool_arguments(&definition, "echo", &json!([])).is_err());

        // No schema means the tool takes whatever it can serialize — both when
        // the key is absent and when `inspect` wrote a null for it.
        let open = json!({ "name": "echo", "description": "a" });
        assert!(check_tool_arguments(&open, "echo", &json!({ "anything": 1 })).is_ok());
        let null = json!({ "name": "echo", "description": "a", "inputSchema": null });
        assert!(check_tool_arguments(&null, "echo", &json!({ "anything": 1 })).is_ok());
        // A boolean schema carries its whole meaning.
        let never = json!({ "name": "echo", "description": "a", "inputSchema": false });
        assert!(check_tool_arguments(&never, "echo", &json!({})).is_err());
        let always = json!({ "name": "echo", "description": "a", "inputSchema": true });
        assert!(check_tool_arguments(&always, "echo", &json!({})).is_ok());
    }

    #[test]
    fn declared_tools_are_validated_before_the_folder_is_trusted() {
        let base = json!({
            "id": "x", "name": "X", "version": "1.0.0",
            "runtime": "wasm", "entry": "tool.wasm"
        });
        let with = |value: Value| {
            let mut copy = base.clone();
            copy["tools"] = value;
            copy
        };
        let parsed = parse_manifest(
            "x",
            &with(json!([
                {"name": "count_words", "description": "Counts words.", "inputSchema": {"type": "object"}},
                {"name": "echo", "description": "Echoes its text.", "entry": "echo.wasm"}
            ])),
        )
        .expect("tools");
        assert_eq!(parsed.tools.len(), 2);
        assert_eq!(parsed.tools[1].entry.as_deref(), Some("echo.wasm"));
        assert!(parsed.tools[0].input_schema.is_some());

        assert!(parse_manifest(
            "x",
            &with(json!([{"name":"dup","description":"a"},{"name":"dup","description":"b"}]))
        )
        .unwrap_err()
        .contains("twice"));
        assert!(
            parse_manifest("x", &with(json!([{"name":"Bad Name","description":"a"}])))
                .unwrap_err()
                .contains("tool name")
        );
        assert!(parse_manifest("x", &with(json!([{"name":"ok"}])))
            .unwrap_err()
            .contains("description"));
        assert!(parse_manifest(
            "x",
            &with(json!([{"name":"ok","description":"a","entry":"main.js"}]))
        )
        .unwrap_err()
        .contains(".wasm path"));
        assert!(parse_manifest(
            "x",
            &with(json!([{"name":"ok","description":"a","inputSchema":7}]))
        )
        .unwrap_err()
        .contains("object or boolean"));
        assert!(parse_manifest("x", &with(json!([])))
            .unwrap_err()
            .contains("not be empty"));
        assert!(parse_manifest("x", &with(json!("nope")))
            .unwrap_err()
            .contains("list of objects"));

        // A tool without the sandbox runtime would be a promise nothing keeps.
        let mut no_runtime = base.clone();
        no_runtime["runtime"] = Value::Null;
        no_runtime["entry"] = Value::Null;
        no_runtime["tools"] = json!([{"name": "ok", "description": "a"}]);
        assert!(parse_manifest("x", &no_runtime)
            .unwrap_err()
            .contains("Tools need runtime"));
    }

    #[test]
    fn a_tool_module_must_exist_and_inspection_lists_the_tools() {
        let root = scratch("tools-inspect");
        let folder = root.join("toolbox");
        fs::create_dir_all(&folder).expect("folder");
        fs::write(
            folder.join("manifest.json"),
            r#"{"id":"toolbox","name":"Toolbox","version":"1.0.0","runtime":"wasm","entry":"run.wasm","tools":[{"name":"echo","description":"Echoes its text.","entry":"echo.wasm"}]}"#,
        )
        .expect("manifest");
        fs::write(folder.join("run.wasm"), b"\0asm").expect("run module");
        fs::write(folder.join("echo.wasm"), b"\0asm").expect("echo module");

        let entries = inspect(&root);
        assert_eq!(entries[0]["valid"], true);
        assert_eq!(entries[0]["tools"][0]["name"], "echo");
        assert_eq!(entries[0]["tools"][0]["description"], "Echoes its text.");

        // The tool's own module is checked like the manifest entry.
        fs::remove_file(folder.join("echo.wasm")).expect("remove tool module");
        let entries = inspect(&root);
        assert_eq!(entries[0]["valid"], false);
        assert!(
            entries[0]["error"].as_str().unwrap_or("").contains("echo"),
            "{:?}",
            entries[0]["error"]
        );
        let _ = fs::remove_dir_all(&root);
    }
}
