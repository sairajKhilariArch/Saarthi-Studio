//! Local folder workspace: browse and edit files from the desktop UI.
//!
//! This router is only mounted on the loopback UI listener. Every path is
//! resolved relative to the folder the user explicitly opened, canonicalised
//! (so symlinks cannot escape it), and checked against size limits. Mutating
//! routes also require a loopback `Host` and, when present, a loopback
//! `Origin`, which blocks cross-site requests and DNS-rebinding writes.

use super::{home_dir, json_error, AppState};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Query, State},
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Component, Path as FsPath, PathBuf},
    process::Command,
    sync::{Arc, RwLock},
    time::UNIX_EPOCH,
};
use uuid::Uuid;

/// Largest file the editor will open or save. Larger files are rejected with a
/// clear message instead of being truncated (a truncated save would destroy
/// data).
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 2 * 1024 * 1024;
/// Largest directory listing returned in one response.
const MAX_ENTRIES: usize = 2000;
const MAX_RECENT_FOLDERS: usize = 6;

#[derive(Debug)]
pub(crate) struct WorkspaceError {
    status: StatusCode,
    message: String,
}

impl WorkspaceError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for WorkspaceError {
    fn into_response(self) -> Response<Body> {
        json_error(self.status, self.message)
    }
}

fn bad_request(message: impl Into<String>) -> WorkspaceError {
    WorkspaceError::new(StatusCode::BAD_REQUEST, message)
}

fn forbidden(message: impl Into<String>) -> WorkspaceError {
    WorkspaceError::new(StatusCode::FORBIDDEN, message)
}

fn not_found(message: impl Into<String>) -> WorkspaceError {
    WorkspaceError::new(StatusCode::NOT_FOUND, message)
}

fn conflict(message: impl Into<String>) -> WorkspaceError {
    WorkspaceError::new(StatusCode::CONFLICT, message)
}

fn too_large(message: impl Into<String>) -> WorkspaceError {
    WorkspaceError::new(StatusCode::PAYLOAD_TOO_LARGE, message)
}

