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

use crate::AppState;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
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
const MAX_PERMISSIONS: usize = 16;
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
    permissions: Vec<String>,
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
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.len() <= 4 && part.chars().all(|c| c.is_ascii_digit()))
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
                    "entry must be a relative path inside the extension folder.".to_string(),
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
        permissions,
    })
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
        "permissions": [],
        "valid": false,
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
        Some(entry) if entry["valid"] == Value::Bool(true) => Ok(manifest.id),
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
    fs::rename(&canonical, removed.join(format!("{id}-{stamp}")))
        .map_err(|error| error.to_string())
}

fn bad(message: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "ok": false, "error": message.to_string() })),
    )
        .into_response()
}

fn server_error(message: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "ok": false, "error": message.to_string() })),
    )
        .into_response()
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
    folders
        .into_iter()
        .map(|folder| {
            let name = folder
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string();
            let manifest_path = folder.join("manifest.json");
            let too_big = fs::metadata(&manifest_path)
                .map(|meta| meta.len() > MAX_MANIFEST_BYTES)
                .unwrap_or(false);
            if too_big {
                return invalid(
                    &name,
                    format!("manifest.json is larger than {MAX_MANIFEST_BYTES} bytes."),
                );
            }
            let parsed = match fs::read_to_string(&manifest_path) {
                Ok(text) => match serde_json::from_str::<Value>(&text) {
                    Ok(value) => parse_manifest(&name, &value),
                    Err(error) => Err(format!("manifest.json is not valid JSON: {error}")),
                },
                Err(_) => Err("manifest.json is missing or unreadable.".to_string()),
            }
            .and_then(|manifest| {
                if let Some(entry) = manifest.entry.as_deref() {
                    let path = folder.join(entry);
                    if !path.is_file() {
                        return Err(format!("entry file \"{entry}\" is missing."));
                    }
                    let base = folder.canonicalize().map_err(|error| error.to_string())?;
                    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
                    if !canonical.starts_with(&base) {
                        return Err(
                            "entry file must stay inside the extension folder.".to_string()
                        );
                    }
                }
                Ok(manifest)
            });
            match parsed {
                Ok(manifest) => {
                    let blocked = run_block(&manifest);
                    json!({
                        "id": manifest.id,
                        "name": manifest.name,
                        "version": manifest.version,
                        "description": manifest.description,
                        "author": manifest.author,
                        "entry": manifest.entry,
                        "runtime": manifest.runtime,
                        "permissions": manifest.permissions,
                        "valid": true,
                        "runnable": blocked.is_none(),
                        "blockedReason": blocked.unwrap_or_default(),
                        "error": "",
                    })
                }
                Err(error) => invalid(&name, error),
            }
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

pub(crate) async fn install(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response {
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
    match remove_extension(&extensions_dir(&state), &id) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(error) => bad(error),
    }
}

/// Gate 2 of ADR 0001: a run request must fail with a reason before any code is
/// loaded. Only then is the module handed to the sandboxed runner. Execution is
/// blocking interpreter work, so it runs on a blocking thread instead of the
/// async runtime's worker.
pub(crate) async fn run(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
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
        return bad(
            entry["error"]
                .as_str()
                .unwrap_or("The extension is not valid."),
        );
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
    // here, so a rule added mid-run cannot widen it. A missing folder leaves an
    // empty root, and every host call then reports that nothing is open.
    let declared: Vec<String> = entry["permissions"]
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let project = crate::active_project_id(&state);
    let grant = crate::extension_run::HostGrant {
        root: state.workspace.root().unwrap_or_default(),
        // Its own folder is the only place an extension can persist anything;
        // the code beside it stays read-only to the module.
        data_root: Some(dir.join(&id).join("data")),
        project: project.clone(),
        rules: crate::permissions::rules(&state),
        read: declared.iter().any(|permission| permission == "workspace.read"),
        write: declared.iter().any(|permission| permission == "workspace.write"),
    };
    let outcome =
        tokio::task::spawn_blocking(move || crate::extension_run::run_file(&module_path, Some(grant)))
            .await;
    let outcome = match outcome {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            crate::oplog::record("warn", "extensions", "run", format!("{id}: {error}"));
            return bad(error);
        }
        Err(error) => {
            let message = format!("The extension stopped unexpectedly: {error}");
            crate::oplog::record("warn", "extensions", "run", format!("{id}: {message}"));
            return server_error(message);
        }
    };
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
    for call in &outcome.audit {
        crate::oplog::record(
            if call.decision == crate::permissions::Decision::Allow {
                "info"
            } else {
                "warn"
            },
            "extensions",
            "host",
            format!("{id} {} \"{}\": {}", call.capability, call.path, call.detail),
        );
        if call.decision != crate::permissions::Decision::Allow {
            let action = if call.capability == "workspace.write" {
                crate::permissions::Action::FsWrite
            } else {
                crate::permissions::Action::FsRead
            };
            crate::permissions::record(
                &state,
                action,
                &call.path,
                call.decision,
                call.scope,
                &project,
                &format!("extension {id} host call"),
            );
        }
    }
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
        assert_eq!(install_into(&source, &target).expect("install"), "hello-tools");
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
        assert!(parse_manifest("wasm-tools", &with("runtime", json!("wasm")))
            .unwrap_err()
            .contains(".wasm entry"));
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
        assert!(run_block(&manifest(Some("wasm"), vec!["workspace.read"]))
            .is_none());
        assert!(
            run_block(&manifest(Some("wasm"), vec!["workspace.read", "workspace.write"]))
                .is_none()
        );
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
        assert!(parse_manifest("hello-tools", &with("version", json!("1.2")))
            .unwrap_err()
            .contains("1.2.3"));
        assert!(parse_manifest("hello-tools", &with("entry", json!("../escape.js")))
            .unwrap_err()
            .contains("relative path"));
        assert!(parse_manifest("hello-tools", &with("entry", json!("/abs.js")))
            .unwrap_err()
            .contains("relative path"));
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
}
