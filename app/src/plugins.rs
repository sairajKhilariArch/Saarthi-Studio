//! Plugins: one catalog over the two things that extend Studio today.
//!
//! An installed **extension** is a folder with `manifest.json` (see
//! [`crate::extensions`]): it can contribute sandboxed tools, HTML panels and
//! declarative provider presets. An installed **skill** is a `SKILL.md`
//! instruction pack (see [`crate::skills`]): prompt text the model follows.
//! Both are plugins from a user's point of view, so this module is the one place
//! that lists them together, remembers whether each is enabled, stores per-plugin
//! settings and talks to a marketplace index.
//!
//! Nothing here executes plugin code. Extension execution stays in
//! [`crate::extension_run`] behind its own permission gates; disabling a plugin
//! here is enforced by the extension and skill entry points, so a disabled
//! plugin is not merely hidden from the list.
//!
//! ## Marketplace
//!
//! There is no bundled catalog and no invented entry: a marketplace exists only
//! after a person adds a registry URL. A registry is a JSON document
//! (`schemaVersion: 1`) listing plugins with a download URL and a SHA-256. An
//! install downloads the archive, verifies the checksum, rejects any archive
//! path that escapes the extraction folder, and then installs the folder through
//! the same validating path as a manual folder install. A registry that cannot
//! be read is reported as an error next to the entries that did load, never
//! papered over.

use crate::AppState;
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A registry document larger than this is refused before it is parsed.
const MAX_INDEX_BYTES: u64 = 512 * 1024;
/// One archive. Mirrors the extension installer's own ceilings, because the same
/// folder ends up in the extensions directory.
const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ARCHIVE_FILES: usize = 500;
const MAX_INDEX_ENTRIES: usize = 500;
const MAX_REGISTRIES: usize = 16;
const MAX_SETTINGS_BYTES: usize = 16 * 1024;
const MAX_SETTINGS_KEYS: usize = 32;
const MAX_SETTING_VALUE_CHARS: usize = 2000;
const MAX_TEXT: usize = 400;
const MAX_TAGS: usize = 8;

/// The kinds of plugin this build knows. A catalog entry from a registry naming
/// anything else is reported as unsupported instead of being guessed at.
const KINDS: [&str; 2] = ["extension", "skill"];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    registries: Vec<Registry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Registry {
    url: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    added_at: f64,
}

/// One marketplace listing, after validation. Every field is bounded and every
/// URL has already been checked, so the rest of the module can trust it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub category: String,
    pub tags: Vec<String>,
    pub permissions: Vec<String>,
    pub requires_studio: String,
    pub kind: String,
    pub download_url: String,
    pub sha256: String,
    pub registry: String,
}

fn plugins_db(state: &AppState) -> &Path {
    &state.database_path
}

fn registry_path(state: &AppState) -> PathBuf {
    state.config_path.with_file_name("plugin-registries.json")
}

fn extensions_root(state: &AppState) -> PathBuf {
    crate::extensions::extensions_dir(state)
}

/// `plugin_state` is the only persistent state this module owns: whether a
/// plugin is enabled, how a tool call reads its settings, and when either last
/// changed. A missing row means "enabled", so installing a folder never needs a
/// second write to make it usable.
pub(crate) fn initialize(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS plugin_state (
            kind TEXT NOT NULL, id TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            settings TEXT NOT NULL DEFAULT '{}',
            updated_at REAL NOT NULL DEFAULT 0,
            PRIMARY KEY(kind, id)
        );",
    )
}

fn read_row(db: &Path, kind: &str, id: &str) -> Option<(bool, String)> {
    let conn = crate::open_db(db).ok()?;
    let _ = initialize(&conn);
    conn.query_row(
        "SELECT enabled, settings FROM plugin_state WHERE kind = ?1 AND id = ?2",
        rusqlite::params![kind, id],
        |row| Ok((row.get::<_, i64>(0)? != 0, row.get::<_, String>(1)?)),
    )
    .ok()
}

fn write_row(
    db: &Path,
    kind: &str,
    id: &str,
    enabled: Option<bool>,
    settings: Option<&str>,
) -> Result<(), String> {
    let conn =
        crate::open_db(db).map_err(|error| format!("Could not open the database: {error}"))?;
    initialize(&conn).map_err(|error| format!("Could not create plugin_state: {error}"))?;
    let (current_enabled, current_settings) = read_row(db, kind, id).unwrap_or((true, "{}".into()));
    let enabled = enabled.unwrap_or(current_enabled);
    let settings = settings.unwrap_or(&current_settings);
    conn.execute(
        "INSERT INTO plugin_state(kind, id, enabled, settings, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(kind, id) DO UPDATE SET
           enabled = excluded.enabled,
           settings = excluded.settings,
           updated_at = excluded.updated_at",
        rusqlite::params![
            kind,
            id,
            if enabled { 1i64 } else { 0i64 },
            settings,
            crate::now_seconds()
        ],
    )
    .map_err(|error| format!("Could not save the plugin state: {error}"))?;
    Ok(())
}

/// Whether a plugin is enabled. A read failure answers `true`: a database that
/// cannot be read must not silently disable everything a person installed. The
/// failure is visible in the log and in the catalog's `stateError`.
fn enabled_at(db: &Path, kind: &str, id: &str) -> bool {
    read_row(db, kind, id)
        .map(|(enabled, _)| enabled)
        .unwrap_or(true)
}