fn internal(error: impl std::fmt::Display) -> WorkspaceError {
    WorkspaceError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

#[derive(Clone, Debug)]
struct OpenedWorkspace {
    root: PathBuf,
    name: String,
}

#[derive(Serialize, Deserialize, Default)]
struct PersistedWorkspace {
    #[serde(default)]
    last: Option<String>,
    #[serde(default)]
    recent: Vec<String>,
}

/// Process-wide shared handle to the folder currently open in the UI.
#[derive(Clone)]
pub(crate) struct WorkspaceStore {
    state_path: PathBuf,
    opened: Arc<RwLock<Option<OpenedWorkspace>>>,
    recent: Arc<RwLock<Vec<String>>>,
}

impl WorkspaceStore {
    /// Load the last folder from disk. A folder that no longer exists is
    /// dropped instead of failing startup.
    pub(crate) fn load(state_path: PathBuf) -> Self {
        let persisted = fs::read(&state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PersistedWorkspace>(&bytes).ok())
            .unwrap_or_default();
        let recent = persisted
            .recent
            .into_iter()
            .filter_map(|path| canonical_dir(&path))
            .map(|path| display_path(&path))
            .fold(Vec::new(), |mut acc: Vec<String>, path| {
                if !acc.contains(&path) && acc.len() < MAX_RECENT_FOLDERS {
                    acc.push(path);
                }
                acc
            });
        let opened = persisted
            .last
            .as_deref()
            .and_then(canonical_dir)
            .map(|root| OpenedWorkspace {
                name: display_name(&root),
                root,
            });
        Self {
            state_path,
            opened: Arc::new(RwLock::new(opened)),
            recent: Arc::new(RwLock::new(recent)),
        }
    }

    fn current(&self) -> Option<OpenedWorkspace> {
        self.opened.read().ok().and_then(|guard| guard.clone())
    }

    fn recent(&self) -> Vec<String> {
        self.recent
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn open(&self, path: &str) -> Result<OpenedWorkspace, WorkspaceError> {
        let root = canonical_dir(path).ok_or_else(|| {
            bad_request("That folder does not exist or cannot be opened. Use an absolute path.")
        })?;
        let opened = OpenedWorkspace {
            name: display_name(&root),
            root,
        };
        if let Ok(mut guard) = self.opened.write() {
            *guard = Some(opened.clone());
        }
        if let Ok(mut guard) = self.recent.write() {
            let display = display_path(&opened.root);
            guard.retain(|entry| *entry != display);
            guard.insert(0, display);
            guard.truncate(MAX_RECENT_FOLDERS);
        }
        self.persist();
        Ok(opened)
    }

    fn close(&self) {
        if let Ok(mut guard) = self.opened.write() {
            *guard = None;
        }
        self.persist();
    }

    fn persist(&self) {
        let opened = self.current();
        let persisted = PersistedWorkspace {
            last: opened.as_ref().map(|entry| display_path(&entry.root)),
            recent: self.recent(),
        };
        let Ok(bytes) = serde_json::to_vec_pretty(&persisted) else {
            return;
        };
        if let Some(parent) = self.state_path.parent() {
            let _ = fs::create_dir_all(parent);
            let _ = super::set_private_permissions(parent, true);
        }
        let temp = self.state_path.with_extension("tmp");
        if fs::write(&temp, bytes).is_ok() {
            let _ = super::set_private_permissions(&temp, false);
            let _ = fs::rename(&temp, &self.state_path);
            let _ = super::set_private_permissions(&self.state_path, false);
        }
    }

    fn root(&self) -> Result<PathBuf, WorkspaceError> {
        self.current()
            .map(|entry| entry.root)
            .ok_or_else(|| conflict("Open a folder first."))
    }

    /// Compact file-tree outline of the open folder: names only, no contents,
    /// with bulky directories skipped. Used to give the model cheap project
    /// awareness without sending file contents.
    pub(crate) fn outline(&self) -> Option<String> {
        let workspace = self.current()?;
        let mut lines = vec![format!("{}/", workspace.name)];
        let mut entries = 0usize;
        push_outline(&workspace.root, 0, &mut lines, &mut entries);
        Some(lines.join("\n"))
    }
}

const OUTLINE_SKIP_DIRS: [&str; 6] = [".git", "node_modules", "target", "build", "dist", ".gradle"];

fn push_outline(dir: &FsPath, depth: usize, lines: &mut Vec<String>, entries: &mut usize) {
    if depth > 4 || *entries >= 300 {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut names: Vec<(String, bool)> = Vec::new();
    for item in read.flatten() {
        let name = item.file_name().to_string_lossy().to_string();
        let is_dir = item.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
            && !item.file_type().map(|kind| kind.is_symlink()).unwrap_or(false);
        if is_dir && (name.starts_with('.') && name != ".github" || OUTLINE_SKIP_DIRS.contains(&name.as_str())) {
            continue;
        }
        names.push((name, is_dir));
    }
    names.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    let indent = "  ".repeat(depth);
    for (name, is_dir) in names {
        if *entries >= 300 {
            lines.push(format!("{indent}…"));
            return;
        }
        *entries += 1;
        lines.push(if is_dir { format!("{indent}{name}/") } else { format!("{indent}{name}") });
        if is_dir {
            push_outline(&dir.join(&name), depth + 1, lines, entries);
        }
    }
}

fn canonical_dir(path: &str) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded = if trimmed == "~" {
        home_dir()
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        PathBuf::from(trimmed)
    };
    if !expanded.is_absolute() {
        return None;
    }
    let canonical = fs::canonicalize(expanded).ok()?;
    canonical.is_dir().then_some(canonical)
}

fn display_name(root: &FsPath) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| display_path(root))
}

/// Keep HOME collapsed to `~` so the recent list stays readable.
fn display_path(root: &FsPath) -> String {
    let home = home_dir();
    match root.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => root.display().to_string(),
    }
}

