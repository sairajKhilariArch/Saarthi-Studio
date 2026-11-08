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

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
/// A remote operation can wait on a server, a proxy or a credential helper, so
/// it needs a ceiling a local command does not. Without one an unreachable host
/// would hold the HTTP request (and the pane) open indefinitely.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(90);
/// Output kept per stream from a remote operation. Progress and ref summaries
/// are small; a misbehaving remote must not be allowed to fill memory.
const MAX_NETWORK_BYTES: u64 = 64 * 1024;

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

/// Git with a deadline and no terminal prompt. A credential prompt on a process
/// with no terminal would wait until the deadline anyway, and an unreachable
/// host would wait forever; `GIT_TERMINAL_PROMPT=0` makes git fail honestly
/// instead. Both streams are drained on their own threads so a chatty remote
/// cannot deadlock on a full pipe while the parent waits for exit.
fn git_network(root: &Path, args: &[&str]) -> Result<GitOutput, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Git is not installed, or not on PATH.".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Git did not give us its output.")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Git did not give us its output.")?;
    let out_thread = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.take(MAX_NETWORK_BYTES).read_to_end(&mut buffer);
        buffer
    });
    let err_thread = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stderr.take(MAX_NETWORK_BYTES).read_to_end(&mut buffer);
        buffer
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < NETWORK_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // Killing the child closes its pipes, so both readers finish before join.
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    match status {
        Some(status) => Ok(GitOutput {
            ok: status.success(),
            stdout: String::from_utf8_lossy(&stdout).to_string(),
            stderr: String::from_utf8_lossy(&stderr).to_string(),
        }),
        None => Err(format!(
            "The remote did not answer within {} seconds. Check the connection and try again.",
            NETWORK_TIMEOUT.as_secs()
        )),
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
            || lower.contains("not possible to fast-forward")
            || lower.contains("non-fast-forward")
            || lower.contains("no upstream branch")
            || lower.contains("could not read username")
            || lower.contains("authentication failed")
            || lower.contains("permission denied (publickey)")
            || lower.contains("could not resolve host")
            || lower.contains("repository not found")
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
        || lower.contains("not possible to fast-forward")
        || lower.contains("non-fast-forward")
        || lower.contains("rejected")
        || lower.contains("diverged")
        || lower.contains("patch failed")
        || lower.contains("does not apply")
    {
        return fail(StatusCode::CONFLICT, message);
    }
    // A remote that cannot be reached is not a bad request; it is an upstream
    // failure, and the person needs to know the difference.
    if lower.contains("could not read username")
        || lower.contains("authentication failed")
        || lower.contains("permission denied (publickey)")
        || lower.contains("could not resolve host")
        || lower.contains("connection refused")
        || lower.contains("repository not found")
        || lower.contains("unable to access")
    {
        return fail(StatusCode::BAD_GATEWAY, message);
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

/// A remote name from the request. Kept narrow because it reaches argv: it must
/// not be able to look like an option or carry punctuation git reinterprets.
fn requested_remote(input: &Value) -> Result<Option<String>, String> {
    let Some(value) = input.get("remote") else {
        return Ok(None);
    };
    let name = value
        .as_str()
        .ok_or("The remote must be a name, not a number or object.")?;
    Ok(Some(valid_remote_name(name)?))
}

/// Narrow a remote name before it reaches argv: no leading dash, no whitespace,
/// and only the punctuation git accepts in a remote name.
fn valid_remote_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("Name the remote first.".to_string());
    }
    if name.starts_with('-') {
        return Err("A remote name cannot start with a dash.".to_string());
    }
    if name.chars().count() > 100
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/'))
    {
        return Err("That remote name is not valid.".to_string());
    }
    Ok(name.to_string())
}

const MAX_REMOTE_URL: usize = 2048;

/// A remote URL from the request. It travels as its own argv entry, so only a
/// leading dash, whitespace or a control character can make it mean something
/// else; scp-style `git@host:path` and file paths are both left alone.
fn remote_url(input: &Value) -> Result<String, String> {
    let url = input
        .get("url")
        .and_then(Value::as_str)
        .ok_or("Provide the remote URL.")?
        .trim()
        .to_string();
    if url.is_empty() {
        return Err("Provide the remote URL.".to_string());
    }
    if url.chars().count() > MAX_REMOTE_URL {
        return Err("That remote URL is too long.".to_string());
    }
    if url.starts_with('-') {
        return Err("A remote URL cannot start with a dash.".to_string());
    }
    if url.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("A remote URL cannot contain spaces or control characters.".to_string());
    }
    Ok(url)
}

/// Every configured remote with the URL git reports, which is what the pane
/// lists. A remote that pushes somewhere else keeps both URLs.
fn remote_list(root: &Path) -> Result<Vec<Value>, String> {
    let output = git(root, &["remote", "-v"])?;
    if !output.ok {
        return Err(git_error(&output, "git remote failed"));
    }
    let mut remotes: Vec<Value> = Vec::new();
    for line in output.stdout.lines() {
        // "origin\thttps://example.com/repo.git (fetch)"
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let (url, kind) = match rest.rsplit_once(" (") {
            Some((url, kind)) => (url.trim(), kind.trim_end_matches(')')),
            None => (rest.trim(), ""),
        };
        if let Some(existing) = remotes
            .iter_mut()
            .find(|item| item["name"].as_str() == Some(name))
        {
            if kind == "push" {
                existing["pushUrl"] = json!(url);
            }
            continue;
        }
        remotes.push(json!({ "name": name, "url": url, "pushUrl": "" }));
    }
    for item in remotes.iter_mut() {
        if item["pushUrl"].as_str().unwrap_or("").is_empty() {
            item["pushUrl"] = item["url"].clone();
        }
    }
    Ok(remotes)
}

fn add_remote(root: &Path, name: &str, url: &str) -> Result<Value, String> {
    let output = git(root, &["remote", "add", name, url])?;
    if !output.ok {
        return Err(git_error(&output, "git remote add failed"));
    }
    Ok(json!({
        "ok": true,
        "name": name,
        "url": url,
        "remotes": remote_list(root)?,
    }))
}

/// Change the URL of a remote that already exists. Refused by name otherwise,
/// so a typo cannot quietly create a second remote.
fn set_remote_url(root: &Path, name: &str, url: &str) -> Result<Value, String> {
    let existing = remote_list(root)?;
    if !existing
        .iter()
        .any(|item| item["name"].as_str() == Some(name))
    {
        return Err(format!("`{name}` is not a remote of this repository."));
    }
    let output = git(root, &["remote", "set-url", name, url])?;
    if !output.ok {
        return Err(git_error(&output, "git remote set-url failed"));
    }
    Ok(json!({
        "ok": true,
        "name": name,
        "url": url,
        "remotes": remote_list(root)?,
    }))
}

fn remove_remote(root: &Path, name: &str) -> Result<Value, String> {
    let existing = remote_list(root)?;
    if !existing
        .iter()
        .any(|item| item["name"].as_str() == Some(name))
    {
        return Err(format!("`{name}` is not a remote of this repository."));
    }
    let output = git(root, &["remote", "remove", name])?;
    if !output.ok {
        return Err(git_error(&output, "git remote remove failed"));
    }
    Ok(json!({
        "ok": true,
        "removed": name,
        "remotes": remote_list(root)?,
    }))
}

/// The remote that actually exists: the requested one, else `origin`, else the
/// first configured remote. Resolving before the operation means the permission
/// target and the error name the thing that was really contacted.
fn remote_target(root: &Path, requested: Option<&str>) -> Result<String, String> {
    let output = git(root, &["remote"])?;
    if !output.ok {
        return Err(git_error(&output, "git remote failed"));
    }
    let remotes: Vec<&str> = output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let Some(first) = remotes.first() else {
        return Err(
            "This repository has no remote. Add one in the Source Control pane first.".to_string(),
        );
    };
    match requested {
        Some(name) if !name.is_empty() => {
            if remotes.contains(&name) {
                Ok(name.to_string())
            } else {
                Err(format!("`{name}` is not a remote of this repository."))
            }
        }
        _ if remotes.contains(&"origin") => Ok("origin".to_string()),
        _ => Ok((*first).to_string()),
    }
}

fn current_branch(root: &Path) -> Result<String, String> {
    let output = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if !output.ok {
        return Err(git_error(&output, "git rev-parse failed"));
    }
    let branch = output.stdout.trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        return Err("HEAD is detached. Check out a branch before pulling or pushing.".to_string());
    }
    Ok(branch)
}

