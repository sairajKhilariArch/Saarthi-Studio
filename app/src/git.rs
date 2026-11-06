//! Real Git operations on the open workspace.
//!
//! Until now the app could only *read* `git status`; every change had to be
//! made in an external terminal. This module adds the operations a person
//! actually needs — diff, stage, unstage, discard, commit, list and switch
//! branches, history — and each one is a real `git` invocation in the open
//! folder. Nothing is simulated and nothing is written outside the workspace.
//!
//! Three rules shape the code:
//!
//! * **Every path is validated before it reaches argv.** Paths are relative to
//!   the workspace, `.git` itself is refused, and `..`, absolute paths, drive
//!   letters and control characters never survive `clean_paths`. Arguments are
//!   passed as an array (never through a shell), so there is no quoting step to
//!   get wrong.
//! * **Every mutation consults the permission store** through
//!   `permissions::guard_user` with the `git` action, exactly like editor saves
//!   use `fs_write`. A saved deny is a 403; a saved ask is a 409 until the
//!   request carries `approve: true`.
//! * **Failures are reported, not swallowed.** Git's own first line is what the
//!   person sees, because it names the real cause (no identity configured, a
//!   branch that would be overwritten, nothing staged).
//!
//! The read routes (`diff`, `branches`, `log`) do not mutate anything and are
//! left ungated, matching the existing `/api/workspace/vcs` read.

use std::path::{Path, PathBuf};
use std::process::Command;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::permissions::{guard_user, Action};
use crate::AppState;

/// Largest diff returned in one response. A diff bigger than this is a code
/// review problem, not a viewer problem, and the tail is reported as truncated.
const MAX_DIFF_BYTES: usize = 128 * 1024;
/// Paths per request. A commit touching more files than this should be staged
/// with `git add -A` instead of a hand-built list.
const MAX_PATHS: usize = 200;
/// Commit message ceiling, in characters.
const MAX_MESSAGE_CHARS: usize = 4000;
/// History entries one request may ask for.
const MAX_LOG_ENTRIES: usize = 200;