/// Resolve a UI-supplied relative path inside `root`.
///
/// Rejects absolute paths and every component that is not a plain name, then
/// canonicalises existing targets (or their parent for new files) so a symlink
/// pointing outside the workspace cannot be used to escape it.
pub(crate) fn resolve_inside(root: &FsPath, relative: &str) -> Result<PathBuf, WorkspaceError> {
    let trimmed = relative.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Ok(root.to_path_buf());
    }
    let candidate = FsPath::new(trimmed);
    if candidate.is_absolute() {
        return Err(bad_request("Paths must be relative to the open folder."));
    }
    for component in candidate.components() {
        match component {
            Component::Normal(name) if !name.is_empty() => {}
            // A leading `./` is harmless; parent/root/prefix components are not.
            Component::CurDir => {}
            _ => return Err(bad_request("Paths must stay inside the open folder.")),
        }
    }
    let joined = root.join(candidate);
    let resolved = if joined.exists() {
        fs::canonicalize(&joined)
            .map_err(|error| not_found(format!("Path unavailable: {error}")))?
    } else {
        let parent = joined
            .parent()
            .ok_or_else(|| bad_request("Invalid path."))?;
        let parent =
            fs::canonicalize(parent).map_err(|_| not_found("The parent folder does not exist."))?;
        match joined.file_name() {
            Some(name) => parent.join(name),
            None => parent,
        }
    };
    if !resolved.starts_with(root) {
        return Err(forbidden("That path is outside the open folder."));
    }
    Ok(resolved)
}