fn upstream_of(root: &Path) -> Option<String> {
    git(
        root,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .ok()
    .filter(|output| output.ok)
    .map(|output| output.stdout.trim().to_string())
    .filter(|name| !name.is_empty())
}

/// The lines worth showing: git's progress and ref summaries without blank
/// noise, bounded so a large fetch cannot become a large JSON response.
fn network_summary(output: &GitOutput) -> Vec<String> {
    output
        .stderr
        .lines()
        .chain(output.stdout.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(30)
        .map(|line| line.chars().take(300).collect())
        .collect()
}

fn fetch_remote(root: &Path, remote: &str) -> Result<Value, String> {
    let output = git_network(root, &["fetch", "--prune", remote])?;
    if !output.ok {
        return Err(git_error(&output, "git fetch failed"));
    }
    Ok(json!({
        "ok": true,
        "remote": remote,
        "summary": network_summary(&output),
    }))
}

fn pull_branch(root: &Path) -> Result<Value, String> {
    let branch = current_branch(root)?;
    let Some(upstream) = upstream_of(root) else {
        return Err(format!(
            "`{branch}` has no upstream branch. Push it first, then pull."
        ));
    };
    // `--ff-only` is the whole policy: a pull never creates a merge commit or
    // leaves conflict markers on disk behind the person's back. When it cannot
    // fast-forward, git's refusal is the answer.
    let output = git_network(root, &["pull", "--ff-only"])?;
    if !output.ok {
        return Err(git_error(&output, "git pull failed"));
    }
    let commit = git(root, &["rev-parse", "--short", "HEAD"])
        .ok()
        .filter(|output| output.ok)
        .map(|output| output.stdout.trim().to_string())
        .unwrap_or_default();
    Ok(json!({
        "ok": true,
        "branch": branch,
        "upstream": upstream,
        "commit": commit,
        "summary": network_summary(&output),
    }))
}

fn push_branch(root: &Path) -> Result<Value, String> {
    let branch = current_branch(root)?;
    let had_upstream = upstream_of(root).is_some();
    let mut args: Vec<String> = vec!["push".to_string()];
    if !had_upstream {
        // A first push that only prints git's "no upstream branch" paragraph is
        // not useful. Name the default remote explicitly instead, and only then.
        let remote = remote_target(root, None)?;
        args.push("-u".to_string());
        args.push(remote);
        args.push(branch.clone());
    }
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = git_network(root, &borrowed)?;
    if !output.ok {
        return Err(git_error(&output, "git push failed"));
    }
    Ok(json!({
        "ok": true,
        "branch": branch,
        "upstream": upstream_of(root),
        "upstreamSet": !had_upstream,
        "summary": network_summary(&output),
    }))
}

/// Conflict markers left in a staged file. `git grep --cached` reads the index,
/// so this answers for exactly the content a commit would record.
fn staged_conflict_markers(root: &Path) -> Result<Vec<String>, String> {
    let output = git(
        root,
        &["grep", "--cached", "-n", "-E", "^(<<<<<<< |>>>>>>> )"],
    )?;
    // `git grep` exits 1 when there is no match, which is the good outcome.
    if !output.ok {
        return Ok(Vec::new());
    }
    Ok(output
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(6)
        .map(|line| line.trim().to_string())
        .collect())
}

fn commit(root: &Path, message: &str, paths: &[String]) -> Result<Value, String> {
    // A merge that left markers behind must not be recorded as history: the
    // conflict is in the content, not in the message.
    let conflicts = staged_conflict_markers(root)?;
    if !conflicts.is_empty() {
        return Err(format!(
            "Refusing to commit: staged files still contain conflict markers ({}). Resolve them first.",
            conflicts.join("; ")
        ));
    }
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

/// What a merge attempt actually did. A conflict is not an error in the same
/// sense as a bad ref: git stops on purpose with markers in the files and
/// `MERGE_HEAD` set, so the person can resolve it or abort. Reporting that as a
/// plain failure would hide the one thing they need — which files to fix.
enum MergeOutcome {
    Merged {
        branch: String,
        current: String,
        fast_forward: bool,
        summary: String,
    },
    Conflict {
        branch: String,
        current: String,
        paths: Vec<String>,
        output: String,
    },
}

/// A branch name from the request. It reaches argv, so it is narrowed the same
/// way a remote name is: no leading dash, no whitespace, no control characters,
/// and nothing that could be read as a revision range.
fn merge_target(raw: &str) -> Result<String, String> {
    if raw.trim().is_empty() {
        return Err("Name the branch to merge in.".to_string());
    }
    valid_ref(raw)
}

/// A ref name that reaches argv, narrowed the way a remote name is: no leading
/// dash, no whitespace or control characters, and nothing that could be read as
/// a revision range.
fn valid_ref(raw: &str) -> Result<String, String> {
    let name = raw.trim().to_string();
    if name.starts_with('-') {
        return Err("A branch name cannot start with a dash.".to_string());
    }
    if name.chars().count() > 200
        || name.contains("..")
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/'))
    {
        return Err("That branch name is not valid.".to_string());
    }
    Ok(name)
}

/// The branch checked out right now, or `None` on a detached HEAD. The result
/// names it, so the person is told what they merged into.
fn branch_at_head(root: &Path) -> Option<String> {
    let output = git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok()?;
    let name = output.stdout.trim();
    if output.ok && !name.is_empty() {
        Some(name.to_string())
    } else {
        None
    }
}

/// Unmerged paths, exactly as `git status` reports them. `git merge` writes
/// these and stops; they are the conflict list the pane shows.
fn unmerged_paths(root: &Path) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let Ok(output) = git(root, &["status", "--porcelain"]) else {
        return paths;
    };
    for line in output.stdout.lines() {
        let Some(code) = line.get(..2) else {
            continue;
        };
        if matches!(code, "UU" | "AA" | "DD" | "AU" | "UA" | "DU" | "UD") {
            let path = line.get(3..).unwrap_or("").trim();
            if !path.is_empty() {
                paths.push(path.to_string());
            }
        }
    }
    paths
}

/// Merge a branch into the checked-out one. Refused while the tree is dirty,
/// because a merge that fails halfway over uncommitted work is hard to explain
/// and harder to undo; committing or stashing first is what git itself asks.
fn merge_branch(root: &Path, raw: &str) -> Result<MergeOutcome, (StatusCode, String)> {
    let branch = merge_target(raw).map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let dirty = dirty_tracked(root)?;
    if dirty > 0 {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "Commit or stash your {dirty} changed file(s) before merging: a merge started over uncommitted work is hard to undo."
            ),
        ));
    }
    let current = branch_at_head(root).unwrap_or_else(|| "HEAD".to_string());
    // `--no-edit` matters: without it git opens an editor for the merge commit,
    // and a process with no terminal would hang until the request timed out.
    let output = git(root, &["merge", "--no-edit", &branch])
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    if output.ok {
        let fast_forward = output.stdout.contains("Fast-forward");
        let summary = output
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(12)
            .collect::<Vec<&str>>()
            .join("\n");
        return Ok(MergeOutcome::Merged {
            branch,
            current,
            fast_forward,
            summary,
        });
    }
    let paths = unmerged_paths(root);
    if !paths.is_empty() {
        let text = format!("{}\n{}", output.stdout.trim(), output.stderr.trim());
        return Ok(MergeOutcome::Conflict {
            branch,
            current,
            paths,
            output: text.trim().to_string(),
        });
    }
    let text = format!("{}\n{}", output.stderr, output.stdout);
    if text.contains("not something we can merge")
        || text.contains("is not a commit")
        || text.contains("unknown revision")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("There is no branch or commit named `{branch}` in this repository."),
        ));
    }
    Err((
        StatusCode::CONFLICT,
        git_error(&output, "git could not merge"),
    ))
}

/// A whole-tree checkpoint: a commit object nothing points at, built from a
/// throwaway index.
pub(crate) struct TreeSnapshot {
    pub commit: String,
    pub paths: usize,
}

/// One path a snapshot restore would touch. `delete` means the checkpoint did
/// not contain the file, so putting the tree back means removing it.
pub(crate) struct SnapshotChange {
    pub path: String,
    pub delete: bool,
}

fn temp_index_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "saarthi-checkpoint-{}-{nanos}.index",
        std::process::id()
    ))
}

/// Snapshot the working tree — tracked edits and untracked files alike — into a
/// commit that no ref points at. `GIT_INDEX_FILE` is what keeps this read-only
/// from the user's point of view: `git add -A` writes into a throwaway index,
/// `write-tree` turns it into a tree, and `commit-tree` records it. The worktree
/// and the real index are never touched, so a staged change stays staged.
///
/// A folder that is not a repository is not a failure worth reporting: the
/// caller gets `Err` and carries on without a checkpoint.
pub(crate) fn snapshot_tree(root: &Path) -> Result<TreeSnapshot, String> {
    let index = temp_index_path();
    let with_index = |args: &[&str]| -> Result<GitOutput, String> {
        match Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_INDEX_FILE", &index)
            .output()
        {
            Ok(output) => Ok(GitOutput {
                ok: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            }),
            Err(_) => Err("Git is not installed, or not on PATH.".to_string()),
        }
    };
    let result = with_index(&["add", "-A", "--", "."]).and_then(|output| {
        if !output.ok {
            return Err(git_error(&output, "a checkpoint needs a Git repository"));
        }
        let tree = with_index(&["write-tree"])?;
        if !tree.ok {
            return Err(git_error(&tree, "git could not write the checkpoint tree"));
        }
        let tree = tree.stdout.trim().to_string();
        let mut args: Vec<String> = vec![
            "-c".to_string(),
            "user.name=Saarthi Studio".to_string(),
            "-c".to_string(),
            "user.email=checkpoint@localhost".to_string(),
            "commit-tree".to_string(),
            tree.clone(),
        ];
        // A repository with no first commit still gets a checkpoint; it just
        // has no parent to record.
        if let Ok(head) = git(root, &["rev-parse", "--verify", "HEAD"]) {
            let sha = head.stdout.trim().to_string();
            if head.ok && !sha.is_empty() {
                args.push("-p".to_string());
                args.push(sha);
            }
        }
        args.push("-m".to_string());
        args.push("Saarthi Studio: checkpoint before an agent run".to_string());
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let commit = git(root, &borrowed)?;
        if !commit.ok {
            return Err(git_error(&commit, "git could not record the checkpoint"));
        }
        let listing = git(root, &["ls-tree", "-r", "--name-only", &tree])?;
        let paths = listing
            .stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        Ok(TreeSnapshot {
            commit: commit.stdout.trim().to_string(),
            paths,
        })
    });
    let _ = std::fs::remove_file(&index);
    result
}