/// The JSON text a plugin's tools read through the sandbox's `config` host call.
fn settings_at(db: &Path, kind: &str, id: &str) -> String {
    read_row(db, kind, id)
        .map(|(_, settings)| settings)
        .filter(|settings| !settings.is_empty())
        .unwrap_or_else(|| "{}".to_string())
}

/* ------------------------------------------------------------------ */
/* extension integration                                               */
/* ------------------------------------------------------------------ */

/// Whether an installed extension may run, be advertised to the model, contribute
/// a provider preset or serve a panel. Used by [`crate::extensions`].
pub(crate) fn extension_enabled(state: &AppState, id: &str) -> bool {
    enabled_at(plugins_db(state), "extension", id)
}

/// The settings text handed to one extension run or tool call.
pub(crate) fn extension_settings(state: &AppState, id: &str) -> String {
    settings_at(plugins_db(state), "extension", id)
}

/* ------------------------------------------------------------------ */
/* catalog                                                             */
/* ------------------------------------------------------------------ */

/// Merge extension and skill rows into one ordered list. Pure, so the merge is
/// tested without a server: extensions first (they can act), then skills.
pub(crate) fn merge(extensions: Vec<Value>, skills: Vec<Value>) -> Vec<Value> {
    let mut out = Vec::with_capacity(extensions.len() + skills.len());
    out.extend(extensions);
    out.extend(skills);
    out
}

/// The unified installed-plugin list: one row per extension and per skill, each
/// with its enabled flag and settings, and the counts the UI summarises.
pub(crate) fn catalog(state: &AppState) -> Vec<Value> {
    let extensions = crate::extensions::inspect(&extensions_root(state))
        .into_iter()
        .map(|entry| {
            let id = entry["id"].as_str().unwrap_or("").to_string();
            let enabled = extension_enabled(state, &id);
            json!({
                "kind": "extension",
                "id": entry["id"],
                "name": entry["name"],
                "version": entry["version"],
                "description": entry["description"],
                "author": entry["author"],
                "path": extensions_root(state).join(&id).display().to_string(),
                "permissions": entry["permissions"],
                "tools": entry["tools"],
                "panels": entry["panels"],
                "providers": entry["providers"],
                "valid": entry["valid"],
                "compatible": entry["compatible"],
                "runnable": enabled && entry["runnable"] == Value::Bool(true),
                "enabled": enabled,
                "blockedReason": entry["blockedReason"],
                "error": entry["error"],
                "settings": serde_json::from_str::<Value>(&settings_at(plugins_db(state), "extension", &id))
                    .unwrap_or_else(|_| json!({})),
            })
        })
        .collect();
    merge(extensions, crate::skills::plugin_catalog(state))
}

fn count(entries: &[Value], enabled: bool) -> usize {
    entries
        .iter()
        .filter(|entry| entry["enabled"] == Value::Bool(enabled))
        .count()
}

/* ------------------------------------------------------------------ */
/* routes: installed plugins                                           */
/* ------------------------------------------------------------------ */

pub(crate) async fn list(State(state): State<AppState>) -> Response<Body> {
    let entries = catalog(&state);
    Json(json!({
        "ok": true,
        "extensionsDirectory": extensions_root(&state).display().to_string(),
        "plugins": entries,
        "counts": {
            "total": entries.len(),
            "enabled": count(&entries, true),
            "disabled": count(&entries, false),
            "extensions": entries.iter().filter(|entry| entry["kind"] == "extension").count(),
            "skills": entries.iter().filter(|entry| entry["kind"] == "skill").count(),
        },
        "kinds": KINDS,
    }))
    .into_response()
}

/// Enable or disable one plugin. A skill's state lives where it always has (the
/// config's `disabled_skills` list) so the agent keeps honouring it; an
/// extension's lives in `plugin_state`.
pub(crate) async fn set_enabled(
    State(state): State<AppState>,
    AxumPath((kind, id)): AxumPath<(String, String)>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let Some(enabled) = input.get("enabled").and_then(Value::as_bool) else {
        return crate::json_error(StatusCode::BAD_REQUEST, "Provide enabled: true or false.");
    };
    if !KINDS.contains(&kind.as_str()) {
        return crate::json_error(StatusCode::BAD_REQUEST, "Unknown plugin kind.");
    }
    if id.is_empty() || id.len() > 64 {
        return crate::json_error(StatusCode::BAD_REQUEST, "Invalid plugin id.");
    }
    let result = if kind == "skill" {
        crate::skills::set_plugin_enabled(&state, &id, enabled)
    } else {
        write_row(plugins_db(&state), &kind, &id, Some(enabled), None)
    };
    match result {
        Ok(()) => {
            crate::oplog::record(
                "info",
                "plugins",
                "state",
                format!(
                    "{kind} {id}: {}",
                    if enabled { "enabled" } else { "disabled" }
                ),
            );
            Json(json!({ "ok": true, "id": id, "kind": kind, "enabled": enabled })).into_response()
        }
        Err(error) => crate::json_error(StatusCode::CONFLICT, error),
    }
}