fn relative_label(root: &FsPath, path: &FsPath) -> String {
    path.strip_prefix(root)
        .map(|rest| rest.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn modified_seconds(meta: &fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

pub(crate) fn language_for(path: &FsPath) -> String {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let language = match extension.as_str() {
        "rs" => "rust",
        "py" | "pyw" => "python",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" | "tsx" => "typescript",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "go" => "go",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "sh" | "bash" | "zsh" => "bash",
        "sql" => "sql",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        "md" | "markdown" => "markdown",
        "txt" | "log" | "" => "text",
        _ => return extension,
    };
    language.to_string()
}

#[derive(Debug, Serialize)]
struct FilePayload {
    path: String,
    content: String,
    language: String,
    size: u64,
    modified: f64,
}

fn read_text_file(root: &FsPath, relative: &str) -> Result<FilePayload, WorkspaceError> {
    let path = resolve_inside(root, relative)?;
    let meta = fs::metadata(&path).map_err(|_| not_found("That file does not exist."))?;
    if meta.is_dir() {
        return Err(bad_request("That path is a folder, not a file."));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(too_large(format!(
            "This file is {} KB. The editor opens files up to {} KB.",
            meta.len() / 1024,
            MAX_FILE_BYTES / 1024
        )));
    }
    let bytes = fs::read(&path).map_err(internal)?;
    if bytes.contains(&0) {
        return Err(WorkspaceError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "This looks like a binary file, so it cannot be opened in the text editor.",
        ));
    }
    let content = String::from_utf8(bytes).map_err(|_| {
        WorkspaceError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "This file is not valid UTF-8 text, so it cannot be opened in the editor.",
        )
    })?;
    Ok(FilePayload {
        path: relative_label(root, &path),
        language: language_for(&path),
        size: meta.len(),
        modified: modified_seconds(&meta),
        content,
    })
}

fn write_text_file(
    root: &FsPath,
    relative: &str,
    content: &str,
) -> Result<FilePayload, WorkspaceError> {
    if content.len() as u64 > MAX_FILE_BYTES {
        return Err(too_large(format!(
            "Content is larger than the {} KB editor limit.",
            MAX_FILE_BYTES / 1024
        )));
    }
    let path = resolve_inside(root, relative)?;
    if path == root {
        return Err(bad_request("Choose a file inside the open folder."));
    }
    if path.is_dir() {
        return Err(bad_request("That path is a folder."));
    }
    let parent = path
        .parent()
        .ok_or_else(|| bad_request("Invalid file path."))?;
    if !parent.is_dir() {
        return Err(not_found("The parent folder does not exist."));
    }
    // Write through a sibling temp file and rename, so an interrupted save
    // never leaves a half-written source file behind.
    let temp = parent.join(format!(".saarthi-save-{}", Uuid::new_v4()));
    fs::write(&temp, content.as_bytes()).map_err(internal)?;
    if let Err(error) = fs::rename(&temp, &path) {
        let _ = fs::remove_file(&temp);
        return Err(internal(error));
    }
    let meta = fs::metadata(&path).map_err(internal)?;
    Ok(FilePayload {
        path: relative_label(root, &path),
        language: language_for(&path),
        size: meta.len(),
        modified: modified_seconds(&meta),
        content: content.to_string(),
    })
}

#[derive(Debug, Serialize)]
struct DirEntry {
    name: String,
    path: String,
    dir: bool,
    symlink: bool,
    hidden: bool,
    size: u64,
}

fn list_directory(root: &FsPath, relative: &str) -> Result<(Vec<DirEntry>, bool), WorkspaceError> {
    let dir = resolve_inside(root, relative)?;
    let meta = fs::metadata(&dir).map_err(|_| not_found("That folder does not exist."))?;
    if !meta.is_dir() {
        return Err(bad_request("That path is not a folder."));
    }
    let base = relative_label(root, &dir);
    let mut entries = Vec::new();
    let mut truncated = false;
    let read = fs::read_dir(&dir).map_err(internal)?;
    for item in read {
        let Ok(item) = item else { continue };
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        let name = item.file_name().to_string_lossy().to_string();
        let item_path = if base.is_empty() {
            name.clone()
        } else {
            format!("{base}/{name}")
        };
        let link_meta = item.metadata();
        let symlink = item
            .file_type()
            .map(|kind| kind.is_symlink())
            .unwrap_or(false);
        let (dir_flag, size) = match link_meta {
            Ok(meta) => (meta.is_dir(), if meta.is_dir() { 0 } else { meta.len() }),
            Err(_) => (false, 0),
        };
        entries.push(DirEntry {
            hidden: name.starts_with('.'),
            name,
            path: item_path,
            dir: dir_flag,
            symlink,
            size,
        });
    }
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok((entries, truncated))
}

fn create_entry(root: &FsPath, relative: &str, dir: bool) -> Result<Value, WorkspaceError> {
    let path = resolve_inside(root, relative)?;
    if path == root {
        return Err(bad_request("Choose a name inside the open folder."));
    }
    if path.exists() {
        return Err(conflict("That name already exists."));
    }
    if dir {
        fs::create_dir_all(&path).map_err(internal)?;
    } else {
        fs::write(&path, b"").map_err(internal)?;
    }
    Ok(json!({
        "ok": true,
        "path": relative_label(root, &path),
        "dir": dir,
    }))
}

fn rename_entry(root: &FsPath, from: &str, to: &str) -> Result<Value, WorkspaceError> {
    let source = resolve_inside(root, from)?;
    if source == root {
        return Err(bad_request(
            "The open folder itself cannot be renamed here.",
        ));
    }
    if !source.exists() {
        return Err(not_found("That file or folder no longer exists."));
    }
    let target = resolve_inside(root, to)?;
    if target == root {
        return Err(bad_request("Choose a name inside the open folder."));
    }
    if target.exists() {
        return Err(conflict("The target name already exists."));
    }
    fs::rename(&source, &target)
        .map_err(|error| bad_request(format!("Could not rename: {error}")))?;
    Ok(json!({
        "ok": true,
        "from": relative_label(root, &source),
        "path": relative_label(root, &target),
    }))
}

fn delete_entry(root: &FsPath, relative: &str) -> Result<Value, WorkspaceError> {
    let path = resolve_inside(root, relative)?;
    if path == root {
        return Err(bad_request(
            "Use Close folder instead of deleting the open folder.",
        ));
    }
    let meta = fs::symlink_metadata(&path)
        .map_err(|_| not_found("That file or folder no longer exists."))?;
    let kind = meta.file_type();
    let result = if kind.is_symlink() || kind.is_file() {
        fs::remove_file(&path)
    } else {
        fs::remove_dir_all(&path)
    };
    result.map_err(internal)?;
    Ok(json!({ "ok": true, "path": relative_label(root, &path) }))
}

#[derive(Deserialize)]
struct PathQuery {
    #[serde(default)]
    path: String,
}

async fn workspace_info(State(state): State<AppState>) -> Json<Value> {
    let current = state.workspace.current();
    let workspace = current.map(|entry| {
        json!({
            "path": display_path(&entry.root),
            "absolute_path": entry.root.display().to_string(),
            "name": entry.name,
        })
    });
    Json(json!({
        "workspace": workspace,
        "recent": state.workspace.recent(),
        "home": display_path(&home_dir()),
    }))
}

async fn open_workspace(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    let Some(path) = input.get("path").and_then(Value::as_str) else {
        return json_error(StatusCode::BAD_REQUEST, "Provide a folder path to open.");
    };
    match state.workspace.open(path) {
        Ok(entry) => Json(json!({
            "ok": true,
            "workspace": {
                "path": display_path(&entry.root),
                "absolute_path": entry.root.display().to_string(),
                "name": entry.name,
            },
            "recent": state.workspace.recent(),
        }))
        .into_response(),
        Err(error) => error.into_response(),
    }
}

async fn close_workspace(State(state): State<AppState>) -> Json<Value> {
    state.workspace.close();
    Json(json!({ "ok": true }))
}

enum PickerResult {
    Picked(String),
    Cancelled,
    Unavailable,
}

#[cfg(target_os = "linux")]
fn run_folder_picker() -> PickerResult {
    // Zenity first (GNOME/most desktops), then KDialog. Both are launched with
    // a fixed, user-independent argument list; no shell is involved.
    let zenity = Command::new("zenity")
        .args([
            "--file-selection",
            "--directory",
            "--title=Open a folder in Free AI Studio",
        ])
        .output();
    match zenity {
        Ok(output) => {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return if path.is_empty() {
                    PickerResult::Cancelled
                } else {
                    PickerResult::Picked(path)
                };
            }
            return PickerResult::Cancelled;
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return PickerResult::Unavailable,
        Err(_) => {}
    }

    let home = home_dir().display().to_string();
    match Command::new("kdialog")
        .args(["--getexistingdirectory", &home])
        .output()
    {
        Ok(output) if output.status.success() => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                PickerResult::Cancelled
            } else {
                PickerResult::Picked(path)
            }
        }
        Ok(_) => PickerResult::Cancelled,
        Err(_) => PickerResult::Unavailable,
    }
}