/// Tracked paths that differ from the checkpoint. Untracked files are not
/// listed: a file nobody ever committed has no snapshot content to go back to,
/// and deleting it would be guessing about the person's work.
pub(crate) fn snapshot_changes(root: &Path, commit: &str) -> Result<Vec<SnapshotChange>, String> {
    let diff = git(
        root,
        &["diff", "--name-status", "--no-renames", commit, "--", "."],
    )?;
    if !diff.ok {
        return Err(git_error(&diff, "git could not compare the checkpoint"));
    }
    let mut changes: Vec<SnapshotChange> = Vec::new();
    for line in diff.stdout.lines() {
        let Some((status, path)) = line.split_once('\t') else {
            continue;
        };
        let path = path.trim().to_string();
        if path.is_empty() {
            continue;
        }
        changes.push(SnapshotChange {
            path,
            delete: status.starts_with('A'),
        });
    }
    Ok(changes)
}

/// Put one path back to the checkpoint, or remove it when the checkpoint never
/// had it. `git restore --source` is used rather than rejoining bytes, so binary
/// files, modes and symlinks come back the way git recorded them.
pub(crate) fn restore_path(
    root: &Path,
    commit: &str,
    path: &str,
    delete: bool,
) -> Result<(), String> {
    if delete {
        let full = crate::workspace::resolve_inside(root, path)
            .map_err(|error| error.message().to_string())?;
        if let Err(error) = std::fs::remove_file(&full) {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(error.to_string());
            }
        }
        return Ok(());
    }
    let source = format!("--source={commit}");
    let output = git(root, &["restore", &source, "--worktree", "--", path])?;
    if !output.ok {
        return Err(git_error(&output, "git could not restore that file"));
    }
    Ok(())
}

/// `git merge --abort` puts the branch back as it was before a conflicted
/// merge. Refused when nothing is in progress, so the button cannot pretend.
fn abort_merge(root: &Path) -> Result<Value, (StatusCode, String)> {
    let in_progress = git(root, &["rev-parse", "--quiet", "--verify", "MERGE_HEAD"])
        .map(|output| output.ok)
        .unwrap_or(false);
    if !in_progress {
        return Err((
            StatusCode::CONFLICT,
            "There is no merge in progress, so there is nothing to abort.".to_string(),
        ));
    }
    let output = git(root, &["merge", "--abort"])
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    if !output.ok {
        return Err((
            StatusCode::CONFLICT,
            git_error(&output, "git could not abort the merge"),
        ));
    }
    Ok(json!({ "ok": true, "aborted": true }))
}

/// What a rebase attempt did. Like a merge, a conflict is an outcome with a tree
/// the person has to finish or abort, not an opaque failure.
enum RebaseOutcome {
    Rebased {
        branch: String,
        current: String,
        summary: String,
    },
    Conflict {
        branch: String,
        current: String,
        paths: Vec<String>,
        output: String,
    },
}

fn rebase_target(raw: &str) -> Result<String, String> {
    if raw.trim().is_empty() {
        return Err("Name the branch to rebase onto.".to_string());
    }
    valid_ref(raw)
}

/// Tracked files with uncommitted changes. Untracked files are ignored: git
/// itself only objects when an operation would overwrite one, and a stray build
/// log in the folder must not make history work impossible.
fn dirty_tracked(root: &Path) -> Result<usize, (StatusCode, String)> {
    let output = git(root, &["status", "--porcelain", "--untracked-files=no"])
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    Ok(output
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count())
}

/// Where git keeps a state directory. Worktrees and submodules move `.git`, so
/// this is asked of git rather than assumed to be `.git/<name>`.
fn git_state_path(root: &Path, name: &str) -> Option<PathBuf> {
    let output = git(root, &["rev-parse", "--git-path", name]).ok()?;
    let text = output.stdout.trim();
    if !output.ok || text.is_empty() {
        return None;
    }
    let path = PathBuf::from(text);
    Some(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

fn rebase_in_progress(root: &Path) -> bool {
    ["rebase-merge", "rebase-apply"].iter().any(|name| {
        git_state_path(root, name)
            .map(|path| path.exists())
            .unwrap_or(false)
    })
}

/// Replay the current branch's own commits on top of another branch. History is
/// rewritten on purpose — that is what a rebase is — so it refuses a dirty tree
/// and a branch onto itself, and leaves a conflict for the person to finish or
/// abort exactly like a merge.
fn rebase_branch(root: &Path, raw: &str) -> Result<RebaseOutcome, (StatusCode, String)> {
    let branch = rebase_target(raw).map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let dirty = dirty_tracked(root)?;
    if dirty > 0 {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "Commit or stash your {dirty} changed file(s) before rebasing: a rebase rewrites commits and cannot be undone over uncommitted work."
            ),
        ));
    }
    let current = branch_at_head(root).unwrap_or_else(|| "HEAD".to_string());
    if current == branch {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "`{branch}` is already checked out; rebasing a branch onto itself would do nothing."
            ),
        ));
    }
    let output = git(root, &["rebase", &branch])
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    if output.ok {
        let summary = output
            .stdout
            .lines()
            .chain(output.stderr.lines())
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(12)
            .collect::<Vec<&str>>()
            .join("\n");
        return Ok(RebaseOutcome::Rebased {
            branch,
            current,
            summary,
        });
    }
    let paths = unmerged_paths(root);
    if !paths.is_empty() || rebase_in_progress(root) {
        let text = format!("{}\n{}", output.stdout.trim(), output.stderr.trim());
        return Ok(RebaseOutcome::Conflict {
            branch,
            current,
            paths,
            output: text.trim().to_string(),
        });
    }
    let text = format!("{}\n{}", output.stderr, output.stdout);
    if text.contains("not something we can merge")
        || text.contains("is not a commit")
        || text.contains("unknown revision")
        || text.contains("invalid upstream")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("There is no branch or commit named `{branch}` in this repository."),
        ));
    }
    Err((
        StatusCode::CONFLICT,
        git_error(&output, "git could not rebase"),
    ))
}

/// Finish a rebase whose conflicts the person resolved and staged. `--continue`
/// opens an editor to confirm each replayed message, so a no-op editor is named
/// explicitly with `-c core.editor=true`; a process with no terminal must never
/// wait on one. Conflicts are named before git is asked, because git's own
/// refusal ("you must edit all merge conflicts") does not say which files.
fn continue_rebase(root: &Path) -> Result<Value, (StatusCode, String)> {
    if !rebase_in_progress(root) {
        return Err((
            StatusCode::CONFLICT,
            "There is no rebase in progress, so there is nothing to continue.".to_string(),
        ));
    }
    let unmerged = unmerged_paths(root);
    if !unmerged.is_empty() {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "{} file(s) are still conflicted: {}. Resolve them and stage them before continuing.",
                unmerged.len(),
                unmerged.join(", ")
            ),
        ));
    }
    let output = git(root, &["-c", "core.editor=true", "rebase", "--continue"])
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    if output.ok {
        let summary = output
            .stdout
            .lines()
            .chain(output.stderr.lines())
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(12)
            .collect::<Vec<&str>>()
            .join("\n");
        return Ok(json!({
            "ok": true,
            "continued": true,
            "branch": branch_at_head(root).unwrap_or_else(|| "HEAD".to_string()),
            "summary": summary,
        }));
    }
    // A second conflict is the same outcome as the first: the tree is intact and
    // the person can resolve again or abort.
    let paths = unmerged_paths(root);
    if !paths.is_empty() {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "The rebase stopped again with {} conflicted file(s): {}",
                paths.len(),
                paths.join(", ")
            ),
        ));
    }
    if rebase_in_progress(root) {
        let text = format!("{}\n{}", output.stdout.trim(), output.stderr.trim());
        return Err((
            StatusCode::CONFLICT,
            format!("The rebase is still in progress: {}", text.trim()),
        ));
    }
    Err((
        StatusCode::CONFLICT,
        git_error(&output, "git could not continue the rebase"),
    ))
}

/// `git rebase --abort` puts the branch back where the rebase started, which is
/// the one thing that makes a conflicted rewrite safe to attempt.
fn abort_rebase(root: &Path) -> Result<Value, (StatusCode, String)> {
    if !rebase_in_progress(root) {
        return Err((
            StatusCode::CONFLICT,
            "There is no rebase in progress, so there is nothing to abort.".to_string(),
        ));
    }
    let output = git(root, &["rebase", "--abort"])
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))?;
    if !output.ok {
        return Err((
            StatusCode::CONFLICT,
            git_error(&output, "git could not abort the rebase"),
        ));
    }
    Ok(json!({ "ok": true, "aborted": true }))
}

/// Split a one-file diff into its preamble (`diff --git`, `index`, `---`,
/// `+++`) and its hunks, each starting at `@@`. Git's own header is reused
/// verbatim, so modes, rename lines and line counts are never re-derived.
fn split_hunks(diff: &str) -> (String, Vec<String>) {
    let mut preamble = String::new();
    let mut hunks: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in diff.split_inclusive('\n') {
        if line.starts_with("@@ ") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            current = Some(line.to_string());
        } else if let Some(hunk) = current.as_mut() {
            hunk.push_str(line);
        } else {
            preamble.push_str(line);
        }
    }
    if let Some(hunk) = current {
        hunks.push(hunk);
    }
    (preamble, hunks)
}

