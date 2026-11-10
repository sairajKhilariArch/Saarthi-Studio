//! Uninstalling a portable install.
//!
//! A package-managed install (.deb, AppImage, MSI, `.app`) is removed by its
//! package manager. A portable tarball or zip is not — a user unpacks it and runs
//! the binary — so this is the way back for exactly that case.
//!
//! It is the most guarded operation in the app: a permission decision, a
//! confirmation token that is the data folder's own path, and a refusal to touch
//! anything that does not look like an install this app made (a source checkout
//! or a `cargo build` output is not). The program folder is removed first, so a
//! refusal never leaves a working install without its data.

use crate::{json_error, now_seconds, permissions, AppState};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path as FsPath, PathBuf};

/// The three trees an uninstall can own.
pub(crate) struct Targets {
    pub install: Option<PathBuf>,
    pub data: PathBuf,
    pub config: PathBuf,
}

impl Targets {
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self {
            install: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(PathBuf::from)),
            data: state
                .database_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_default(),
            config: state
                .config_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_default(),
        }
    }
}

/// Files and bytes below a folder. Bounded, because this runs on whatever a user
/// points the app at and must not walk forever.
fn counts(dir: &FsPath) -> (u64, u64) {
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((current, depth)) = stack.pop() {
        if depth > 12 || seen > 20_000 {
            break;
        }
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push((entry.path(), depth + 1));
            } else if kind.is_file() {
                files += 1;
                if let Ok(meta) = entry.metadata() {
                    bytes += meta.len();
                }
            }
        }
    }
    (files, bytes)
}

/// A portable install is the binary beside the UI it serves. A source checkout
/// also has `static/`, so `Cargo.toml`, `.git` and `target/` mark a tree this app
/// did not install and must not delete.
fn looks_like_portable_install(dir: &FsPath) -> bool {
    let binary = dir.join("free-ai-studio").is_file() || dir.join("free-ai-studio.exe").is_file();
    let ui = dir.join("static/index.html").is_file();
    let foreign =
        dir.join("Cargo.toml").exists() || dir.join(".git").exists() || dir.join("target").is_dir();
    if foreign {
        return false;
    }
    if binary && ui {
        return true;
    }
    // A half-removed install keeps only the renamed binary until the user deletes
    // the folder (Windows cannot delete a running program). A second call has to
    // recognise that state and report it, not refuse as if nothing had happened.
    fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains(".uninstalling-")
            })
        })
        .unwrap_or(false)
}

/// Whether the program folder can be removed, and why not when it cannot.
fn install_check(dir: &FsPath) -> (bool, String) {
    let text = dir.to_string_lossy().to_string();
    if text.trim().is_empty() || dir.parent().is_none() {
        return (
            false,
            "it is a filesystem root, not an install folder".to_string(),
        );
    }
    if !dir.is_dir() {
        return (false, "it is not a directory".to_string());
    }
    if !looks_like_portable_install(dir) {
        return (
            false,
            "it does not look like a portable install (no binary beside a static tree, or it is a source checkout)".to_string(),
        );
    }
    // Windows scheduling passes this path to PowerShell in single quotes, so a
    // path holding characters a shell would treat as syntax is refused rather
    // than escaped.
    if text.contains(['"', '\'', '%', '&', '|', '<', '>', '\n', '\r']) {
        return (
            false,
            "its path holds characters this app will not hand to a shell".to_string(),
        );
    }
    (true, String::new())
}

/// What an uninstall would remove. Read-only: this is what the panel shows before
/// it offers the button, so nothing disappears that was not named.
pub(crate) fn preview(targets: &Targets) -> Value {
    let mut parts = serde_json::Map::new();
    for (key, path) in [
        ("install", targets.install.as_deref()),
        ("data", Some(targets.data.as_path())),
        ("config", Some(targets.config.as_path())),
    ] {
        let Some(path) = path else {
            continue;
        };
        let exists = path.is_dir();
        let (files, bytes) = if exists { counts(path) } else { (0, 0) };
        let mut entry = json!({
            "path": path.to_string_lossy(),
            "exists": exists,
            "files": files,
            "bytes": bytes,
        });
        if key == "install" {
            let (removable, reason) = install_check(path);
            entry["portable"] = json!(looks_like_portable_install(path));
            entry["removable"] = json!(removable);
            entry["reason"] = json!(reason);
        }
        parts.insert(key.to_string(), entry);
    }
    json!({
        "ok": true,
        // The token a call has to name: the data folder's own path. It makes a
        // stray POST impossible and forces the caller to know what it is deleting.
        "confirmation": targets.data.to_string_lossy(),
        "keeps_data_option": true,
        "keeps_config_option": true,
        "targets": Value::Object(parts),
    })
}