async fn pick_folder(State(state): State<AppState>) -> Response<Body> {
    match tokio::task::spawn_blocking(run_folder_picker).await {
        Ok(PickerResult::Picked(path)) => match state.workspace.open(&path) {
            Ok(entry) => Json(json!({
                "ok": true,
                "picked": true,
                "workspace": {
                    "path": display_path(&entry.root),
                    "absolute_path": entry.root.display().to_string(),
                    "name": entry.name,
                },
                "recent": state.workspace.recent(),
            }))
            .into_response(),
            Err(error) => error.into_response(),
        },
        Ok(PickerResult::Cancelled) => Json(json!({ "ok": true, "picked": false })).into_response(),
        Ok(PickerResult::Unavailable) => Json(json!({
            "ok": true,
            "picked": false,
            "message": "No system folder dialog is available. Type the folder path instead.",
        }))
        .into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

async fn list_entries(
    State(state): State<AppState>,
    Query(query): Query<PathQuery>,
) -> Result<Json<Value>, WorkspaceError> {
    let root = state.workspace.root()?;
    let (entries, truncated) = list_directory(&root, &query.path)?;
    Ok(Json(json!({
        "path": query.path.trim().trim_matches('/'),
        "entries": entries,
        "truncated": truncated,
    })))
}

async fn read_file(
    State(state): State<AppState>,
    Query(query): Query<PathQuery>,
) -> Result<Json<FilePayload>, WorkspaceError> {
    let root = state.workspace.root()?;
    read_text_file(&root, &query.path).map(Json)
}

async fn write_file(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Result<Json<FilePayload>, WorkspaceError> {
    let root = state.workspace.root()?;
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_request("Provide the file path to save."))?;
    let content = input
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_request("Provide the file content to save."))?;
    write_text_file(&root, path, content).map(Json)
}

async fn create(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, WorkspaceError> {
    let root = state.workspace.root()?;
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_request("Provide a name for the new item."))?;
    let dir = input.get("dir").and_then(Value::as_bool).unwrap_or(false);
    create_entry(&root, path, dir).map(Json)
}

async fn rename(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, WorkspaceError> {
    let root = state.workspace.root()?;
    let from = input
        .get("from")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_request("Provide the current path."))?;
    let to = input
        .get("to")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_request("Provide the new path."))?;
    rename_entry(&root, from, to).map(Json)
}