/// Replace one extension's settings. The document is bounded and flat: a
/// setting is a string, number or boolean, so a plugin cannot smuggle a deep
/// structure into the sandbox or grow the database without limit.
pub(crate) async fn set_settings(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let Some(settings) = input.get("settings") else {
        return crate::json_error(StatusCode::BAD_REQUEST, "Provide a settings object.");
    };
    if let Err(error) = check_settings(settings) {
        return crate::json_error(StatusCode::BAD_REQUEST, error);
    }
    let text = serde_json::to_string(settings).unwrap_or_else(|_| "{}".to_string());
    match write_row(plugins_db(&state), "extension", &id, None, Some(&text)) {
        Ok(()) => {
            crate::oplog::record("info", "plugins", "settings", format!("{id}: updated"));
            Json(json!({ "ok": true, "id": id, "settings": settings })).into_response()
        }
        Err(error) => crate::json_error(StatusCode::CONFLICT, error),
    }
}

fn check_settings(settings: &Value) -> Result<(), String> {
    let Some(map) = settings.as_object() else {
        return Err("Settings must be a JSON object.".to_string());
    };
    if map.len() > MAX_SETTINGS_KEYS {
        return Err(format!("At most {MAX_SETTINGS_KEYS} settings are allowed."));
    }
    for (key, value) in map {
        if key.is_empty() || key.len() > 64 {
            return Err("A setting name must be 1-64 characters.".to_string());
        }
        match value {
            Value::String(text) if text.chars().count() <= MAX_SETTING_VALUE_CHARS => {}
            Value::String(_) => {
                return Err(format!(
                    "The setting \"{key}\" is longer than {MAX_SETTING_VALUE_CHARS} characters."
                ))
            }
            Value::Number(_) | Value::Bool(_) => {}
            _ => {
                return Err(format!(
                    "The setting \"{key}\" must be a string, number or boolean."
                ))
            }
        }
    }
    let size = serde_json::to_string(settings)
        .map(|text| text.len())
        .unwrap_or(0);
    if size > MAX_SETTINGS_BYTES {
        return Err(format!(
            "Settings are larger than {MAX_SETTINGS_BYTES} bytes."
        ));
    }
    Ok(())
}

/* ------------------------------------------------------------------ */
/* marketplace: registries                                             */
/* ------------------------------------------------------------------ */

fn load_registries_at(path: &Path) -> RegistryFile {
    let mut file: RegistryFile = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    // A registry added twice is one registry.
    file.registries
        .dedup_by(|left, right| left.url.eq_ignore_ascii_case(&right.url));
    file
}

fn save_registries_at(path: &Path, file: &RegistryFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(file).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| format!("Could not write {}: {error}", path.display()))
}

/// Only HTTPS, plus plain-http loopback so a registry can be served by a local
/// process during development. Anything else would let a typo send the app's
/// plugin installs over a cleartext hop.
fn check_url(raw: &str) -> Result<String, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Err("A registry URL is required.".to_string());
    }
    if text.chars().count() > 2000 {
        return Err("That URL is too long.".to_string());
    }
    let url = reqwest::Url::parse(text).map_err(|error| format!("That is not a URL: {error}"))?;
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let loopback = matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]" | "::1");
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        "http" => {
            return Err(
                "Use an https:// URL. Plain http is allowed only for localhost.".to_string(),
            )
        }
        other => return Err(format!("Unsupported URL scheme: {other}")),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("A registry URL must not carry credentials.".to_string());
    }
    Ok(url.to_string())
}

fn add_registry_at(path: &Path, raw: &str, name: &str) -> Result<Registry, String> {
    let url = check_url(raw)?;
    let mut file = load_registries_at(path);
    if let Some(existing) = file
        .registries
        .iter()
        .find(|registry| registry.url.eq_ignore_ascii_case(&url))
    {
        return Ok(existing.clone());
    }
    if file.registries.len() >= MAX_REGISTRIES {
        return Err(format!("At most {MAX_REGISTRIES} registries can be added."));
    }
    let registry = Registry {
        url,
        name: name.trim().chars().take(80).collect(),
        added_at: crate::now_seconds(),
    };
    file.registries.push(registry.clone());
    file.schema = 1;
    save_registries_at(path, &file)?;
    Ok(registry)
}

fn remove_registry_at(path: &Path, raw: &str) -> Result<bool, String> {
    let url = check_url(raw)?;
    let mut file = load_registries_at(path);
    let before = file.registries.len();
    file.registries
        .retain(|registry| !registry.url.eq_ignore_ascii_case(&url));
    if file.registries.len() == before {
        return Ok(false);
    }
    save_registries_at(path, &file)?;
    Ok(true)
}

/* ------------------------------------------------------------------ */
/* marketplace: index parsing                                          */
/* ------------------------------------------------------------------ */

fn text_of(value: &Value, key: &str, max: usize) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .take(max)
        .collect()
}

fn check_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn check_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 4 && part.chars().all(|c| c.is_ascii_digit())
        })
}

