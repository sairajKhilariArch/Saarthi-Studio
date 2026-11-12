//! Build and test runs inside the open folder.
//!
//! The agent can already run commands, but a person had no way to ask "does
//! this compile?" without leaving the app. This module adds the smallest honest
//! version of that: it detects the project's own build and test commands
//! (`cargo build`/`cargo test`, or the `build`/`test` scripts of a
//! `package.json` through npm, pnpm or yarn), runs one of them with the
//! workspace as its working directory, and turns the real compiler output into
//! clickable problems.
//!
//! Three deliberate limits:
//!
//! * **Only presets run, never request text.** The request names a preset id —
//!   one detected from the project, or one the user saved in Settings — and the
//!   id is resolved against the same list the UI was shown. A saved command is a
//!   command line (the user wrote one), so it runs through the platform shell,
//!   but it comes from the settings file and never from the request. The
//!   `command` permission action still gates it, with the exact line as target.
//! * **One run at a time.** A second request while a build is in flight is a
//!   409 rather than two compilers fighting over the same `target/` directory.
//! * **Output is real and bounded.** Both streams are captured, capped at
//!   1 MiB each, and the response says when it was truncated. Nothing here
//!   invents a result: a failed build is a 200 with `success: false`, its exit
//!   code and its diagnostics.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::permissions::{guard_user, Action};
use crate::AppState;

/// A build or test may take this long before it is killed.
const RUN_TIMEOUT: Duration = Duration::from_secs(300);
/// Bytes kept per stream while draining it.
const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
/// Characters of combined output returned to the UI. A compiler error is
/// usually near the end, so the tail is what is shown.
const MAX_DISPLAY_CHARS: usize = 200_000;
/// Problems carried in one response.
const MAX_PROBLEMS: usize = 200;
/// Failing test names carried in one response.
const MAX_FAILURES: usize = 20;

/// One command the Run panel can start: detected from the project, or saved by
/// the user in Settings.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Preset {
    /// Stable id the UI posts back: `cargo-build`, `npm-test`, `custom-lint`…
    pub id: String,
    pub label: String,
    /// `build`, `test` or `custom`.
    pub kind: String,
    /// The command line as displayed, audited and used as the permission target.
    pub command: String,
    program: String,
    args: Vec<String>,
    /// npm/pnpm/yarn are `.cmd` shims on Windows, which `CreateProcess` cannot
    /// start directly; those need `cmd /C`, and only on Windows.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    needs_shell: bool,
}

impl Preset {
    fn new(
        id: &str,
        label: &str,
        kind: &str,
        command: &str,
        program: &str,
        args: &[&str],
        needs_shell: bool,
    ) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            kind: kind.to_string(),
            command: command.to_string(),
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            needs_shell,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "label": self.label,
            "kind": self.kind,
            "command": self.command,
        })
    }
}

/// The package manager a repository is already using, from its lockfile.
fn package_manager(root: &Path) -> &'static str {
    if root.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if root.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm"
    }
}