/// A valid patch for one hunk of one file. The preamble is required: without
/// `---`/`+++` git apply has no file to patch.
fn hunk_patch(diff: &str, index: usize) -> Result<String, String> {
    let (preamble, hunks) = split_hunks(diff);
    if preamble.trim().is_empty() || hunks.is_empty() {
        return Err(
            "This file has no hunks to move. Stage or unstage the whole file instead.".to_string(),
        );
    }
    let Some(hunk) = hunks.get(index) else {
        return Err(format!(
            "Hunk {} does not exist; this diff has {}.",
            index + 1,
            hunks.len()
        ));
    };
    Ok(format!("{preamble}{hunk}"))
}

/// The two start lines of a `@@ -a,b +c,d @@` header.
fn parse_hunk_start(header: &str) -> Result<(usize, usize), String> {
    let body = header
        .trim_start_matches("@@ ")
        .split(" @@")
        .next()
        .unwrap_or("");
    let mut parts = body.split_whitespace();
    let first = |spec: &str| -> Option<usize> {
        spec.split(',')
            .next()
            .and_then(|value| value.parse::<usize>().ok())
    };
    let old = parts
        .next()
        .and_then(|part| part.strip_prefix('-'))
        .and_then(first);
    let new = parts
        .next()
        .and_then(|part| part.strip_prefix('+'))
        .and_then(first);
    match (old, new) {
        (Some(old), Some(new)) => Ok((old, new)),
        _ => Err("The hunk header could not be read.".to_string()),
    }
}

/// Rewrite one hunk change block with the selected lines only.
///
/// git checks the `@@` counts, so an unselected line has to look the same in the
/// patch's pre-image and post-image. Which text that is depends on the side the
/// index holds: the old text when staging, because the patch runs forward onto
/// the index, and the new text when unstaging, because it runs in reverse. A line
/// that the current side does not contain is dropped instead.
#[allow(clippy::too_many_arguments)]
fn flush_block(
    body: &mut String,
    removals: &mut Vec<(usize, String)>,
    additions: &mut Vec<(usize, String)>,
    picked: &std::collections::HashSet<usize>,
    reversed: bool,
    old_count: &mut usize,
    new_count: &mut usize,
    chosen: &mut usize,
) {
    for i in 0..removals.len().max(additions.len()) {
        let removal = removals.get(i);
        let addition = additions.get(i);
        let take_removal = removal.is_some_and(|(index, _)| picked.contains(index));
        let take_addition = addition.is_some_and(|(index, _)| picked.contains(index));
        match (removal, addition) {
            (Some((_, old)), Some((_, new))) => {
                // The index holds the new text when unstaging, so a replacement
                // pair is one line there: moving either half puts the old text
                // back, which is what clicking the line asks for.
                let move_pair = if reversed {
                    take_removal || take_addition
                } else {
                    take_removal && take_addition
                };
                if move_pair {
                    emit(body, '-', old, old_count, new_count, chosen);
                    emit(body, '+', new, old_count, new_count, chosen);
                } else if take_removal {
                    emit(body, '-', old, old_count, new_count, chosen);
                } else if take_addition {
                    emit(body, ' ', old, old_count, new_count, chosen);
                    emit(body, '+', new, old_count, new_count, chosen);
                } else {
                    let kept = if reversed { new } else { old };
                    emit(body, ' ', kept, old_count, new_count, chosen);
                }
            }
            (Some((_, old)), None) => {
                if take_removal {
                    emit(body, '-', old, old_count, new_count, chosen);
                } else if !reversed {
                    emit(body, ' ', old, old_count, new_count, chosen);
                }
            }
            (None, Some((_, new))) => {
                if take_addition {
                    emit(body, '+', new, old_count, new_count, chosen);
                } else if reversed {
                    emit(body, ' ', new, old_count, new_count, chosen);
                }
            }
            (None, None) => {}
        }
    }
    removals.clear();
    additions.clear();
}

/// Emit a hunk line with a given marker and keep the header counts in step.
fn emit(
    body: &mut String,
    marker: char,
    raw: &str,
    old_count: &mut usize,
    new_count: &mut usize,
    chosen: &mut usize,
) {
    body.push(marker);
    body.push_str(raw.get(1..).unwrap_or(""));
    match marker {
        '-' => *old_count += 1,
        '+' => *new_count += 1,
        _ => {
            *old_count += 1;
            *new_count += 1;
        }
    }
    if marker != ' ' {
        *chosen += 1;
    }
}

/// A patch that moves only the selected change lines of one hunk.
///
/// git's `apply` checks the `@@` counts, so an unselected addition or removal is
/// rewritten as a context line and both counts are recomputed from the lines
/// actually in the patch. `selected` holds indexes into the hunk's changed
/// lines in the order git printed them, which is exactly what the pane shows.
/// `reversed` builds the patch for unstaging, where the index holds the new side.
fn hunk_lines_patch(
    diff: &str,
    index: usize,
    selected: &[usize],
    reversed: bool,
) -> Result<String, String> {
    let (preamble, hunks) = split_hunks(diff);
    if preamble.trim().is_empty() || hunks.is_empty() {
        return Err(
            "This file has no hunks to move. Stage or unstage the whole file instead.".to_string(),
        );
    }
    let Some(hunk) = hunks.get(index) else {
        return Err(format!(
            "Hunk {} does not exist; this diff has {}.",
            index + 1,
            hunks.len()
        ));
    };
    let mut lines = hunk.split_inclusive('\n');
    let header = lines
        .next()
        .unwrap_or("")
        .trim_end_matches('\n')
        .to_string();
    let (old_start, new_start) = parse_hunk_start(&header)?;
    let change_count = hunk
        .lines()
        .skip(1)
        .filter(|line| line.starts_with('+') || line.starts_with('-'))
        .count();
    if selected.is_empty() {
        return Err("Select at least one changed line to move.".to_string());
    }
    if let Some(bad) = selected.iter().find(|line| **line >= change_count) {
        return Err(format!(
            "Line {bad} is not a changed line in this hunk; it has {change_count}."
        ));
    }
    let picked: std::collections::HashSet<usize> = selected.iter().copied().collect();
    let mut body = String::new();
    let mut old_count = 0usize;
    let mut new_count = 0usize;
    let mut change_index = 0usize;
    let mut chosen = 0usize;
    // A change block is git's removals followed by its additions; pairing them
    // positionally is what keeps the unselected middle of a replacement from
    // being emitted twice.
    let mut removals: Vec<(usize, String)> = Vec::new();
    let mut additions: Vec<(usize, String)> = Vec::new();
    for raw in lines {
        let marker = raw.chars().next().unwrap_or(' ');
        if marker == '\\' {
            flush_block(
                &mut body,
                &mut removals,
                &mut additions,
                &picked,
                reversed,
                &mut old_count,
                &mut new_count,
                &mut chosen,
            );
            // "\\ No newline at end of file" belongs to the line before it.
            body.push_str(raw);
            continue;
        }
        if marker == '+' {
            additions.push((change_index, raw.to_string()));
            change_index += 1;
            continue;
        }
        if marker == '-' {
            removals.push((change_index, raw.to_string()));
            change_index += 1;
            continue;
        }
        flush_block(
            &mut body,
            &mut removals,
            &mut additions,
            &picked,
            reversed,
            &mut old_count,
            &mut new_count,
            &mut chosen,
        );
        body.push_str(raw);
        old_count += 1;
        new_count += 1;
    }
    flush_block(
        &mut body,
        &mut removals,
        &mut additions,
        &picked,
        reversed,
        &mut old_count,
        &mut new_count,
        &mut chosen,
    );
    if chosen == 0 {
        return Err("Select at least one changed line to move.".to_string());
    }
    Ok(format!(
        "{preamble}@@ -{old_start},{old_count} +{new_start},{new_count} @@\n{body}"
    ))
}