fn fail(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

fn bad(message: impl Into<String>) -> Response {
    fail(StatusCode::BAD_REQUEST, message)
}

/// One finished `git` run. Kept as text because both streams are shown to the
/// person when something goes wrong.
struct GitOutput {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn git(root: &Path, args: &[&str]) -> Result<GitOutput, String> {
    match Command::new("git").arg("-C").arg(root).args(args).output() {
        Ok(output) => Ok(GitOutput {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }),
        Err(_) => Err("Git is not installed, or not on PATH.".to_string()),
    }
}

/// Git's own useful line, because it names the real cause. The first non-empty
/// line is often just context ("On branch main"); a `fatal:`/`error:` line or
/// the sentence that actually explains the refusal is what the person needs.
fn git_error(output: &GitOutput, fallback: &str) -> String {
    let text = if output.stderr.trim().is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    fn interesting(line: &str) -> bool {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("fatal:")
            || lower.starts_with("error:")
            || lower.contains("nothing to commit")
            || lower.contains("no changes added to commit")
            || lower.contains("would be overwritten")
            || lower.contains("not a git repository")
            || lower.contains("unmerged files")
    }
    let chosen = lines
        .iter()
        .find(|line| interesting(line))
        .or_else(|| lines.first())
        .copied()
        .unwrap_or("");
    if chosen.is_empty() {
        fallback.to_string()
    } else {
        chosen.chars().take(400).collect()
    }
}

fn is_missing_repo(message: &str) -> bool {
    message
        .to_ascii_lowercase()
        .contains("not a git repository")
}

/// Turn a git failure into an HTTP failure with a useful status: a missing
/// repository is a 400 ("open a repository"), a refused checkout is a 409
/// ("the tree would be overwritten"), anything else is a 400 with git's text.
fn git_failure(message: String) -> Response {
    if is_missing_repo(&message) {
        return bad("This folder is not a Git repository. Open a repository first.");
    }
    let lower = message.to_ascii_lowercase();
    if lower.contains("would be overwritten")
        || lower.contains("not possible because you have unmerged files")
        || lower.contains("nothing to commit")
        || lower.contains("no changes added to commit")
    {
        return fail(StatusCode::CONFLICT, message);
    }
    bad(message)
}

// ---------------------------------------------------------------------------
// Path and message validation

/// Read `paths: [...]` (or a single `path`) from a request body and return the
/// workspace-relative spellings git expects.
pub(crate) fn clean_paths(input: &Value) -> Result<Vec<String>, String> {
    let mut raw: Vec<String> = Vec::new();
    if let Some(list) = input.get("paths").and_then(Value::as_array) {
        for item in list {
            let Some(text) = item.as_str() else {
                return Err("Every path must be a string.".to_string());
            };
            raw.push(text.to_string());
        }
    } else if let Some(text) = input.get("path").and_then(Value::as_str) {
        raw.push(text.to_string());
    }
    if raw.is_empty() {
        return Err("Provide the file path(s) to work on.".to_string());
    }
    if raw.len() > MAX_PATHS {
        return Err(format!("At most {MAX_PATHS} paths per request."));
    }
    let mut paths: Vec<String> = Vec::new();
    for text in raw {
        let path = normalise_path(&text)?;
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok(paths)
}

/// The single gate every path goes through. Git accepts forward slashes on
/// every platform, so separators are normalised first; then anything that
/// could escape the folder or reach git's own metadata is refused.
pub(crate) fn normalise_path(raw: &str) -> Result<String, String> {
    let cleaned = raw.trim().replace('\\', "/");
    if cleaned.is_empty() {
        return Err("A path cannot be empty.".to_string());
    }
    if cleaned.chars().any(char::is_control) {
        return Err("A path cannot contain control characters.".to_string());
    }
    if cleaned.starts_with('/') {
        return Err("Paths must be relative to the open folder.".to_string());
    }
    let bytes = cleaned.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return Err("Paths must be relative to the open folder.".to_string());
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in cleaned.split('/') {
        match part {
            "" | "." => continue,
            ".." => return Err("Paths must stay inside the open folder.".to_string()),
            ".git" => return Err("Git's own folder cannot be staged or reverted.".to_string()),
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err("A path cannot be empty.".to_string());
    }
    Ok(parts.join("/"))
}

fn commit_message(input: &Value) -> Result<String, String> {
    let message = input
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if message.is_empty() {
        return Err("Write a commit message first.".to_string());
    }
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(format!(
            "A commit message can be at most {MAX_MESSAGE_CHARS} characters."
        ));
    }
    if message.contains('\0') {
        return Err("A commit message cannot contain a NUL byte.".to_string());
    }
    Ok(message)
}

fn truncate(text: &str, limit: usize) -> (String, bool) {
    if text.len() <= limit {
        return (text.to_string(), false);
    }
    let mut cut = limit;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    (text[..cut].to_string(), true)
}

// ---------------------------------------------------------------------------
// Operations

fn diff_for(root: &Path, path: &str, staged: bool) -> Result<Value, String> {
    let mut args: Vec<&str> = vec!["diff", "--no-color", "--no-ext-diff", "-U3"];
    if staged {
        args.push("--cached");
    }
    args.push("--");
    args.push(path);
    let output = git(root, &args)?;
    if !output.ok {
        return Err(git_error(&output, "git diff failed"));
    }
    if !output.stdout.is_empty() {
        let (text, truncated) = truncate(&output.stdout, MAX_DIFF_BYTES);
        return Ok(json!({
            "path": path,
            "staged": staged,
            "untracked": false,
            "diff": text,
            "truncated": truncated,
        }));
    }
    if staged {
        return Ok(json!({
            "path": path, "staged": true, "untracked": false, "diff": "", "truncated": false,
        }));
    }
    // An untracked file has no `git diff`; showing an empty box would claim it
    // is unchanged. Present it as a new file instead, from the real bytes.
    let tracked = git(root, &["ls-files", "--error-unmatch", "--", path])
        .map(|output| output.ok)
        .unwrap_or(false);
    if tracked {
        return Ok(json!({
            "path": path, "staged": false, "untracked": false, "diff": "", "truncated": false,
        }));
    }
    let full = crate::workspace::resolve_inside(root, path)
        .map_err(|error| error.message().to_string())?;
    let bytes = std::fs::read(&full).map_err(|_| format!("{path} is not a readable file."))?;
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_DIFF_BYTES + 1)]).to_string();
    let mut body = format!("new file: {path}\n");
    for line in text.lines() {
        body.push('+');
        body.push_str(line);
        body.push('\n');
    }
    let (body, mut truncated) = truncate(&body, MAX_DIFF_BYTES);
    truncated = truncated || bytes.len() > MAX_DIFF_BYTES;
    Ok(json!({
        "path": path,
        "staged": false,
        "untracked": true,
        "diff": body,
        "truncated": truncated,
    }))
}