/// Which of `scripts.build` / `scripts.test` exist, without guessing.
fn package_scripts(root: &Path) -> Option<(bool, bool)> {
    let text = std::fs::read_to_string(root.join("package.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let scripts = value.get("scripts")?.as_object()?;
    Some((scripts.contains_key("build"), scripts.contains_key("test")))
}

/// The commands this folder actually supports. Nothing aspirational: a preset
/// exists only when its manifest and script exist.
pub(crate) fn detect(root: &Path) -> Vec<Preset> {
    let mut presets = Vec::new();
    if root.join("Cargo.toml").is_file() {
        presets.push(Preset::new(
            "cargo-build",
            "cargo build",
            "build",
            "cargo build",
            "cargo",
            &["build"],
            false,
        ));
        presets.push(Preset::new(
            "cargo-test",
            "cargo test",
            "test",
            "cargo test",
            "cargo",
            &["test"],
            false,
        ));
    }
    if let Some((has_build, has_test)) = package_scripts(root) {
        let manager = package_manager(root);
        if has_build {
            let command = if manager == "npm" {
                "npm run build".to_string()
            } else {
                format!("{manager} run build")
            };
            presets.push(Preset::new(
                &format!("{manager}-build"),
                &command,
                "build",
                &command,
                manager,
                &["run", "build"],
                true,
            ));
        }
        if has_test {
            let (command, args) = if manager == "npm" {
                ("npm test".to_string(), vec!["test"])
            } else {
                (format!("{manager} test"), vec!["test"])
            };
            presets.push(Preset::new(
                &format!("{manager}-test"),
                &command,
                "test",
                &command,
                manager,
                &args,
                true,
            ));
        }
    }
    presets
}

/// Limits for a saved command, so both the config file and the Run panel stay
/// legible.
const MAX_CUSTOM_PRESETS: usize = 20;
const MAX_PRESET_LABEL_CHARS: usize = 60;
const MAX_PRESET_COMMAND_CHARS: usize = 500;

/// A stable id for a saved command, derived from its name: `custom-lint-fix`.
/// Two names that slug the same are refused at save time, so one id never means
/// two different commands.
pub(crate) fn custom_id(label: &str) -> String {
    let mut slug = String::new();
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.extend(ch.to_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_end_matches('-').chars().take(40).collect();
    if slug.is_empty() {
        "custom-command".to_string()
    } else {
        format!("custom-{slug}")
    }
}

/// Validate what the settings form posts: one entry per command, trimmed, free of
/// control characters, capped, and with no two names that would share an id.
pub(crate) fn validate_custom(values: &[Value]) -> Result<Vec<(String, String)>, String> {
    if values.len() > MAX_CUSTOM_PRESETS {
        return Err(format!("Save at most {MAX_CUSTOM_PRESETS} commands."));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    for value in values {
        let label = value
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let command = value
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if label.is_empty() || command.is_empty() {
            return Err("Every saved command needs a name and a command line.".into());
        }
        if label.chars().count() > MAX_PRESET_LABEL_CHARS {
            return Err(format!(
                "A command name is longer than {MAX_PRESET_LABEL_CHARS} characters."
            ));
        }
        if command.chars().count() > MAX_PRESET_COMMAND_CHARS {
            return Err(format!(
                "A command line is longer than {MAX_PRESET_COMMAND_CHARS} characters."
            ));
        }
        if label.chars().chain(command.chars()).any(char::is_control) {
            return Err("A saved command cannot contain control characters or line breaks.".into());
        }
        let id = custom_id(label);
        if ids.contains(&id) {
            return Err(format!(
                "Two commands are named \"{label}\"; give one a different name."
            ));
        }
        ids.push(id);
        out.push((label.to_string(), command.to_string()));
    }
    Ok(out)
}

/// The saved commands as runnable presets. They go through the platform shell
/// (`sh -c` / `cmd /C`) because the user wrote a command line, not an argv, and
/// the permission prompt shows exactly that line before it runs.
pub(crate) fn custom(pairs: &[(String, String)]) -> Vec<Preset> {
    pairs
        .iter()
        .map(|(label, command)| {
            #[cfg(target_os = "windows")]
            let (program, args) = ("cmd", ["/C", command.as_str()]);
            #[cfg(not(target_os = "windows"))]
            let (program, args) = ("sh", ["-c", command.as_str()]);
            Preset::new(
                &custom_id(label),
                label,
                "custom",
                command,
                program,
                &args,
                false,
            )
        })
        .collect()
}

/// Every command this folder can run: what it detects plus what the user saved.
pub(crate) fn all(root: &Path, saved: &[(String, String)]) -> Vec<Preset> {
    let mut presets = detect(root);
    presets.extend(custom(saved));
    presets
}

fn spawn_command(preset: &Preset) -> Command {
    #[cfg(target_os = "windows")]
    if preset.needs_shell {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(&preset.program).args(&preset.args);
        return command;
    }
    let mut command = Command::new(&preset.program);
    command.args(&preset.args);
    command
}

struct Capture {
    text: String,
    truncated: bool,
}

/// Drain a pipe, keeping at most `cap` bytes of it. The rest is still read so
/// the child never blocks on a full pipe.
fn read_capped(mut reader: impl Read, cap: usize) -> Capture {
    let mut buffer = vec![0u8; 16 * 1024];
    let mut text = String::new();
    let mut truncated = false;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let chunk = String::from_utf8_lossy(&buffer[..count]).to_string();
                if text.len() + chunk.len() <= cap {
                    text.push_str(&chunk);
                } else {
                    truncated = true;
                    let remaining = cap.saturating_sub(text.len());
                    let mut cut = remaining.min(chunk.len());
                    while cut > 0 && !chunk.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    text.push_str(&chunk[..cut]);
                }
            }
            Err(_) => break,
        }
    }
    Capture { text, truncated }
}

/// The last `limit` characters of `text`, cut on a character boundary.
fn tail(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let skip = text.chars().count() - limit;
    text.chars().skip(skip).collect()
}

fn fail(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

fn bad(message: impl Into<String>) -> Response {
    fail(StatusCode::BAD_REQUEST, message)
}

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

/// Only one build/test at a time. The guard clears the flag when the run ends,
/// including on an early return.
struct RunSlot;

static RUNNING: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

fn acquire_slot() -> Option<RunSlot> {
    let mut running = RUNNING.lock().ok()?;
    if *running {
        return None;
    }
    *running = true;
    Some(RunSlot)
}

impl Drop for RunSlot {
    fn drop(&mut self) {
        if let Ok(mut running) = RUNNING.lock() {
            *running = false;
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostics

/// Parse compiler and test output into problems the editor can open.
///
/// Handles the three shapes this app's own projects emit:
///
/// * Rust: an `error[E0308]: …` / `warning: …` line followed by `--> path:l:c`.
/// * TypeScript: `path(l,c): error TS2322: …`.
/// * Generic tools: `path:l:c: error: …` (also without a column).
///
/// Anything without a file location is counted in the output but not offered
/// as a clickable problem, because there is nowhere to jump to.
pub(crate) fn parse_problems(output: &str, root: &Path) -> Vec<Value> {
    let mut problems: Vec<Value> = Vec::new();
    let mut seen: Vec<(String, usize, usize, String)> = Vec::new();
    let mut pending_rust: Option<(String, String)> = None;

    for raw in output.lines() {
        let trimmed = raw.trim_start();
        if let Some((severity, message)) = rust_head(trimmed) {
            pending_rust = Some((severity, message));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("--> ") {
            if let Some((severity, message)) = pending_rust.take() {
                if let Some((path, line, column)) = parse_location(rest.trim()) {
                    push_problem(
                        &mut problems,
                        &mut seen,
                        root,
                        &path,
                        line,
                        column,
                        &severity,
                        &message,
                    );
                }
            }
            continue;
        }
        if let Some((path, line, column, severity, message)) = parse_paren(trimmed) {
            push_problem(
                &mut problems,
                &mut seen,
                root,
                &path,
                line,
                column,
                &severity,
                &message,
            );
            continue;
        }
        if let Some((path, line, column, severity, message)) = parse_generic(trimmed) {
            push_problem(
                &mut problems,
                &mut seen,
                root,
                &path,
                line,
                column,
                &severity,
                &message,
            );
        }
    }
    problems
}

fn rust_head(line: &str) -> Option<(String, String)> {
    let (severity, rest) = match line.strip_prefix("error") {
        Some(rest) => ("error", rest),
        None => ("warning", line.strip_prefix("warning")?),
    };
    let rest = rest.trim_start();
    let rest = if rest.starts_with('[') {
        let end = rest.find(']')?;
        rest[end + 1..].trim_start()
    } else {
        rest
    };
    let rest = rest.strip_prefix(':')?;
    let message = rest.trim();
    if message.is_empty() {
        return None;
    }
    Some((
        severity.to_string(),
        message.chars().take(300).collect::<String>(),
    ))
}

fn parse_location(text: &str) -> Option<(String, usize, usize)> {
    let mut parts = text.rsplitn(3, ':');
    let last = parts.next()?.trim();
    let second = parts.next()?.trim();
    match parts.next() {
        Some(rest) if !rest.trim().is_empty() => {
            let column = last.parse::<usize>().ok()?;
            let line = second.parse::<usize>().ok()?;
            Some((rest.trim().to_string(), line, column))
        }
        _ => {
            // Two components: path:line.
            let line = last.parse::<usize>().ok()?;
            if second.is_empty() {
                return None;
            }
            Some((second.to_string(), line, 0))
        }
    }
}

/// `path(line,col): error …` — TypeScript and several linters.
fn parse_paren(line: &str) -> Option<(String, usize, usize, String, String)> {
    let close = line.find("): ")?;
    let head = &line[..close];
    let open = head.rfind('(')?;
    let path = head[..open].trim();
    if path.is_empty() {
        return None;
    }
    let (line_part, column_part) = head[open + 1..].split_once(',')?;
    let line_no = line_part.trim().parse::<usize>().ok()?;
    let column = column_part.trim().parse::<usize>().ok()?;
    let (severity, message) = split_severity(line[close + 3..].trim())?;
    Some((
        path.to_string(),
        line_no,
        column,
        severity,
        message.to_string(),
    ))
}

/// `path:line[:col]: error: …` — the common compiler/linter shape.
fn parse_generic(line: &str) -> Option<(String, usize, usize, String, String)> {
    let mut search = 0;
    while let Some(found) = line[search..].find(": ") {
        let at = search + found;
        let head = &line[..at];
        let rest = line[at + 2..].trim();
        if let Some((severity, message)) = split_severity(rest) {
            if let Some((path, line_no, column)) = parse_location(head) {
                if !path.is_empty() && !path.chars().all(|c| c.is_ascii_digit()) {
                    return Some((path, line_no, column, severity, message.to_string()));
                }
            }
        }
        search = at + 2;
    }
    None
}

fn split_severity(text: &str) -> Option<(String, &str)> {
    let lower = text.to_ascii_lowercase();
    for severity in ["error", "warning", "note", "fatal"] {
        if let Some(rest) = lower.strip_prefix(severity) {
            if rest.starts_with(':') || rest.starts_with(' ') {
                let message = text[severity.len()..].trim_start_matches([':', ' ']).trim();
                if !message.is_empty() {
                    return Some((severity.to_string(), message));
                }
            }
        }
    }
    None
}

/// Turn a compiler path into a workspace-relative one. A diagnostic outside
/// the open folder is dropped: the editor cannot open it, and pretending it is
/// clickable would be worse than leaving it in the output.
fn relative_problem_path(path: &str, root: &Path) -> Option<String> {
    let candidate = Path::new(path);
    let relative = if candidate.is_absolute() {
        candidate.strip_prefix(root).ok()?.to_path_buf()
    } else if path.starts_with('/') || path.starts_with('\\') {
        // Windows does not call a rooted path like `/other/src/b.rs` absolute,
        // because it has no drive prefix. It is still outside the open folder,
        // and offering it as a clickable problem would be a lie.
        return None;
    } else {
        candidate.to_path_buf()
    };
    let text = relative.to_string_lossy().replace('\\', "/");
    let cleaned = text.trim_start_matches("./").to_string();
    if cleaned.is_empty() || cleaned.starts_with("../") {
        None
    } else {
        Some(cleaned)
    }
}

#[allow(clippy::too_many_arguments)]
fn push_problem(
    problems: &mut Vec<Value>,
    seen: &mut Vec<(String, usize, usize, String)>,
    root: &Path,
    path: &str,
    line: usize,
    column: usize,
    severity: &str,
    message: &str,
) {
    if problems.len() >= MAX_PROBLEMS {
        return;
    }
    let Some(path) = relative_problem_path(path, root) else {
        return;
    };
    let message = message.chars().take(400).collect::<String>();
    let key = (path.clone(), line, column, message.clone());
    if seen.contains(&key) {
        return;
    }
    seen.push(key);
    problems.push(json!({
        "path": path,
        "line": line,
        "column": column,
        "severity": severity,
        "message": message,
    }));
}

// ---------------------------------------------------------------------------
// Test results

/// Count what a test run actually reported.
///
/// Four shapes are recognised, because those are the four a detected preset can
/// produce: libtest (`cargo test`), Jest, Vitest, and node's own `--test`
/// reporter. Nothing is inferred — with no summary line the answer is `None`,
/// and the UI says it has no counts rather than showing zeros.
/// The `count label` pair inside one summary segment. libtest puts its verdict
/// first (`FAILED. 1 passed; 2 failed; …`), so the pair is searched for rather
/// than assumed to start the segment.
fn count_pair(part: &str) -> Option<(i64, &str)> {
    let words: Vec<&str> = part.split_whitespace().collect();
    words.windows(2).find_map(|pair| {
        let count = pair[0].parse::<i64>().ok()?;
        Some((count, pair[1]))
    })
}

pub(crate) fn parse_test_summary(output: &str) -> Option<Value> {
    let mut passed: i64 = 0;
    let mut failed: i64 = 0;
    let mut ignored: i64 = 0;
    let mut total: i64 = 0;
    let mut failures: Vec<String> = Vec::new();
    let mut framework = "";
    let mut saw_counts = false;
    let mut in_failures = false;

    for raw in output.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            in_failures = false;
            continue;
        }

        // libtest prints one result line per test binary, so they are summed.
        if let Some(rest) = trimmed.strip_prefix("test result:") {
            framework = "cargo";
            for part in rest.split(';') {
                let Some((count, label)) = count_pair(part) else {
                    continue;
                };
                match label {
                    "passed" => passed += count,
                    "failed" => failed += count,
                    "ignored" => ignored += count,
                    _ => {}
                }
            }
            saw_counts = true;
            continue;
        }

        // libtest's failure list: the second `failures:` header is followed by
        // indented test names.
        if trimmed == "failures:" {
            in_failures = true;
            continue;
        }
        if in_failures && line.starts_with("    ") && !trimmed.starts_with("----") {
            if failures.len() < MAX_FAILURES {
                failures.push(trimmed.to_string());
            }
            continue;
        }

        // Jest: `Tests:       1 failed, 2 passed, 3 total`.
        if let Some(rest) = trimmed.strip_prefix("Tests:") {
            framework = "jest";
            for part in rest.split(',') {
                let Some((count, label)) = count_pair(part) else {
                    continue;
                };
                match label {
                    "passed" => passed += count,
                    "failed" => failed += count,
                    "total" => total += count,
                    "skipped" | "pending" | "todo" => ignored += count,
                    _ => {}
                }
            }
            saw_counts = true;
            continue;
        }

        // Vitest: `Tests  1 failed | 2 passed (3)`.
        if let Some(rest) = trimmed.strip_prefix("Tests") {
            let rest = rest.trim();
            if rest.starts_with(|ch: char| ch.is_ascii_digit()) && rest.contains('|') {
                framework = "vitest";
                let (body, tail_total) = match rest.rsplit_once('(') {
                    Some((body, total_part)) => (
                        body,
                        total_part.trim_end_matches(')').trim().parse::<i64>().ok(),
                    ),
                    None => (rest, None),
                };
                for part in body.split('|') {
                    let mut words = part.split_whitespace();
                    let (Some(count), Some(label)) = (words.next(), words.next()) else {
                        continue;
                    };
                    let Ok(count) = count.parse::<i64>() else {
                        continue;
                    };
                    match label {
                        "passed" => passed += count,
                        "failed" => failed += count,
                        "skipped" | "todo" => ignored += count,
                        _ => {}
                    }
                }
                if let Some(total_part) = tail_total {
                    total += total_part;
                }
                saw_counts = true;
                continue;
            }
        }

        // node --test's TAP summary.
        if let Some(rest) = trimmed.strip_prefix("# pass ") {
            framework = "node";
            passed += rest.trim().parse::<i64>().unwrap_or(0);
            saw_counts = true;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# fail ") {
            framework = "node";
            failed += rest.trim().parse::<i64>().unwrap_or(0);
            saw_counts = true;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# skip ") {
            framework = "node";
            ignored += rest.trim().parse::<i64>().unwrap_or(0);
            saw_counts = true;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# tests ") {
            framework = "node";
            total += rest.trim().parse::<i64>().unwrap_or(0);
            saw_counts = true;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("not ok ") {
            if let Some((_, name)) = rest.split_once(" - ") {
                if failures.len() < MAX_FAILURES {
                    failures.push(name.trim().to_string());
                }
            }
        }
    }

    if !saw_counts {
        return None;
    }
    if total == 0 {
        total = passed + failed + ignored;
    }
    Some(json!({
        "framework": framework,
        "total": total,
        "passed": passed,
        "failed": failed,
        "ignored": ignored,
        "failures": failures,
    }))
}

// ---------------------------------------------------------------------------
// Running

fn run_preset(root: &Path, preset: &Preset) -> Result<Value, String> {
    let started = Instant::now();
    let mut command = spawn_command(preset);
    command
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start `{}`: {error}", preset.command))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "The command's output could not be captured.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "The command's error stream could not be captured.".to_string())?;
    let stdout_thread = std::thread::spawn(move || read_capped(stdout, MAX_CAPTURE_BYTES));
    let stderr_thread = std::thread::spawn(move || read_capped(stderr, MAX_CAPTURE_BYTES));

    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if started.elapsed() >= RUN_TIMEOUT {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(120));
    };
    let out = stdout_thread.join().unwrap_or(Capture {
        text: String::new(),
        truncated: false,
    });
    let err = stderr_thread.join().unwrap_or(Capture {
        text: String::new(),
        truncated: false,
    });

    let mut combined = out.text;
    if !err.text.is_empty() {
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&err.text);
    }
    let problems = parse_problems(&combined, root);
    // Only a test preset reports tests: a build's output is not searched for
    // counts it cannot have.
    let tests = if preset.kind == "test" {
        parse_test_summary(&combined)
    } else {
        None
    };
    let exit_code = status.and_then(|status| status.code()).unwrap_or(-1);
    let success = !timed_out && exit_code == 0;

    Ok(json!({
        "ok": true,
        "id": preset.id,
        "command": preset.command,
        "success": success,
        "exitCode": exit_code,
        "timedOut": timed_out,
        "durationMs": started.elapsed().as_millis() as u64,
        "output": tail(&combined, MAX_DISPLAY_CHARS),
        "outputTruncated": out.truncated || err.truncated || combined.chars().count() > MAX_DISPLAY_CHARS,
        "problems": problems,
        "problemCount": problems.len(),
        "tests": tests,
    }))
}

// ---------------------------------------------------------------------------
// Routes

async fn presets_route(State(state): State<AppState>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let list: Vec<Value> = all(&root, &crate::command_presets(&state))
        .iter()
        .map(Preset::to_json)
        .collect();
    let running = RUNNING.lock().map(|flag| *flag).unwrap_or(false);
    Json(json!({ "presets": list, "running": running })).into_response()
}

async fn run_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match workspace_root(&state) {
        Ok(root) => root,
        Err(response) => return *response,
    };
    let id = input.get("id").and_then(Value::as_str).unwrap_or("");
    let Some(preset) = all(&root, &crate::command_presets(&state))
        .into_iter()
        .find(|preset| preset.id == id)
    else {
        return bad("Pick one of the detected or saved commands.");
    };
    if let Some(response) = guard_user(
        &state,
        Action::Command,
        &preset.command,
        "",
        approval(&input),
        "run a command in this folder",
    ) {
        return response;
    }
    let Some(_slot) = acquire_slot() else {
        return fail(
            StatusCode::CONFLICT,
            "A command is already running in this folder. Wait for it to finish.",
        );
    };
    let root_for_task = root.clone();
    let preset_for_task = preset.clone();
    match tokio::task::spawn_blocking(move || run_preset(&root_for_task, &preset_for_task)).await {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(message)) => bad(message),
        Err(_) => fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The build task stopped unexpectedly.",
        ),
    }
}