async fn remove(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, WorkspaceError> {
    let root = state.workspace.root()?;
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_request("Provide the path to delete."))?;
    delete_entry(&root, path).map(Json)
}

/// Blocks cross-site requests and DNS-rebinding: the request must target the
/// loopback listener by name, and a browser-supplied `Origin` must be loopback.
pub(crate) async fn guard_local_origin(
    State(guard): State<LocalOriginGuard>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    let host_ok = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(|host| guard.matches_host(host))
        .unwrap_or(false);
    if !host_ok {
        return json_error(
            StatusCode::FORBIDDEN,
            "This endpoint only accepts loopback requests.",
        );
    }
    if let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        if !guard.matches_origin(origin) {
            return json_error(
                StatusCode::FORBIDDEN,
                "Cross-site requests are not allowed.",
            );
        }
    }
    next.run(request).await
}

#[derive(Clone)]
pub(crate) struct LocalOriginGuard {
    port: u16,
}

impl LocalOriginGuard {
    pub(crate) fn new(port: u16) -> Self {
        Self { port }
    }

    fn host_names(&self) -> [String; 3] {
        if self.port == 80 {
            [
                "127.0.0.1".to_string(),
                "localhost".to_string(),
                "[::1]".to_string(),
            ]
        } else {
            [
                format!("127.0.0.1:{}", self.port),
                format!("localhost:{}", self.port),
                format!("[::1]:{}", self.port),
            ]
        }
    }

    fn matches_host(&self, host: &str) -> bool {
        self.host_names()
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(host))
    }

    fn matches_origin(&self, origin: &str) -> bool {
        let origin = origin.trim_end_matches('/');
        self.host_names().iter().any(|name| {
            origin.eq_ignore_ascii_case(&format!("http://{name}"))
                || origin.eq_ignore_ascii_case(&format!("https://{name}"))
        })
    }
}

