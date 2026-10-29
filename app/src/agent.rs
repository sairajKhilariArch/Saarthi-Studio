//! Agent runs: a bounded, permissioned autonomous execution loop.
//!
//! The model plans one step at a time. Each step is parsed from the reply as a
//! single JSON action — think, list, read, write, command, done or fail — and
//! executed against the open workspace (the same path-confined helpers the file
//! editor uses). File writes and command execution only run when the run was
//! started with those permissions, and when auto-approve is off the run pauses
//! in `awaiting_approval` until the user answers. Every step is persisted to
//! SQLite, so a crash leaves an honest `interrupted` record instead of a
//! silently repeated half-completed run.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicBool, Ordering};
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::process::Command;

use crate::AppState;

pub(crate) const DEFAULT_MAX_STEPS: i64 = 12;
pub(crate) const HARD_MAX_STEPS: i64 = 30;
/// One model step's captured output never exceeds this, in characters.
const STEP_OUTPUT_CAP: usize = 4_000;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(90);
/// How much step history the model sees on every prompt, newest first.
const HISTORY_STEPS: i64 = 40;
/// Restorable snapshot cap: a bigger file is still diffed live, but its old
/// content is not persisted for rollback.
const CHECKPOINT_CAP: usize = 200_000;
/// How many characters of unified diff one step may log.
const DIFF_CAP: usize = 8_000;

const RUNNING: &str = "running";
const AWAITING_APPROVAL: &str = "awaiting_approval";
const SUCCEEDED: &str = "succeeded";
const FAILED: &str = "failed";
const CANCELLED: &str = "cancelled";
const INTERRUPTED: &str = "interrupted";

/// Live runs, for cancellation. Flags live exactly as long as the spawned loop
/// task.
static ACTIVE: LazyLock<Mutex<HashMap<String, Arc<AtomicBool>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// Actions

/// One JSON action the model emits per step.
#[derive(Debug, PartialEq)]
enum Action {
    Think { summary: String },
    List { path: String },
    Read { path: String },
    Write { path: String, content: String },
    Command { command: String },
    Done { summary: String },
    Fail { reason: String },
}

impl Action {
    fn tag(&self) -> &'static str {
        match self {
            Action::Think { .. } => "think",
            Action::List { .. } => "list",
            Action::Read { .. } => "read",
            Action::Write { .. } => "write",
            Action::Command { .. } => "command",
            Action::Done { .. } => "done",
            Action::Fail { .. } => "fail",
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Action::Think { summary } => json!({ "action": "think", "summary": summary }),
            Action::List { path } => json!({ "action": "list", "path": path }),
            Action::Read { path } => json!({ "action": "read", "path": path }),
            Action::Write { path, content } => {
                json!({ "action": "write", "path": path, "content": content })
            }
            Action::Command { command } => json!({ "action": "command", "command": command }),
            Action::Done { summary } => json!({ "action": "done", "summary": summary }),
            Action::Fail { reason } => json!({ "action": "fail", "reason": reason }),
        }
    }

    fn from_json(value: &Value) -> Result<Action, String> {
        let tag = value
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing \"action\"".to_string())?;
        let field = |name: &str, value: &Value| {
            value
                .get(name)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("missing \"{name}\""))
                .map(str::to_string)
        };
        match tag {
            "think" => Ok(Action::Think {
                summary: field("summary", value)?
,
            }),
            "list" => Ok(Action::List { path: field("path", value)? }),
            "read" => Ok(Action::Read { path: field("path", value)? }),
            "write" => Ok(Action::Write {
                path: field("path", value)?,
                content: field("content", value)?,
            }),
            "command" => Ok(Action::Command { command: field("command", value)? }),
            "done" => Ok(Action::Done {
                summary: field("summary", value)?
,
            }),
            "fail" => Ok(Action::Fail {
                reason: field("reason", value)?
,
            }),
            other => Err(format!("unknown action \"{other}\"")),
        }
    }
}

#[derive(Debug)]
enum ParsedAction {
    Ok(Action),
    /// The reply could not become an action; carries the parse error so the
    /// step is logged honestly and the model can correct itself.
    Malformed(String),
}

/// Pull the first JSON object out of the reply — fenced or not — and map it to
/// an action. Pure prose without any braces counts as a think step, so the
/// loop always records something.
fn parse_action(text: &str) -> ParsedAction {
    let trimmed = text.trim();
    match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(open), Some(close)) if close > open => {
            match serde_json::from_str::<Value>(&trimmed[open..=close]) {
                Ok(value) => match Action::from_json(&value) {
                    Ok(action) => ParsedAction::Ok(action),
                    Err(error) => ParsedAction::Malformed(format!("invalid action: {error}")),
                },
                Err(error) => ParsedAction::Malformed(format!("invalid JSON: {error}")),
            }
        }
        _ if !trimmed.is_empty() => ParsedAction::Ok(Action::Think {
            summary: cap_chars(trimmed, 300),
        }),
        _ => ParsedAction::Malformed("empty reply".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Persistence

pub(crate) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_runs (
            id TEXT PRIMARY KEY, project_id TEXT NOT NULL,
            goal TEXT NOT NULL, status TEXT NOT NULL,
            allow_writes INTEGER NOT NULL, allow_commands INTEGER NOT NULL,
            auto_approve INTEGER NOT NULL, max_steps INTEGER NOT NULL,
            pending_action TEXT NOT NULL DEFAULT '',
            error TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL, updated_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS agent_steps (
            id INTEGER PRIMARY KEY AUTOINCREMENT, run_id TEXT NOT NULL,
            seq INTEGER NOT NULL, action TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '', output TEXT NOT NULL DEFAULT '',
            checkpoint TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL,
            FOREIGN KEY(run_id) REFERENCES agent_runs(id) ON DELETE CASCADE
        );",
    )?;
    // Databases created before checkpoints predate the column.
    if let Err(error) = conn.execute(
        "ALTER TABLE agent_steps ADD COLUMN checkpoint TEXT NOT NULL DEFAULT ''",
        [],
    ) {
        if !error.to_string().contains("duplicate column") {
            return Err(error);
        }
    }
    // Reproducible-run record (M6): additive columns, safe on any vintage.
    for statement in [
        "ALTER TABLE agent_runs ADD COLUMN model TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE agent_runs ADD COLUMN router_config TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE agent_runs ADD COLUMN profile TEXT NOT NULL DEFAULT 'coding'",
    ] {
        if let Err(error) = conn.execute(statement, []) {
            if !error.to_string().contains("duplicate column") {
                return Err(error);
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
struct RunRow {
    id: String,
    project_id: String,
    goal: String,
    status: String,
    allow_writes: bool,
    allow_commands: bool,
    auto_approve: bool,
    max_steps: i64,
    pending_action: String,
    error: String,
    created_at: f64,
    updated_at: f64,
    router_config: String,
    model: String,
    profile: String,
}

fn row_to_run(row: &rusqlite::Row) -> rusqlite::Result<RunRow> {
    Ok(RunRow {
        id: row.get(0)?,
        project_id: row.get(1)?,
        goal: row.get(2)?,
        status: row.get(3)?,
        allow_writes: row.get(4)?,
        allow_commands: row.get(5)?,
        auto_approve: row.get(6)?,
        max_steps: row.get(7)?,
        pending_action: row.get(8)?,
        error: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        router_config: row.get(12)?,
        model: row.get(13)?,
        profile: row.get(14)?,
    })
}

fn db(state: &AppState) -> Result<Connection, String> {
    crate::open_db(&state.database_path).map_err(|error| format!("Database error: {error}"))
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Crash recovery: a run still marked live after a restart never resumed, so
/// it becomes an honest `interrupted` record instead of a stale spinner.
pub(crate) fn mark_interrupted(state: &AppState) {
    if let Ok(conn) = crate::open_db(&state.database_path) {
        let _ = mark_interrupted_rows(&conn);
    }
}

fn mark_interrupted_rows(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE agent_runs SET status=?1, error='App restarted mid-run.', updated_at=?2
         WHERE status IN (?3, ?4)",
        params![INTERRUPTED, now(), RUNNING, AWAITING_APPROVAL],
    )
}

fn add_step(
    state: &AppState,
    run_id: &str,
    action: &str,
    summary: &str,
    output: &str,
) -> Result<(), String> {
    add_step_with_checkpoint(state, run_id, action, summary, output, "")
}

/// Same as `add_step`, plus the pre-write snapshot that makes the change
/// reversible from the run log.
fn add_step_with_checkpoint(
    state: &AppState,
    run_id: &str,
    action: &str,
    summary: &str,
    output: &str,
    checkpoint: &str,
) -> Result<(), String> {
    let mut conn = db(state)?;
    insert_step(&mut conn, run_id, action, summary, output, checkpoint).map_err(|e| e.to_string())
}

fn insert_step(
    conn: &mut Connection,
    run_id: &str,
    action: &str,
    summary: &str,
    output: &str,
    checkpoint: &str,
) -> rusqlite::Result<()> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq),0)+1 FROM agent_steps WHERE run_id=?1",
        [run_id],
        |r| r.get(0),
    )?;
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO agent_steps(run_id,seq,action,summary,output,checkpoint,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![run_id, seq, action, cap_chars(summary, 600), cap_chars(output, STEP_OUTPUT_CAP), checkpoint, now()],
    )?;
    tx.commit()
}