fn check_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// One registry document, validated. An entry that fails validation is named in
/// `errors` and left out of `entries`; the registry is not discarded because one
/// listing is wrong.
pub(crate) fn parse_index(body: &str, registry_url: &str) -> (Vec<Entry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    let document: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(error) => return (entries, vec![format!("The registry is not JSON: {error}")]),
    };
    let schema = document
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if schema != 1 {
        return (
            entries,
            vec![format!(
                "This registry uses schemaVersion {schema}; this build reads 1."
            )],
        );
    }
    let Some(list) = document.get("plugins").and_then(Value::as_array) else {
        return (
            entries,
            vec!["The registry has no plugins list.".to_string()],
        );
    };
    if list.len() > MAX_INDEX_ENTRIES {
        errors.push(format!(
            "The registry lists {} plugins; only the first {MAX_INDEX_ENTRIES} are read.",
            list.len()
        ));
    }
    for (index, raw) in list.iter().take(MAX_INDEX_ENTRIES).enumerate() {
        let id = text_of(raw, "id", 40);
        let label = if id.is_empty() {
            format!("listing {}", index + 1)
        } else {
            id.clone()
        };
        let kind = match raw.get("kind").and_then(Value::as_str) {
            Some(kind) if KINDS.contains(&kind) => kind.to_string(),
            None => "extension".to_string(),
            Some(kind) => {
                errors.push(format!("{label}: unsupported plugin kind \"{kind}\"."));
                continue;
            }
        };
        if !check_id(&id) {
            errors.push(format!(
                "{label}: id must be 1-40 lowercase letters, digits, - or _."
            ));
            continue;
        }
        let version = text_of(raw, "version", 20);
        if !check_version(&version) {
            errors.push(format!("{label}: version must look like 1.2.3."));
            continue;
        }
        let sha256 = text_of(raw, "sha256", 64).to_ascii_lowercase();
        if !check_sha256(&sha256) {
            errors.push(format!("{label}: sha256 must be 64 hex characters."));
            continue;
        }
        let download = match raw.get("downloadUrl").and_then(Value::as_str) {
            Some(url) => match check_url(url) {
                Ok(url) => url,
                Err(error) => {
                    errors.push(format!("{label}: downloadUrl: {error}"));
                    continue;
                }
            },
            None => {
                errors.push(format!("{label}: a downloadUrl is required."));
                continue;
            }
        };
        let permissions: Vec<String> = raw
            .get("permissions")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(|permission| permission.trim().chars().take(40).collect())
                    .filter(|permission: &String| !permission.is_empty())
                    .take(16)
                    .collect()
            })
            .unwrap_or_default();
        let tags: Vec<String> = raw
            .get("tags")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(|tag| tag.trim().to_ascii_lowercase().chars().take(24).collect())
                    .filter(|tag: &String| !tag.is_empty())
                    .take(MAX_TAGS)
                    .collect()
            })
            .unwrap_or_default();
        entries.push(Entry {
            id,
            name: {
                let name = text_of(raw, "name", 80);
                if name.is_empty() {
                    errors.push(format!("{label}: a name is required."));
                    continue;
                }
                name
            },
            version,
            description: text_of(raw, "description", MAX_TEXT),
            author: text_of(raw, "author", 80),
            category: text_of(raw, "category", 40).to_ascii_lowercase(),
            tags,
            permissions,
            requires_studio: text_of(raw, "requiresStudio", 20),
            kind,
            download_url: download,
            sha256,
            registry: registry_url.to_string(),
        });
    }
    (entries, errors)
}

/* ------------------------------------------------------------------ */
/* marketplace: fetch and install                                      */
/* ------------------------------------------------------------------ */

async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(15))
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|error| format!("Could not reach the registry: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("The registry answered {}.", response.status()));
    }
    if let Some(length) = response.content_length() {
        if length > MAX_INDEX_BYTES {
            return Err(format!(
                "The registry is larger than {MAX_INDEX_BYTES} bytes."
            ));
        }
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Could not read the registry: {error}"))?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err(format!(
            "The registry is larger than {MAX_INDEX_BYTES} bytes."
        ));
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| "The registry is not UTF-8 text.".to_string())
}

/// Every registry's entries, plus one error per registry that could not be read.
/// One broken URL must not hide the catalogs that work.
async fn collect(client: &reqwest::Client, registries: &[Registry]) -> (Vec<Entry>, Vec<Value>) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for registry in registries {
        match fetch_text(client, &registry.url).await {
            Ok(body) => {
                let (mut found, problems) = parse_index(&body, &registry.url);
                for problem in problems {
                    errors.push(json!({ "registry": registry.url, "error": problem }));
                }
                // Two registries may list the same id; the first one wins and the
                // duplicate is reported rather than silently shadowed.
                for entry in found.drain(..) {
                    if let Some(first) = entries
                        .iter()
                        .find(|existing: &&Entry| existing.id == entry.id)
                    {
                        errors.push(json!({
                            "registry": registry.url,
                            "error": format!(
                                "\"{}\" is already listed by {}.",
                                entry.id, first.registry
                            ),
                        }));
                        continue;
                    }
                    entries.push(entry);
                }
            }
            Err(error) => errors.push(json!({ "registry": registry.url, "error": error })),
        }
    }
    (entries, errors)
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|error| format!("Could not download the plugin: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("The download answered {}.", response.status()));
    }
    if let Some(length) = response.content_length() {
        if length > MAX_ARCHIVE_BYTES {
            return Err(format!(
                "The archive is larger than {MAX_ARCHIVE_BYTES} bytes."
            ));
        }
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Could not read the download: {error}"))?;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "The archive is larger than {MAX_ARCHIVE_BYTES} bytes."
        ));
    }
    Ok(bytes.to_vec())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// A zip member name is usable only when it is a plain relative path. Zip's own