#[cfg(not(windows))]
fn remove_install(dir: &FsPath) -> Result<(), String> {
    // POSIX unlinks a running executable happily; the process keeps its inode.
    fs::remove_dir_all(dir).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn remove_install(dir: &FsPath) -> Result<(), String> {
    // A running executable cannot be deleted. It can be renamed, so it is moved
    // aside and everything beside it goes now; the folder itself is reported as
    // pending, because Windows will not release it while this process runs it.
    if let Ok(exe) = std::env::current_exe() {
        if exe.parent() == Some(dir) {
            let stem = exe
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_else(|| "free-ai-studio".to_string());
            let extension = exe
                .extension()
                .map(|extension| format!(".{}", extension.to_string_lossy()))
                .unwrap_or_default();
            let aside = dir.join(format!(
                "{stem}.uninstalling-{}{extension}",
                std::process::id()
            ));
            fs::rename(&exe, &aside)
                .map_err(|error| format!("the running binary could not be moved aside: {error}"))?;
        }
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
        }
    }
    let _ = fs::remove_dir(dir);
    Ok(())
}

/// Remove the program folder, then the data and settings the caller did not ask
/// to keep. A per-part failure is reported instead of hidden, and `complete` says
/// whether everything asked for actually went.
pub(crate) fn uninstall(
    targets: &Targets,
    keep_data: bool,
    keep_config: bool,
) -> Result<Value, String> {
    let mut report = serde_json::Map::new();
    let mut complete = true;

    if let Some(dir) = targets.install.as_deref().filter(|dir| dir.is_dir()) {
        let (removable, reason) = install_check(dir);
        if !removable {
            return Err(format!(
                "{} was not removed because {reason}. Nothing was changed.",
                dir.display()
            ));
        }
        let (files, bytes) = counts(dir);
        remove_install(dir).map_err(|error| {
            format!(
                "The program folder could not be removed ({error}), so nothing else was touched."
            )
        })?;
        // Report what is actually on disk, not what was attempted: a running
        // Windows binary keeps its folder alive, and claiming otherwise would be
        // a lie the user finds a folder later.
        let gone = !dir.exists();
        if !gone {
            complete = false;
        }
        report.insert(
            "install".to_string(),
            json!({
                "path": dir.to_string_lossy(),
                "existed": true,
                "files": files,
                "bytes": bytes,
                "removed": gone,
                "pending": if gone { Value::Null } else { json!(dir.to_string_lossy()) },
            }),
        );
    }

    for (key, path, keep) in [
        ("data", targets.data.as_path(), keep_data),
        ("config", targets.config.as_path(), keep_config),
    ] {
        let exists = path.is_dir();
        let (files, bytes) = if exists { counts(path) } else { (0, 0) };
        if !exists {
            report.insert(
                key.to_string(),
                json!({
                    "path": path.to_string_lossy(),
                    "existed": false,
                    "removed": false,
                    "kept": keep,
                }),
            );
            continue;
        }
        if keep {
            report.insert(
                key.to_string(),
                json!({
                    "path": path.to_string_lossy(),
                    "existed": true,
                    "removed": false,
                    "kept": true,
                    "files": files,
                    "bytes": bytes,
                }),
            );
            continue;
        }
        match fs::remove_dir_all(path) {
            Ok(()) => {
                report.insert(
                    key.to_string(),
                    json!({
                        "path": path.to_string_lossy(),
                        "existed": true,
                        "removed": true,
                        "kept": false,
                        "files": files,
                        "bytes": bytes,
                    }),
                );
            }
            Err(error) => {
                complete = false;
                report.insert(
                    key.to_string(),
                    json!({
                        "path": path.to_string_lossy(),
                        "existed": true,
                        "removed": false,
                        "kept": false,
                        "files": files,
                        "bytes": bytes,
                        "error": error.to_string(),
                    }),
                );
            }
        }
    }

    Ok(json!({
        "removed_at": now_seconds(),
        "complete": complete,
        "targets": Value::Object(report),
    }))
}

/// `GET /api/uninstall/preview` — what would go, and whether it may be removed.
/// Read-only, so it is not gated.
pub(crate) async fn preview_route(State(state): State<AppState>) -> axum::response::Response {
    Json(preview(&Targets::from_state(&state))).into_response()
}