fn set_run(
    state: &AppState,
    run_id: &str,
    status: &str,
    error: Option<&str>,
    pending: Option<&str>,
) -> Result<(), String> {
    db(state)?
        .execute(
            "UPDATE agent_runs SET status=?1, error=?2, pending_action=?3, updated_at=?4 WHERE id=?5",
            params![status, error.unwrap_or(""), pending.unwrap_or(""), now(), run_id],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn finish(state: &AppState, run_id: &str, status: &str, error: &str) -> Result<(), String> {
    set_run(state, run_id, status, Some(error), Some(""))
}

fn load_run(state: &AppState, run_id: &str) -> Result<Option<RunRow>, String> {
    let conn = db(state)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, goal, status, allow_writes, allow_commands, auto_approve, \
             max_steps, pending_action, error, created_at, updated_at, router_config, model, profile FROM agent_runs WHERE id=?1",
        )
        .map_err(|e| e.to_string())?;
    stmt.query_row([run_id], row_to_run)
        .optional()
        .map_err(|e| e.to_string())
}

fn step_count(state: &AppState, run_id: &str) -> Result<i64, String> {
    db(state)?.query_row(
        "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1",
        [run_id],
        |row| row.get(0),
    ).map_err(|e| e.to_string())
}

fn run_status(state: &AppState, run_id: &str) -> Result<Option<String>, String> {
    db(state)?
        .query_row("SELECT status FROM agent_runs WHERE id=?1", [run_id], |row| row.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

fn run_json(row: &RunRow, steps: i64) -> Value {
    json!({
        "id": row.id, "projectId": row.project_id, "goal": row.goal,
        "status": row.status, "allowWrites": row.allow_writes,
        "allowCommands": row.allow_commands, "autoApprove": row.auto_approve,
        "maxSteps": row.max_steps, "pendingAction": read_pending(row),
        "error": row.error, "steps": steps,
        "createdAt": row.created_at, "updatedAt": row.updated_at,
        "model": row.model, "routerConfig": row.router_config,
        "profile": row.profile,
    })
}

/// The approval UI never needs the file body — just what kind of action waits.
fn read_pending(row: &RunRow) -> Value {
    if row.pending_action.is_empty() {
        return json!(null);
    }
    serde_json::from_str::<Value>(&row.pending_action)
        .ok()
        .and_then(|value| Action::from_json(&value).ok())
        .map(|action| {
            json!({
                "tag": action.tag(),
                "summary": pending_summary(&action),
            })
        })
        .unwrap_or(json!({ "tag": "unknown", "summary": "unreadable pending action" }))
}

/// A concrete human-readable line for what a tool action does.
fn pending_summary(action: &Action) -> String {
    match action {
        Action::Write { path, content } => {
            format!("write {} ({} bytes)", path, content.len())
        }
        Action::Command { command } => format!("run {command}"),
        Action::Read { path } => format!("read {path}"),
        Action::List { path } => format!("list {}", if path.is_empty() { "." } else { path }),
        _ => String::new(),
    }
}

/// Live refresh hint for one run.
fn notify(state: &AppState, run_id: Option<&str>) {
    state.events.publish(json!({
        "type": "agent-run-changed",
        "run_id": run_id,
        "at": now(),
    }));
}

// ---------------------------------------------------------------------------
// Checkpoints & diffs: every write stays previewable and reversible.

/// Snapshot the file a write is about to replace. The checkpoint (a JSON
/// record of the old content) is only kept when the file existed and is small
/// enough to store; the old content is still returned for a live diff whenever
/// it can be read.
fn capture_checkpoint(root: &FsPath, path: &str) -> (String, Option<String>) {
    let Ok(file) = crate::workspace::read_text_file(root, path) else {
        return (String::new(), None); // new file: nothing to roll back to
    };
    let old = file.content;
    if old.chars().count() > CHECKPOINT_CAP {
        return (String::new(), Some(old)); // too big to persist, still diffable
    }
    let checkpoint = json!({ "path": path, "old": old }).to_string();
    (checkpoint, Some(old))
}

/// One hunk of unified diff: the common prefix and suffix stay as context and
/// the changed middle becomes -old/+new lines. Bounded, so a run's execution
/// log can never grow without limit.
fn unified_diff(old: Option<&str>, new: &str, path: &str) -> String {
    const CONTEXT: usize = 3;
    let old_lines: Vec<&str> = old.unwrap_or("").lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    if old.is_some() && old_lines == new_lines {
        return "(no changes — the new content is identical)".to_string();
    }
    let mut start = 0;
    while start < old_lines.len() && start < new_lines.len() && old_lines[start] == new_lines[start] {
        start += 1;
    }
    let mut end_old = old_lines.len();
    let mut end_new = new_lines.len();
    while end_old > start && end_new > start && old_lines[end_old - 1] == new_lines[end_new - 1] {
        end_old -= 1;
        end_new -= 1;
    }
    let ctx_start = start.saturating_sub(CONTEXT);
    let ctx_end_old = (end_old + CONTEXT).min(old_lines.len());
    let ctx_end_new = (end_new + CONTEXT).min(new_lines.len());
    let mut out = format!(
        "--- {path} ({})\n+++ {path} (after)\n@@ -{},{} +{},{} @@\n",
        if old.is_some() { "before" } else { "new file" },
        if old.is_some() { ctx_start + 1 } else { 0 },
        ctx_end_old.saturating_sub(ctx_start),
        ctx_start + 1,
        ctx_end_new.saturating_sub(ctx_start),
    );
    for line in &old_lines[ctx_start..start] {
        out.push_str(&format!(" {line}\n"));
    }
    for line in &old_lines[start..end_old] {
        out.push_str(&format!("-{line}\n"));
    }
    for line in &new_lines[start..end_new] {
        out.push_str(&format!("+{line}\n"));
    }
    for line in &old_lines[end_old..ctx_end_old] {
        out.push_str(&format!(" {line}\n"));
    }
    cap_chars(&out, DIFF_CAP)
}

// ---------------------------------------------------------------------------
// Tool execution

/// Files, listings and commands go through the same path-confined workspace
/// helpers the file editor uses, so the agent can never reach outside the
/// open folder even with every permission granted.
async fn dispatch_action(root: &FsPath, action: &Action) -> Result<String, String> {
    match action {
        Action::Write { path, content } => crate::workspace::write_text_file(root, path, content)
            .map(|file| format!("Wrote {} bytes to {}.", file.content.len(), file.path))
            .map_err(|e| e.message().to_string()),
        Action::Read { path } => crate::workspace::read_text_file(root, path)
            .map(|file| {
                format!(
                    "{} ({} bytes)\n---\n{}",
                    file.path,
                    file.size,
                    cap_chars(&file.content, STEP_OUTPUT_CAP)
                )
            })
            .map_err(|e| e.message().to_string()),
        Action::List { path } => crate::workspace::list_directory(root, path)
            .map(|(entries, truncated)| {
                let names = entries
                    .iter()
                    .filter(|e| !e.hidden)
                    .take(200)
                    .map(|e| if e.dir { format!("{}/", e.path) } else { e.path.clone() })
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut out = cap_chars(&names, STEP_OUTPUT_CAP);
                if truncated {
                    out.push_str("\n(list truncated)");
                }
                out
            })
            .map_err(|e| e.message().to_string()),
        Action::Command { command } => run_command(root, command).await,
        _ => Err("Only list, read, write and command actions execute.".to_string()),
    }
}

async fn run_command(root: &FsPath, command: &str) -> Result<String, String> {
    if command.trim().is_empty() {
        return Err("Empty command.".to_string());
    }
    #[cfg(unix)]
    let mut cmd = {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    };
    #[cfg(not(unix))]
    let mut cmd = {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    };
    cmd.current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = cmd.spawn().map_err(|e| format!("Failed to spawn: {e}"))?;
    // kill_on_drop guarantees a dead child when the timeout drops the future.
    let result = tokio::time::timeout(COMMAND_TIMEOUT, child.wait_with_output()).await;
    match result {
        Err(_) => Err("Command timed out after 90 seconds and was killed.".to_string()),
        Ok(Err(error)) => Err(format!("Failed to run command: {error}")),
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(format!(
                "exit: {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                output.status.code().unwrap_or(-1),
                cap_chars(stdout.trim(), STEP_OUTPUT_CAP),
                cap_chars(stderr.trim(), STEP_OUTPUT_CAP)
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Permission gate: the single source of truth for what a run may do.

enum GateDecision {
    Allowed,
    NeedsApproval,
    Denied(&'static str),
}

struct PermissionGate;

impl PermissionGate {
    fn decides(run: &RunRow, action: &Action) -> GateDecision {
        // Profile is Trip mode: planner, testing and review run read-only by
        // design — writes and commands are denied before the model even
        // reaches them, no matter which checkboxes were ticked.
        if let Action::Write {..} | Action::Command {..} = action {
            if !matches!(run.profile.as_str(), "coding") {
                if let Action::Write {..} = action {
                    return GateDecision::Denied("file writes in this profile");
                }
                if !matches!(run.profile.as_str(), "testing") {
                    return GateDecision::Denied("command execution in this profile");
                }
            }
        }
        match action {
            Action::Write { .. } if !run.allow_writes => GateDecision::Denied("file writes"),
            Action::Command { .. } if !run.allow_commands => {
                GateDecision::Denied("command execution")
            }
            Action::Write { .. } | Action::Command { .. } if !run.auto_approve => {
                GateDecision::NeedsApproval
            }
            _ => GateDecision::Allowed,
        }
    }
}

// ---------------------------------------------------------------------------
// The bounded loop

/// The standing instruction: what one JSON action looks like and which tools
/// this particular run may use.
fn build_messages(run: &RunRow, history: &str) -> Vec<Value> {
    // Profile rules override what the checkboxes say: the gate backs the
    // prompt up, so a denied tool is both forbidden in text and refused.
    let write_rule = if !profile_rule(&run.profile, true).is_empty() {
        profile_rule(&run.profile, true).to_string()
    } else if run.allow_writes {
        "write, list".to_string()
    } else {
        "list (write FORBIDDEN — never emit it)".to_string()
    };
    let command_rule = if !profile_rule(&run.profile, false).is_empty() {
        profile_rule(&run.profile, false).to_string()
    } else if run.allow_commands {
        if run.auto_approve {
            "command".to_string()
        } else {
            "command (each command first waits for user approval)".to_string()
        }
    } else {
        "command FORBIDDEN — never emit it".to_string()
    };
    vec![
        json!({
            "role": "system",
            "content": format!(
                "{} Reply with \
                 EXACTLY ONE JSON object and nothing else — no markdown fences, no prose. \
                 Choose one:\n\
                 {{\"action\":\"think\",\"summary\":\"...\"}} next thought, open question or plan\n\
                 {{\"action\":\"list\",\"path\":\"relative/path\"}} inspect the folder\n\
                 {{\"action\":\"read\",\"path\":\"relative/path\"}} read a file (always read before editing)\n\
                 {{\"action\":\"write\",\"path\":\"relative/path\",\"content\":\"full file text\"}} create or overwrite a file\n\
                 {{\"action\":\"command\",\"command\":\"build/test command\"}} run a shell command in the workspace\n\
                 {{\"action\":\"done\",\"summary\":\"...\"}} the goal is met and you verified it\n\
                 {{\"action\":\"fail\",\"reason\":\"...\"}} you cannot meet the goal\n\n\
                 Permitted tools for this run: read: always; write: {write_rule}; command: {command_rule}.",
                profile_role(&run.profile)
            )
        }),
        json!({
            "role": "user",
            "content": format!(
                "Workspace goal:\n{}\n\nSteps so far:\n{}\n\nChoose the next single step as one JSON action. \
                 If a previous step errored, diagnose it and repair your approach; if you already verified \
                 success, emit done.",
                run.goal, history
            )
        }),
    ]
}

/// Every logged step so far, oldest first with a cap so a long run's prompt
/// never grows unboundedly.
fn build_history(state: &AppState, run_id: &str) -> Result<String, String> {
    let conn = db(state)?;
    let mut stmt = conn
        .prepare(
            "SELECT seq, action, summary, output FROM agent_steps WHERE run_id=?1 \
             ORDER BY seq DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![run_id, HISTORY_STEPS], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|row| row.ok())
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    for (seq, action, summary, output) in rows.iter().rev() {
        let head = if summary.is_empty() {
            format!("{seq}. [{action}]")
        } else {
            format!("{seq}. [{action}] {summary}")
        };
        if output.is_empty() {
            lines.push(head);
        } else {
            // Anything a previous step saw is masked before it rides the next
            // prompt, so a credential-shaped value never reaches the provider
            // twice.
            let (masked, _) = crate::privacy::sanitize(&cap_chars(output, 300));
            lines.push(format!("{head}\n{masked}"));
        }
    }
    Ok(if lines.is_empty() {
        "(no steps yet)".to_string()
    } else {
        lines.join("\n")
    })
}

/// The previous step's action tag, used to pick the adaptive tier. Error
/// steps are reported separately so repair always upgrades.
fn last_step_kind(state: &AppState, run_id: &str) -> Result<(String, bool), String> {
    let conn = db(state)?;
    let last: Option<String> = conn
        .query_row(
            "SELECT action FROM agent_steps WHERE run_id=?1 ORDER BY seq DESC LIMIT 1",
            [run_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match last {
        Some(action) => Ok((action.clone(), action == "error")),
        None => Ok((String::new(), false)),
    }
}

/// Reproducible-run record: keep which model actually answered a step. Empty
/// stays empty (adaptive routing off), so untouched runs look like before.
fn record_model(state: &AppState, run_id: &str, model: Option<&str>) {
    if let Some(model) = model.filter(|m| !m.is_empty()) {
        let _ = db(state).and_then(|conn|
            conn.execute(
                "UPDATE agent_runs SET model=?1 WHERE id=?2 AND model<>?1",
                params![model, run_id],
            ).map_err(|e| e.to_string())
        );
    }
}

/// Feed the goal plus complete history into the model and get the next action.
async fn step_once(state: &AppState, run: &RunRow) -> Result<bool, String> {
    if run_status(state, &run.id)?.as_deref() != Some(RUNNING) {
        return Ok(false); // cancelled while we were working
    }
    // Adaptive routing (off unless configured): routine inspection steps go to
    // the cheap tier; planning, editing and post-failure repair go strong.
    let (last_action, had_error) = last_step_kind(state, &run.id)?;
    let tier = crate::adaptive_tier(state, &last_action, had_error);
    let messages = build_messages(run, &build_history(state, &run.id)?);
    // call_model routes to the local runtime in Local AI mode and to the
    // remote router otherwise; a local miss never silently goes remote.
    let outcome = crate::call_model(state, messages, tier.as_deref()).await;
    let (reply, model_used) = match outcome {
        Ok((reply, _)) => (reply, tier.clone()),
        // Visible failover: a tier miss falls back to the configured base
        // model instead of killing a long run over one bad cheap pick.
        Err(tier_error) if tier.is_some() => {
            add_step(
                state,
                &run.id,
                "failover",
                "Tier call failed; retrying on the base model.",
                &cap_chars(&tier_error, 600),
            )?;
            let base_messages = build_messages(run, &build_history(state, &run.id)?);
            let (reply, _) = crate::call_model(state, base_messages, None).await?;
            (reply, None)
        }
        Err(error) => return Err(error),
    };
    record_model(state, &run.id, model_used.as_deref());
    // A cancel can land while the model is thinking: a stopped run must not
    // act on the reply that arrives afterwards.
    if run_status(state, &run.id)?.as_deref() != Some(RUNNING) {
        return Ok(false);
    };

    match parse_action(&reply) {
        ParsedAction::Ok(Action::Think { summary }) => {
            add_step(state, &run.id, "think", &summary, "")?;
        }
        ParsedAction::Ok(Action::Done { summary }) => {
            add_step(state, &run.id, "done", "Run complete", &summary)?;
            finish(state, &run.id, SUCCEEDED, "")?;
            return Ok(false);
        }
        ParsedAction::Ok(Action::Fail { reason }) => {
            add_step(state, &run.id, "fail", "Model gave up", &reason)?;
            finish(state, &run.id, FAILED, &reason)?;
            return Ok(false);
        }
        ParsedAction::Ok(action) => return execute_action_step(state, run, &action).await,
        ParsedAction::Malformed(error) => {
            add_step(
                state,
                &run.id,
                "unparsed",
                "Model reply was not a usable action; retrying with counts against retries.",
                &format!("{error}\nreply:\n{}", cap_chars(&reply, 600)),
            )?;
            let retries: i64 = db(state)?
                .query_row(
                    "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1 AND action='unparsed'",
                    [&run.id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if retries >= 3 {
                finish(state, &run.id, FAILED, "The model kept replying with unparseable text.")?;
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The model sometimes re-emits the exact action it just executed (most often
/// right after an approval). Repeating is waste at best and approval
/// ping-pong at worst: detect it from the execution log and skip straight
/// back to the model with a corrective note.
fn is_repeat_of_last_step(state: &AppState, run_id: &str, action: &Action) -> Result<bool, String> {
    let conn = db(state)?;
    match conn.query_row(
        "SELECT action, summary FROM agent_steps WHERE run_id=?1 \
         AND action IN ('command','write') ORDER BY seq DESC LIMIT 1",
        [run_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    ) {
        Ok((tag, summary)) => Ok(tag == action.tag() && summary == pending_summary(action)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

/// Performs one tool step: permission gate, approval gate, execute, log.
/// Returns false when the run reached a terminal state; every gate keeps the
/// execution log honest.
async fn execute_action_step(
    state: &AppState,
    run: &RunRow,
    action: &Action,
) -> Result<bool, String> {
    match PermissionGate::decides(run, action) {
        GateDecision::Denied(need) => {
            add_step(
                state,
                &run.id,
                action.tag(),
                &pending_summary(action),
                &format!("Attempted {need}, but this run started without that permission."),
            )?;
            finish(
                state,
                &run.id,
                FAILED,
                &format!(
                    "The model needed {need}, but the run started without that permission. Start a new run with it allowed."
                ),
            )?;
            return Ok(false);
        }
        GateDecision::NeedsApproval => {
            if is_repeat_of_last_step(state, &run.id, action)? {
                add_step(
                    state,
                    &run.id,
                    action.tag(),
                    &pending_summary(action),
                    "This exact action already ran — continuing instead of repeating it.",
                )?;
                return Ok(true);
            }
            let pending = action.to_json().to_string();
            add_step(
                state,
                &run.id,
                "approval-requested",
                &pending_summary(action),
                "Paused. Approve or reject in the Runs panel.",
            )?;
            set_run(state, &run.id, AWAITING_APPROVAL, None, Some(&pending))?;
            return Ok(false);
        }
        GateDecision::Allowed => {}
    }

    let root = state
        .workspace
        .root()
        .map_err(|error| error.message().to_string())?;
    let summary = pending_summary(action);
    // Snapshot before writing: the change is then previewable as a diff and
    // reversible from the run log.
    let (checkpoint, old_content) = match action {
        Action::Write { path, .. } => capture_checkpoint(&root, path),
        _ => (String::new(), None),
    };
    // Privacy: a read or listing of an excluded path is replaced by a notice,
    // so credential-bearing files never reach the provider in a run prompt.
    let exclusions = crate::load_config(state).privacy_exclusions.clone();
    let excluded_path = match action {
        Action::Read { path } | Action::List { path }
            if crate::privacy::is_excluded(path, &exclusions) =>
        {
            Some((action.tag(), path.clone()))
        }
        _ => None,
    };
    let outcome: Result<String, String> = match &excluded_path {
        Some((_, path)) => Ok(format!(
            "[blocked by privacy policy: {path} is on the outbound exclusion list; contents were not sent and are not shown here.]"
        )),
        None => dispatch_action(&root, action).await,
    };
    let output = match outcome {
        Ok(output) => {
            let diff = match action {
                Action::Write { path, content } => format!(
                    "\n--- diff ---\n{}",
                    unified_diff(old_content.as_deref(), content, path)
                ),
                _ => String::new(),
            };
            format!("{output}{diff}")
        }
        Err(error) => {
            add_step(
                state,
                &run.id,
                &format!("{}-error", action.tag()),
                &summary,
                &error,
            )?;
            // Not fatal: the next model step sees the error and can repair.
            return Ok(true);
        }
    };
    add_step_with_checkpoint(state, &run.id, action.tag(), &summary, &output, &checkpoint)?;
    // The next model step sees this result in the history.
    Ok(true)
}

/// Drives a run to a terminal state: keeps stepping the model until the goal
/// completes, approval is requested, the user cancels, or the hard step
/// ceiling is reached.
async fn run_loop(state: AppState, run_id: String) {
    while step_count(&state, &run_id).unwrap_or(0) < HARD_MAX_STEPS {
        let run = match load_run(&state, &run_id) {
            Ok(Some(run)) if run.status == RUNNING => run,
            Ok(_) => return, // cancelled, approved externally, or gone
            Err(_) => return,
        };
        match step_once(&state, &run).await {
            Ok(true) => notify(&state, Some(&run_id)),
            Ok(false) => break,
            Err(error) => {
                let database_ok = add_step(&state, &run_id, "error", "The model call failed:", &error).is_ok();
                if database_ok {
                    let _ = finish(&state, &run_id, FAILED, &error);
                }
                break;
            }
        }
    }
    // Bounded-loop guard: reached the ceiling without `done`.
    if let Ok(final_status) = run_status(&state, &run_id) {
        if final_status.as_deref() == Some(RUNNING) {
            let _ = add_step(
                &state,
                &run_id,
                "error",
                "",
                "Run stopped: the step ceiling was reached without completion.",
            );
            let _ = finish(&state, &run_id, FAILED, "Run exceeded the step ceiling.");
        }
    }
    ACTIVE.lock().unwrap().remove(&run_id);
    notify(&state, Some(&run_id));
}

/// Starts (or resumes) the loop for one run. Both a fresh run and an approval
/// resume land here.
fn spawn_run(state: AppState, run_id: String) {
    let flag = Arc::new(AtomicBool::new(false));
    ACTIVE.lock().unwrap().insert(run_id.clone(), flag.clone());
    tokio::spawn(run_loop(state, run_id));
}

// ---------------------------------------------------------------------------
// HTTP surface

pub(crate) fn router() -> Router<AppState> {
    run_routes("/api/runs")
}

/// The same run surface under the host-listener prefix: remote clients (VS
/// Code, Android) reach it behind the host bearer wall instead of local UI
/// auth.
pub(crate) fn host_router() -> Router<AppState> {
    run_routes("/api/host/v1/runs")
}

fn run_routes(prefix: &'static str) -> Router<AppState> {
    Router::new()
        .route(prefix, post(create_run).get(list_runs))
        .route(format!("{prefix}/:id").as_str(), get(get_run))
        .route(format!("{prefix}/:id/approve").as_str(), post(approve_run))
        .route(format!("{prefix}/:id/cancel").as_str(), post(cancel_run))
        .route(
            format!("{prefix}/:id/steps/:seq/restore").as_str(),
            post(restore_step),
        )
}

fn error_response(status: StatusCode, message: impl std::fmt::Display) -> Response {
    (status, Json(json!({ "error": message.to_string() }))).into_response()
}

/// The profile picker: what this run is FOR. An unknown name is rejected at
/// the door; an empty name is the default coding profile.
const PROFILES: [&str; 4] = ["coding", "planner", "testing", "review"];

fn normalize_profile(input: &str) -> String {
    let trimmed = input.trim().to_lowercase();
    if trimmed.is_empty() {
        return "coding".to_string();
    }
    if PROFILES.contains(&trimmed.as_str()) {
        trimmed
    } else {
        String::new()
    }
}

fn profile_role(profile: &str) -> &'static str {
    match profile {
        "planner" => "You are the Saarthi Studio PLANNER. Analyze the task and inspect the workspace as \
needed, then produce a plan. When the plan is complete, finish with done whose summary IS \
the plan (steps, likely files, dependencies). You may NOT write files or run commands.",
        "testing" => "You are the Saarthi Studio TESTING agent. Find and run the relevant tests for the \
goal, capture real exit codes and output, and separate infrastructure failures (missing \
tools or deps) from actual test failures. You may run commands but may NOT write files. \
Never claim tests passed unless a run said so.",
        "review" => "You are the Saarthi Studio REVIEWER. Inspect the current code and recent diffs, and \
report concrete findings (bugs, security risks, regressions, maintainability issues) with \
file paths and line references where possible. You may NOT write files or run commands. \
Finish with done whose summary is the findings list.",
        _ => "You are the Saarthi Studio autonomous coding agent working inside an open workspace \
folder. You complete the task one step at a time: inspect, plan, edit, verify, and keep \
going until the goal is actually met.",
    }
}

fn profile_rule(profile: &str, write: bool) -> &'static str {
    match (profile, write) {
        ("planner" | "review" | "testing", true) => {
            "write FORBIDDEN — this profile never modifies files"
        }
        ("planner" | "review", false) => {
            "command FORBIDDEN — this profile never runs commands"
        }
        _ => "",
    }
}

async fn create_run(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let goal = input.get("goal").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if goal.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "Describe what the agent should do.");
    }
    if goal.chars().count() > 5_000 {
        return error_response(StatusCode::BAD_REQUEST, "Goal is too long.");
    }
    let allow_writes = input.get("allowWrites").and_then(Value::as_bool).unwrap_or(false);
    let allow_commands = input.get("allowCommands").and_then(Value::as_bool).unwrap_or(false);
    let auto_approve = input.get("autoApprove").and_then(Value::as_bool).unwrap_or(false);
    let max_steps = input
        .get("maxSteps")
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_MAX_STEPS)
        .clamp(1, HARD_MAX_STEPS);
    let profile = normalize_profile(input.get("profile").and_then(Value::as_str).unwrap_or(""));
    if profile.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "Unknown agent profile.");
    }

    // The agent works inside the open workspace folder; without one there is
    // nothing safe to read or edit.
    if state.workspace.root().is_err() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Open a workspace folder first — agent runs only work inside an open workspace.",
        );
    }
    let project_id = crate::active_project_id(&state);
    let id = uuid::Uuid::new_v4().to_string();
    let stamp = now();
    // Reproducible-run record: freeze the routing config as it was when the
    // run started, so the run shows which model setup produced it.
    let (snapshot, base_model) = crate::run_record_snapshot(&state);
    if let Err(error) = db(&state).and_then(|conn|
        conn.execute(
            "INSERT INTO agent_runs(id,project_id,goal,status,allow_writes,allow_commands,auto_approve,max_steps,pending_action,error,created_at,updated_at,router_config,model,profile) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'','',?9,?9,?10,?11,?12)",
            params![
                id, project_id, goal, RUNNING,
                allow_writes, allow_commands, auto_approve, max_steps, stamp,
                snapshot, base_model, profile
            ],
        ).map_err(|e| e.to_string())
    ) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, error);
    }
    spawn_run(state.clone(), id.clone());
    notify(&state, Some(&id));
    Json(json!({ "ok": true, "run": { "id": id, "status": RUNNING } })).into_response()
}

async fn list_runs(State(state): State<AppState>) -> Response {
    let conn = match db(&state) {
        Ok(conn) => conn,
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let mut stmt = match conn.prepare(
        "SELECT r.id, r.project_id, r.goal, r.status, r.allow_writes, r.allow_commands, r.auto_approve, \
         r.max_steps, r.pending_action, r.error, r.created_at, r.updated_at, r.router_config, r.profile, \
         (SELECT COUNT(*) FROM agent_steps s WHERE s.run_id = r.id) AS steps \
         FROM agent_runs r ORDER BY r.created_at DESC LIMIT 50",
    ) {
        Ok(stmt) => stmt,
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };    let rows = stmt
        .query_map([], |row| {
            let run = row_to_run(row)?;
            let steps: i64 = row.get(15)?;

            Ok(run_json(&run, steps))
        })
        .map_err(|e| e.to_string());
    match rows {
        Ok(rows) => {
            let runs = rows.filter_map(|row| row.ok()).collect::<Vec<_>>();
            Json(json!({ "runs": runs })).into_response()
        }
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn get_run(State(state): State<AppState>, Path(run_id): Path<String>) -> Response {
    let run = match load_run(&state, &run_id) {
        Ok(Some(run)) => run,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "No such run."),
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let conn = match db(&state) {
        Ok(conn) => conn,
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let mut stmt = match conn
        .prepare("SELECT seq, action, summary, output, created_at, checkpoint FROM agent_steps WHERE run_id=?1 ORDER BY seq")
    {
        Ok(stmt) => stmt,
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let rows = match stmt
        .query_map([&run_id], |row| {
            Ok(json!({
                "seq": row.get::<_, i64>(0)?,
                "action": row.get::<_, String>(1)?,
                "summary": row.get::<_, String>(2)?,
                "output": row.get::<_, String>(3)?,
                "at": row.get::<_, f64>(4)?,
                "hasCheckpoint": !row.get::<_, String>(5)?.is_empty(),
            }))
        })
        .map_err(|e| e.to_string())
    {
        Ok(rows) => rows.filter_map(|row| row.ok()).collect::<Vec<_>>(),
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let mut payload = run_json(&run, rows.len() as i64);
    payload["stepList"] = json!(rows);
    // A pending write is previewed as a diff against the file on disk right
    // now, so approval happens with the actual change in view.
    if run.status == AWAITING_APPROVAL {
        if let Some(Action::Write { path, content }) = serde_json::from_str::<Value>(&run.pending_action)
            .ok()
            .and_then(|value| Action::from_json(&value).ok())
        {
            let current = state
                .workspace
                .root()
                .ok()
                .and_then(|root| crate::workspace::read_text_file(&root, &path).ok())
                .map(|file| file.content);
            payload["pendingDiff"] = json!(unified_diff(current.as_deref(), &content, &path));
        }
    }
    Json(payload).into_response()
}

/// Restore a written file to the content snapshotted before an agent wrote to
/// it. Long-run safety valve, not a full undo system: other files stay as the
/// run left them, and edits made after the step overwrite the restored content.
async fn restore_step(
    State(state): State<crate::AppState>,
    Path((run_id, seq)): Path<(String, i64)>,
) -> Response {
    match load_run(&state, &run_id) {
        Ok(Some(_)) => {}
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "No such run."),
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
    let checkpoint = match db(&state)
        .and_then(|conn| {
            conn.query_row(
                "SELECT checkpoint FROM agent_steps WHERE run_id=?1 AND seq=?2 AND action='write'",
                params![run_id, seq],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())
        }) {
        Ok(text) => text.unwrap_or_default(),
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    if checkpoint.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "This step has no checkpoint to restore.");
    }
    let parsed: Value = serde_json::from_str(&checkpoint).unwrap_or(Value::Null);
    let (path, old) = match (
        parsed.get("path").and_then(Value::as_str),
        parsed.get("old").and_then(Value::as_str),
    ) {
        (Some(path), Some(old)) => (path.to_string(), old.to_string()),
        _ => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Checkpoint is unreadable; nothing was restored.",
            )
        }
    };
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error.message().to_string()),
    };
    // Re-validate through the same path-confined write the run itself used.
    if let Err(error) = crate::workspace::write_text_file(&root, &path, &old) {
        return error_response(StatusCode::BAD_REQUEST, error.message().to_string());
    }
    let _ = add_step(
        &state,
        &run_id,
        "restored",
        &format!("restore {path}"),
        &format!(
            "Rolled back step {seq}: {path} now holds the {} bytes it had before the write.",
            old.len()
        ),
    );
    notify(&state, Some(&run_id));
    Json(json!({ "ok": true, "path": path })).into_response()
}

async fn cancel_run(State(state): State<AppState>, Path(run_id): Path<String>) -> Response {
    let run = match load_run(&state, &run_id) {
        Ok(Some(run)) => run,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "No such run."),
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    if !matches!(run.status.as_str(), RUNNING | AWAITING_APPROVAL) {
        return error_response(StatusCode::BAD_REQUEST, "Run is not running.");
    }
    // 1) wake the cancel flag so a live loop exits, 2) persist the state, and
    // the loop's status check will never overwrite a terminal state.
    if let Some(flag) = ACTIVE.lock().unwrap().get(&run_id) {
        flag.store(true, Ordering::Relaxed);
    }
    let outcome = finish(&state, &run_id, CANCELLED, "Cancelled by user.");
    notify(&state, Some(&run_id));
    match outcome {
        Ok(()) => Json(json!({ "ok": true, "status": CANCELLED })).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn approve_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    let approved = match input.get("approve").and_then(Value::as_bool) {
        Some(choice) => choice,
        None => return error_response(StatusCode::BAD_REQUEST, "Provide approve: true or false."),
    };
    let run = match load_run(&state, &run_id) {
        Ok(Some(run)) => run,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "No such run."),
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    if run.status != AWAITING_APPROVAL || run.pending_action.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "This run has no action waiting for approval.");
    }
    let action = match serde_json::from_str::<Value>(&run.pending_action)
        .ok()
        .ok_or_else(|| "unreadable".to_string())
        .and_then(|value| Action::from_json(&value))
    {
        Ok(action) => action,
        Err(error) => {
            let _ = finish(&state, &run_id, FAILED, "Approval data was unreadable.");
            notify(&state, Some(&run_id));
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, error);
        }
    };

    if !approved {
        let _ = add_step(&state, &run_id, "rejected", &pending_summary(&action), "Rejected by the user.");
        let _ = finish(&state, &run_id, FAILED, "You rejected the action the agent needed.");
        notify(&state, Some(&run_id));
        return Json(json!({ "ok": true, "status": FAILED })).into_response();
    }

    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => {
            let message = error.message().to_string();
            let _ = finish(&state, &run_id, FAILED, &format!("Workspace closed: {message}"));
            notify(&state, Some(&run_id));
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, message);
        }
    };
    let summary = pending_summary(&action);
    let (checkpoint, old_content) = match &action {
        Action::Write { path, .. } => capture_checkpoint(&root, path),
        _ => (String::new(), None),
    };
    match dispatch_action(&root, &action).await {
        Ok(output) => {
            let _ = set_run(&state, &run_id, RUNNING, None, Some(""));
            let output = match &action {
                Action::Write { path, content } => format!(
                    "{output}\n--- diff ---\n{}",
                    unified_diff(old_content.as_deref(), content, path)
                ),
                _ => output,
            };
            let _ = add_step_with_checkpoint(&state, &run_id, action.tag(), &summary, &output, &checkpoint);
            if let Err(error) = set_run(&state, &run_id, RUNNING, None, Some("")) {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, error);
            }
            spawn_run(state, run_id.clone());
            Json(json!({ "ok": true, "status": RUNNING })).into_response()
        }
        Err(error) => {
            let _ = finish(&state, &run_id, FAILED, &error);
            notify(&state, Some(&run_id));
            Json(json!({ "ok": false, "error": error })).into_response()
        }
    }
}

// ---------------------------------------------------------------------------

fn cap_chars(text: &str, limit: usize) -> String {
    let total = text.chars().count();
    if total <= limit {
        return text.to_string();
    }
    let cut: String = text.chars().take(limit).collect();
    format!("{cut}… [{overflow} chars truncated]", overflow = total - limit)
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_and_plain_json_replies() {
        let fenced = "Sure, here:\n```json\n{\"action\":\"read\",\"path\":\"src/main.rs\"}\n```";
        match parse_action(fenced) {
            ParsedAction::Ok(Action::Read { path }) => assert_eq!(path, "src/main.rs"),
            other => panic!("expected read, got {other:?}"),
        }
        let plain = "{\"action\":\"write\",\"path\":\"hi.txt\",\"content\":\"Hi there\"}";
        match parse_action(plain) {
            ParsedAction::Ok(Action::Write { path, content }) => {
                assert_eq!(path, "hi.txt");
                assert_eq!(content, "Hi there");
            }
            other => panic!("expected write, got {other:?}"),
        }
        let garbage = "{\"action\":\"teleport\",\"x\":3}";
        match parse_action(garbage) {
            ParsedAction::Malformed(text) => assert!(text.contains("invalid action")),
            other => panic!("expected malformed, got {other:?}"),
        }
        let broken = "{\"action\": wrong}";
        match parse_action(broken) {
            ParsedAction::Malformed(text) => assert!(text.contains("invalid JSON")),
            other => panic!("expected malformed, got {other:?}"),
        }
        let prose = "I will first list the project folder, then read main.rs.";
        match parse_action(prose) {
            ParsedAction::Ok(Action::Think { summary }) => assert!(summary.contains("list")),
            other => panic!("expected think, got {other:?}"),
        }
        match parse_action("   ") {
            ParsedAction::Malformed(text) => assert_eq!(text, "empty reply"),
            other => panic!("expected malformed, got {other:?}"),
        }
    }

    #[test]
    fn action_json_roundtrip_preserves_every_variant() {
        let actions = vec![
            Action::Think { summary: "checking".into() },
            Action::List { path: ".".into() },
            Action::Read { path: "a/b.rs".into() },
            Action::Write { path: "a.txt".into(), content: "héllo ★".into() },
            Action::Command { command: "cargo test".into() },
            Action::Done { summary: "all good".into() },
            Action::Fail { reason: "broken".into() },
        ];
        for action in actions {
            let json = action.to_json();
            assert_eq!(Action::from_json(&json).unwrap(), action);
        }
    }

    #[test]
    fn cap_chars_reports_the_true_overflow_and_never_slices_utf8() {
        let text = "xé".repeat(10);
        assert!(cap_chars(&text, 4).contains("[16 chars truncated]"));
        assert_eq!(cap_chars("hi", 4), "hi");
        let multibyte = "命運命運命運";
        let capped = cap_chars(multibyte, 3);
        assert!(capped.contains('…') && capped.ends_with(']'), "{capped}");
    }

    #[test]
    fn profile_gate_forces_plan_or_review_read_only() {
        let base = RunRow {
            id: "r".into(),
            project_id: "p".into(),
            goal: "g".into(),
            status: RUNNING.into(),
            allow_writes: true,
            allow_commands: true,
            auto_approve: true,
            max_steps: 5,
            pending_action: String::new(),
            error: String::new(),
            created_at: 0.0,
            updated_at: 0.0,
            router_config: String::new(),
            model: String::new(),
            profile: "coding".into(),
        };
        // Coding + every flag on: writes are allowed outright.
        assert!(matches!(
            PermissionGate::decides(&base, &Action::Write { path: "a".into(), content: "b".into() }),
            GateDecision::Allowed
        ));
        // Planner/review are read-only even with every checkbox ticked.
        for profile in ["planner", "review"] {
            let run = gated_run(profile);
            assert!(
                matches!(
                    PermissionGate::decides(&run, &Action::Write { path: "a".into(), content: "b".into() }),
                    GateDecision::Denied(_)
                ),
                "profile {profile} must not write"
            );
            assert!(
                matches!(
                    PermissionGate::decides(&run, &Action::Command { command: "ls".into() }),
                    GateDecision::Denied(_)
                ),
                "profile {profile} must not run commands"
            );
        }
        // Testing runs commands but never writes.
        let testing = gated_run("testing");
        assert!(matches!(
            PermissionGate::decides(&testing, &Action::Command { command: "cargo test".into() }),
            GateDecision::Allowed
        ));
        assert!(matches!(
            PermissionGate::decides(&testing, &Action::Write { path: "a".into(), content: "b".into() }),
            GateDecision::Denied(_)
        ));
    }

    /// Convenience: an all-allowed coding run with one profile swapped.
    fn gated_run(profile: &str) -> RunRow {
        RunRow {
            id: "r".into(),
            project_id: "p".into(),
            goal: "g".into(),
            status: RUNNING.into(),
            allow_writes: true,
            allow_commands: true,
            auto_approve: true,
            max_steps: 5,
            pending_action: String::new(),
            error: String::new(),
            created_at: 0.0,
            updated_at: 0.0,
            router_config: String::new(),
            model: String::new(),
            profile: profile.into(),
        }
    }

    #[test]
    fn unknown_profile_names_are_rejected() {
        assert_eq!(normalize_profile(""), "coding");
        assert_eq!(normalize_profile(" coding "), "coding");
        assert_eq!(normalize_profile("PLANNER"), "planner");
        assert_eq!(normalize_profile("root"), "");
    }

    #[test]
    fn permission_matrix_gate_writes_and_commands() {
        let run = RunRow {
            id: "r".into(),
            project_id: "p".into(),
            goal: "g".into(),
            status: RUNNING.into(),
            allow_writes: false,
            allow_commands: true,
            auto_approve: true,
            max_steps: 5,
            pending_action: String::new(),
            error: String::new(),
            created_at: 0.0,
            updated_at: 0.0,
            router_config: String::new(),
            model: String::new(),
            profile: "coding".into(),
        };
        assert!(!run.allow_writes);
        assert!(run.allow_commands);
        // The message should never merge what is forbidden and what is allowed.
        let messages = build_messages(&run, "history");
        assert!(messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("write FORBIDDEN"));
    }

    #[test]
    fn list_and_read_are_always_permitted_for_every_tool_run() {
        let run = RunRow {
            id: "r".into(),
            project_id: "p".into(),
            goal: "g".into(),
            status: RUNNING.into(),
            allow_writes: false,
            allow_commands: false,
            auto_approve: true,
            max_steps: 5,
            pending_action: String::new(),
            error: String::new(),
            created_at: 0.0,
            updated_at: 0.0,
            router_config: String::new(),
            model: String::new(),
            profile: "coding".into(),
        };
        assert!(matches!(
            PermissionGate::decides(&run, &Action::Read { path: "x".into() }),
            GateDecision::Allowed
        ));
        assert!(matches!(
            PermissionGate::decides(&run, &Action::List { path: ".".into() }),
            GateDecision::Allowed
        ));
    }

    #[test]
    fn interrupted_recovery_marks_live_rows_only() {
        let conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();
        for (id, status) in [
            ("a", RUNNING),
            ("b", AWAITING_APPROVAL),
            ("c", SUCCEEDED),
            ("d", FAILED),
            ("e", CANCELLED),
        ] {
            conn.execute(
                "INSERT INTO agent_runs(id,project_id,goal,status,allow_writes,allow_commands,auto_approve,max_steps,pending_action,error,created_at,updated_at) \
                 VALUES(?1, 'p', 'g', ?2, 0, 0, 0, 5, '', '', 1.0, 1.0)",
                params![id, status],
            )
            .unwrap();
        }
        assert_eq!(mark_interrupted_rows(&conn).unwrap(), 2);
        for (id, expected) in [("a", INTERRUPTED), ("b", INTERRUPTED), ("c", SUCCEEDED), ("d", FAILED), ("e", CANCELLED)] {
            let status: String = conn
                .query_row("SELECT status FROM agent_runs WHERE id=?1", [id], |r| r.get(0))
                .unwrap();
            assert_eq!(status, expected, "{id}");
        }
    }

    #[tokio::test]
    async fn run_command_captures_stdout_and_stderr() {
        let dir = std::env::temp_dir().join(format!("saarthi-agent-cmd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = FsPath::new(&dir);
        let output = run_command(&root, "echo marker-stdout; echo marker-stderr 1>&2").await.unwrap();
        assert!(output.contains("marker-stdout"), "{output}");
        assert!(output.contains("marker-stderr"), "{output}");
        assert!(output.contains("exit: 0"), "{output}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn run_command_reports_the_real_exit_code() {
        let dir = std::env::temp_dir().join(format!("saarthi-agent-cmd2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let output = run_command(&FsPath::new(&dir), "exit 7").await.unwrap();
        assert!(output.contains("exit: 7"), "{output}");
        std::fs::remove_dir_all(&dir).unwrap();
    }


    #[test]
    fn unified_diff_marks_new_modified_and_identical_files() {
        let new_file = unified_diff(None, "a\nb\nc\n", "x.txt");
        assert!(new_file.contains("-0,0"), "{new_file}");
        assert!(new_file.contains("+a"), "{new_file}");

        let old = "line1\nline2\nline3\nline4\nline5\n";
        let changed = "line1\nline2\nCHANGED\nline4\nline5\n";
        let diff = unified_diff(Some(old), changed, "y.txt");
        assert!(diff.contains("-line3"), "{diff}");
        assert!(diff.contains("+CHANGED"), "{diff}");
        assert!(diff.contains(" line1"), "{diff}"); // prefix context kept

        let appended = "line1\nline2\nline3\nline4\nline5\nline6\n";
        let diff = unified_diff(Some(old), appended, "y.txt");
        assert!(diff.contains("+line6"), "{diff}");
        assert!(!diff.contains("-line1"), "{diff}");

        assert!(unified_diff(Some(old), old, "y.txt").contains("identical"));
    }

    #[test]
    fn capture_checkpoint_snapshots_only_restorable_files() {
        let salt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let dir = std::env::temp_dir().join(format!("saarthi-agent-ckpt-{}", salt));
        std::fs::create_dir_all(&dir).unwrap();
        let root = FsPath::new(&dir);

        // Missing file: nothing was replaced, so there is nothing to roll back to.
        let (checkpoint, old) = capture_checkpoint(&root, "missing.txt");
        assert!(checkpoint.is_empty() && old.is_none());

        std::fs::write(dir.join("real.txt"), "previous content").unwrap();
        let (checkpoint, old) = capture_checkpoint(&root, "real.txt");
        assert_eq!(old.as_deref(), Some("previous content"));
        let parsed: Value = serde_json::from_str(&checkpoint).unwrap();
        assert_eq!(parsed["path"], "real.txt");
        assert_eq!(parsed["old"], "previous content");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn steps_persist_checkpoints_and_initialize_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        initialize(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO agent_runs(id,project_id,goal,status,allow_writes,allow_commands,auto_approve,max_steps,created_at,updated_at) VALUES('r','','t','running',0,0,0,12,0,0)",
            [],
        )
        .unwrap();
        insert_step(&mut conn, "r", "write", "w", "out", "{\"path\":\"a\",\"old\":\"b\"}").unwrap();
        insert_step(&mut conn, "r", "think", "t", "", "").unwrap();
        let (output, checkpoint): (String, String) = conn
            .query_row(
                "SELECT output, checkpoint FROM agent_steps WHERE action='write'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(output, "out");
        assert_eq!(checkpoint, "{\"path\":\"a\",\"old\":\"b\"}");
        // A second initialize must succeed against a database that already has the column.
        initialize(&mut conn).unwrap();
    }

}