/// `enclosed_name` rejects `..` and absolute names; this adds the Windows
/// separators and drive letters it does not see on every platform.
fn safe_archive_path(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.starts_with('/') || name.contains('\\') || name.contains(':') {
        return None;
    }
    let mut path = PathBuf::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        path.push(part);
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

/// Extract an archive under `target`, refusing traversal, excess file counts and
/// excess total size. Returns the number of files written.
fn extract(bytes: &[u8], target: &Path) -> Result<usize, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("The download is not a zip archive: {error}"))?;
    if archive.len() > MAX_ARCHIVE_FILES {
        return Err(format!(
            "The archive holds {} files; the limit is {MAX_ARCHIVE_FILES}.",
            archive.len()
        ));
    }
    fs::create_dir_all(target)
        .map_err(|error| format!("Could not create the staging folder: {error}"))?;
    let mut files = 0usize;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut member = archive
            .by_index(index)
            .map_err(|error| format!("Could not read archive entry {index}: {error}"))?;
        let name = member
            .name()
            .map_err(|error| format!("Could not read archive entry {index}: {error}"))?
            .to_string();
        let Some(relative) = safe_archive_path(&name) else {
            return Err(format!(
                "The archive entry \"{name}\" leaves the plugin folder."
            ));
        };
        let destination = target.join(&relative);
        if member.is_dir() {
            fs::create_dir_all(&destination)
                .map_err(|error| format!("Could not create {}: {error}", destination.display()))?;
            continue;
        }
        total = total.saturating_add(member.size());
        if total > MAX_ARCHIVE_BYTES {
            return Err(format!(
                "The archive expands past {MAX_ARCHIVE_BYTES} bytes."
            ));
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        }
        let mut buffer = Vec::new();
        member
            .read_to_end(&mut buffer)
            .map_err(|error| format!("Could not read {name}: {error}"))?;
        fs::write(&destination, &buffer)
            .map_err(|error| format!("Could not write {}: {error}", destination.display()))?;
        files += 1;
    }
    Ok(files)
}

/// The folder inside an extracted archive that holds `manifest.json`: either the
/// archive root or its single top-level directory (the shape `zip -r name.zip
/// name/` produces).
fn find_plugin_folder(root: &Path) -> Result<PathBuf, String> {
    if root.join("manifest.json").is_file() {
        return Ok(root.to_path_buf());
    }
    let mut candidates: Vec<PathBuf> = fs::read_dir(root)
        .map_err(|error| format!("Could not read the staging folder: {error}"))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    candidates.sort();
    let matches: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|path| path.join("manifest.json").is_file())
        .collect();
    match matches.len() {
        1 => Ok(matches[0].clone()),
        0 => Err("The archive holds no manifest.json, so it is not a plugin.".to_string()),
        _ => Err("The archive holds more than one plugin; install one at a time.".to_string()),
    }
}