fn stage_paths(root: &Path, paths: &[String]) -> Result<Value, String> {
    let mut args: Vec<&str> = vec!["add", "-A", "--"];
    for path in paths {
        args.push(path);
    }
    let output = git(root, &args)?;
    if !output.ok {
        return Err(git_error(&output, "git add failed"));
    }
    Ok(json!({ "ok": true, "staged": paths.len() }))
}

fn unstage_paths(root: &Path, paths: &[String]) -> Result<Value, String> {
    let build = |sub: &str| {
        let mut args: Vec<&str> = vec![sub];
        if sub == "restore" {
            args.push("--staged");
        } else if sub == "reset" {
            args.push("-q");
        } else {
            args.push("--cached");
            args.push("-r");
            args.push("-q");
        }
        args.push("--");
        for path in paths {
            args.push(path);
        }
        git(root, &args)
    };
    // `restore --staged` (git 2.23+) is the precise answer; `reset` covers older
    // git, and `rm --cached` is the one that works in a repository with no
    // first commit yet, where neither of the other two can resolve HEAD.
    let mut last = build("restore")?;
    if !last.ok {
        last = build("reset")?;
    }
    if !last.ok {
        last = build("rm")?;
    }
    if !last.ok {
        return Err(git_error(&last, "git could not unstage these paths"));
    }
    Ok(json!({ "ok": true, "unstaged": paths.len() }))
}