pub(crate) fn router(port: u16) -> Router<AppState> {
    Router::new()
        .route("/api/workspace", get(workspace_info))
        .route("/api/workspace/open", post(open_workspace))
        .route("/api/workspace/pick", post(pick_folder))
        .route("/api/workspace/close", post(close_workspace))
        .route("/api/workspace/entries", get(list_entries))
        .route("/api/workspace/file", get(read_file).put(write_file))
        .route("/api/workspace/create", post(create))
        .route("/api/workspace/rename", post(rename))
        .route("/api/workspace/delete", post(remove))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .route_layer(middleware::from_fn_with_state(
            LocalOriginGuard { port },
            guard_local_origin,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("saarthi-workspace-{name}-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).expect("create fixture workspace");
            Self { root }
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.root.join(relative)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn resolve_rejects_traversal_and_absolute_paths() {
        let fixture = Fixture::new("resolve");
        fs::create_dir_all(fixture.path("src")).unwrap();
        let root = fs::canonicalize(&fixture.root).unwrap();

        for attempt in [
            "../secret",
            "src/../../secret",
            "/etc/passwd",
            "src/./../../x",
        ] {
            assert!(
                resolve_inside(&root, attempt).is_err(),
                "expected {attempt} to be rejected"
            );
        }
        assert_eq!(
            resolve_inside(&root, "src/main.rs").unwrap(),
            root.join("src/main.rs")
        );
        assert_eq!(resolve_inside(&root, "").unwrap(), root);
        assert_eq!(resolve_inside(&root, "./src").unwrap(), root.join("src"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_refuses_symlinks_that_escape_the_workspace() {
        let fixture = Fixture::new("symlink");
        let outside = Fixture::new("outside");
        let root = fs::canonicalize(&fixture.root).unwrap();
        let outside_root = fs::canonicalize(&outside.root).unwrap();
        let link = fixture.path("escape");
        std::os::unix::fs::symlink(&outside_root, &link).expect("create symlink");

        assert!(resolve_inside(&root, "escape/secret.txt").is_err());
        assert!(resolve_inside(&root, "escape").is_err());
    }

    #[test]
    fn reads_and_writes_text_files_inside_the_workspace() {
        let fixture = Fixture::new("read-write");
        let root = fs::canonicalize(&fixture.root).unwrap();
        fs::create_dir_all(fixture.path("src")).unwrap();
        fs::write(fixture.path("src/lib.rs"), "fn main() {}\n").unwrap();

        let payload = read_text_file(&root, "src/lib.rs").unwrap();
        assert_eq!(payload.content, "fn main() {}\n");
        assert_eq!(payload.language, "rust");
        assert_eq!(payload.path, "src/lib.rs");

        let saved =
            write_text_file(&root, "src/lib.rs", "fn main() { println!(\"hi\"); }\n").unwrap();
        assert_eq!(saved.language, "rust");
        assert_eq!(
            fs::read_to_string(fixture.path("src/lib.rs")).unwrap(),
            "fn main() { println!(\"hi\"); }\n"
        );
        // No temp files are left behind after an atomic save.
        let leftovers: Vec<_> = fs::read_dir(fixture.path("src"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("lib.rs")]);
    }

    #[test]
    fn rejects_binary_files_and_oversized_writes() {
        let fixture = Fixture::new("binary");
        let root = fs::canonicalize(&fixture.root).unwrap();
        fs::write(fixture.path("image.bin"), [0u8, 159, 146, 150]).unwrap();
        assert!(read_text_file(&root, "image.bin").is_err());

        let oversized = "x".repeat((MAX_FILE_BYTES as usize) + 1);
        let error = write_text_file(&root, "big.txt", &oversized).unwrap_err();
        assert_eq!(error.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(!fixture.path("big.txt").exists());
    }

    #[test]
    fn listing_puts_folders_first_and_marks_hidden_entries() {
        let fixture = Fixture::new("list");
        let root = fs::canonicalize(&fixture.root).unwrap();
        fs::create_dir_all(fixture.path("src")).unwrap();
        fs::write(fixture.path("README.md"), "hello").unwrap();
        fs::write(fixture.path(".env"), "secret").unwrap();

        let (entries, truncated) = list_directory(&root, "").unwrap();
        assert!(!truncated);
        assert!(entries[0].dir);
        assert_eq!(entries[0].name, "src");
        let names: Vec<_> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["src", ".env", "README.md"]);
        let readme = entries
            .iter()
            .find(|entry| entry.name == "README.md")
            .unwrap();
        assert_eq!(readme.size, 5);
        assert_eq!(readme.path, "README.md");
        assert!(
            entries
                .iter()
                .find(|entry| entry.name == ".env")
                .unwrap()
                .hidden
        );

        let nested = list_directory(&root, "src").unwrap().0;
        assert!(nested.is_empty());
    }

    #[test]
    fn create_rename_and_delete_stay_inside_the_workspace() {
        let fixture = Fixture::new("mutate");
        let root = fs::canonicalize(&fixture.root).unwrap();

        create_entry(&root, "notes", true).unwrap();
        create_entry(&root, "notes/todo.md", false).unwrap();
        assert!(fixture.path("notes/todo.md").is_file());
        assert_eq!(
            create_entry(&root, "notes/todo.md", false)
                .unwrap_err()
                .status,
            StatusCode::CONFLICT
        );

        rename_entry(&root, "notes/todo.md", "notes/done.md").unwrap();
        assert!(fixture.path("notes/done.md").is_file());
        assert!(rename_entry(&root, "notes/done.md", "../escape.md").is_err());

        delete_entry(&root, "notes").unwrap();
        assert!(!fixture.path("notes").exists());
        assert!(delete_entry(&root, "").is_err());
        assert!(delete_entry(&root, "../").is_err());
    }

    #[test]
    fn store_persists_and_restores_the_open_folder() {
        let fixture = Fixture::new("store");
        let state_path = fixture.path("workspace.json");
        let store = WorkspaceStore::load(state_path.clone());
        assert!(store.current().is_none());

        let opened = store.open(&fixture.root.display().to_string()).unwrap();
        assert_eq!(
            opened.name,
            display_name(&fs::canonicalize(&fixture.root).unwrap())
        );
        assert_eq!(store.recent().len(), 1);

        let reloaded = WorkspaceStore::load(state_path);
        let restored = reloaded.current().expect("restore last workspace");
        assert_eq!(restored.root, fs::canonicalize(&fixture.root).unwrap());

        reloaded.close();
        assert!(WorkspaceStore::load(fixture.path("workspace.json"))
            .current()
            .is_none());
    }

    #[test]
    fn store_drops_folders_that_no_longer_exist() {
        let fixture = Fixture::new("stale");
        let missing = fixture.path("gone");
        let state_path = fixture.path("workspace.json");
        fs::write(
            &state_path,
            serde_json::to_vec(&json!({ "last": missing.display().to_string(), "recent": [missing.display().to_string()] })).unwrap(),
        )
        .unwrap();
        let store = WorkspaceStore::load(state_path);
        assert!(store.current().is_none());
        assert!(store.recent().is_empty());
    }

    #[test]
    fn local_origin_guard_only_accepts_loopback_hosts() {
        let guard = LocalOriginGuard { port: 8765 };
        assert!(guard.matches_host("127.0.0.1:8765"));
        assert!(guard.matches_host("localhost:8765"));
        assert!(!guard.matches_host("127.0.0.1:9999"));
        assert!(!guard.matches_host("evil.example:8765"));
        assert!(guard.matches_origin("http://127.0.0.1:8765"));
        assert!(guard.matches_origin("http://localhost:8765/"));
        assert!(!guard.matches_origin("http://evil.example"));
        assert!(!guard.matches_origin("null"));
    }

    #[test]
    fn language_detection_covers_common_source_files() {
        for (path, expected) in [
            ("src/main.rs", "rust"),
            ("index.html", "html"),
            ("app.js", "javascript"),
            ("App.kt", "kotlin"),
            ("README.md", "markdown"),
            ("Cargo.toml", "toml"),
            ("LICENSE", "text"),
            ("data.zzz", "zzz"),
        ] {
            assert_eq!(language_for(FsPath::new(path)), expected, "{path}");
        }
    }
}

/// Windows folder picker: PowerShell's FolderBrowserDialog ships with every
/// supported Windows version, so no extra runtime has to be installed. The
/// script is a fixed string; nothing from the request is interpolated into it.
#[cfg(target_os = "windows")]
fn run_folder_picker() -> PickerResult {
    const SCRIPT: &str = concat!(
        "Add-Type -AssemblyName System.Windows.Forms; ",
        "$dialog = New-Object System.Windows.Forms.FolderBrowserDialog; ",
        "$dialog.Description = 'Open a folder in Free AI Studio'; ",
        "if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { ",
        "[Console]::Out.Write($dialog.SelectedPath) }"
    );
    match Command::new("powershell")
        .args(["-NoProfile", "-STA", "-Command", SCRIPT])
        .output()
    {
        Ok(output) if output.status.success() => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                PickerResult::Cancelled
            } else {
                PickerResult::Picked(path)
            }
        }
        Ok(_) => PickerResult::Cancelled,
        Err(_) => PickerResult::Unavailable,
    }
}

/// macOS folder picker via the built-in AppleScript choose-folder dialog.
#[cfg(target_os = "macos")]
fn run_folder_picker() -> PickerResult {
    match Command::new("osascript")
        .args([
            "-e",
            "POSIX path of (choose folder with prompt \"Open a folder in Free AI Studio\")",
        ])
        .output()
    {
        Ok(output) if output.status.success() => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                PickerResult::Cancelled
            } else {
                PickerResult::Picked(path)
            }
        }
        Ok(_) => PickerResult::Cancelled,
        Err(_) => PickerResult::Unavailable,
    }
}

/// Other platforms fall back to typing a path in the dialog.
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn run_folder_picker() -> PickerResult {
    PickerResult::Unavailable
}