/// `POST /api/uninstall` — remove the program folder, settings and data.
///
/// Gated on `uninstall` (default `ask`) *and* on the confirmation token, because
/// a saved allow rule must not be enough on its own to delete someone's projects.
pub(crate) async fn uninstall_route(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> axum::response::Response {
    let targets = Targets::from_state(&state);
    let confirmation = targets.data.to_string_lossy().to_string();
    let confirm = input
        .get("confirm")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if confirm != confirmation {
        return json_error(
            StatusCode::BAD_REQUEST,
            format!("Type {confirmation} to confirm the uninstall; nothing was removed."),
        );
    }
    // The person's yes, not merely a program's request. Other privileged routes
    // treat the click as consent, which is fine for saving settings; deleting an
    // install and its data asks for both knowing the path and saying yes.
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !approve {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "uninstall needs an explicit approval: send approve: true beside the confirmation path.", "needs_approval": true })),
        )
            .into_response();
    }
    if let Some(response) = permissions::guard_user(
        &state,
        permissions::Action::Uninstall,
        "install",
        "",
        true,
        "remove Studio's program folder, settings and data",
    ) {
        return response;
    }
    let keep_data = input
        .get("keepData")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let keep_config = input
        .get("keepConfig")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match uninstall(&targets, keep_data, keep_config) {
        Ok(report) => {
            let verdict = permissions::check(&state, permissions::Action::Uninstall, "install", "");
            permissions::record(
                &state,
                permissions::Action::Uninstall,
                "install",
                permissions::Decision::Allow,
                verdict.scope,
                "",
                &format!(
                    "uninstalled (complete: {}); program folder removed, data {}, settings {}",
                    report["complete"],
                    if keep_data { "kept" } else { "removed" },
                    if keep_config { "kept" } else { "removed" }
                ),
            );
            Json(json!({ "ok": true, "uninstall": report })).into_response()
        }
        Err(error) => json_error(StatusCode::CONFLICT, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "free-ai-studio-uninstall-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn portable_install(dir: &FsPath) {
        fs::create_dir_all(dir.join("static")).expect("static");
        fs::write(dir.join("free-ai-studio"), b"binary").expect("binary");
        fs::write(dir.join("static/index.html"), b"<html></html>").expect("ui");
    }

    fn data_tree(dir: &FsPath) {
        fs::create_dir_all(dir.join("projects")).expect("projects");
        fs::write(dir.join("studio.db"), b"database").expect("db");
        fs::write(dir.join("projects/notes.md"), b"notes").expect("notes");
    }

    #[test]
    fn the_preview_names_every_tree_and_whether_the_install_may_go() {
        let root = scratch("preview");
        let install = root.join("app");
        let data = root.join("home/data");
        let config = root.join("home/config");
        portable_install(&install);
        data_tree(&data);
        fs::create_dir_all(&config).expect("config");
        fs::write(config.join("config.json"), b"{}").expect("settings");

        let preview = preview(&Targets {
            install: Some(install.clone()),
            data: data.clone(),
            config: config.clone(),
        });
        assert_eq!(preview["targets"]["install"]["portable"], true);
        assert_eq!(preview["targets"]["install"]["removable"], true);
        assert_eq!(preview["targets"]["data"]["files"], 2);
        assert_eq!(preview["targets"]["config"]["files"], 1);
        // The token a call must echo is the data folder itself.
        assert_eq!(preview["confirmation"], data.to_string_lossy().as_ref());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_source_checkout_is_never_removable() {
        let root = scratch("checkout");
        let install = root.join("repo");
        portable_install(&install);
        // What a `cargo run` from the repository looks like.
        fs::write(install.join("Cargo.toml"), b"[package]").expect("cargo");
        assert!(!looks_like_portable_install(&install));
        let preview = preview(&Targets {
            install: Some(install.clone()),
            data: root.join("data"),
            config: root.join("config"),
        });
        assert_eq!(preview["targets"]["install"]["removable"], false);
        assert!(preview["targets"]["install"]["reason"]
            .as_str()
            .unwrap()
            .contains("portable install"));

        // A refusal leaves everything alone, including the data.
        let error = uninstall(
            &Targets {
                install: Some(install.clone()),
                data: root.join("data"),
                config: root.join("config"),
            },
            false,
            false,
        )
        .expect_err("not an install");
        assert!(error.contains("Nothing was changed"), "{error}");
        assert!(install.join("static/index.html").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstalling_removes_data_and_settings_and_can_keep_either() {
        let root = scratch("remove");
        let data = root.join("data");
        let config = root.join("config");
        data_tree(&data);
        fs::create_dir_all(&config).expect("config");
        fs::write(config.join("config.json"), b"{}").expect("settings");

        let report = uninstall(
            &Targets {
                install: None,
                data: data.clone(),
                config: config.clone(),
            },
            false,
            false,
        )
        .expect("uninstall");
        assert_eq!(report["complete"], true);
        assert_eq!(report["targets"]["data"]["removed"], true);
        assert_eq!(report["targets"]["config"]["removed"], true);
        assert!(!data.exists());
        assert!(!config.exists());

        // The keep flags leave that tree exactly where it was.
        data_tree(&data);
        fs::create_dir_all(&config).expect("config");
        fs::write(config.join("config.json"), b"{}").expect("settings");
        let report = uninstall(
            &Targets {
                install: None,
                data: data.clone(),
                config: config.clone(),
            },
            true,
            false,
        )
        .expect("uninstall keeping data");
        assert_eq!(report["targets"]["data"]["kept"], true);
        assert_eq!(report["targets"]["data"]["removed"], false);
        assert!(data.join("studio.db").is_file());
        assert!(!config.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_tree_is_reported_rather_than_guessed_at() {
        let root = scratch("missing");
        let report = uninstall(
            &Targets {
                install: None,
                data: root.join("absent-data"),
                config: root.join("absent-config"),
            },
            false,
            false,
        )
        .expect("uninstall");
        assert_eq!(report["targets"]["data"]["existed"], false);
        assert_eq!(report["targets"]["config"]["existed"], false);
        let _ = fs::remove_dir_all(&root);
    }
}