fn discard_paths(root: &Path, paths: &[String]) -> Result<Value, String> {
    for path in paths {
        let tracked = git(root, &["ls-files", "--error-unmatch", "--", path])
            .map(|output| output.ok)
            .unwrap_or(false);
        if tracked {
            let output = git(root, &["restore", "--worktree", "--", path])?;
            if !output.ok {
                return Err(git_error(&output, "git restore failed"));
            }
        } else {
            // An untracked file has nothing to restore to; discarding it means
            // removing it. The route is approval-gated, and the UI confirms.
            let full = crate::workspace::resolve_inside(root, path)
                .map_err(|error| error.message().to_string())?;
            let meta = std::fs::metadata(&full).map_err(|_| format!("{path} no longer exists."))?;
            if meta.is_dir() {
                std::fs::remove_dir_all(&full).map_err(|error| error.to_string())?;
            } else {
                std::fs::remove_file(&full).map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(json!({ "ok": true, "discarded": paths.len() }))
}

fn commit(root: &Path, message: &str, paths: &[String]) -> Result<Value, String> {
    let mut args: Vec<&str> = vec!["commit", "-m", message];
    if !paths.is_empty() {
        args.push("--");
        for path in paths {
            args.push(path);
        }
    }
    let output = git(root, &args)?;
    if !output.ok {
        return Err(git_error(&output, "git commit failed"));
    }
    let short = git(root, &["rev-parse", "--short", "HEAD"])
        .ok()
        .filter(|output| output.ok)
        .map(|output| output.stdout.trim().to_string())
        .unwrap_or_default();
    Ok(json!({
        "ok": true,
        "commit": short,
        "summary": output.stdout.lines().take(4).map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>(),
    }))
}

fn branches(root: &Path) -> Result<Value, String> {
    let output = git(
        root,
        &[
            "branch",
            "--all",
            "--format=%(HEAD)%09%(refname)%09%(refname:short)",
        ],
    )?;
    if !output.ok {
        return Err(git_error(&output, "git branch failed"));
    }
    let mut current = String::new();
    let mut detached = false;
    let mut list = Vec::new();
    for line in output.stdout.lines() {
        let mut parts = line.splitn(3, '\t');
        let head = parts.next().unwrap_or("").trim();
        let full = parts.next().unwrap_or("").trim();
        let short = parts.next().unwrap_or("").trim();
        if short.is_empty() {
            continue;
        }
        let is_remote = full.starts_with("refs/remotes/");
        let is_current = head == "*";
        if is_current {
            if short == "(detached)" {
                detached = true;
            } else {
                current = short.to_string();
            }
        }
        list.push(json!({
            "name": short,
            "remote": is_remote,
            "current": is_current && !detached,
        }));
    }
    if detached || current.is_empty() {
        if let Ok(output) = git(root, &["rev-parse", "--short", "HEAD"]) {
            if output.ok {
                current = format!("detached at {}", output.stdout.trim());
                detached = true;
            }
        }
    }
    Ok(json!({ "current": current, "branches": list, "detached": detached }))
}

fn validate_branch(root: &Path, raw: &str) -> Result<String, String> {
    let name = raw.trim().to_string();
    if name.is_empty() {
        return Err("Name the branch first.".to_string());
    }
    if name.starts_with('-') {
        return Err("A branch name cannot start with a dash.".to_string());
    }
    if name.chars().count() > 200 || name.chars().any(char::is_control) {
        return Err("That branch name is not valid.".to_string());
    }
    let output = git(root, &["check-ref-format", "--branch", &name])?;
    if !output.ok {
        return Err(format!("`{name}` is not a valid branch name."));
    }
    Ok(name)
}

fn create_branch(root: &Path, raw: &str) -> Result<Value, String> {
    let name = validate_branch(root, raw)?;
    let output = git(root, &["checkout", "-b", &name])?;
    if !output.ok {
        return Err(git_error(&output, "git could not create the branch"));
    }
    Ok(json!({ "ok": true, "branch": name }))
}

fn checkout_branch(root: &Path, raw: &str) -> Result<Value, String> {
    let name = validate_branch(root, raw)?;
    let output = git(root, &["checkout", &name])?;
    if !output.ok {
        return Err(git_error(&output, "git could not switch branches"));
    }
    Ok(json!({ "ok": true, "branch": name }))
}

pub(crate) fn parse_log(output: &str) -> Vec<Value> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(5, '\t');
            let hash = parts.next()?.trim();
            let short = parts.next().unwrap_or("").trim();
            let author = parts.next().unwrap_or("").trim();
            let date = parts.next().unwrap_or("").trim();
            let subject = parts.next().unwrap_or("").trim();
            if hash.is_empty() {
                return None;
            }
            Some(json!({
                "hash": hash,
                "short": short,
                "author": author,
                "date": date,
                "subject": subject,
            }))
        })
        .collect()
}

