//! Extensions: installed folders with a `manifest.json`.
//!
//! This is the first Phase 15 slice and it is deliberately read-only: a manifest
//! is discovered, validated and reported, never executed. That keeps the
//! security boundary honest — nothing here can run code, so no sandbox claim is
//! made. When execution arrives it must add the sandbox first, not after.
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
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// Closed permission vocabulary. Adding one is a product decision, not a typo.
const ALLOWED_PERMISSIONS: [&str; 5] = [
    "workspace.read",
    "workspace.write",
    "terminal.run",
    "network",
    "mcp.call",
];
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
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
        permissions,
    })
}

fn invalid(folder: &str, error: String) -> Value {
    json!({
        "id": folder,
        "name": "",
        "version": "",
        "description": "",
        "author": "",
        "entry": Value::Null,
        "permissions": [],
        "valid": false,
        "error": error,
    })
}

fn inspect(dir: &Path) -> Vec<Value> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
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
                Ok(manifest) => json!({
                    "id": manifest.id,
                    "name": manifest.name,
                    "version": manifest.version,
                    "description": manifest.description,
                    "author": manifest.author,
                    "entry": manifest.entry,
                    "permissions": manifest.permissions,
                    "valid": true,
                    "error": "",
                }),
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