/// Apply one hunk to the index. Staging takes the worktree-vs-index diff and
/// applies it forward; unstaging takes the index-vs-HEAD diff and reverses it.
/// The patch travels on stdin, never in argv.
fn apply_hunk(
    root: &Path,
    path: &str,
    index: usize,
    reverse: bool,
    selected: Option<&[usize]>,
) -> Result<Value, String> {
    let mut diff_args: Vec<&str> = vec!["diff", "--no-color", "--no-ext-diff", "-U3"];
    if reverse {
        diff_args.push("--cached");
    }
    diff_args.push("--");
    diff_args.push(path);
    let diff = git(root, &diff_args)?;
    if !diff.ok {
        return Err(git_error(&diff, "git diff failed"));
    }
    let patch = match selected {
        Some(lines) => hunk_lines_patch(&diff.stdout, index, lines, reverse)?,
        None => hunk_patch(&diff.stdout, index)?,
    };
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        // --unidiff-zero: a hunk that is all change lines has no context for
        // git to anchor on, and it refuses those without this flag.
        .args(["apply", "--cached", "--whitespace=nowarn", "--unidiff-zero"]);
    if reverse {
        command.arg("--reverse");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Git is not installed, or not on PATH.".to_string())?;
    {
        let mut stdin = child.stdin.take().ok_or("Git did not accept the patch.")?;
        stdin
            .write_all(patch.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    let result = GitOutput {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    };
    if !result.ok {
        return Err(git_error(&result, "git could not apply that hunk"));
    }
    Ok(json!({
        "ok": true,
        "path": path,
        "hunk": index,
        "reverse": reverse,
        "lines": selected.map(|lines| lines.len()).unwrap_or(0),
    }))
}

fn valid_commit_hash(raw: &str) -> Result<String, String> {
    let hash = raw.trim().to_ascii_lowercase();
    if hash.len() < 4 || hash.len() > 40 || !hash.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("That is not a commit hash.".to_string());
    }
    Ok(hash)
}

fn show_commit(root: &Path, raw: &str) -> Result<Value, String> {
    let hash = valid_commit_hash(raw)?;
    let meta = git(
        root,
        &[
            "show",
            "-s",
            "--no-color",
            "--format=%H%x09%h%x09%an%x09%aI%x09%s",
            &hash,
        ],
    )?;
    if !meta.ok {
        return Err(git_error(&meta, "git show failed"));
    }
    let line = meta.stdout.lines().next().unwrap_or("");
    let mut parts = line.splitn(5, '\t');
    let full = parts.next().unwrap_or("").trim().to_string();
    let short = parts.next().unwrap_or("").trim().to_string();
    let author = parts.next().unwrap_or("").trim().to_string();
    let date = parts.next().unwrap_or("").trim().to_string();
    let subject = parts.next().unwrap_or("").trim().to_string();
    let patch = git(
        root,
        &["show", "--no-color", "--no-ext-diff", "--format=", &hash],
    )?;
    if !patch.ok {
        return Err(git_error(&patch, "git show failed"));
    }
    let (diff, truncated) = truncate(&patch.stdout, MAX_DIFF_BYTES);
    Ok(json!({
        "hash": full,
        "short": short,
        "author": author,
        "date": date,
        "subject": subject,
        "diff": diff,
        "truncated": truncated,
    }))
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

async fn merge_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let branch = input.get("branch").and_then(Value::as_str).unwrap_or("");
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        &format!("merge {}", branch.trim()),
        "",
        approve,
        "merge a branch into the current one",
    ) {
        return response;
    }
    match merge_branch(&root, branch) {
        Ok(MergeOutcome::Merged {
            branch,
            current,
            fast_forward,
            summary,
        }) => Json(json!({
            "ok": true,
            "branch": branch,
            "current": current,
            "fastForward": fast_forward,
            "summary": summary,
        }))
        .into_response(),
        Ok(MergeOutcome::Conflict {
            branch,
            current,
            paths,
            output,
        }) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!(
                    "Merging `{branch}` stopped with {} conflicted file(s). Resolve them and commit, or abort the merge.",
                    paths.len()
                ),
                "branch": branch,
                "current": current,
                "conflicted": paths,
                "output": output,
                "inProgress": true,
            })),
        )
            .into_response(),
        Err((status, message)) => fail(status, message),
    }
}

async fn abort_merge_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        "merge --abort",
        "",
        approve,
        "abort the merge in progress",
    ) {
        return response;
    }
    match abort_merge(&root) {
        Ok(value) => Json(value).into_response(),
        Err((status, message)) => fail(status, message),
    }
}

async fn rebase_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let branch = input.get("branch").and_then(Value::as_str).unwrap_or("");
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        &format!("rebase {}", branch.trim()),
        "",
        approve,
        "rebase the current branch",
    ) {
        return response;
    }
    match rebase_branch(&root, branch) {
        Ok(RebaseOutcome::Rebased {
            branch,
            current,
            summary,
        }) => Json(json!({
            "ok": true,
            "branch": branch,
            "current": current,
            "summary": summary,
        }))
        .into_response(),
        Ok(RebaseOutcome::Conflict {
            branch,
            current,
            paths,
            output,
        }) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": format!(
                    "Rebasing onto `{branch}` stopped with {} conflicted file(s). Resolve them and continue, or abort the rebase.",
                    paths.len()
                ),
                "branch": branch,
                "current": current,
                "conflicted": paths,
                "output": output,
                "inProgress": true,
            })),
        )
            .into_response(),
        Err((status, message)) => fail(status, message),
    }
}

async fn abort_rebase_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        "rebase --abort",
        "",
        approve,
        "abort the rebase in progress",
    ) {
        return response;
    }
    match abort_rebase(&root) {
        Ok(value) => Json(value).into_response(),
        Err((status, message)) => fail(status, message),
    }
}

async fn continue_rebase_route(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        "rebase --continue",
        "",
        approve,
        "continue the rebase in progress",
    ) {
        return response;
    }
    match continue_rebase(&root) {
        Ok(value) => Json(value).into_response(),
        Err((status, message)) => fail(status, message),
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

async fn fetch_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let requested = match requested_remote(&input) {
        Ok(remote) => remote,
        Err(message) => return bad(message),
    };
    let remote = match remote_target(&root, requested.as_deref()) {
        Ok(remote) => remote,
        Err(message) => return bad(message),
    };
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        &format!("fetch {remote}"),
        "",
        approve,
        "fetch from a remote",
    ) {
        return response;
    }
    match fetch_remote(&root, &remote) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn pull_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        "pull",
        "",
        approve,
        "pull the current branch",
    ) {
        return response;
    }
    match pull_branch(&root) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn push_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        "push",
        "",
        approve,
        "push the current branch",
    ) {
        return response;
    }
    match push_branch(&root) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn hunk_route(state: AppState, input: Value, reverse: bool) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let raw = match input.get("path").and_then(Value::as_str) {
        Some(value) => value,
        None => return bad("Provide the file path."),
    };
    let path = match normalise_path(raw) {
        Ok(path) => path,
        Err(message) => return bad(message),
    };
    let index = match input.get("hunk").and_then(Value::as_u64) {
        Some(value) if value < 10_000 => value as usize,
        _ => return bad("Name the hunk by its index, starting at 0."),
    };
    // Optional selection inside the hunk: indexes of the changed lines in the
    // order the pane shows them. Absent means the whole hunk, which is what the
    // hunk button has always done.
    let selected = match input.get("lines") {
        None => None,
        Some(Value::Array(items)) => {
            if items.is_empty() {
                return bad("Select at least one changed line, or move the whole hunk.");
            }
            if items.len() > 500 {
                return bad("Select at most 500 lines at a time.");
            }
            let mut lines: Vec<usize> = Vec::with_capacity(items.len());
            for item in items {
                match item.as_u64() {
                    Some(value) if value < 10_000 => lines.push(value as usize),
                    _ => {
                        return bad("Line indexes must be numbers, counted from 0 inside the hunk.")
                    }
                }
            }
            Some(lines)
        }
        Some(_) => return bad("The lines field must be an array of changed-line indexes."),
    };
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        &path,
        "",
        approve,
        if reverse && selected.is_some() {
            "unstage selected lines in a hunk"
        } else if reverse {
            "unstage a hunk"
        } else if selected.is_some() {
            "stage selected lines in a hunk"
        } else {
            "stage a hunk"
        },
    ) {
        return response;
    }
    match apply_hunk(&root, &path, index, reverse, selected.as_deref()) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn stage_hunk_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    hunk_route(state, input, false).await
}

async fn remotes_route(State(state): State<AppState>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    match remote_list(&root) {
        Ok(remotes) => Json(json!({ "ok": true, "remotes": remotes })).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn add_remote_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let name = match input.get("name").and_then(Value::as_str) {
        Some(value) => match valid_remote_name(value) {
            Ok(name) => name,
            Err(message) => return bad(message),
        },
        None => return bad("Name the remote first."),
    };
    let url = match remote_url(&input) {
        Ok(url) => url,
        Err(message) => return bad(message),
    };
    let approve = approval(&input);
    if let Some(response) = guard_user(&state, Action::Git, &name, "", approve, "add a git remote")
    {
        return response;
    }
    match add_remote(&root, &name, &url) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn set_remote_url_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let name = match input.get("name").and_then(Value::as_str) {
        Some(value) => match valid_remote_name(value) {
            Ok(name) => name,
            Err(message) => return bad(message),
        },
        None => return bad("Name the remote first."),
    };
    let url = match remote_url(&input) {
        Ok(url) => url,
        Err(message) => return bad(message),
    };
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        &name,
        "",
        approve,
        "change a git remote URL",
    ) {
        return response;
    }
    match set_remote_url(&root, &name, &url) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn remove_remote_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let name = match input.get("name").and_then(Value::as_str) {
        Some(value) => match valid_remote_name(value) {
            Ok(name) => name,
            Err(message) => return bad(message),
        },
        None => return bad("Name the remote first."),
    };
    let approve = approval(&input);
    if let Some(response) = guard_user(
        &state,
        Action::Git,
        &name,
        "",
        approve,
        "remove a git remote",
    ) {
        return response;
    }
    match remove_remote(&root, &name) {
        Ok(value) => Json(value).into_response(),
        Err(message) => git_failure(message),
    }
}

async fn unstage_hunk_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    hunk_route(state, input, true).await
}