/// Merged into `workspace::router`, so both routes inherit the loopback
/// `Host`/`Origin` guard and the request-body cap.
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/workspace/runner", get(presets_route))
        .route("/api/workspace/run", post(run_route))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "sarthi-runner-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detects_cargo_only_when_the_manifest_exists() {
        let dir = temp_dir("cargo");
        assert!(detect(&dir).is_empty());
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let ids: Vec<String> = detect(&dir).into_iter().map(|preset| preset.id).collect();
        assert_eq!(ids, vec!["cargo-build", "cargo-test"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_npm_scripts_and_follows_the_lockfile() {
        let dir = temp_dir("npm");
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"build":"node build.mjs","test":"node --test"}}"#,
        )
        .unwrap();
        let presets = detect(&dir);
        assert_eq!(
            presets.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["npm-build", "npm-test"]
        );
        assert_eq!(presets[0].command, "npm run build");
        assert_eq!(presets[1].command, "npm test");
        assert!(presets[0].needs_shell);

        // A pnpm lockfile switches the commands, not their ids' meaning.
        std::fs::write(dir.join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
        let presets = detect(&dir);
        assert_eq!(presets[0].command, "pnpm run build");
        assert_eq!(presets[1].command, "pnpm test");

        // Only declared scripts become presets.
        std::fs::write(dir.join("package.json"), r#"{"scripts":{"lint":"eslint"}}"#).unwrap();
        assert!(detect(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_commands_get_stable_ids_and_run_through_the_platform_shell() {
        let pairs = vec![
            ("Lint the tree".to_string(), "echo lint".to_string()),
            ("Two  words -- here".to_string(), "echo two".to_string()),
        ];
        let presets = custom(&pairs);
        assert_eq!(
            presets.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["custom-lint-the-tree", "custom-two-words-here"]
        );
        assert_eq!(presets[0].label, "Lint the tree");
        assert_eq!(presets[0].command, "echo lint");
        assert_eq!(
            presets[0].kind, "custom",
            "never mistaken for a build or test"
        );
        // The user wrote a command line, so it goes to the platform shell.
        #[cfg(target_os = "windows")]
        assert_eq!(
            (presets[0].program.as_str(), presets[0].args.as_slice()),
            ("cmd", &["/C".to_string(), "echo lint".to_string()][..])
        );
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            (presets[0].program.as_str(), presets[0].args.as_slice()),
            ("sh", &["-c".to_string(), "echo lint".to_string()][..])
        );
        // A name with nothing alphanumeric still gets an id, not a panic.
        assert_eq!(
            custom(&[("!!!".into(), "echo".into())])[0].id,
            "custom-command"
        );
    }

    #[test]
    fn a_saved_command_is_refused_when_it_is_blank_duplicated_or_oversized() {
        assert!(validate_custom(&[json!({ "label": "lint", "command": "npm run lint" })]).is_ok());

        // Both halves are required, after trimming.
        for entry in [
            json!({ "label": "  ", "command": "npm run lint" }),
            json!({ "label": "lint", "command": "" }),
            json!({ "command": "npm run lint" }),
        ] {
            let error = validate_custom(&[entry]).expect_err("blank entry");
            assert!(error.contains("name and a command line"), "{error}");
        }

        // A line break would split one saved command into two, so it is refused.
        let error =
            validate_custom(&[json!({ "label": "lint", "command": "npm run lint\nnpm test" })])
                .expect_err("control character");
        assert!(error.contains("control characters"), "{error}");

        // Two names that slug the same would share an id, so the second is refused.
        let error = validate_custom(&[
            json!({ "label": "Lint", "command": "npm run lint" }),
            json!({ "label": "lint!", "command": "npm run lint:fix" }),
        ])
        .expect_err("duplicate id");
        assert!(error.contains("different name"), "{error}");

        // Lengths are bounded so neither the config nor the panel grows without limit.
        let long = "x".repeat(501);
        assert!(validate_custom(&[json!({ "label": "lint", "command": long })]).is_err());
        let many: Vec<Value> = (0..21)
            .map(|index| json!({ "label": format!("c{index}"), "command": "echo" }))
            .collect();
        let error = validate_custom(&many).expect_err("count");
        assert!(error.contains("at most 20"), "{error}");
    }

    #[test]
    fn a_saved_command_runs_and_returns_its_output() {
        let dir = temp_dir("custom");
        let presets = custom(&[("Say hi".to_string(), "echo hi".to_string())]);
        let result = run_preset(&dir, &presets[0]).expect("a saved command runs");
        assert_eq!(result["success"], json!(true), "{result}");
        assert_eq!(result["command"], json!("echo hi"));
        assert!(
            result["output"].as_str().unwrap_or("").contains("hi"),
            "{result}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_commands_are_listed_after_the_detected_ones() {
        let dir = temp_dir("merge");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let ids: Vec<String> = all(&dir, &[("Lint".to_string(), "echo lint".to_string())])
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, vec!["cargo-build", "cargo-test", "custom-lint"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_rust_errors_and_warnings_with_their_locations() {
        let output = "   Compiling demo v0.1.0\n\
                      error[E0308]: mismatched types\n\
                      \x20--> src/main.rs:12:9\n\
                      \x20  |\n\
                      12 |     foo();\n\
                      warning: unused variable: `x`\n\
                      \x20--> src/lib.rs:3:5\n";
        let problems = parse_problems(output, Path::new(""));
        assert_eq!(problems.len(), 2);
        assert_eq!(problems[0]["path"], "src/main.rs");
        assert_eq!(problems[0]["line"], 12);
        assert_eq!(problems[0]["column"], 9);
        assert_eq!(problems[0]["severity"], "error");
        assert!(problems[0]["message"]
            .as_str()
            .unwrap()
            .contains("mismatched types"));
        assert_eq!(problems[1]["severity"], "warning");
        assert_eq!(problems[1]["path"], "src/lib.rs");
    }

    #[test]
    fn parses_typescript_and_generic_diagnostics() {
        let output =
            "src/app.ts(7,5): error TS2322: Type 'string' is not assignable to type 'number'.\n\
                      src/util.js:18:11: warning: unused import\n\
                      lib/plain.rs:4: error: expected `;`\n";
        let problems = parse_problems(output, Path::new(""));
        assert_eq!(problems.len(), 3);
        assert_eq!(problems[0]["path"], "src/app.ts");
        assert_eq!(problems[0]["line"], 7);
        assert_eq!(problems[0]["column"], 5);
        assert_eq!(problems[0]["severity"], "error");
        assert!(problems[0]["message"].as_str().unwrap().contains("TS2322"));
        assert_eq!(problems[1]["path"], "src/util.js");
        assert_eq!(problems[1]["column"], 11);
        assert_eq!(problems[2]["path"], "lib/plain.rs");
        assert_eq!(problems[2]["line"], 4);
        assert_eq!(problems[2]["column"], 0);
    }

    #[test]
    fn drops_locations_outside_the_workspace_and_duplicates() {
        // Real absolute paths, so the test means the same thing on Windows
        // (where `/other/x` is rooted but not absolute) and on Unix.
        let root = temp_dir("paths");
        let inside = root.join("src").join("a.rs");
        let outside = std::env::temp_dir()
            .join("sarthi-runner-elsewhere")
            .join("b.rs");
        let output = format!(
            "{0}:1:1: error: boom\n{0}:1:1: error: boom\n{1}:2:2: error: elsewhere\nerror: link failed without a location\n",
            inside.display(),
            outside.display()
        );
        let problems = parse_problems(&output, &root);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0]["path"], "src/a.rs");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn caps_problem_lists() {
        let mut output = String::new();
        for line in 1..=(MAX_PROBLEMS + 20) {
            output.push_str(&format!("src/a.rs:{line}:1: error: e{line}\n"));
        }
        let problems = parse_problems(&output, Path::new(""));
        assert_eq!(problems.len(), MAX_PROBLEMS);
    }

    #[test]
    fn runs_a_real_command_and_captures_both_streams() {
        let dir = temp_dir("run");
        let preset = Preset::new(
            "cargo-version",
            "cargo --version",
            "build",
            "cargo --version",
            "cargo",
            &["--version"],
            false,
        );
        match run_preset(&dir, &preset) {
            Ok(value) => {
                assert_eq!(value["success"], true);
                assert_eq!(value["exitCode"], 0);
                assert!(value["output"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .contains("cargo"));
                assert_eq!(value["timedOut"], false);
            }
            // A host without cargo on PATH cannot exercise the spawn path; the
            // rest of the suite already proves the process shape compiles.
            Err(message) => assert!(message.contains("Could not start")),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_run_is_refused_while_one_holds_the_slot() {
        let first = acquire_slot().expect("first slot");
        assert!(acquire_slot().is_none(), "a second build must not start");
        drop(first);
        assert!(
            acquire_slot().is_some(),
            "the slot is released when the run ends"
        );
    }

    #[test]
    fn tails_long_output_without_splitting_a_character() {
        let text = "é".repeat(MAX_DISPLAY_CHARS + 10);
        let cut = tail(&text, MAX_DISPLAY_CHARS);
        assert_eq!(cut.chars().count(), MAX_DISPLAY_CHARS);
        assert!(cut.is_char_boundary(0));
        assert_eq!(tail("short", MAX_DISPLAY_CHARS), "short");
    }

    #[test]
    fn sums_libtest_counts_across_binaries_and_lists_its_failures() {
        let output = "running 2 tests\n\
                      test demo::ok ... ok\n\
                      test demo::fails ... FAILED\n\n\
                      failures:\n\n\
                      ---- demo::fails stdout ----\n\
                      thread panicked at src/lib.rs:3\n\n\
                      failures:\n\
                      \x20   demo::fails\n\
                      \x20   demo::other\n\n\
                      test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n\
                      running 3 tests\n\
                      test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.02s\n";
        let summary = parse_test_summary(output).expect("a summary");
        assert_eq!(summary["framework"], "cargo");
        assert_eq!(summary["passed"], 4);
        assert_eq!(summary["failed"], 2);
        assert_eq!(summary["ignored"], 1);
        assert_eq!(summary["total"], 7);
        assert_eq!(summary["failures"], json!(["demo::fails", "demo::other"]));
    }

    #[test]
    fn reads_jest_vitest_and_node_summaries() {
        let jest = "PASS src/a.test.js\nTests:       1 failed, 2 passed, 3 total\n";
        let summary = parse_test_summary(jest).expect("jest summary");
        assert_eq!(summary["framework"], "jest");
        assert_eq!(summary["total"], 3);
        assert_eq!(summary["failed"], 1);
        assert_eq!(summary["passed"], 2);

        let vitest = "Tests  1 failed | 2 passed (3)\n";
        let summary = parse_test_summary(vitest).expect("vitest summary");
        assert_eq!(summary["framework"], "vitest");
        assert_eq!(summary["total"], 3);
        assert_eq!(summary["failed"], 1);
        assert_eq!(summary["passed"], 2);

        let node = "ok 1 - adds\nnot ok 2 - subtracts\n# tests 2\n# pass 1\n# fail 1\n";
        let summary = parse_test_summary(node).expect("node summary");
        assert_eq!(summary["framework"], "node");
        assert_eq!(summary["total"], 2);
        assert_eq!(summary["failed"], 1);
        assert_eq!(summary["failures"], json!(["subtracts"]));
    }

    #[test]
    fn a_run_with_no_summary_line_reports_no_counts() {
        // A plain build prints no test result, and neither does a compiler
        // error: reporting zeroes would claim tests ran when none did.
        assert!(parse_test_summary("   Compiling demo v0.1.0\n    Finished dev\n").is_none());
        assert!(parse_test_summary("error[E0308]: mismatched types\n").is_none());
    }
}