/// Install a verified archive. The manifest's id must match the listing's id, so
/// a registry cannot hand over a different plugin than the one that was chosen.
fn install_archive(bytes: &[u8], entry: &Entry, extensions_root: &Path) -> Result<String, String> {
    let staging = std::env::temp_dir().join(format!(
        "sarthi-plugin-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        extract(bytes, &staging)?;
        let folder = find_plugin_folder(&staging)?;
        let raw = crate::extensions::raw_manifest(&folder)?;
        let declared = raw.get("id").and_then(Value::as_str).unwrap_or("");
        if declared != entry.id {
            return Err(format!(
                "The archive declares id \"{declared}\", not \"{}\".",
                entry.id
            ));
        }
        // The installer keys on the folder name, so stage the folder under the
        // id the listing promised instead of the name the archive happened to use.
        let named = staging.join(&entry.id);
        fs::rename(&folder, &named)
            .map_err(|error| format!("Could not stage the plugin folder: {error}"))?;
        crate::extensions::install_into(&named, extensions_root)
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

fn format_permissions(permissions: &[String]) -> String {
    if permissions.is_empty() {
        "no permissions".to_string()
    } else {
        permissions.join(", ")
    }
}

/* ------------------------------------------------------------------ */
/* routes: marketplace                                                 */
/* ------------------------------------------------------------------ */

pub(crate) async fn marketplace(State(state): State<AppState>) -> Response<Body> {
    let registries = load_registries_at(&registry_path(&state));
    let (entries, errors) = collect(&state.client, &registries.registries).await;
    let listings: Vec<Value> = entries
        .iter()
        .map(|entry| {
            json!({
                "id": entry.id,
                "name": entry.name,
                "version": entry.version,
                "description": entry.description,
                "author": entry.author,
                "category": entry.category,
                "tags": entry.tags,
                "permissions": entry.permissions,
                "requiresStudio": entry.requires_studio,
                "kind": entry.kind,
                "registry": entry.registry,
            })
        })
        .collect();
    Json(json!({
        "ok": true,
        "configured": !registries.registries.is_empty(),
        "registries": registries
            .registries
            .iter()
            .map(|registry| json!({
                "url": registry.url,
                "name": registry.name,
                "addedAt": registry.added_at,
            }))
            .collect::<Vec<Value>>(),
        "entries": listings,
        "errors": errors,
    }))
    .into_response()
}

pub(crate) async fn add_registry(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let url = input.get("url").and_then(Value::as_str).unwrap_or("");
    let name = input.get("name").and_then(Value::as_str).unwrap_or("");
    match add_registry_at(&registry_path(&state), url, name) {
        Ok(registry) => {
            crate::oplog::record("info", "plugins", "registry-add", &registry.url);
            Json(json!({ "ok": true, "registry": { "url": registry.url, "name": registry.name } }))
                .into_response()
        }
        Err(error) => crate::json_error(StatusCode::BAD_REQUEST, error),
    }
}

pub(crate) async fn remove_registry(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let url = input.get("url").and_then(Value::as_str).unwrap_or("");
    match remove_registry_at(&registry_path(&state), url) {
        Ok(true) => {
            crate::oplog::record("info", "plugins", "registry-remove", url.trim());
            Json(json!({ "ok": true })).into_response()
        }
        Ok(false) => crate::json_error(StatusCode::NOT_FOUND, "That registry is not configured."),
        Err(error) => crate::json_error(StatusCode::BAD_REQUEST, error),
    }
}

pub(crate) async fn install(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let id = input.get("id").and_then(Value::as_str).unwrap_or("").trim();
    if !check_id(id) {
        return crate::json_error(StatusCode::BAD_REQUEST, "Invalid plugin id.");
    }
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let root = extensions_root(&state);
    let target = root.join(id).display().to_string();
    if let Some(response) = crate::permissions::guard_user(
        &state,
        crate::permissions::Action::FsWrite,
        &target,
        "",
        approve,
        "install a plugin from the marketplace",
    ) {
        return response;
    }
    let registries = load_registries_at(&registry_path(&state));
    if registries.registries.is_empty() {
        return crate::json_error(
            StatusCode::CONFLICT,
            "No marketplace registry is configured. Add one before installing.",
        );
    }
    let (entries, errors) = collect(&state.client, &registries.registries).await;
    let Some(entry) = entries.into_iter().find(|entry| entry.id == id) else {
        let detail = errors
            .first()
            .and_then(|error| error["error"].as_str())
            .unwrap_or("No configured registry lists that plugin.");
        return crate::json_error(StatusCode::NOT_FOUND, detail);
    };
    if entry.kind != "extension" {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "Skills install from the Skills page, not the plugin marketplace.",
        );
    }
    let bytes = match download(&state.client, &entry.download_url).await {
        Ok(bytes) => bytes,
        Err(error) => return crate::json_error(StatusCode::BAD_GATEWAY, error),
    };
    let digest = hex(Sha256::digest(&bytes).as_slice());
    if digest != entry.sha256 {
        return crate::json_error(
            StatusCode::CONFLICT,
            format!(
                "The download does not match its checksum (expected {}, got {digest}).",
                entry.sha256
            ),
        );
    }
    // What the listing promised, kept for the receipt before the entry moves into
    // the blocking install task.
    let version = entry.version.clone();
    let permissions = entry.permissions.clone();
    let installed =
        tokio::task::spawn_blocking(move || install_archive(&bytes, &entry, &root)).await;
    match installed {
        Ok(Ok(installed)) => {
            crate::oplog::record(
                "info",
                "plugins",
                "marketplace-install",
                format!(
                    "{installed}: {version} ({})",
                    format_permissions(&permissions)
                ),
            );
            Json(json!({
                "ok": true,
                "id": installed,
                "version": version,
                "permissions": permissions,
            }))
            .into_response()
        }
        Ok(Err(error)) => crate::json_error(StatusCode::CONFLICT, error),
        Err(error) => crate::json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("The install task failed: {error}"),
        ),
    }
}

/* ------------------------------------------------------------------ */
/* routes: validation                                                  */
/* ------------------------------------------------------------------ */

/// Validate a manifest without installing anything: a folder path when one is
/// given, otherwise the manifest object itself. This is the same parser the
/// installer uses (see [`crate::extensions::validation_report`]), so a manifest
/// that validates here installs there.
pub(crate) async fn validate(Json(input): Json<Value>) -> Response<Body> {
    if let Some(path) = input
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return Json(crate::extensions::folder_report(Path::new(path))).into_response();
    }
    let Some(manifest) = input.get("manifest") else {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "Provide a manifest object or the path of a plugin folder.",
        );
    };
    let folder = input.get("folder").and_then(Value::as_str).unwrap_or("");
    Json(crate::extensions::validation_report(folder, manifest)).into_response()
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/plugins", get(list))
        .route("/api/plugins/marketplace", get(marketplace))
        .route("/api/plugins/marketplace/registries", post(add_registry))
        .route(
            "/api/plugins/marketplace/registries/remove",
            post(remove_registry),
        )
        .route("/api/plugins/marketplace/install", post(install))
        .route("/api/plugins/validate", post(validate))
        .route("/api/plugins/:kind/:id/state", post(set_enabled))
        .route("/api/plugins/extension/:id/settings", post(set_settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sarthi-plugins-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn db(dir: &Path) -> PathBuf {
        let path = dir.join("studio.db");
        let conn = crate::open_db(&path).unwrap();
        initialize(&conn).unwrap();
        path
    }

    #[test]
    fn a_plugin_is_enabled_until_it_is_turned_off() {
        let dir = scratch("enabled");
        let path = db(&dir);
        assert!(enabled_at(&path, "extension", "hello-tools"));
        write_row(&path, "extension", "hello-tools", Some(false), None).unwrap();
        assert!(!enabled_at(&path, "extension", "hello-tools"));
        // The kind is part of the key: a skill with the same id is untouched.
        assert!(enabled_at(&path, "skill", "hello-tools"));
        write_row(&path, "extension", "hello-tools", Some(true), None).unwrap();
        assert!(enabled_at(&path, "extension", "hello-tools"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn disabling_keeps_the_settings_and_settings_keep_the_state() {
        let dir = scratch("both");
        let path = db(&dir);
        write_row(
            &path,
            "extension",
            "hello-tools",
            None,
            Some("{\"mode\":\"fast\"}"),
        )
        .unwrap();
        assert_eq!(
            settings_at(&path, "extension", "hello-tools"),
            "{\"mode\":\"fast\"}"
        );
        assert!(enabled_at(&path, "extension", "hello-tools"));
        write_row(&path, "extension", "hello-tools", Some(false), None).unwrap();
        assert_eq!(
            settings_at(&path, "extension", "hello-tools"),
            "{\"mode\":\"fast\"}"
        );
        assert!(!enabled_at(&path, "extension", "hello-tools"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_plugin_reads_as_enabled_with_empty_settings() {
        let dir = scratch("unknown");
        let path = db(&dir);
        assert_eq!(settings_at(&path, "extension", "never-installed"), "{}");
        assert!(enabled_at(&path, "extension", "never-installed"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_are_flat_bounded_and_typed() {
        assert!(
            check_settings(&json!({ "endpoint": "https://example.com", "retries": 2 })).is_ok()
        );
        assert!(check_settings(&json!("not an object")).is_err());
        assert!(check_settings(&json!({ "nested": { "a": 1 } })).is_err());
        assert!(check_settings(&json!({ "list": [1, 2] })).is_err());
        assert!(check_settings(&json!({ "": "empty name" })).is_err());
        let long = "x".repeat(MAX_SETTING_VALUE_CHARS + 1);
        assert!(check_settings(&json!({ "long": long })).is_err());
        let many = (0..MAX_SETTINGS_KEYS + 1)
            .map(|index| (format!("k{index}"), json!("v")))
            .collect::<serde_json::Map<String, Value>>();
        assert!(check_settings(&Value::Object(many)).is_err());
    }

    #[test]
    fn a_registry_must_be_https_or_loopback() {
        assert!(check_url("https://plugins.example.com/index.json").is_ok());
        assert!(check_url("http://127.0.0.1:8080/index.json").is_ok());
        assert!(check_url("http://plugins.example.com/index.json").is_err());
        assert!(check_url("ftp://example.com/index.json").is_err());
        assert!(check_url("https://user:secret@example.com/index.json").is_err());
        assert!(check_url("").is_err());
    }

    #[test]
    fn adding_a_registry_twice_keeps_one_entry() {
        let dir = scratch("registries");
        let path = dir.join("plugin-registries.json");
        let first =
            add_registry_at(&path, "https://plugins.example.com/index.json", "Official").unwrap();
        let again = add_registry_at(
            &path,
            "https://plugins.example.com/index.json",
            "Other name",
        )
        .unwrap();
        assert_eq!(first.url, again.url);
        assert_eq!(load_registries_at(&path).registries.len(), 1);
        assert!(remove_registry_at(&path, "https://plugins.example.com/index.json").unwrap());
        assert!(!remove_registry_at(&path, "https://plugins.example.com/index.json").unwrap());
        assert!(load_registries_at(&path).registries.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_valid_index_is_read_with_every_field() {
        let body = json!({
            "schemaVersion": 1,
            "plugins": [{
                "id": "hello-tools",
                "name": "Hello Tools",
                "version": "0.1.0",
                "description": "Two read-only workspace tools.",
                "author": "Someone",
                "category": "Productivity",
                "tags": ["Workspace", "read-only"],
                "permissions": ["workspace.read"],
                "requiresStudio": "4.0.0",
                "downloadUrl": "https://plugins.example.com/hello-tools.zip",
                "sha256": "a".repeat(64),
            }],
        })
        .to_string();
        let (entries, errors) = parse_index(&body, "https://plugins.example.com/index.json");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.id, "hello-tools");
        assert_eq!(entry.category, "productivity");
        assert_eq!(entry.tags, vec!["workspace", "read-only"]);
        assert_eq!(entry.kind, "extension");
        assert_eq!(entry.registry, "https://plugins.example.com/index.json");
    }

    #[test]
    fn a_malformed_entry_is_named_and_skipped() {
        let body = json!({
            "plugins": [
                { "id": "Bad Id", "name": "x", "version": "1.0.0", "downloadUrl": "https://e.com/a.zip", "sha256": "a".repeat(64) },
                { "id": "no-sha", "name": "x", "version": "1.0.0", "downloadUrl": "https://e.com/a.zip" },
                { "id": "short-sha", "name": "x", "version": "1.0.0", "downloadUrl": "https://e.com/a.zip", "sha256": "abc" },
                { "id": "cleartext", "name": "x", "version": "1.0.0", "downloadUrl": "http://e.com/a.zip", "sha256": "a".repeat(64) },
                { "id": "bad-version", "name": "x", "version": "1.0", "downloadUrl": "https://e.com/a.zip", "sha256": "a".repeat(64) },
                { "id": "bad-kind", "kind": "binary", "name": "x", "version": "1.0.0", "downloadUrl": "https://e.com/a.zip", "sha256": "a".repeat(64) },
                { "id": "no-name", "version": "1.0.0", "downloadUrl": "https://e.com/a.zip", "sha256": "a".repeat(64) },
                { "id": "fine", "name": "Fine", "version": "1.0.0", "downloadUrl": "https://e.com/a.zip", "sha256": "a".repeat(64) },
            ],
        })
        .to_string();
        let (entries, errors) = parse_index(&body, "https://e.com/index.json");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "fine");
        assert_eq!(errors.len(), 7, "{errors:?}");
        assert!(errors.iter().any(|error| error.contains("Bad Id")));
        assert!(errors.iter().any(|error| error.contains("sha256")));
        assert!(errors.iter().any(|error| error.contains("cleartext")));
        assert!(errors.iter().any(|error| error.contains("bad-kind")));
    }

    #[test]
    fn a_future_schema_is_refused_instead_of_read_partially() {
        let body = json!({ "schemaVersion": 2, "plugins": [] }).to_string();
        let (entries, errors) = parse_index(&body, "https://e.com/index.json");
        assert!(entries.is_empty());
        assert!(errors.iter().any(|error| error.contains("schemaVersion 2")));
        let (_, errors) = parse_index("not json", "https://e.com/index.json");
        assert!(errors.iter().any(|error| error.contains("not JSON")));
        let (_, errors) = parse_index("{}", "https://e.com/index.json");
        assert!(errors.iter().any(|error| error.contains("no plugins")));
    }

    #[test]
    fn an_archive_path_cannot_leave_the_plugin_folder() {
        assert_eq!(
            safe_archive_path("hello-tools/manifest.json"),
            Some(PathBuf::from("hello-tools/manifest.json"))
        );
        assert_eq!(safe_archive_path("./a/b.js"), Some(PathBuf::from("a/b.js")));
        assert_eq!(safe_archive_path("../escape.js"), None);
        assert_eq!(safe_archive_path("a/../../escape.js"), None);
        assert_eq!(safe_archive_path("/absolute.js"), None);
        assert_eq!(safe_archive_path("C:/windows.js"), None);
        assert_eq!(safe_archive_path("a\\b.js"), None);
        assert_eq!(safe_archive_path(""), None);
    }

    #[test]
    fn extracting_rejects_a_member_that_escapes() {
        let dir = scratch("traversal");
        let mut buffer = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
            writer
                .start_file("../escape.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, b"nope").unwrap();
            writer.finish().unwrap();
        }
        let error = extract(&buffer, &dir.join("out")).unwrap_err();
        assert!(error.contains("leaves the plugin folder"), "{error}");
        assert!(!dir.join("escape.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    fn archive_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buffer = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
            for (name, body) in files {
                writer
                    .start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut writer, body.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        buffer
    }

    fn listing(id: &str) -> Entry {
        Entry {
            id: id.to_string(),
            name: "Hello Studio".to_string(),
            version: "0.1.0".to_string(),
            description: String::new(),
            author: String::new(),
            category: String::new(),
            tags: Vec::new(),
            permissions: Vec::new(),
            requires_studio: String::new(),
            kind: "extension".to_string(),
            download_url: "https://example.com/x.zip".to_string(),
            sha256: "a".repeat(64),
            registry: "https://example.com/index.json".to_string(),
        }
    }

    /// The archive's own folder name is not the id: a `zip -r` of a differently
    /// named folder used to install as that name and then fail the id check.
    #[test]
    fn a_verified_archive_installs_under_its_listing_id() {
        let dir = scratch("archive-install");
        let root = dir.join("extensions");
        let manifest = json!({
            "id": "hello-studio",
            "name": "Hello Studio",
            "version": "0.1.0",
            "panels": [{ "id": "p", "title": "P", "entry": "p.html" }],
        })
        .to_string();
        let bytes = archive_of(&[
            ("hello-studio-from-zip/manifest.json", &manifest),
            (
                "hello-studio-from-zip/p.html",
                "<!doctype html><title>P</title>",
            ),
        ]);
        let id = install_archive(&bytes, &listing("hello-studio"), &root).expect("installs");
        assert_eq!(id, "hello-studio");
        assert!(root.join("hello-studio/manifest.json").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_for_another_id_is_refused() {
        let dir = scratch("archive-swap");
        let root = dir.join("extensions");
        let manifest = json!({
            "id": "hello-studio",
            "name": "Hello Studio",
            "version": "0.1.0",
        })
        .to_string();
        let bytes = archive_of(&[("hello-studio/manifest.json", &manifest)]);
        let error = install_archive(&bytes, &listing("something-else"), &root).unwrap_err();
        assert!(error.contains("declares id"), "{error}");
        assert!(!root.join("something-else").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_catalog_puts_extensions_before_skills() {
        let merged = merge(
            vec![json!({ "kind": "extension", "id": "a" })],
            vec![json!({ "kind": "skill", "id": "b" })],
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0]["kind"], "extension");
        assert_eq!(merged[1]["kind"], "skill");
        assert_eq!(count(&merged, true), 0);
    }
}