async fn show_route(
    State(state): State<AppState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let hash = query.get("hash").map(String::as_str).unwrap_or("");
    match show_commit(&root, hash) {
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
        .route("/api/workspace/git/merge", post(merge_route))
        .route("/api/workspace/git/merge/abort", post(abort_merge_route))
        .route("/api/workspace/git/rebase", post(rebase_route))
        .route("/api/workspace/git/rebase/abort", post(abort_rebase_route))
        .route(
            "/api/workspace/git/rebase/continue",
            post(continue_rebase_route),
        )
        .route("/api/workspace/git/checkout", post(checkout_route))
        .route("/api/workspace/git/remotes", get(remotes_route))
        .route("/api/workspace/git/remotes/add", post(add_remote_route))
        .route(
            "/api/workspace/git/remotes/remove",
            post(remove_remote_route),
        )
        .route(
            "/api/workspace/git/remotes/set-url",
            post(set_remote_url_route),
        )
        .route("/api/workspace/git/fetch", post(fetch_route))
        .route("/api/workspace/git/pull", post(pull_route))
        .route("/api/workspace/git/push", post(push_route))
        .route("/api/workspace/git/stage-hunk", post(stage_hunk_route))
        .route("/api/workspace/git/unstage-hunk", post(unstage_hunk_route))
        .route("/api/workspace/git/show", get(show_route))
        .route("/api/workspace/git/log", get(log_route))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A repository whose `main` holds `a.txt = "base"`, ready for a branch to
    /// be built on top of it.
    fn branch_fixture(name: &str) -> Option<PathBuf> {
        let root = temp_repo(name)?;
        run_git(&root, &["checkout", "-q", "-B", "main"]);
        std::fs::write(root.join("a.txt"), "base\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "base"]);
        Some(root)
    }

    #[test]
    fn merges_a_branch_that_is_ahead_and_reports_fast_forward() {
        let Some(root) = branch_fixture("merge-ff") else {
            return;
        };
        run_git(&root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("b.txt"), "two\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "two"]);
        run_git(&root, &["checkout", "-q", "main"]);
        // An untracked file is not a reason to refuse: git itself only objects
        // when the merge would overwrite it.
        std::fs::write(root.join("notes.txt"), "stray\n").unwrap();
        match merge_branch(&root, "feature").expect("merge runs") {
            MergeOutcome::Merged {
                branch,
                current,
                fast_forward,
                ..
            } => {
                assert_eq!(branch, "feature");
                assert_eq!(current, "main");
                assert!(fast_forward, "feature was ahead of main");
            }
            MergeOutcome::Conflict { paths, .. } => panic!("unexpected conflict: {paths:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(root.join("b.txt")).unwrap(),
            "two\n"
        );
        assert!(root.join("a.txt").exists());
        assert!(root.join("notes.txt").exists());
    }

    #[test]
    fn merges_two_diverged_branches_without_conflicts() {
        let Some(root) = branch_fixture("merge-three-way") else {
            return;
        };
        run_git(&root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("b.txt"), "feature\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "feature"]);
        run_git(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("c.txt"), "main\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "main"]);
        match merge_branch(&root, "feature").expect("merge runs") {
            MergeOutcome::Merged { fast_forward, .. } => {
                assert!(
                    !fast_forward,
                    "main had its own commit, so this is a real merge"
                );
            }
            MergeOutcome::Conflict { paths, .. } => panic!("unexpected conflict: {paths:?}"),
        }
        assert!(root.join("b.txt").exists());
        assert!(root.join("c.txt").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "base\n"
        );
    }

    #[test]
    fn reports_conflicted_paths_and_aborts_back_to_a_clean_tree() {
        let Some(root) = branch_fixture("merge-conflict") else {
            return;
        };
        run_git(&root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("a.txt"), "feature\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "feature"]);
        run_git(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "main\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "main"]);
        match merge_branch(&root, "feature").expect("a conflict is an outcome, not an error") {
            MergeOutcome::Conflict { paths, .. } => assert_eq!(paths, vec!["a.txt".to_string()]),
            MergeOutcome::Merged { .. } => {
                panic!("both branches changed a.txt; this must conflict")
            }
        }
        assert!(std::fs::read_to_string(root.join("a.txt"))
            .unwrap()
            .contains("<<<<<<<"));
        abort_merge(&root).expect("abort works");
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "main\n"
        );
        let status = run_git(&root, &["status", "--porcelain"]);
        assert!(
            status.trim().is_empty(),
            "the tree should be clean after abort: {status}"
        );
        assert!(
            abort_merge(&root).is_err(),
            "with nothing in progress, abort must refuse"
        );
    }

    #[test]
    fn refuses_to_merge_over_uncommitted_work() {
        let Some(root) = branch_fixture("merge-dirty") else {
            return;
        };
        run_git(&root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("b.txt"), "two\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "two"]);
        run_git(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "edited\n").unwrap();
        let (status, message) = match merge_branch(&root, "feature") {
            Ok(_) => panic!("a dirty tree must be refused"),
            Err(error) => error,
        };
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(message.contains("Commit or stash"), "{message}");
        assert!(!root.join("b.txt").exists(), "nothing may be merged yet");
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "edited\n",
            "the uncommitted edit must be left alone"
        );
    }

    #[test]
    fn names_a_missing_branch_and_refuses_an_option_like_name() {
        let Some(root) = branch_fixture("merge-missing") else {
            return;
        };
        let (status, message) = match merge_branch(&root, "no-such-branch") {
            Ok(_) => panic!("a missing branch must not merge"),
            Err(error) => error,
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("no branch or commit named"), "{message}");
        assert!(matches!(
            merge_branch(&root, "-x"),
            Err((StatusCode::BAD_REQUEST, _))
        ));
    }

    #[test]
    fn a_snapshot_records_edits_and_untracked_files_without_touching_the_index() {
        let Some(root) = branch_fixture("snapshot-index") else {
            return;
        };
        std::fs::write(root.join("a.txt"), "edited\n").unwrap();
        std::fs::write(root.join("notes.txt"), "new\n").unwrap();
        std::fs::write(root.join("staged.txt"), "staged\n").unwrap();
        run_git(&root, &["add", "staged.txt"]);

        let snapshot = snapshot_tree(&root).expect("snapshot runs");
        assert_eq!(snapshot.paths, 3, "a.txt, staged.txt and notes.txt");
        assert!(!snapshot.commit.is_empty());

        // The real index is untouched: the staged file is still staged, and the
        // snapshot's commit object holds files git never tracked.
        let status = run_git(&root, &["status", "--porcelain"]);
        assert!(
            status.contains("A  staged.txt"),
            "the staged file must stay staged: {status}"
        );
        assert!(
            status.contains("M a.txt") || status.contains(" M a.txt"),
            "{status}"
        );
        let listing = run_git(&root, &["ls-tree", "-r", "--name-only", &snapshot.commit]);
        assert!(listing.contains("notes.txt"), "{listing}");
        assert!(listing.contains("staged.txt"), "{listing}");
    }

    #[test]
    fn a_snapshot_restore_puts_tracked_files_back_and_removes_new_ones() {
        let Some(root) = branch_fixture("snapshot-restore") else {
            return;
        };
        std::fs::write(root.join("gone.txt"), "keep me\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "more"]);
        let snapshot = snapshot_tree(&root).expect("snapshot runs");

        // What a command step could do next: change one file, delete another,
        // and add a third.
        std::fs::write(root.join("a.txt"), "changed after\n").unwrap();
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        std::fs::write(root.join("added.txt"), "after\n").unwrap();
        run_git(&root, &["add", "added.txt"]);

        let changes = snapshot_changes(&root, &snapshot.commit).expect("diff runs");
        assert!(changes.iter().any(|c| c.path == "a.txt" && !c.delete));
        assert!(changes.iter().any(|c| c.path == "gone.txt" && !c.delete));
        assert!(changes.iter().any(|c| c.path == "added.txt" && c.delete));
        for change in &changes {
            restore_path(&root, &snapshot.commit, &change.path, change.delete)
                .expect("restore works");
        }

        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "base\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("gone.txt")).unwrap(),
            "keep me\n"
        );
        assert!(
            !root.join("added.txt").exists(),
            "a file the snapshot never had is removed"
        );
        assert!(
            snapshot_changes(&root, &snapshot.commit)
                .unwrap()
                .is_empty(),
            "nothing should differ from the checkpoint afterwards"
        );
    }

    #[test]
    fn a_snapshot_refuses_a_folder_that_is_not_a_repository() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let root =
            std::env::temp_dir().join(format!("saarthi-not-a-repo-{}-{nanos}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(snapshot_tree(&root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rebases_the_current_branch_onto_an_advanced_one() {
        let Some(root) = branch_fixture("rebase-clean") else {
            return;
        };
        run_git(&root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("b.txt"), "feature\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "feature work"]);
        run_git(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("c.txt"), "main\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "main work"]);
        run_git(&root, &["checkout", "-q", "feature"]);
        let before = run_git(&root, &["rev-parse", "HEAD"]);

        match rebase_branch(&root, "main").expect("rebase runs") {
            RebaseOutcome::Rebased {
                branch, current, ..
            } => {
                assert_eq!(branch, "main");
                assert_eq!(current, "feature");
            }
            RebaseOutcome::Conflict { paths, .. } => panic!("unexpected conflict: {paths:?}"),
        }
        assert_ne!(
            run_git(&root, &["rev-parse", "HEAD"]),
            before,
            "the commit was replayed onto a new parent"
        );
        let subjects = run_git(&root, &["log", "--format=%s", "-3"]);
        assert_eq!(
            subjects.lines().collect::<Vec<_>>(),
            vec!["feature work", "main work", "base"]
        );
        assert!(
            root.join("c.txt").exists(),
            "the new parent's file is present"
        );
        assert!(root.join("b.txt").exists());
    }

    #[test]
    fn reports_rebase_conflicts_and_aborts_back_to_the_original_branch() {
        let Some(root) = branch_fixture("rebase-conflict") else {
            return;
        };
        run_git(&root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("a.txt"), "feature\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "feature"]);
        let feature_head = run_git(&root, &["rev-parse", "HEAD"]);
        run_git(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "main\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "main"]);
        run_git(&root, &["checkout", "-q", "feature"]);

        match rebase_branch(&root, "main").expect("a conflict is an outcome") {
            RebaseOutcome::Conflict { paths, .. } => assert_eq!(paths, vec!["a.txt".to_string()]),
            RebaseOutcome::Rebased { .. } => panic!("both branches changed a.txt"),
        }
        abort_rebase(&root).expect("abort works");
        assert_eq!(run_git(&root, &["rev-parse", "HEAD"]), feature_head);
        assert_eq!(
            run_git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
            "feature"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "feature\n"
        );
        assert!(run_git(&root, &["status", "--porcelain"]).trim().is_empty());
        assert!(abort_rebase(&root).is_err(), "nothing is left to abort");
    }

    #[test]
    fn refuses_to_rebase_over_uncommitted_work() {
        let Some(root) = branch_fixture("rebase-dirty") else {
            return;
        };
        run_git(&root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("b.txt"), "feature\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "feature"]);
        run_git(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "edited\n").unwrap();
        let (status, message) = match rebase_branch(&root, "feature") {
            Ok(_) => panic!("a dirty tree must be refused"),
            Err(error) => error,
        };
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(message.contains("Commit or stash"), "{message}");
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "edited\n",
            "the uncommitted edit must be left alone"
        );
    }

    #[test]
    fn refuses_an_unknown_branch_an_option_like_name_and_a_branch_onto_itself() {
        let Some(root) = branch_fixture("rebase-guards") else {
            return;
        };
        let (status, message) = match rebase_branch(&root, "nope") {
            Ok(_) => panic!("a missing branch must not rebase"),
            Err(error) => error,
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("no branch or commit named"), "{message}");
        assert!(matches!(
            rebase_branch(&root, "-x"),
            Err((StatusCode::BAD_REQUEST, _))
        ));
        let (status, message) = match rebase_branch(&root, "main") {
            Ok(_) => panic!("a branch cannot be rebased onto itself"),
            Err(error) => error,
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("already checked out"), "{message}");
    }

    #[test]
    fn continues_a_rebase_after_the_conflict_is_resolved_and_staged() {
        let Some(root) = branch_fixture("rebase-continue") else {
            return;
        };
        run_git(&root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("a.txt"), "feature\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "feature"]);
        run_git(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "main\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "main"]);
        run_git(&root, &["checkout", "-q", "feature"]);
        assert!(matches!(
            rebase_branch(&root, "main"),
            Ok(RebaseOutcome::Conflict { .. })
        ));

        // Continuing while the marker file is still there must refuse and name it.
        let (status, message) = match continue_rebase(&root) {
            Ok(_) => panic!("unresolved conflicts must not be continued"),
            Err(error) => error,
        };
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(message.contains("still conflicted"), "{message}");
        assert!(message.contains("a.txt"), "{message}");

        std::fs::write(root.join("a.txt"), "resolved\n").unwrap();
        run_git(&root, &["add", "a.txt"]);
        let value = continue_rebase(&root).expect("the rebase continues");
        assert_eq!(value["continued"], true, "{value}");
        assert_eq!(value["branch"], "feature");
        assert!(!rebase_in_progress(&root), "the rebase is finished");
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "resolved\n"
        );
        let subjects = run_git(&root, &["log", "--format=%s", "-2"]);
        assert_eq!(
            subjects.lines().collect::<Vec<_>>(),
            vec!["feature", "main"]
        );
        assert!(run_git(&root, &["status", "--porcelain"]).trim().is_empty());
        // With the rebase finished, continue has nothing to do.
        assert!(continue_rebase(&root).is_err());
    }

    #[test]
    fn refuses_to_continue_with_no_rebase_in_progress() {
        let Some(root) = branch_fixture("rebase-continue-idle") else {
            return;
        };
        let (status, message) = match continue_rebase(&root) {
            Ok(_) => panic!("there is nothing to continue"),
            Err(error) => error,
        };
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(message.contains("no rebase in progress"), "{message}");
    }

    /// A file whose three lines all change, so one `-U3` diff holds a single
    /// hunk with three removals followed by three additions.
    fn changed_lines_fixture(name: &str) -> Option<PathBuf> {
        let root = branch_fixture(name)?;
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["commit", "-q", "-m", "lines"]);
        std::fs::write(root.join("a.txt"), "ONE\ntwo\nTHREE\n").unwrap();
        Some(root)
    }

    #[test]
    fn stages_only_the_selected_change_lines_of_a_hunk() {
        let Some(root) = changed_lines_fixture("lines-stage") else {
            return;
        };
        // Change lines: 0 = -one, 1 = +ONE, 2 = -three, 3 = +THREE.
        apply_hunk(&root, "a.txt", 0, false, Some(&[0, 1])).expect("selected lines stage");
        assert_eq!(
            run_git(&root, &["show", ":a.txt"]),
            "ONE\ntwo\nthree\n",
            "the index holds only the first change"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "ONE\ntwo\nTHREE\n",
            "the worktree is untouched"
        );
        let cached = run_git(&root, &["diff", "--cached", "--", "a.txt"]);
        assert!(cached.contains("-one"), "{cached}");
        assert!(cached.contains("+ONE"), "{cached}");
        assert!(
            !cached.contains("THREE"),
            "the third line must still be unstaged: {cached}"
        );
        let remaining = run_git(&root, &["diff", "--", "a.txt"]);
        assert!(remaining.contains("THREE"), "{remaining}");
    }

    #[test]
    fn unstages_only_the_selected_change_lines_of_a_hunk() {
        let Some(root) = changed_lines_fixture("lines-unstage") else {
            return;
        };
        run_git(&root, &["add", "a.txt"]);
        apply_hunk(&root, "a.txt", 0, true, Some(&[0, 1])).expect("selected lines unstage");
        assert_eq!(
            run_git(&root, &["show", ":a.txt"]),
            "one\ntwo\nTHREE\n",
            "only the first change left the index"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "ONE\ntwo\nTHREE\n"
        );
    }

    #[test]
    fn refuses_a_line_selection_that_picks_nothing_or_does_not_exist() {
        let Some(root) = changed_lines_fixture("lines-guard") else {
            return;
        };
        assert!(apply_hunk(&root, "a.txt", 0, false, Some(&[])).is_err());
        assert!(apply_hunk(&root, "a.txt", 0, false, Some(&[99])).is_err());
        assert_eq!(
            run_git(&root, &["show", ":a.txt"]),
            "one\ntwo\nthree\n",
            "a refused selection must not stage anything"
        );
    }

    #[test]
    fn adds_lists_and_removes_a_remote() {
        let Some(root) = temp_repo("remote-crud") else {
            return;
        };
        assert!(remote_list(&root).unwrap().is_empty());

        let added = add_remote(&root, "origin", "https://example.com/repo.git").unwrap();
        assert_eq!(added["name"], "origin");
        let remotes = remote_list(&root).unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0]["name"], "origin");
        assert_eq!(remotes[0]["url"], "https://example.com/repo.git");
        assert_eq!(remotes[0]["pushUrl"], "https://example.com/repo.git");

        // git refuses a second remote of the same name; nothing is added.
        assert!(add_remote(&root, "origin", "https://example.com/other.git").is_err());
        assert_eq!(remote_list(&root).unwrap().len(), 1);

        let removed = remove_remote(&root, "origin").unwrap();
        assert_eq!(removed["removed"], "origin");
        assert!(removed["remotes"].as_array().unwrap().is_empty());
        assert!(remote_list(&root).unwrap().is_empty());
        // Removing what is not there is refused rather than silently succeeding.
        assert!(remove_remote(&root, "origin").is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reports_a_push_url_that_differs_from_the_fetch_url() {
        let Some(root) = temp_repo("remote-push-url") else {
            return;
        };
        add_remote(&root, "origin", "https://example.com/fetch.git").unwrap();
        run_git(
            &root,
            &[
                "remote",
                "set-url",
                "--push",
                "origin",
                "https://example.com/push.git",
            ],
        );
        let remotes = remote_list(&root).unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0]["url"], "https://example.com/fetch.git");
        assert_eq!(remotes[0]["pushUrl"], "https://example.com/push.git");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_remote_names_and_urls_that_would_reach_argv_oddly() {
        assert_eq!(valid_remote_name(" upstream ").unwrap(), "upstream");
        assert!(valid_remote_name("-x").is_err());
        assert!(valid_remote_name("a b").is_err());
        assert!(valid_remote_name("  ").is_err());
        assert!(valid_remote_name("a;b").is_err());
        assert!(valid_remote_name(&"a".repeat(101)).is_err());

        assert_eq!(
            remote_url(&json!({ "url": " git@host:repo.git " })).unwrap(),
            "git@host:repo.git"
        );
        assert!(remote_url(&json!({})).is_err());
        assert!(remote_url(&json!({ "url": "" })).is_err());
        assert!(remote_url(&json!({ "url": "-x" })).is_err());
        assert!(remote_url(&json!({ "url": "https://exa mple.com" })).is_err());
        assert!(remote_url(&json!({ "url": "https://example.com/\u{7}" })).is_err());
        assert!(remote_url(
            &json!({ "url": format!("https://example.com/{}", "a".repeat(MAX_REMOTE_URL)) })
        )
        .is_err());
    }

    #[test]
    fn changes_an_existing_remote_url_in_place() {
        let Some(root) = temp_repo("remote-set-url") else {
            return;
        };
        add_remote(&root, "origin", "https://example.com/old.git").unwrap();
        let changed = set_remote_url(&root, "origin", "https://example.com/new.git").unwrap();
        assert_eq!(changed["url"], "https://example.com/new.git");
        let remotes = remote_list(&root).unwrap();
        assert_eq!(remotes.len(), 1, "set-url must not add a second remote");
        assert_eq!(remotes[0]["url"], "https://example.com/new.git");
        assert_eq!(remotes[0]["pushUrl"], "https://example.com/new.git");
        // A remote that is not configured is refused by name.
        assert!(set_remote_url(&root, "upstream", "https://example.com/new.git").is_err());
        assert_eq!(remote_list(&root).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

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
    fn refuses_to_commit_staged_conflict_markers() {
        let Some(root) = temp_repo("markers") else {
            return;
        };
        std::fs::write(root.join("a.txt"), "one
").unwrap();
        run_git(&root, &["add", "a.txt"]);
        run_git(&root, &["commit", "-q", "-m", "base"]);
        std::fs::write(
            root.join("a.txt"),
            "<<<<<<< HEAD
one
=======
two
>>>>>>> other
",
        )
        .unwrap();
        run_git(&root, &["add", "a.txt"]);
        let error = commit(&root, "bad merge", &[]).unwrap_err();
        assert!(error.contains("conflict markers"), "{error}");
        assert!(error.contains("a.txt"), "{error}");
        // The refusal really refused: history still holds only the base commit.
        assert_eq!(run_git(&root, &["rev-list", "--count", "HEAD"]).trim(), "1");
        // Resolving the markers lets the same commit through.
        std::fs::write(root.join("a.txt"), "one
two
").unwrap();
        run_git(&root, &["add", "a.txt"]);
        assert!(commit(&root, "resolved", &[]).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn run_git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    fn temp_bare(name: &str) -> Option<PathBuf> {
        if !git_available() {
            return None;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "saarthi-bare-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let output = Command::new("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .arg(&root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Some(root)
    }

    fn clone_repo(remote: &Path, name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dest = std::env::temp_dir().join(format!(
            "saarthi-clone-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dest);
        let output = Command::new("git")
            .arg("clone")
            .arg("-q")
            .arg(remote)
            .arg(&dest)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        for (key, value) in [
            ("user.email", "test@example.com"),
            ("user.name", "Test"),
            ("commit.gpgsign", "false"),
        ] {
            run_git(&dest, &["config", key, value]);
        }
        dest
    }

    fn commit_file(root: &Path, name: &str, body: &str, message: &str) {
        std::fs::write(root.join(name), body).unwrap();
        let paths = clean_paths(&json!({ "path": name })).unwrap();
        stage_paths(root, &paths).unwrap();
        commit(root, message, &[]).unwrap();
    }

    #[test]
    fn pushes_to_a_real_remote_and_fetches_another_clone_s_work() {
        let (Some(source), Some(remote)) = (temp_repo("push-source"), temp_bare("push")) else {
            return;
        };
        commit_file(&source, "a.txt", "one\n", "first");
        run_git(
            &source,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );

        let pushed = push_branch(&source).unwrap();
        assert_eq!(pushed["branch"], "main");
        assert_eq!(pushed["upstreamSet"], true);
        assert_eq!(pushed["upstream"], "origin/main");
        // The second push uses the upstream it just set instead of setting it again.
        assert_eq!(push_branch(&source).unwrap()["upstreamSet"], false);

        let clone = clone_repo(&remote, "push");
        commit_file(&clone, "b.txt", "two\n", "second");
        push_branch(&clone).unwrap();

        let fetched = fetch_remote(&source, "origin").unwrap();
        assert_eq!(fetched["remote"], "origin");
        assert_eq!(
            run_git(&source, &["log", "-1", "--format=%s", "origin/main"]).trim(),
            "second"
        );

        let pulled = pull_branch(&source).unwrap();
        assert_eq!(pulled["upstream"], "origin/main");
        assert_eq!(
            std::fs::read_to_string(source.join("b.txt")).unwrap(),
            "two\n"
        );

        let _ = std::fs::remove_dir_all(&source);
        let _ = std::fs::remove_dir_all(&remote);
        let _ = std::fs::remove_dir_all(&clone);
    }

    #[test]
    fn refuses_a_pull_that_is_not_a_fast_forward() {
        let (Some(source), Some(remote)) = (temp_repo("div-source"), temp_bare("div")) else {
            return;
        };
        commit_file(&source, "a.txt", "one\n", "first");
        run_git(
            &source,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        push_branch(&source).unwrap();
        let clone = clone_repo(&remote, "div");

        commit_file(&clone, "b.txt", "clone\n", "clone work");
        push_branch(&clone).unwrap();

        commit_file(&source, "c.txt", "source\n", "source work");
        let error = pull_branch(&source).unwrap_err();
        assert!(
            error.to_ascii_lowercase().contains("fast-forward"),
            "{error}"
        );
        assert_eq!(git_failure(error).status(), StatusCode::CONFLICT);
        // The refusal must leave both histories alone: no merge, no conflict markers.
        assert!(!source.join("b.txt").exists());
        assert_eq!(
            std::fs::read_to_string(source.join("c.txt")).unwrap(),
            "source\n"
        );

        let _ = std::fs::remove_dir_all(&source);
        let _ = std::fs::remove_dir_all(&remote);
        let _ = std::fs::remove_dir_all(&clone);
    }

    #[test]
    fn validates_remote_names_and_requires_a_configured_remote() {
        assert_eq!(requested_remote(&json!({})).unwrap(), None);
        assert_eq!(
            requested_remote(&json!({"remote": " upstream "}))
                .unwrap()
                .as_deref(),
            Some("upstream")
        );
        assert!(requested_remote(&json!({"remote": "-x"})).is_err());
        assert!(requested_remote(&json!({"remote": "a b"})).is_err());
        assert!(requested_remote(&json!({"remote": "  "})).is_err());
        assert!(requested_remote(&json!({"remote": 7})).is_err());

        let Some(root) = temp_repo("no-remote") else {
            return;
        };
        assert!(remote_target(&root, None)
            .unwrap_err()
            .contains("no remote"));
        assert!(remote_target(&root, Some("origin"))
            .unwrap_err()
            .contains("no remote"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn maps_remote_failures_to_the_right_status() {
        let auth = git_failure(
            "fatal: could not read Username for 'https://example.com': terminal prompts disabled"
                .to_string(),
        );
        assert_eq!(auth.status(), StatusCode::BAD_GATEWAY);
        let rejected = git_failure("! [rejected] main -> main (non-fast-forward)".to_string());
        assert_eq!(rejected.status(), StatusCode::CONFLICT);
        let unreachable =
            git_failure("fatal: unable to access 'https://example.com/x.git'".to_string());
        assert_eq!(unreachable.status(), StatusCode::BAD_GATEWAY);
    }

    #[test]
    fn stages_and_unstages_a_single_hunk() {
        let Some(root) = temp_repo("hunks") else {
            return;
        };
        let original: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        std::fs::write(root.join("a.txt"), &original).unwrap();
        let paths = clean_paths(&json!({"path": "a.txt"})).unwrap();
        stage_paths(&root, &paths).unwrap();
        commit(&root, "base", &[]).unwrap();

        let mut changed: Vec<String> = original.lines().map(str::to_string).collect();
        changed[1] = "line 2 changed".to_string();
        changed[17] = "line 18 changed".to_string();
        std::fs::write(root.join("a.txt"), changed.join("\n") + "\n").unwrap();

        let diff = git(&root, &["diff", "--unified=3", "--", "a.txt"]).unwrap();
        let (_, hunks) = split_hunks(&diff.stdout);
        assert_eq!(hunks.len(), 2, "{}", diff.stdout);

        apply_hunk(&root, "a.txt", 0, false, None).unwrap();
        let staged = git(&root, &["diff", "--cached", "--", "a.txt"]).unwrap();
        assert!(staged.stdout.contains("line 2 changed"));
        assert!(!staged.stdout.contains("line 18 changed"));
        let work = git(&root, &["diff", "--", "a.txt"]).unwrap();
        assert!(!work.stdout.contains("line 2 changed"));
        assert!(work.stdout.contains("line 18 changed"));

        apply_hunk(&root, "a.txt", 0, true, None).unwrap();
        let after = git(&root, &["diff", "--cached", "--", "a.txt"]).unwrap();
        assert!(after.stdout.trim().is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_a_hunk_index_the_diff_does_not_have() {
        let diff = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n";
        assert!(hunk_patch(diff, 0).is_ok());
        assert!(hunk_patch(diff, 1).is_err());
        assert!(hunk_patch("", 0).is_err());
    }

    #[test]
    fn shows_a_commit_with_its_metadata_and_patch() {
        let Some(root) = temp_repo("show") else {
            return;
        };
        commit_file(&root, "a.txt", "one\n", "first commit");
        let hash = run_git(&root, &["rev-parse", "HEAD"]).trim().to_string();
        let shown = show_commit(&root, &hash).unwrap();
        assert_eq!(shown["subject"], "first commit");
        assert!(shown["short"].as_str().unwrap().len() >= 7);
        assert!(shown["diff"].as_str().unwrap().contains("+one"));
        assert_eq!(shown["truncated"], false);

        assert!(show_commit(&root, "not-a-hash").is_err());
        assert!(show_commit(&root, "main; rm -rf /").is_err());
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