fn log_entries(root: &Path, limit: usize) -> Result<Value, String> {
    let limit = limit.clamp(1, MAX_LOG_ENTRIES);
    let format = "--format=%H%x09%h%x09%an%x09%aI%x09%s";
    let count = format!("-n{limit}");
    let output = git(root, &["log", &count, format])?;
    if !output.ok {
        let message = git_error(&output, "git log failed");
        // A repository with no commits is not an error; it is an empty history.
        if message
            .to_ascii_lowercase()
            .contains("does not have any commits")
            || message.to_ascii_lowercase().contains("unknown revision")
        {
            return Ok(json!({ "commits": [] }));
        }
        return Err(message);
    }
    Ok(json!({ "commits": parse_log(&output.stdout) }))
}

// ---------------------------------------------------------------------------
// Routes

#[derive(serde::Deserialize)]
struct DiffQuery {
    path: String,
    #[serde(default)]
    staged: Option<String>,
}

/// The open folder, or the response explaining why there is none. Boxed because
/// an axum `Response` is large enough that clippy rightly objects to carrying
/// it in every `Ok` on the stack.
fn workspace_root(state: &AppState) -> Result<PathBuf, Box<Response>> {
    state
        .workspace
        .root()
        .map_err(|error| Box::new(error.into_response()))
}

fn approval(input: &Value) -> bool {
    input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

async fn diff(State(state): State<AppState>, Query(query): Query<DiffQuery>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let path = match normalise_path(&query.path) {
        Ok(path) => path,
        Err(message) => return bad(message),
    };
    let staged = matches!(
        query.staged.as_deref(),
        Some("1") | Some("true") | Some("yes")
    );
    match diff_for(&root, &path, staged) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn stage(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let paths = match clean_paths(&input) {
        Ok(paths) => paths,
        Err(message) => return bad(message),
    };
    let approve = approval(&input);
    for path in &paths {
        if let Some(response) = guard_user(
            &state,
            Action::Git,
            path,
            "",
            approve,
            "stage a file for commit",
        ) {
            return response;
        }
    }
    match stage_paths(&root, &paths) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn unstage(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let paths = match clean_paths(&input) {
        Ok(paths) => paths,
        Err(message) => return bad(message),
    };
    let approve = approval(&input);
    for path in &paths {
        if let Some(response) = guard_user(&state, Action::Git, path, "", approve, "unstage a file")
        {
            return response;
        }
    }
    match unstage_paths(&root, &paths) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn discard(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let paths = match clean_paths(&input) {
        Ok(paths) => paths,
        Err(message) => return bad(message),
    };
    let approve = approval(&input);
    for path in &paths {
        if let Some(response) = guard_user(
            &state,
            Action::Git,
            path,
            "",
            approve,
            "discard changes to a file",
        ) {
            return response;
        }
    }
    match discard_paths(&root, &paths) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn commit_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let message = match commit_message(&input) {
        Ok(message) => message,
        Err(message) => return bad(message),
    };
    let paths = match input.get("paths").and_then(Value::as_array) {
        Some(_) => match clean_paths(&input) {
            Ok(paths) => paths,
            Err(message) => return bad(message),
        },
        None => Vec::new(),
    };
    let approve = approval(&input);
    let target = if paths.is_empty() {
        "commit".to_string()
    } else {
        paths.join(",")
    };
    if let Some(response) = guard_user(&state, Action::Git, &target, "", approve, "create a commit")
    {
        return response;
    }
    match commit(&root, &message, &paths) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn branches_route(State(state): State<AppState>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match branches(&root) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn create_branch_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let name = input.get("name").and_then(Value::as_str).unwrap_or("");
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        name.trim(),
        "",
        approve,
        "create a branch",
    ) {
        return response;
    }
    match create_branch(&root, name) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn checkout_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let name = input.get("name").and_then(Value::as_str).unwrap_or("");
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        name.trim(),
        "",
        approve,
        "switch branches",
    ) {
        return response;
    }
    match checkout_branch(&root, name) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn log_route(
    State(state): State<AppState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(30);
    match log_entries(&root, limit) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

/// Routes are merged into `workspace::router`, so they inherit the same
/// loopback-origin guard and body limit as every other workspace route.
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/workspace/git/diff", get(diff))
        .route("/api/workspace/git/stage", post(stage))
        .route("/api/workspace/git/unstage", post(unstage))
        .route("/api/workspace/git/discard", post(discard))
        .route("/api/workspace/git/commit", post(commit_route))
        .route("/api/workspace/git/branches", get(branches_route))
        .route("/api/workspace/git/branch", post(create_branch_route))
        .route("/api/workspace/git/checkout", post(checkout_route))
        .route("/api/workspace/git/log", get(log_route))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn temp_repo(name: &str) -> Option<PathBuf> {
        if !git_available() {
            return None;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("saarthi-git-{name}-{}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // Pin the line endings: the fixture writes LF and the assertions read
        // back what was written, so the host's autocrlf setting must not turn a
        // restored file into CRLF and make the test platform-dependent.
        let setup: [&[&str]; 6] = [
            &["init", "-q", "-b", "main"],
            &["config", "user.email", "test@example.com"],
            &["config", "user.name", "Test"],
            &["config", "commit.gpgsign", "false"],
            &["config", "core.autocrlf", "false"],
            &["config", "core.eol", "lf"],
        ];
        for args in setup {
            let output = git(&root, args).unwrap();
            assert!(output.ok, "git {args:?}: {}", output.stderr);
        }
        Some(root)
    }

    #[test]
    fn rejects_paths_that_could_escape_or_touch_git_metadata() {
        assert!(normalise_path("../secrets.txt").is_err());
        assert!(normalise_path("src/../../secrets.txt").is_err());
        assert!(normalise_path("/etc/passwd").is_err());
        assert!(normalise_path("C:\\Windows\\win.ini").is_err());
        assert!(normalise_path(".git/config").is_err());
        assert!(normalise_path("src/.git/config").is_err());
        assert!(normalise_path("src/\u{7}file").is_err());
        assert!(normalise_path("   ").is_err());
        assert_eq!(normalise_path("src\\main.rs").unwrap(), "src/main.rs");
        assert_eq!(normalise_path("./src//lib.rs").unwrap(), "src/lib.rs");
        assert_eq!(normalise_path("src/lib.rs").unwrap(), "src/lib.rs");
        // A file merely named like git's folder is not git's folder.
        assert_eq!(normalise_path(".gitignore").unwrap(), ".gitignore");
    }

    #[test]
    fn deduplicates_and_refuses_empty_or_oversized_lists() {
        let paths =
            clean_paths(&json!({"paths": ["src/a.rs", "./src/a.rs", "src\\b.rs"]})).unwrap();
        assert_eq!(paths, vec!["src/a.rs", "src/b.rs"]);
        assert!(clean_paths(&json!({})).is_err());
        assert!(clean_paths(&json!({"paths": []})).is_err());
        let too_many: Vec<String> = (0..201).map(|n| format!("f{n}.txt")).collect();
        assert!(clean_paths(&json!({ "paths": too_many })).is_err());
    }

    #[test]
    fn validates_commit_messages() {
        assert!(commit_message(&json!({"message": "   "})).is_err());
        assert!(commit_message(&json!({})).is_err());
        assert_eq!(
            commit_message(&json!({"message": "  add login  "})).unwrap(),
            "add login"
        );
        let long = "x".repeat(MAX_MESSAGE_CHARS + 1);
        assert!(commit_message(&json!({ "message": long })).is_err());
    }

    #[test]
    fn parses_log_lines_with_tabs_in_the_subject() {
        let commits = parse_log(
            "abc123\tdef456\tAda\t2026-01-02T03:04:05+00:00\tfix: a\tb\n\
             \n\
             def789\tdef789\tBo\t2026-01-03T00:00:00+00:00\tsecond",
        );
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0]["short"], "def456");
        assert_eq!(commits[0]["subject"], "fix: a\tb");
        assert_eq!(commits[1]["subject"], "second");
    }

    #[test]
    fn caps_large_diffs_without_splitting_a_character() {
        let text = "é".repeat(MAX_DIFF_BYTES);
        let (cut, truncated) = truncate(&text, MAX_DIFF_BYTES - 1);
        assert!(truncated);
        assert!(cut.len() < MAX_DIFF_BYTES);
        assert!(cut.is_char_boundary(cut.len()));
        let (whole, truncated) = truncate("short", MAX_DIFF_BYTES);
        assert_eq!(whole, "short");
        assert!(!truncated);
    }

    #[test]
    fn stages_diffs_commits_and_reverts_a_real_repository() {
        let Some(root) = temp_repo("round-trip") else {
            return;
        };
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        let paths = clean_paths(&json!({"path": "a.txt"})).unwrap();

        stage_paths(&root, &paths).unwrap();
        let committed = commit(&root, "add a.txt", &[]).unwrap();
        assert_eq!(committed["ok"], true);
        assert!(!committed["commit"].as_str().unwrap().is_empty());

        let history = log_entries(&root, 10).unwrap();
        assert_eq!(history["commits"][0]["subject"], "add a.txt");

        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        let diff = diff_for(&root, "a.txt", false).unwrap();
        assert!(diff["diff"].as_str().unwrap().contains("+two"));
        assert_eq!(diff["untracked"], false);
        assert_eq!(diff["staged"], false);

        discard_paths(&root, &["a.txt".to_string()]).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "one\n"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shows_an_untracked_file_as_a_new_file_and_discards_it() {
        let Some(root) = temp_repo("untracked") else {
            return;
        };
        std::fs::write(root.join("new.txt"), "hello\n").unwrap();
        let diff = diff_for(&root, "new.txt", false).unwrap();
        assert_eq!(diff["untracked"], true);
        let body = diff["diff"].as_str().unwrap();
        assert!(body.contains("new file: new.txt"));
        assert!(body.contains("+hello"));

        let paths = clean_paths(&json!({"path": "new.txt"})).unwrap();
        stage_paths(&root, &paths).unwrap();
        let diff = diff_for(&root, "new.txt", true).unwrap();
        assert_eq!(diff["staged"], true);
        assert!(diff["diff"].as_str().unwrap().contains("+hello"));

        unstage_paths(&root, &paths).unwrap();
        discard_paths(&root, &paths).unwrap();
        assert!(!root.join("new.txt").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn creates_and_switches_branches() {
        let Some(root) = temp_repo("branches") else {
            return;
        };
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        let paths = clean_paths(&json!({"path": "a.txt"})).unwrap();
        stage_paths(&root, &paths).unwrap();
        commit(&root, "first", &[]).unwrap();

        let before = branches(&root).unwrap();
        assert_eq!(before["current"], "main");
        let listed = before["branches"].as_array().unwrap();
        assert!(listed
            .iter()
            .any(|branch| branch["name"] == "main" && branch["current"] == true));

        create_branch(&root, "feature/login").unwrap();
        assert_eq!(branches(&root).unwrap()["current"], "feature/login");

        checkout_branch(&root, "main").unwrap();
        assert_eq!(branches(&root).unwrap()["current"], "main");

        assert!(create_branch(&root, "-evil").is_err());
        assert!(create_branch(&root, "bad name").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reports_an_empty_repository_as_empty_history() {
        let Some(root) = temp_repo("empty") else {
            return;
        };
        let history = log_entries(&root, 10).unwrap();
        assert_eq!(history["commits"].as_array().unwrap().len(), 0);
        // Committing with nothing staged is a conflict, not a crash.
        let error = commit(&root, "nothing", &[]).unwrap_err();
        assert!(!error.is_empty());
        let response = git_failure(error);
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_repository_is_reported_as_a_bad_request() {
        let response = git_failure(
            "fatal: not a git repository (or any of the parent directories): .git".to_string(),
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
