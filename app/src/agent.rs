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
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
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
/// How many discovered tools one agent prompt may list.
const MCP_PROMPT_TOOLS: usize = 30;

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
    Think {
        summary: String,
    },
    List {
        path: String,
    },
    Read {
        path: String,
    },
    Write {
        path: String,
        content: String,
    },
    Command {
        command: String,
    },
    /// One call to a tool an MCP server advertises. The permission target is
    /// `server.tool`, the same string a rule is written against.
    Mcp {
        server: String,
        tool: String,
        arguments: Value,
    },
    Done {
        summary: String,
    },
    Fail {
        reason: String,
    },
}

impl Action {
    fn tag(&self) -> &'static str {
        match self {
            Action::Think { .. } => "think",
            Action::List { .. } => "list",
            Action::Read { .. } => "read",
            Action::Write { .. } => "write",
            Action::Command { .. } => "command",
            Action::Mcp { .. } => "mcp",
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
            Action::Mcp {
                server,
                tool,
                arguments,
            } => json!({
                "action": "mcp", "server": server, "tool": tool, "arguments": arguments
            }),
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
            // Reviewer findings echoed by the executor itself (it reads the
            // handoff and thinks it must review): findings are a report, so
            // treat them as a think step, not a parse strike.
            "think" | "review" | "review-issue" => Ok(Action::Think {
                summary: field("summary", value)?,
            }),
            "list" => Ok(Action::List {
                path: field("path", value)?,
            }),
            "read" => Ok(Action::Read {
                path: field("path", value)?,
            }),
            // Small executors say "create" for "write"; same meaning. A
            // create/write with no content is an incomplete reply, not an
            // executed step: report it and let the next turn do the real write.
            "write" | "create" if value.get("content").and_then(Value::as_str).is_none() => {
                Ok(Action::Think {
                    summary: "The file action carried no content; emitting a real write now."
                        .to_string(),
                })
            }
            "write" | "create" => Ok(Action::Write {
                path: field("path", value)?,
                content: field("content", value)?,
            }),
            "command" => Ok(Action::Command {
                command: field("command", value)?,
            }),
            // Arguments are passed through unchanged: the tool's own schema is
            // the authority, and a non-object here becomes a step error the
            // model can read instead of an unparseable reply.
            "mcp" | "mcp-call" | "mcp_call" => Ok(Action::Mcp {
                server: field("server", value)?,
                tool: field("tool", value)?,
                arguments: match value.get("arguments") {
                    None | Some(Value::Null) => json!({}),
                    Some(arguments) => arguments.clone(),
                },
            }),
            "done" => Ok(Action::Done {
                summary: field("summary", value)?,
            }),
            "fail" => Ok(Action::Fail {
                reason: field("reason", value)?,
            }),
            // Small executors echo the execution log back and emit observed
            // error tags ("write-error", …) or no-op reports ("verify") as
            // their action. Those are step reports, not requests: map them to
            // think steps so the loop continues instead of failing on
            // unparseable replies.
            "verify" => Ok(Action::Think {
                summary: field("summary", value).unwrap_or_else(|_| "Verify the result".into()),
            }),
            other if other.ends_with("-error") => Ok(Action::Think {
                summary: value
                    .get("summary")
                    .or_else(|| value.get("output"))
                    .and_then(Value::as_str)
                    .unwrap_or("Reporting an earlier step error.")
                    .to_string(),
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
            let raw = &trimmed[open..=close];
            // Small models sometimes wrap multi-line file content in Python
            // triple quotes, which is not JSON at all. Repair that shape
            // before anything else; real JSON never contains `"""`.
            let docs = repair_triple_quotes(raw);
            // Small models often emit raw newlines or tabs inside a string
            // value (file content), which is invalid JSON.
            let doc = match serde_json::from_str::<Value>(&docs) {
                Ok(doc) => doc,
                Err(first) => {
                    // Second chance: the control characters escaped.
                    let escaped = escape_raw_controls(&docs);
                    // Third chance for the 4B-class shape that defeats the
                    // triple-quote pairing: content as a Python-style value
                    // fence that itself contains docstrings (five `"""` per
                    // document). Rebuild the string value directly.
                    let rebuilt = escape_raw_controls(&repair_value_fence(raw).unwrap_or_default());
                    match serde_json::from_str::<Value>(&escaped)
                        .ok()
                        .or_else(|| serde_json::from_str::<Value>(&rebuilt).ok())
                    {
                        Some(doc) => doc,
                        None => return ParsedAction::Malformed(format!("invalid JSON: {first}")),
                    }
                }
            };
            match Action::from_json(&doc) {
                Ok(action) => ParsedAction::Ok(action),
                Err(error) => ParsedAction::Malformed(format!("invalid action: {error}")),
            }
        }
        _ if !trimmed.is_empty() => ParsedAction::Ok(Action::Think {
            summary: cap_chars(trimmed, 300),
        }),
        _ => ParsedAction::Malformed("empty reply".to_string()),
    }
}

/// Escape malformed JSON that small models emit: raw control characters and
/// unescaped quotes that sit inside string literals so a reply with
/// multi-line file content parses as the model meant it. Only runs as a
/// second chance — well-formed replies are parsed first, untouched.
fn repair_triple_quotes(raw: &str) -> String {
    if !raw.contains("\"\"\"") {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len() + 64);
    let mut rest = raw;
    while let Some(pos) = rest.find("\"\"\"") {
        out.push_str(&rest[..pos]);
        out.push('"'); // opening fence becomes a normal string start
        let after = &rest[pos + 3..];
        match after.find("\"\"\"") {
            Some(end) => {
                out.push_str(&escape_json_body(&after[..end]));
                out.push('"');
                rest = &after[end + 3..];
            }
            None => {
                // Unclosed fence: escape to the end of the document.
                out.push_str(&escape_json_body(after));
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Escape malformed JSON that small models emit: quotes and backslashes
/// escaped, control characters mapped to JSON escapes.
/// The 4B-class executors write file content as a Python-style value fence
/// (`"content": """` ... final `"""`) whose body itself contains docstrings,
/// so there are five `"""` per document and fence-pairing cannot recover the
/// original shape. The unambiguous anchors are the first fence in value
/// position and the very last fence in the document: rebuild the string
/// value from them with the body escaped.
fn repair_value_fence(doc: &str) -> Option<String> {
    // Anchor 1: the colon-space-fence of the content field; anchor 2: the
    // very last fence in the document — or, when the model never closes it
    // (a stray ```json fence follows instead), the boundary of that fence.
    let open = doc.find(": \"\"\"")?;
    let body_start = open + 5; // past `: """`
    let close = doc.rfind("\"\"\"")?;
    if close < body_start {
        // Unclosed open: the model ended the content at the next code fence.
        // Keep body up to that boundary, drop the trailing junk (the intent
        // is one action per reply), and undo the `\'` pairs it double-escaped.
        let boundary = doc[body_start..].find("\n```")?;
        let body = doc[body_start..body_start + boundary].replace("\\\'", "'");
        let tail = &doc[body_start + boundary..].trim_start();
        let tail = if tail.starts_with('}') { *tail } else { "}" };
        return Some(format!(
            "{}\"{}\"{}",
            &doc[..open + 2],
            escape_json_body(&body),
            tail
        ));
    }
    let body = escape_json_body(&doc[body_start..close]);
    Some(format!(
        "{}\"{}\"{}",
        &doc[..open + 2],
        body,
        &doc[close + 3..] // the JSON tail after the closing fence (e.g. `}`)
    ))
}

fn escape_json_body(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    for c in content.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

fn escape_raw_controls(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 32);
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string && escaped {
            escaped = false;
            out.push(c);
        } else if in_string {
            match c {
                '\\' => {
                    escaped = true;
                    out.push(c);
                }
                '"' => {
                    // A quote ending a real JSON string is always followed by
                    // a separator (`,`, `}`, `]`, `:`, whitespace, or the end
                    // of the document). A quote before a letter or digit
                    // therefore belongs inside the content — escape it.
                    let follows_separator = matches!(
                        chars.peek(),
                        Some(',')
                            | Some('}')
                            | Some(']')
                            | Some(':')
                            | Some('\n')
                            | Some('\r')
                            | Some('\t')
                            | Some(' ')
                            | None
                    );
                    if follows_separator {
                        in_string = false;
                        out.push(c);
                    } else {
                        out.push_str("\\\"");
                    }
                }
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                _ => out.push(c),
            }
        } else {
            out.push(c);
            if c == '"' {
                in_string = true;
            }
        }
    }
    out
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
        // "" is the normal single-model run; "hybrid" is cloud-plan/local-execute.
        "ALTER TABLE agent_runs ADD COLUMN mode TEXT NOT NULL DEFAULT ''",
        // Crew line-up for multi-agent runs (JSON array of role names).
        "ALTER TABLE agent_runs ADD COLUMN crew TEXT NOT NULL DEFAULT ''",
        // Crew stage budgets (Step 5): per-stage step ceiling and what to do
        // when a stage cannot finish inside it ("fail" is the default).
        "ALTER TABLE agent_runs ADD COLUMN stage_policy TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE agent_runs ADD COLUMN stage_max_steps INTEGER NOT NULL DEFAULT 0",
        // MCP tool use is opt-in per run, like writes and commands: a run that
        // was not started with it can never call a server tool.
        "ALTER TABLE agent_runs ADD COLUMN allow_mcp INTEGER NOT NULL DEFAULT 0",
        // The catalogue discovered at run start (JSON: tools + errors), so a
        // step prompt never has to spawn a server to describe it.
        "ALTER TABLE agent_runs ADD COLUMN mcp_tools TEXT NOT NULL DEFAULT ''",
        // Tokens this run's model calls reported. Summed as the run works, so the
        // monitor can show a per-agent total without re-reading every request.
        "ALTER TABLE agent_runs ADD COLUMN tokens INTEGER NOT NULL DEFAULT 0",
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
    /// "" for a normal run, "hybrid" when the cloud plans and the local
    /// runtime executes, "team" for the executor→reviewer pipeline, "crew"
    /// for a sequential multi-agent chain of specialist stages.
    mode: String,
    /// Crew line-up for crew runs: JSON list of role names ("" elsewhere).
    crew: String,
    /// What happens when a crew stage hits its budget: ""/"fail" stops the
    /// run honestly, "skip" records the incomplete stage and moves on.
    stage_policy: String,
    /// Per-stage step ceiling for crew runs (0 = disabled).
    stage_max_steps: i64,
    /// Whether this run may call tools on registered MCP servers.
    allow_mcp: bool,
    /// Catalogue snapshot taken at run start (empty until discovery runs).
    mcp_tools: String,
    /// Total tokens the run's own model calls reported.
    tokens: i64,
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
        mode: row.get(15)?,
        crew: row.get(16)?,
        stage_policy: row.get(17)?,
        stage_max_steps: row.get(18)?,
        allow_mcp: row.get(19)?,
        mcp_tools: row.get(20)?,
        tokens: row.get(21)?,
    })
}

/// Add the tokens a run's own model call reported to its running total. Fire and
/// forget: the monitor is a convenience, and a failed count must never fail a
/// run that otherwise worked.
fn record_tokens(state: &AppState, run_id: &str, meta: &Value) {
    let tokens = meta
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if tokens <= 0 {
        return;
    }
    if let Ok(conn) = db(state) {
        let _ = conn.execute(
            "UPDATE agent_runs SET tokens=tokens+?1 WHERE id=?2",
            rusqlite::params![tokens, run_id],
        );
    }
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
             max_steps, pending_action, error, created_at, updated_at, router_config, model, profile, mode, crew, stage_policy, stage_max_steps, allow_mcp, mcp_tools, tokens FROM agent_runs WHERE id=?1",
        )
        .map_err(|e| e.to_string())?;
    stmt.query_row([run_id], row_to_run)
        .optional()
        .map_err(|e| e.to_string())
}

fn step_count(state: &AppState, run_id: &str) -> Result<i64, String> {
    db(state)?
        .query_row(
            "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1",
            [run_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())
}

fn run_status(state: &AppState, run_id: &str) -> Result<Option<String>, String> {
    db(state)?
        .query_row(
            "SELECT status FROM agent_runs WHERE id=?1",
            [run_id],
            |row| row.get(0),
        )
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
        "profile": row.profile, "mode": row.mode,
        "stagePolicy": row.stage_policy, "stageMaxSteps": row.stage_max_steps,
        "allowMcp": row.allow_mcp, "mcpTools": mcp_tool_count(&row.mcp_tools),
        "tokens": row.tokens,
        // A live run has no finished duration yet: measure from its start and say
        // how long it has been quiet, rather than reporting "0 s" for a run that
        // started ten minutes ago.
        "elapsedSeconds": elapsed_seconds(row),
        "idleSeconds": (now() - row.updated_at).max(0.0),
        "crew": serde_json::from_str::<Value>(&row.crew)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
    })
}

/// How long a run has been going: measured from its start while it is live, and
/// from its last write once it has stopped.
fn elapsed_seconds(row: &RunRow) -> f64 {
    let end = match row.status.as_str() {
        "running" | "awaiting_approval" | "paused" => now(),
        _ => row.updated_at,
    };
    (end - row.created_at).max(0.0)
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
        Action::Mcp {
            server,
            tool,
            arguments,
        } => {
            let rendered = arguments.to_string();
            if rendered == "{}" {
                format!("call MCP tool {server}.{tool}")
            } else {
                format!(
                    "call MCP tool {server}.{tool} with {}",
                    cap_chars(&rendered, 200)
                )
            }
        }
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
    while start < old_lines.len() && start < new_lines.len() && old_lines[start] == new_lines[start]
    {
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
        Action::Write { path, content } => {
            // Small executors write into subfolders that do not exist yet and
            // then stall on "parent does not exist" errors. Write permission
            // already covers creating the parent inside the workspace, so
            // create it here instead of failing the step. Only plain-name
            // components are accepted, so the folder can never escape root.
            if let Some(parent) = std::path::Path::new(path).parent() {
                let plain = !parent.as_os_str().is_empty()
                    && parent
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)));
                if plain {
                    let target = root.join(parent);
                    if !target.is_dir() {
                        std::fs::create_dir_all(&target)
                            .map_err(|e| format!("Could not create the parent folder: {e}"))?;
                    }
                }
            }
            crate::workspace::write_text_file(root, path, content)
                .map(|file| format!("Wrote {} bytes to {}.", file.content.len(), file.path))
                .map_err(|e| e.message().to_string())
        }
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
                    .map(|e| {
                        if e.dir {
                            format!("{}/", e.path)
                        } else {
                            e.path.clone()
                        }
                    })
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
        Action::Mcp { .. } => {
            Err("MCP actions run through the MCP path, not this one.".to_string())
        }
        _ => Err("Only list, read, write and command actions execute.".to_string()),
    }
}

/// Executes one non-terminal action. Workspace actions go through the
/// path-confined helpers; an MCP call goes through the same `invoke` the HTTP
/// route uses, so validation, argument checks and the audit trail are shared.
async fn dispatch_step(state: &AppState, root: &FsPath, action: &Action) -> Result<String, String> {
    match action {
        Action::Mcp {
            server,
            tool,
            arguments,
        } => match crate::mcp::invoke(state, server, tool, arguments).await {
            Ok(payload) => {
                let text = payload
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let is_error = payload
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Ok(format!(
                    "{}via MCP {server}.{tool}\n---\n{}",
                    if is_error {
                        "the tool reported an error\n"
                    } else {
                        ""
                    },
                    cap_chars(&text, STEP_OUTPUT_CAP)
                ))
            }
            Err(failure) => Err(failure.message().to_string()),
        },
        other => dispatch_action(root, other).await,
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
        // Profile is the authority: inspectors (planner, review) run read-only
        // by design — writes and commands are denied before the model even
        // reaches them, no matter which checkboxes were ticked. Builders and
        // running inspectors get their role's tools.
        if let Action::Write { .. } | Action::Command { .. } = action {
            if !profile_can_write(&run.profile) {
                if let Action::Write { .. } = action {
                    return GateDecision::Denied("file writes in this profile");
                }
                if !profile_can_command(&run.profile) {
                    return GateDecision::Denied("command execution in this profile");
                }
            }
        }
        match action {
            Action::Write { .. } if !run.allow_writes => GateDecision::Denied("file writes"),
            Action::Command { .. } if !run.allow_commands => {
                GateDecision::Denied("command execution")
            }
            // An MCP call starts a child process and can do anything that
            // process does, so it is opt-in per run and a pause by default —
            // exactly like a command. A deny rule on `server.tool` is applied
            // before this gate ever runs.
            Action::Mcp { .. } if !run.allow_mcp => GateDecision::Denied("MCP tool calls"),
            Action::Write { .. } | Action::Command { .. } | Action::Mcp { .. }
                if !run.auto_approve =>
            {
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
    // The mcp action is only advertised when the run was started with it AND
    // discovery actually found a tool: telling the model about an action that
    // would be refused is worse than saying nothing.
    let mcp_block = if run.allow_mcp {
        mcp_catalog_block(&run.mcp_tools)
    } else {
        None
    };
    let mcp_rule = match (&mcp_block, run.allow_mcp) {
        (Some(_), _) if run.auto_approve => "mcp (enabled for this run)",
        (Some(_), _) => "mcp (each call first waits for user approval)",
        (None, true) => "mcp (no tool was discovered — do not emit it)",
        (None, false) => "mcp FORBIDDEN — never emit it",
    };
    let mcp_action_line = if mcp_block.is_some() {
        "{\"action\":\"mcp\",\"server\":\"<server>\",\"tool\":\"<tool>\",\"arguments\":{}} call one MCP tool from the list below (arguments follow its schema)\n"
    } else {
        ""
    };
    let mut messages = vec![
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
                 {{\"action\":\"command\",\"command\":\"echo ready\"}} run a shell command in the workspace\n\
                 {mcp_action_line}\
                 {{\"action\":\"done\",\"summary\":\"...\"}} the goal is met and you verified it\n\
                 {{\"action\":\"fail\",\"reason\":\"...\"}} you cannot meet the goal\n\n\
                 Permitted tools for this run: read: always; write: {write_rule}; command: {command_rule}; mcp: {mcp_rule}.",
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
    ];
    // The discovered tools ride along as their own message, so the action list
    // stays short and the catalogue is easy to read.
    if let Some(catalog) = mcp_block {
        messages.push(json!({ "role": "system", "content": catalog }));
    }
    messages
}

/// How many tools the run's snapshot has, for the run card. A corrupt or
/// absent snapshot is zero, never an error the UI has to understand.
fn mcp_tool_count(snapshot: &str) -> usize {
    serde_json::from_str::<Value>(snapshot)
        .ok()
        .and_then(|parsed| parsed.get("tools").and_then(Value::as_array).map(Vec::len))
        .unwrap_or(0)
}

/// How many refreshes a run has already spent. The count lives in the snapshot,
/// so a resumed run keeps the allowance it used; a snapshot written before
/// refreshes existed — or one that cannot be parsed — reads as none used.
const MCP_REFRESHES_PER_RUN: u64 = 3;

fn mcp_refresh_count(snapshot: &str) -> u64 {
    serde_json::from_str::<Value>(snapshot)
        .ok()
        .and_then(|parsed| parsed.get("refreshes").and_then(Value::as_u64))
        .unwrap_or(0)
}

/// The catalogue as prompt text: server.tool, a capped description and the
/// required argument names. None when nothing was discovered, so a run with no
/// usable tool is not told about an action it cannot take.
fn mcp_catalog_block(snapshot: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(snapshot).ok()?;
    let tools = parsed.get("tools").and_then(Value::as_array)?;
    if tools.is_empty() {
        return None;
    }
    let mut lines =
        vec!["MCP tools this run may call (server.tool — required arguments):".to_string()];
    for tool in tools.iter().take(MCP_PROMPT_TOOLS) {
        let Some(server) = tool.get("server").and_then(Value::as_str) else {
            continue;
        };
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let required = tool
            .get("required")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let mut line = format!("- {server}.{name}");
        if !description.is_empty() {
            line.push_str(&format!(": {}", cap_chars(description, 160)));
        }
        if !required.is_empty() {
            line.push_str(&format!(" (required: {required})"));
        }
        lines.push(line);
    }
    if tools.len() > MCP_PROMPT_TOOLS {
        lines.push(format!(
            "... and {} more tool(s) not shown.",
            tools.len() - MCP_PROMPT_TOOLS
        ));
    }
    if let Some(errors) = parsed.get("errors").and_then(Value::as_array) {
        if !errors.is_empty() {
            let listed = errors
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; ");
            lines.push(format!(
                "Unavailable MCP servers: {}",
                cap_chars(&listed, 400)
            ));
        }
    }
    Some(lines.join("\n"))
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

// ---------------------------------------------------------------------------
// Hybrid runs: cloud plans, local executes with bounded escalation

/// Hybrid phase 1: the cloud model turns the goal into an implementation
/// plan. The plan is logged as a `plan` step, so it reaches the executor
/// through the ordinary step history — no chat history is transferred at all,
/// only the goal, the workspace outline and the step log.
/// A resumed run reuses its existing plan instead of paying for a new one.
async fn plan_with_cloud(state: &AppState, run: &RunRow) -> Result<(), String> {
    let has_plan: bool = db(state)?
        .query_row(
            "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1 AND action='plan'",
            [&run.id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())?
        > 0;
    if has_plan {
        return Ok(());
    }
    let outline = state.workspace.outline().unwrap_or_default();
    let messages = vec![
        json!({
            "role": "system",
            "content": "You are the Saarthi Studio PLANNER running in the cloud. You cannot read \
        files or run commands. Given a coding goal and a workspace outline, reply with a compact \
        implementation plan: numbered steps, one line each, naming the exact files to create or edit \
        and the command that verifies the result. No preamble, no code blocks, at most 40 lines."
        }),
        json!({
            "role": "user",
            "content": format!(
                "Workspace goal:\n{}\n\nWorkspace files:\n{}\n\nReply with the plan only.",
                run.goal,
                if outline.is_empty() { "(no workspace outline available)" } else { &outline }
            )
        }),
    ];
    // Always the cloud: a local model planning its own work defeats the
    // split this mode exists for.
    let (plan, meta) = crate::call_router(state, messages, None).await?;
    record_tokens(state, &run.id, &meta);
    let plan = plan.trim();
    if plan.is_empty() {
        return Err("The cloud model returned an empty plan.".to_string());
    }
    add_step(
        state,
        &run.id,
        "plan",
        "Cloud plan ready — the local model executes it.",
        plan,
    )?;
    notify(state, Some(&run.id));
    Ok(())
}

/// Hybrid phase 2: one execution step on the local runtime. When the local
/// call fails, the cloud is asked for guidance once, that exchange is logged
/// as an `escalate` step, and the local model retries the same step with the
/// guidance in view. A second failure fails the run honestly instead of
/// looping.
async fn hybrid_execute(
    state: &AppState,
    run: &RunRow,
    messages: Vec<Value>,
) -> Result<(String, String), String> {
    let cfg = crate::load_config(state);
    let model = cfg.local_model.trim().to_string();
    if model.is_empty() {
        return Err(
            "Hybrid mode needs a local model: pick one on the Local AI page, then start a new run."
                .to_string(),
        );
    }
    // Small executors drift: a 1.5B model copies the placeholder payloads
    // from the action list verbatim and treats `read` of a not-yet-created
    // file as fatal. Repeating the ground rules costs a few tokens and
    // removes the most common wasted turns. (Pushed for every mode in
    // step_once now; see PLACEHOLDER_RULES.)
    let base = crate::local::effective_base_url(&cfg.local_base_url);
    match crate::local::chat_once(&base, &model, &messages).await {
        Ok((reply, _)) => Ok((reply, format!("local:{model}"))),
        Err(local_error) => {
            // Stuck locally: buy one piece of cloud guidance; the work stays
            // on this machine.
            let guidance = match ask_cloud_for_guidance(state, run, &local_error).await {
                Ok(guidance) => guidance,
                Err(cloud_error) => {
                    return Err(format!(
                        "{local_error} (the cloud guidance call also failed: {cloud_error})"
                    ))
                }
            };
            add_step(
                state,
                &run.id,
                "escalate",
                "Local step failed — the cloud model was asked for guidance.",
                &format!(
                    "local error: {}\n\nguidance:\n{}",
                    cap_chars(&local_error, 800),
                    cap_chars(&guidance, 2_000)
                ),
            )?;
            let mut retry = messages;
            retry.push(json!({
                "role": "user",
                "content": format!(
                    "Your previous step could not run: {}\n\nCloud guidance for this step:\n{}\n\n\
                     Emit the next single JSON action now, applying this guidance.",
                    cap_chars(&local_error, 800),
                    cap_chars(&guidance, 2_000)
                )
            }));
            match crate::local::chat_once(&base, &model, &retry).await {
                Ok((reply, _)) => Ok((reply, format!("local:{model}"))),
                Err(second) => Err(format!(
                    "The local model failed twice on this step: {second}"
                )),
            }
        }
    }
}

/// One bounded guidance request to the cloud, scoped to the current step: the
/// goal, the capped step log and the local error only.
async fn ask_cloud_for_guidance(
    state: &AppState,
    run: &RunRow,
    local_error: &str,
) -> Result<String, String> {
    let history = build_history(state, &run.id)?;
    let messages = vec![
        json!({
            "role": "system",
            "content": "You are the Saarthi Studio cloud advisor. A small local model is \
        executing an approved plan and got stuck. Reply with at most 6 short bullet points of \
        concrete guidance for its next step: what to read, what to change and how to verify. \
        No code dumps."
        }),
        json!({
            "role": "user",
            "content": format!(
                "Goal:\n{}\n\nExecution log:\n{}\n\nLocal error:\n{}",
                run.goal,
                cap_chars(&history, 4_000),
                cap_chars(local_error, 1_000)
            )
        }),
    ];
    let (guidance, meta) = crate::call_router(state, messages, None).await?;
    record_tokens(state, &run.id, &meta);
    let guidance = guidance.trim().to_string();
    if guidance.is_empty() {
        return Err("The cloud model returned empty guidance.".to_string());
    }
    Ok(guidance)
}

/// The previous step's action tag, used to pick the adaptive tier. Error
/// steps are reported separately so repair always upgrades.
/// Team orchestration after the executor claims done: the run switches to the
/// REVIEWER agent (its role prompt, through the same shared model router —
/// every role shares one router, nothing installs a private model). Verdicts:
/// APPROVE finishes the run; a first ISSUES verdict hands the findings back to
/// the executor for one repair round through the persisted step log; a second
/// ISSUES verdict ends the run with the open findings recorded, never hidden.
/// When the executor claimed `fail`, the honest terminal status is FAILED even
/// if the pipeline is otherwise identical.
async fn team_review_done(
    state: &AppState,
    run: &RunRow,
    summary: &str,
    executor_failed: bool,
) -> Result<bool, String> {
    let end_status = if executor_failed { FAILED } else { SUCCEEDED };
    let rounds: i64 = db(state)?
        .query_row(
            "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1 AND action='review'",
            [&run.id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if rounds >= 2 {
        // Both rounds failed the result: report that, do not loop forever.
        add_step(
            state,
            &run.id,
            if executor_failed { "fail" } else { "done" },
            if executor_failed {
                "Run stopped on the executor's fail plus unresolved review findings"
            } else {
                "Run complete with unresolved review findings"
            },
            &format!(
                "{summary}\nTwo review rounds reported findings and the run stopped there. \
                 Every finding is in the review steps of this log."
            ),
        )?;
        finish(state, &run.id, end_status, "")?;
        return Ok(false);
    }
    let reply = call_reviewer(state, run).await?;
    let (approved, findings) = interpret_verdict(&reply);
    if approved {
        add_step(
            state,
            &run.id,
            "review",
            "The REVIEWER approved the executor's result.",
            &reply,
        )?;
        add_step(
            state,
            &run.id,
            "done",
            "Run complete (reviewer-approved)",
            summary,
        )?;
        // The reviewer verified against the goal, so its approval also
        // overrides the executor's failure claim.
        finish(state, &run.id, SUCCEEDED, "")?;
        return Ok(false);
    }
    add_step(
        state,
        &run.id,
        "review",
        "The REVIEWER agent reports findings:",
        &findings,
    )?;
    if rounds == 0 {
        add_step(
            state,
            &run.id,
            "handoff",
            "Findings handed back to the EXECUTOR for one repair round.",
            "Fix every finding above, re-verify the result, and only then claim done again.",
        )?;
        return Ok(true); // the loop continues: the executor acts on the findings
    }
    add_step(
        state,
        &run.id,
        if executor_failed { "fail" } else { "done" },
        if executor_failed {
            "Run stopped on the executor's fail plus unresolved review findings"
        } else {
            "Run complete with unresolved review findings"
        },
        &format!(
            "{summary}\nThe second review round still reported findings. They are kept in \
             this log verbatim; nothing was marked fixed that was not fixed."
        ),
    )?;
    finish(state, &run.id, end_status, "")?;
    Ok(false)
}

/// The REVIEWER agent sees the goal and the full persisted execution log and
/// answers with APPROVE or ISSUES. Same call_model surface as every other
/// agent step: the shared model router decides which provider serves it.
async fn call_reviewer(state: &AppState, run: &RunRow) -> Result<String, String> {
    let history = build_history(state, &run.id)?;
    let messages = vec![
        json!({
            "role": "system",
            "content": format!(
                "{}\n\nYou are reviewing one finished agent run. Start your reply with the \
                 single word APPROVE (when the goal is genuinely met) or ISSUES (followed by \
                 each concrete, checkable finding). No verdict word, no approval.",
                profile_role("review")
            ),
        }),
        json!({
            "role": "user",
            "content": format!(
                "The agent was asked:\n{}\n\nFull execution log:\n{}\n\nVerify the \
                 result against the goal — read what you need, run what you may — and \
                 give your verdict.",
                run.goal,
                cap_chars(&history, 4_000)
            )
        }),
    ];
    let (reply, _meta) = crate::call_model(state, messages, None).await?;
    if reply.trim().is_empty() {
        return Err("The reviewer agent returned an empty reply.".into());
    }
    Ok(reply)
}

/// Read the reviewer's verdict word from its reply. Anything ambiguous counts
/// as ISSUES: an unverified approval would be a lie, an extra repair round is
/// only a slower run.
fn interpret_verdict(reply: &str) -> (bool, String) {
    // Markdown fences are plausible model output; opening backticks and any
    // newlines before the verdict word do not change it.
    let body = reply.trim().trim_start_matches(['`', '\n', '\r', ' ']);
    let head = body.chars().take(48).collect::<String>().to_uppercase();
    if head.starts_with("APPROVE") {
        (true, reply.trim().to_string())
    } else {
        (false, reply.trim().to_string())
    }
}

/// A run seen through one agent role: same row, different specialist. Used by
/// crew stages so prompts and tool gates follow the acting agent, not the
/// run's default profile.
fn view_run(run: &RunRow, profile: &str) -> RunRow {
    RunRow {
        id: run.id.clone(),
        project_id: run.project_id.clone(),
        goal: run.goal.clone(),
        status: run.status.clone(),
        allow_writes: run.allow_writes,
        allow_commands: run.allow_commands,
        auto_approve: run.auto_approve,
        max_steps: run.max_steps,
        pending_action: run.pending_action.clone(),
        error: run.error.clone(),
        created_at: run.created_at,
        updated_at: run.updated_at,
        router_config: run.router_config.clone(),
        model: run.model.clone(),
        profile: profile.to_string(),
        mode: run.mode.clone(),
        crew: run.crew.clone(),
        stage_policy: run.stage_policy.clone(),
        stage_max_steps: run.stage_max_steps,
        allow_mcp: run.allow_mcp,
        mcp_tools: run.mcp_tools.clone(),
        tokens: run.tokens,
    }
}

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
        let _ = db(state).and_then(|conn| {
            conn.execute(
                "UPDATE agent_runs SET model=?1 WHERE id=?2 AND model<>?1",
                params![model, run_id],
            )
            .map_err(|e| e.to_string())
        });
    }
}

/// Ground rules every executor gets, in every mode: the example payloads in
/// the action list keep being echoed literally by models that think they are
/// real values. Stated here once and shown each step — a few tokens against
/// the most common wasted turns.
const PLACEHOLDER_RULES: &str = "Working rules: the example values in the action list \
(relative/path, echo ready) are placeholders — replace them with real \
file names in this workspace and real shell commands. A file that does not exist \
yet is expected: create it with the write action BEFORE running any command that \
needs it. One small step per reply.";

/// Feed the goal plus complete history into the model and get the next action.
async fn step_once(state: &AppState, run: &RunRow) -> Result<bool, String> {
    if run_status(state, &run.id)?.as_deref() != Some(RUNNING) {
        return Ok(false); // cancelled while we were working
    }
    // Crew orchestration: the log itself is the orchestrator state. Stage k
    // (number of recorded handoffs) decides which specialist agent acts, so
    // prompts and tool gates always follow the stage, and the handoff steps
    // hand work from one agent to the next through shared history.
    let crew: Vec<String> = if run.mode == "crew" {
        serde_json::from_str(&run.crew).unwrap_or_default()
    } else {
        Vec::new()
    };
    let handoffs: i64 = if run.mode == "crew" {
        db(state)?
            .query_row(
                "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1 AND action='handoff'",
                [&run.id],
                |r| r.get(0),
            )
            .unwrap_or(0)
    } else {
        0
    };
    let staged_run = view_run(
        run,
        crew.get(crew_stage_index(handoffs, crew.len()))
            .unwrap_or(&run.profile),
    );
    let mut messages = build_messages(&staged_run, &build_history(state, &run.id)?);
    messages.push(json!({ "role": "system", "content": PLACEHOLDER_RULES }));
    // Team handoff: the run just came back from the REVIEWER with findings.
    // The step log already carries them; this message tells the executor what
    // to do with them so it does not re-claim done immediately.
    if run.mode == "team" {
        let (last, _) = last_step_kind(state, &run.id)?;
        if last == "review" || last == "handoff" {
            messages.push(json!({
                "role": "user",
                "content": "Your earlier result was reviewed and got findings (see the review step in the log). Address every finding now by writing real, fixed file content — never write instructions or issue summaries into the file — then re-verify and only then claim done again. Do not re-issue the same done summary."
            }));
        }
    }
    // Crew stage start: the model must know whose job it is now. Findings and
    // prior work ride on in the log; only the role boundary needs stating.
    if run.mode == "crew" {
        let (last, _) = last_step_kind(state, &run.id)?;
        if last == "handoff" {
            messages.push(json!({
                "role": "user",
                "content": format!(
                    "You are the {} agent in a crew. Earlier stages' actions and results are in the step log. Do your role's part of the goal now, and finish with done when your stage's work is complete — not the whole task unless your role is the last builder.",
                    staged_run.profile
                ),
            }));
        }
    }
    // Hybrid runs execute on the local runtime; the cloud was already used
    // for the plan. Every other run keeps the existing single-model path.
    let (reply, model_used) = if run.mode == "hybrid" {
        let (reply, model) = hybrid_execute(state, run, messages.clone()).await?;
        // A model that declares `fail` is just as stuck as one whose local
        // call errored out: buy the same one-shot piece of cloud guidance and
        // retry, unless this run already spent its rescue.
        let escalate_so_far: i64 = db(state)?
            .query_row(
                "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1 AND action='escalate'",
                [&run.id],
                |r| r.get(0),
            )
            .unwrap_or(i64::MAX);
        let executed_so_far = executed_tool_steps(state, &run.id).unwrap_or(1);
        let stuck_reason = match parse_action(&reply) {
            ParsedAction::Ok(Action::Fail { reason }) => Some(format!(
                "The local model gave up without completing the task: {reason}"
            )),
            // A `done` with no executed tool step is an unverified claim: no
            // file was written and no command ran. Hold the run to the plan.
            ParsedAction::Ok(Action::Done { .. }) if executed_so_far == 0 => Some(
                "The local model claimed done but executed no plan action yet: no file was written, no command was run and no MCP tool was called."
                    .to_string(),
            ),
            _ => None,
        };
        match stuck_reason {
            Some(reason) if escalate_so_far == 0 => {
                let guidance = ask_cloud_for_guidance(state, run, &reason).await?;
                add_step(
                    state,
                    &run.id,
                    "escalate",
                    "Local model stuck — the cloud model was asked for guidance.",
                    &format!(
                        "reason: {}\n\nguidance:\n{}",
                        cap_chars(&reason, 800),
                        cap_chars(&guidance, 2_000)
                    ),
                )?;
                let mut retry = messages;
                retry.push(json!({
                    "role": "user",
                    "content": format!(
                        "Since your last step did not advance the task ({}).\n\nCloud guidance for getting unstuck:\n{}\n\nEmit the next single JSON action now, applying this guidance. Do not claim done until an executed action (file write or verified command) actually implements the plan.",
                        cap_chars(&reason, 800),
                        cap_chars(&guidance, 2_000)
                    )
                }));
                // If the guided retry still errors out, the run fails honestly.
                let (retry_reply, _) = hybrid_execute(state, run, retry).await?;
                (retry_reply, Some(model))
            }
            _ => (reply, Some(model)),
        }
    } else {
        // Adaptive routing (off unless configured): routine inspection steps
        // go to the cheap tier; planning, edits and repair go strong.
        let (last_action, had_error) = last_step_kind(state, &run.id)?;
        let tier = crate::adaptive_tier(state, &last_action, had_error);
        // call_model routes to the local runtime in Local AI mode and to the
        // remote router otherwise; a local miss never silently goes remote.
        let outcome = crate::call_model(state, messages, tier.as_deref()).await;
        match outcome {
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
        }
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
            // Crew advancing: a finished stage hands off to the next role and
            // the run continues; only the last stage's done ends the run.
            if run.mode == "crew" && handoffs + 1 < crew.len() as i64 {
                let next_role = crew[(handoffs + 1) as usize].clone();
                add_step(
                    state,
                    &run.id,
                    "handoff",
                    &format!(
                        "Stage {} finished; {} agent takes over.",
                        staged_run.profile, next_role
                    ),
                    &summary,
                )?;
                notify(state, Some(&run.id));
                return Ok(true);
            }
            // Honesty gate for hybrid executors: a run that says `done` while
            // nothing was ever written or executed has nothing real to show
            // for it. Reporting it as success would be a lie.
            if run.mode == "hybrid" && executed_tool_steps(state, &run.id).unwrap_or(i64::MAX) == 0
            {
                add_step(
                    state,
                    &run.id,
                    "fail",
                    "Done without any executed action",
                    "The model claimed the goal is complete, but no file was written, no command was run and no MCP tool was called, so nothing was actually delivered or verified.",
                )?;
                finish(
                    state,
                    &run.id,
                    FAILED,
                    "The model claimed done without executing any plan action.",
                )?;
                return Ok(false);
            }
            // Team orchestration: the executor's done is not the run's end —
            // the result goes to the REVIEWER agent (same entry point a human
            // review has, same shared model router).
            if run.mode == "team" {
                return team_review_done(state, run, &summary, false).await;
            }
            add_step(state, &run.id, "done", "Run complete", &summary)?;
            finish(state, &run.id, SUCCEEDED, "")?;
            return Ok(false);
        }
        ParsedAction::Ok(Action::Fail { reason }) => {
            // Team runs do not die on the executor's word alone: the failure
            // claim itself goes to the REVIEWER, which reads the whole log —
            // including that one green command the executor managed to forget.
            if run.mode == "team" {
                return team_review_done(state, run, &reason, true).await;
            }
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
                finish(
                    state,
                    &run.id,
                    FAILED,
                    "The model kept replying with unparseable text.",
                )?;
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// How many real tool steps (a written file or a run command) this run has
/// produced. Used by the hybrid honesty gate: `done` with zero of these
/// delivered nothing.
fn executed_tool_steps(state: &AppState, run_id: &str) -> Result<i64, String> {
    db(state)?
        .query_row(
            "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1 AND action IN ('write','command','mcp')",
            [run_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())
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
    // The example payloads in the action list get copied verbatim by drifting
    // models, which then chase a phantom `relative/path` until they give up.
    // Caught here as an instruction misread, not an execution error: one clear
    // correction step, no strike, loop continues.
    let placeholder_echo = match action {
        Action::List { path } | Action::Read { path } | Action::Write { path, .. } => {
            path == "relative/path"
                || path.starts_with("relative/path/")
                || path.starts_with("build/test/")
        }
        Action::Command { command } => {
            command == "build/test" || command.starts_with("build/test ")
        }
        _ => false,
    };
    if placeholder_echo {
        add_step(
            state,
            &run.id,
            action.tag(),
            "Used the example placeholder instead of a real value",
            "\"relative/path\" and \"build/test\" are the placeholder examples from the \
             instructions, not real values. Substitute a real file name in this \
             workspace or a real shell command, then continue the task.",
        )?;
        notify(state, Some(&run.id));
        return Ok(true);
    }

    // The central policy can veto a tool this run was started with. It can
    // never grant more than the run's own flags, so the only outcome that
    // changes behaviour here is a refusal.
    let (policy_action, policy_target) = match action {
        Action::Write { path, .. } => (crate::permissions::Action::FsWrite, path.clone()),
        Action::Command { command } => (crate::permissions::Action::Command, command.clone()),
        Action::Read { path } | Action::List { path } => {
            (crate::permissions::Action::FsRead, path.clone())
        }
        Action::Mcp { server, tool, .. } => (
            crate::permissions::Action::McpTool,
            crate::mcp::tool_target(server, tool),
        ),
        _ => (crate::permissions::Action::FsRead, String::new()),
    };
    let verdict = crate::permissions::check(state, policy_action, &policy_target, &run.project_id);
    if verdict.decision == crate::permissions::Decision::Deny {
        crate::permissions::record(
            state,
            policy_action,
            &policy_target,
            crate::permissions::Decision::Deny,
            verdict.scope,
            &run.project_id,
            "agent step refused by a saved rule",
        );
        add_step(
            state,
            &run.id,
            action.tag(),
            &pending_summary(action),
            &format!(
                "Refused: a saved permission rule denies {} for \"{}\".",
                policy_action.as_str(),
                policy_target
            ),
        )?;
        finish(
            state,
            &run.id,
            FAILED,
            &format!(
                "A saved permission rule denies {} on \"{}\". Change it in Settings → Permissions, then start a new run.",
                policy_action.as_str(),
                policy_target
            ),
        )?;
        return Ok(false);
    }

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
        None => dispatch_step(state, &root, action).await,
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
            // A failed MCP call is the moment the catalogue is most likely to
            // be stale — the server may have restarted with different tools.
            // Re-read it before the repair step, bounded per run.
            if matches!(action, Action::Mcp { .. }) {
                refresh_mcp_tools(state, &run.id, &error).await;
            }
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
/// Discovers the MCP catalogue once per run and records it, because discovery
/// starts each enabled server. The snapshot is what every step prompt reads,
/// so a long run neither respawns servers per step nor loses the list when a
/// server dies mid-run. A resumed run keeps the snapshot it already has.
async fn catalog_mcp_tools(state: &AppState, run_id: &str) {
    let Ok(Some(run)) = load_run(state, run_id) else {
        return;
    };
    if !run.allow_mcp || !run.mcp_tools.is_empty() {
        return;
    }
    let (tools, errors) = crate::mcp::discover_for_prompt(state).await;
    let snapshot = json!({ "tools": tools, "errors": errors }).to_string();
    let stored = db(state).and_then(|conn| {
        conn.execute(
            "UPDATE agent_runs SET mcp_tools=?1 WHERE id=?2",
            params![snapshot, run_id],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    });
    if let Err(error) = stored {
        let _ = add_step(
            state,
            run_id,
            "error",
            "The MCP catalogue could not be saved",
            &error,
        );
        return;
    }
    let listed = tools
        .iter()
        .map(|tool| {
            let server = tool.get("server").and_then(Value::as_str).unwrap_or("");
            let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
            format!("{server}.{name}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = if listed.is_empty() {
        "No tool was discovered on the enabled MCP servers.".to_string()
    } else {
        listed
    };
    for error in &errors {
        output.push_str(&format!("\nunavailable: {error}"));
    }
    let _ = add_step(
        state,
        run_id,
        "mcp-tools",
        &format!("{} MCP tool(s) available to this run", tools.len()),
        &cap_chars(&output, STEP_OUTPUT_CAP),
    );
    notify(state, Some(run_id));
}

/// Re-reads the catalogue after an MCP action failed, so the repair step sees
/// the tools the servers offer *now* — a server that restarted with different
/// tools is the exact case a run-start snapshot cannot cover. Bounded per run,
/// because each refresh contacts every enabled server; a miss loop must not be
/// able to respawn them step after step.
async fn refresh_mcp_tools(state: &AppState, run_id: &str, reason: &str) -> bool {
    let Ok(Some(run)) = load_run(state, run_id) else {
        return false;
    };
    if !run.allow_mcp {
        return false;
    }
    let used = mcp_refresh_count(&run.mcp_tools);
    if used >= MCP_REFRESHES_PER_RUN {
        return false;
    }
    let (tools, errors) = crate::mcp::discover_for_prompt(state).await;
    let snapshot = json!({
        "tools": tools,
        "errors": errors,
        "refreshes": used + 1,
    })
    .to_string();
    let stored = db(state).and_then(|conn| {
        conn.execute(
            "UPDATE agent_runs SET mcp_tools=?1 WHERE id=?2",
            params![snapshot, run_id],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    });
    if let Err(error) = stored {
        let _ = add_step(
            state,
            run_id,
            "error",
            "The MCP catalogue could not be refreshed",
            &error,
        );
        return false;
    }
    let listed = tools
        .iter()
        .map(|tool| {
            let server = tool.get("server").and_then(Value::as_str).unwrap_or("");
            let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
            format!("{server}.{name}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = format!(
        "An MCP call failed, so the catalogue was read again ({used} of {MCP_REFRESHES_PER_RUN} refreshes used).\nReason: {}",
        cap_chars(reason, 300)
    );
    if listed.is_empty() {
        output.push_str("\nNo tool was discovered on the enabled MCP servers.");
    } else {
        output.push_str("\n\n");
        output.push_str(&listed);
    }
    for error in &errors {
        output.push_str(&format!("\nunavailable: {error}"));
    }
    let _ = add_step(
        state,
        run_id,
        "mcp-refresh",
        &format!("{} MCP tool(s) after a refresh", tools.len()),
        &cap_chars(&output, STEP_OUTPUT_CAP),
    );
    notify(state, Some(run_id));
    true
}

async fn run_loop(state: AppState, run_id: String) {
    // The MCP catalogue comes first: the very first prompt is built from it.
    catalog_mcp_tools(&state, &run_id).await;
    // Hybrid phase 1: the cloud plans before the local model touches anything.
    if let Ok(Some(run)) = load_run(&state, &run_id) {
        if run.mode == "hybrid" && run.status == RUNNING {
            if let Err(error) = plan_with_cloud(&state, &run).await {
                let _ = add_step(&state, &run_id, "error", "The cloud plan failed:", &error);
                let _ = finish(&state, &run_id, FAILED, &error);
                ACTIVE.lock().unwrap().remove(&run_id);
                notify(&state, Some(&run_id));
                return;
            }
        }
    }
    while step_count(&state, &run_id).unwrap_or(0) < HARD_MAX_STEPS {
        let run = match load_run(&state, &run_id) {
            Ok(Some(run)) if run.status == RUNNING => run,
            Ok(_) => return, // cancelled, approved externally, or gone
            Err(_) => return,
        };
        // Crew stage budgets (Step 5): the count restarts at each handoff, so
        // a stage that burns its ceiling is reported honestly instead of
        // quietly eating the rest of the run.
        let stage_steps = if run.mode == "crew" && run.stage_max_steps > 0 {
            db(&state)
                .and_then(|conn| {
                    conn.query_row(
                        "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1 AND \
                         seq > COALESCE((SELECT MAX(seq) FROM agent_steps WHERE run_id=?1 AND action IN ('handoff', 'resume')), 0)",
                        [&run.id],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())
                })
                .unwrap_or(0)
        } else {
            0
        };
        if run.mode == "crew" && run.stage_max_steps > 0 && stage_steps >= run.stage_max_steps {
            let crew: Vec<String> = serde_json::from_str(&run.crew).unwrap_or_default();
            let handoffs: i64 = db(&state)
                .and_then(|conn| {
                    conn.query_row(
                        "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1 AND action='handoff'",
                        [&run.id],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())
                })
                .unwrap_or(0);
            let stage_name = crew.get(handoffs as usize).cloned().unwrap_or_default();
            let next_role = crew
                .get((handoffs + 1) as usize)
                .cloned()
                .unwrap_or_default();
            // The final stage cannot be skipped: after it there is nobody
            // left to deliver the goal, so the run stops here either way.
            if run.stage_policy != "skip" || handoffs + 1 >= crew.len() as i64 {
                add_step(
                    &state,
                    &run.id,
                    "fail",
                    "Stage budget exhausted",
                    &format!("The {} stage used its {} step budget without finishing; the run stops there instead of pretending the stage completed.", stage_name, run.stage_max_steps),
                ).ok();
                let _ = finish(
                    &state,
                    &run.id,
                    FAILED,
                    "A crew stage ran over its step budget without finishing.",
                );
                break;
            }
            add_step(
                &state,
                &run.id,
                "handoff",
                &format!("Stage {} budget exhausted; {} agent takes over.", stage_name, next_role),
                &format!("The {} stage did not finish inside its {} step budget. Incomplete stage work is handed over recorded, not hidden.", stage_name, run.stage_max_steps),
            ).ok();
        }
        match step_once(&state, &run).await {
            Ok(true) => notify(&state, Some(&run_id)),
            Ok(false) => break,
            Err(error) => {
                let database_ok =
                    add_step(&state, &run_id, "error", "The model call failed:", &error).is_ok();
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
        .route(format!("{prefix}/overview").as_str(), get(runs_overview))
        .route(format!("{prefix}/:id").as_str(), get(get_run))
        .route(format!("{prefix}/:id/approve").as_str(), post(approve_run))
        .route(format!("{prefix}/:id/resume").as_str(), post(resume_run))
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
// Specialist agent roles (multi-agent orchestration). Every role shares the
// same model router; what differs is the role prompt and its tool permissions:
// builders (coding, frontend, backend, database) edit files, inspectors
// (planner, testing, debugger, review) produce reports and never modify code.
const PROFILES: [&str; 8] = [
    "coding", "frontend", "backend", "database", "planner", "testing", "debugger", "review",
];

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
        "planner" => {
            "You are the Saarthi Studio ARCHITECT/PLANNER. Analyze the task and inspect the \
workspace as needed, plan system structure and boundaries, and produce the plan the \
implementation agents will follow. When the plan is complete, finish with done whose \
summary IS the plan (steps, likely files, dependencies). You may NOT write files or \
run commands."
        }
        "frontend" => {
            "You are the Saarthi Studio FRONTEND developer. Build and fix UI code (markup, \
styles, frontend scripts) in the workspace, one step at a time, and verify what you \
can with commands. Finish with done only once the frontend actually addresses the goal."
        }
        "backend" => {
            "You are the Saarthi Studio BACKEND developer. Build and fix server-side code, \
routes and business logic in the workspace, one step at a time, and verify what you \
can with commands. Finish with done only once the backend actually addresses the goal."
        }
        "database" => {
            "You are the Saarthi Studio DATABASE engineer. Work on schema definitions, \
migrations and data-access code in the workspace, one step at a time, and verify \
what you can with commands. Finish with done only once the data layer actually \
addresses the goal."
        }
        "debugger" => {
            "You are the Saarthi Studio DEBUGGING agent. Reproduce failures with real \
commands, read the relevant code and logs, and diagnose the exact cause with file \
paths and line references. You may run commands but may NOT write files — your \
summary is the diagnosis (and optionally the suggested fix), not a repair."
        }
        "testing" => {
            "You are the Saarthi Studio TESTING agent. Find and run the relevant tests for the \
goal, capture real exit codes and output, and separate infrastructure failures (missing \
tools or deps) from actual test failures. You may run commands but may NOT write files. \
Never claim tests passed unless a run said so."
        }
        "review" => {
            "You are the Saarthi Studio REVIEWER. Inspect the current code and recent diffs, and \
report concrete findings (bugs, security risks, regressions, maintainability issues) with \
file paths and line references where possible. You may NOT write files or run commands. \
Finish with done whose summary is the findings list."
        }
        _ => {
            "You are the Saarthi Studio autonomous coding agent working inside an open workspace \
folder. You complete the task one step at a time: inspect, plan, edit, verify, and keep \
going until the goal is actually met."
        }
    }
}

fn profile_rule(profile: &str, write: bool) -> &'static str {
    match (profile, write) {
        ("planner" | "review" | "testing" | "debugger", true) => {
            "write FORBIDDEN — this profile never modifies files"
        }
        ("planner" | "review", false) => "command FORBIDDEN — this profile never runs commands",
        _ => "",
    }
}

/// Who may drive tools, independent of any checkboxes: builders edit files,
/// inspectors (planner, review) never touch either, running inspectors
/// (testing, debugger) get commands but no writes.
fn profile_can_write(profile: &str) -> bool {
    matches!(profile, "coding" | "frontend" | "backend" | "database")
}

fn profile_can_command(profile: &str) -> bool {
    !matches!(profile, "planner" | "review")
}

/// The run-mode picker: "" is the normal single-model run, "hybrid" is
/// cloud-plans/local-executes, "team" runs the executor and then hands the
/// result to the REVIEWER agent (a one-round repair loop), "crew" drives a
/// sequential multi-agent pipeline of specialist stages. Anything else is
/// rejected at the door.
fn normalize_mode(input: &str) -> Option<String> {
    match input.trim().to_lowercase().as_str() {
        "" => Some(String::new()),
        "hybrid" => Some("hybrid".to_string()),
        "team" => Some("team".to_string()),
        "crew" => Some("crew".to_string()),
        _ => None,
    }
}

/// Validated crew line-up for crew-mode runs: a JSON list of known profiles,
/// 1–6 stages, defaults to executor → reviewer. Builders are required when
/// the run is allowed to write files — an all-inspector crew cannot write.
/// The crew stage-budget policy: ""/"fail" stops the run honestly when a
/// stage cannot finish inside its budget; "skip" records the incomplete
/// stage as a handoff and lets the next stage try.
fn normalize_stage_policy(input: &str) -> Result<String, String> {
    match input.trim().to_lowercase().as_str() {
        "" => Ok(String::new()),
        "fail" => Ok("fail".to_string()),
        "skip" => Ok("skip".to_string()),
        _ => Err("Unknown stage policy - use fail, skip, or leave it out.".to_string()),
    }
}

fn parse_crew(input: &Value, mode: &str, allow_writes: bool) -> Result<Vec<String>, String> {
    if mode != "crew" {
        return Ok(Vec::new());
    }
    let parsed: Vec<String> = match input.get("crew") {
        None => vec!["coding".to_string(), "review".to_string()],
        Some(Value::Array(items)) => {
            let mut roles = Vec::new();
            for item in items {
                let profile = item.as_str().map(normalize_profile).unwrap_or_default();
                if profile.is_empty() {
                    return Err(format!(
                        "Unknown crew agent role: {}. Known roles: {}.",
                        item,
                        PROFILES.join(", ")
                    ));
                }
                roles.push(profile);
            }
            if roles.is_empty() || roles.len() > 6 {
                return Err("A crew has 1–6 agent stages.".to_string());
            }
            roles
        }
        Some(_) => return Err("The crew field must be a JSON array of role names.".to_string()),
    };
    if allow_writes && !parsed.iter().any(|role| profile_can_write(role)) {
        return Err(
            "This crew has no building agent, but file writes are allowed — add a building role (coding, frontend, backend, database) or turn writes off."
                .to_string(),
        );
    }
    Ok(parsed)
}

/// Validate a saved crew roster without a full run payload: templates live
/// as { name, crew } pairs and are edited as plain JSON.
pub(crate) fn validate_crew(crew: &Value) -> Result<Vec<String>, String> {
    parse_crew(&json!({ "crew": crew }), "crew", false)
}

/// Orchestrator state is the step log itself: each crew stage ends with a
/// `handoff` step naming the next role, so the current stage index is simply
/// how many handoffs have happened (clamped to the last stage).
fn crew_stage_index(handoff_steps: i64, crew_len: usize) -> usize {
    (handoff_steps as usize).min(crew_len.saturating_sub(1))
}

async fn create_run(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let goal = input
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if goal.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Describe what the agent should do.",
        );
    }
    if goal.chars().count() > 5_000 {
        return error_response(StatusCode::BAD_REQUEST, "Goal is too long.");
    }
    let allow_writes = input
        .get("allowWrites")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let allow_commands = input
        .get("allowCommands")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let auto_approve = input
        .get("autoApprove")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // MCP tool use is opt-in per run and refused at the door when there is
    // nothing to call: a checkbox that can only ever do nothing is a lie.
    let allow_mcp = input
        .get("allowMcp")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if allow_mcp && crate::mcp::enabled_servers(&state).is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "No enabled MCP server is registered - add one in Settings \u{2192} MCP first, or start the run without MCP tools.",
        );
    }
    let max_steps = input
        .get("maxSteps")
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_MAX_STEPS)
        .clamp(1, HARD_MAX_STEPS);
    let profile = normalize_profile(input.get("profile").and_then(Value::as_str).unwrap_or(""));
    if profile.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "Unknown agent profile.");
    }
    let mode = match normalize_mode(input.get("mode").and_then(Value::as_str).unwrap_or("")) {
        Some(mode) => mode,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Unknown agent mode — use \"hybrid\", \"team\", \"crew\", or leave it out.",
            )
        }
    };
    // Hybrid runs need a local executor up front, so a run can never be
    // started down a path that is guaranteed to fail on its first step.
    if mode == "hybrid" && crate::load_config(&state).local_model.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Hybrid mode needs a local model: pick and pull one on the Local AI page first.",
        );
    }
    // Crew line-up validates before any run row exists: an unknown role or a
    // write-permissionless crew should never leave a broken run behind.
    let crew = match parse_crew(&input, &mode, allow_writes) {
        Ok(crew) => crew,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, message),
    };
    let crew_json = serde_json::to_string(&crew).unwrap_or_default();
    let stage_policy = match normalize_stage_policy(
        input
            .get("stagePolicy")
            .and_then(Value::as_str)
            .unwrap_or(""),
    ) {
        Ok(policy) => policy,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, message),
    };
    // The per-stage budget only exists for crew runs.
    let stage_max_steps = if mode == "crew" {
        input
            .get("stageMaxSteps")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .clamp(0, HARD_MAX_STEPS)
    } else {
        0
    };

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
    // run started, so the run shows which model setup produced it. A run may
    // name its own model (task scope), above the agent, project and global
    // defaults.
    let task_model = input
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty());
    let (snapshot, base_model) = crate::run_record_snapshot(&state, task_model);
    if let Err(error) = db(&state).and_then(|conn|
        conn.execute(
            "INSERT INTO agent_runs(id,project_id,goal,status,allow_writes,allow_commands,auto_approve,max_steps,pending_action,error,created_at,updated_at,router_config,model,profile,mode,crew,stage_policy,stage_max_steps,allow_mcp) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'','',?9,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![
                id, project_id, goal, RUNNING,
                allow_writes, allow_commands, auto_approve, max_steps, stamp,
                snapshot, base_model, profile, mode, crew_json, stage_policy, stage_max_steps,
                allow_mcp
            ],
        ).map_err(|e| e.to_string())
    ) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, error);
    }
    spawn_run(state.clone(), id.clone());
    notify(&state, Some(&id));
    Json(json!({ "ok": true, "run": { "id": id, "status": RUNNING } })).into_response()
}

/// Column order must match `row_to_run` exactly, `model` included: a list that
/// skipped it shifted every later field, the rows failed to map, and the panel
/// read "No runs yet" while the runs existed. The step count is the last column.
const RUN_LIST_SQL: &str = "SELECT r.id, r.project_id, r.goal, r.status, r.allow_writes, \
 r.allow_commands, r.auto_approve, r.max_steps, r.pending_action, r.error, r.created_at, \
 r.updated_at, r.router_config, r.model, r.profile, r.mode, r.crew, r.stage_policy, \
 r.stage_max_steps, r.allow_mcp, r.mcp_tools, r.tokens, \
 (SELECT COUNT(*) FROM agent_steps s WHERE s.run_id = r.id) AS steps \
 FROM agent_runs r ORDER BY r.created_at DESC LIMIT 50";

async fn list_runs(State(state): State<AppState>) -> Response {
    let conn = match db(&state) {
        Ok(conn) => conn,
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let mut stmt = match conn.prepare(RUN_LIST_SQL) {
        Ok(stmt) => stmt,
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let rows = stmt
        .query_map([], |row| {
            let run = row_to_run(row)?;
            let steps: i64 = row.get(22)?;

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

/// The monitoring dashboard: every recent run with its timing and totals, plus
/// the counts its header shows. It reuses the same rows as the list, so the board
/// and the cards can never disagree about a run.
async fn runs_overview(State(state): State<AppState>) -> Response {
    let conn = match db(&state) {
        Ok(conn) => conn,
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    let mut stmt = match conn.prepare(RUN_LIST_SQL) {
        Ok(stmt) => stmt,
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let rows = match stmt.query_map([], |row| {
        let run = row_to_run(row)?;
        let steps: i64 = row.get(22)?;
        Ok(run_json(&run, steps))
    }) {
        Ok(rows) => rows.filter_map(|row| row.ok()).collect::<Vec<Value>>(),
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let mut counts = serde_json::Map::new();
    let mut tokens = 0i64;
    let mut steps = 0i64;
    for run in &rows {
        let status = run["status"].as_str().unwrap_or("unknown").to_string();
        let entry = counts.entry(status).or_insert_with(|| json!(0));
        if let Some(value) = entry.as_i64() {
            *entry = json!(value + 1);
        }
        tokens += run["tokens"].as_i64().unwrap_or(0);
        steps += run["steps"].as_i64().unwrap_or(0);
    }
    // "Live" is the honest union: a run can be working, or stopped waiting for a
    // human answer, and both block the queue in different ways.
    let live = rows
        .iter()
        .filter(|run| {
            matches!(
                run["status"].as_str(),
                Some("running" | "awaiting_approval" | "paused")
            )
        })
        .count();
    Json(json!({
        "ok": true,
        "counts": counts,
        "totals": { "runs": rows.len(), "tokens": tokens, "steps": steps, "live": live },
        "runs": rows,
    }))
    .into_response()
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
        if let Some(Action::Write { path, content }) =
            serde_json::from_str::<Value>(&run.pending_action)
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
    let checkpoint = match db(&state).and_then(|conn| {
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
        return error_response(
            StatusCode::BAD_REQUEST,
            "This step has no checkpoint to restore.",
        );
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

/// Emergency stop: cancel every live agent run in one call. A run counts as
/// live when its stored status is `running` or `awaiting_approval` — exactly
/// the statuses the per-run cancel endpoint refuses to leave behind — and the
/// ACTIVE map carries a cancel flag for every loop that is actually executing
/// a step. The two lists can disagree in both directions (a finished loop that
/// has not been reaped, a run paused at approval), so each mechanism is applied
/// on its own terms: the flag wakes a live loop, `finish` closes the record no
/// matter which side was live, and the loop's terminal check can never
/// overwrite a status the record no longer holds. Mirrors `cancel_run`,
/// repeated over every live run, without re-loading each row.
pub(crate) fn cancel_all_active(state: &AppState, reason: &str) -> Vec<String> {
    let run_ids: Vec<String> = ACTIVE.lock().unwrap().keys().cloned().collect();
    let mut stopped = Vec::new();
    for run_id in run_ids {
        let live = matches!(
            run_status(state, &run_id).ok().flatten().as_deref(),
            Some(RUNNING) | Some(AWAITING_APPROVAL)
        );
        if !live {
            continue;
        }
        if let Some(flag) = ACTIVE.lock().unwrap().get(&run_id) {
            flag.store(true, Ordering::Relaxed);
        }
        let _ = add_step(state, &run_id, "emergency-stop", "Emergency stop.", reason);
        let _ = finish(state, &run_id, CANCELLED, reason);
        notify(state, Some(&run_id));
        stopped.push(run_id);
    }
    stopped
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

/// Resume from stage k: the step log IS the orchestrator state, so a
/// failed, cancelled or interrupted run keeps its handoffs (finished
/// stages), its saved plan and its current stage. "Run again"
/// re-activates THIS run instead of starting a new one; the stage budget
/// restarts at the resume marker exactly like it does at a handoff.
async fn resume_run(State(state): State<AppState>, Path(run_id): Path<String>) -> Response {
    let run = match load_run(&state, &run_id) {
        Ok(Some(run)) => run,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "No such run."),
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    if run.status == RUNNING || run.status == AWAITING_APPROVAL {
        return error_response(
            StatusCode::BAD_REQUEST,
            "This run is still live - approve or cancel it instead.",
        );
    }
    if run.status == SUCCEEDED {
        return error_response(
            StatusCode::BAD_REQUEST,
            "This run already delivered its goal - start a new run instead.",
        );
    }
    let (steps_present, handoffs) = match db(&state) {
        Ok(conn) => (
            conn.query_row(
                "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1",
                [&run_id],
                |r| r.get(0),
            )
            .unwrap_or(0i64),
            conn.query_row(
                "SELECT COUNT(*) FROM agent_steps WHERE run_id=?1 AND action='handoff'",
                [&run_id],
                |r| r.get(0),
            )
            .unwrap_or(0i64),
        ),
        Err(error) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, error),
    };
    if steps_present == 0 {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Nothing to resume: this run has no steps. Start a new run instead.",
        );
    }
    let stage_note = if run.mode == "crew" {
        let crew: Vec<String> = serde_json::from_str(&run.crew).unwrap_or_default();
        match crew.get(handoffs as usize) {
            Some(role) => format!(" Resuming the {} stage.", role),
            None => " Every recorded handoff is in the log; resuming the final stage.".to_string(),
        }
    } else {
        " Resuming from the recorded step log.".to_string()
    };
    let _ = add_step(
        &state,
        &run_id,
        "resume",
        &format!("Run resumed.{}", stage_note),
        "Picking up from the recorded step log: finished stages stay finished, unfinished work continues.",
    );
    if let Err(error) = set_run(&state, &run_id, RUNNING, None, Some("")) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, error);
    }
    notify(&state, Some(&run_id));
    spawn_run(state, run_id);
    Json(json!({ "ok": true, "status": RUNNING })).into_response()
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
        return error_response(
            StatusCode::BAD_REQUEST,
            "This run has no action waiting for approval.",
        );
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
        let _ = add_step(
            &state,
            &run_id,
            "rejected",
            &pending_summary(&action),
            "Rejected by the user.",
        );
        let _ = finish(
            &state,
            &run_id,
            FAILED,
            "You rejected the action the agent needed.",
        );
        notify(&state, Some(&run_id));
        return Json(json!({ "ok": true, "status": FAILED })).into_response();
    }

    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => {
            let message = error.message().to_string();
            let _ = finish(
                &state,
                &run_id,
                FAILED,
                &format!("Workspace closed: {message}"),
            );
            notify(&state, Some(&run_id));
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, message);
        }
    };
    let summary = pending_summary(&action);
    let (checkpoint, old_content) = match &action {
        Action::Write { path, .. } => capture_checkpoint(&root, path),
        _ => (String::new(), None),
    };
    match dispatch_step(&state, &root, &action).await {
        Ok(output) => {
            let _ = set_run(&state, &run_id, RUNNING, None, Some(""));
            let output = match &action {
                Action::Write { path, content } => format!(
                    "{output}\n--- diff ---\n{}",
                    unified_diff(old_content.as_deref(), content, path)
                ),
                _ => output,
            };
            let _ = add_step_with_checkpoint(
                &state,
                &run_id,
                action.tag(),
                &summary,
                &output,
                &checkpoint,
            );
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
    format!(
        "{cut}… [{overflow} chars truncated]",
        overflow = total - limit
    )
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_monitor_payload_carries_tokens_and_timing() {
        let path =
            std::env::temp_dir().join(format!("saarthi-runs-overview-{}.db", uuid::Uuid::new_v4()));
        let conn = crate::open_db(&path).expect("temp db");
        initialize(&conn).expect("schema");
        conn.execute(
            "INSERT INTO agent_runs(id,project_id,goal,status,allow_writes,allow_commands,auto_approve,max_steps,created_at,updated_at,tokens) \
             VALUES('run-1','proj','goal','running',0,0,0,8,10.0,20.0,1200)",
            [],
        )
        .expect("insert run");
        let mut stmt = conn.prepare(RUN_LIST_SQL).expect("list sql");
        let payload = stmt
            .query_row([], |row| {
                let run = row_to_run(row)?;
                let steps: i64 = row.get(22)?;
                Ok(run_json(&run, steps))
            })
            .expect("row maps");
        assert_eq!(payload["tokens"], json!(1200));
        assert_eq!(payload["steps"], json!(0));
        // A live run measures from its start, never from its last write.
        assert!(
            payload["elapsedSeconds"].as_f64().unwrap() > 1_000.0,
            "{payload}"
        );
        assert!(payload["idleSeconds"].as_f64().unwrap() > 0.0, "{payload}");
        let _ = std::fs::remove_file(&path);
    }

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
            other => panic!("expected write with escaping, got {other:?}"),
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
    fn write_action_repairs_raw_newlines_in_content() {
        // Exactly how 1.5B executors emit multi-line file content.
        let json_doc = "{\"action\": \"write\", \"path\": \"even_check.py\", \"content\": \"def is_even(n):\n    return n % 2 == 0\"}";
        let raw = format!("```json\n{json_doc}\n```");
        match parse_action(&raw) {
            ParsedAction::Ok(Action::Write { path, content }) => {
                assert_eq!(path, "even_check.py");
                assert!(content.contains("def is_even(n):"));
                assert!(content.contains('\n'));
            }
            other => panic!("expected write with escaping, got {other:?}"),
        }
        // Content with unescaped inner quotes parses as written, too.
        let quoted = r#"{"action": "write", "path": "even_check.py", "content": "print("ODD")
print("EVEN")
"}"#;
        match parse_action(quoted) {
            ParsedAction::Ok(Action::Write { path, content }) => {
                assert_eq!(path, "even_check.py");
                assert!(content.contains("print(\"ODD\")\nprint(\"EVEN\")\n"));
            }
            other => panic!("expected write with escaping, got {other:?}"),
        }
        // Repair-on-escape gives up cleanly on genuinely broken JSON.
        let broken = "{\"action\": wrong}";
        match parse_action(broken) {
            ParsedAction::Malformed(text) => assert!(text.contains("invalid JSON")),
            other => panic!("expected malformed, got {other:?}"),
        }
    }

    #[test]
    fn write_action_rebuilds_value_fence_with_docstring_inside() {
        // The 4B-class shape that defeats fence pairing: the content value is
        // a Python fence whose body itself has docstrings — more `"""` in
        // one document than pairing can recover. Rebuilt from the first
        // value-position fence to the last fence in the document.
        let raw = "```json\n{\n  \"action\": \"write\",\n  \"path\": \"greet.py\",\n  \"content\": \"\"\"\\
def greet(name):\n    \"\"\"\n    Returns a greeting.\n    \"\"\"\n    return f'Hello, {name}!'\n\"\"\"\n}\n```";
        let parsed = parse_action(raw);
        match parsed {
            ParsedAction::Ok(Action::Write { path, content }) => {
                assert_eq!(path, "greet.py");
                assert!(content.contains("def greet(name):"));
                assert!(content.contains("Returns a greeting."));
                assert!(content.contains("return f'Hello"));
                assert!(
                    content.contains('\"'),
                    "docstring quotes must become escaped quotes"
                );
            }
            other => panic!("expected rebuilt write action, got {other:?}"),
        }
    }

    #[test]
    fn write_action_rebuilds_unclosed_value_fence() {
        // Wire shape from a real team repair round: the model opens a """
        // value fence but closes it with a ```json block, then appends a
        // second object. Rebuild the body up to the fence boundary and
        // undo the backslash-escaped quotes it double-escaped.
        let raw = "```json\n{\n  \"action\": \"write\",\n  \"path\": \"greet.py\",\n  \"content\": \"\"\"def greet(name):\n    return f\\'Hello, {name}!\\'\n```\n\n```json\n{ \"action\": \"command\" }\n```";
        match parse_action(raw) {
            ParsedAction::Ok(Action::Write { path, content }) => {
                assert_eq!(path, "greet.py");
                assert!(content.contains("def greet(name):"));
                assert!(content.contains("return f'Hello, {name}!'"));
            }
            other => panic!("expected rebuilt write action, got {other:?}"),
        }
    }
    #[test]
    fn write_action_repairs_python_triple_quoted_content() {
        // The real crash: the 1.5B executor wraps file content in Python
        // docstring fences instead of a JSON string.
        let raw = "```json\n{\n  \"action\": \"write\",\n  \"path\": \"even_check.py\",\n  \"content\": \"\"\"\\
if __name__ == \"__main__\":\n    num = int(sys.argv[1])\n    print(\"EVEN\" if is_even(num) else \"ODD\")\n\"\"\"\n}\n```";
        match parse_action(raw) {
            ParsedAction::Ok(Action::Write { path, content }) => {
                assert_eq!(path, "even_check.py");
                assert!(content.contains("int(sys.argv[1])"));
                assert!(content.contains('"'), "inner quotes must survive");
                assert!(content.contains('\n'));
                assert!(!content.contains("\"\"\""));
            }
            other => panic!("expected write from triple-quoted content, got {other:?}"),
        }
    }

    #[test]
    fn observed_error_tag_echo_becomes_a_think_report() {
        // The 1.5B executor re-emits "write-error" from its own step log as
        // its action; that must keep the loop alive, not fail the run.
        let raw = "{\"action\": \"write-error\", \"summary\": \"Write even_check.py\"}";
        match parse_action(raw) {
            ParsedAction::Ok(Action::Think { summary }) => {
                assert!(summary.contains("even_check"));
            }
            other => panic!("expected think report, got {other:?}"),
        }
        // Genuinely unknown tags still fail loudly.
        match parse_action("{\"action\": \"teleport\"}") {
            ParsedAction::Malformed(text) => assert!(text.contains("unknown action")),
            other => panic!("expected malformed, got {other:?}"),
        }
    }

    #[test]
    fn action_json_roundtrip_preserves_every_variant() {
        let actions = vec![
            Action::Think {
                summary: "checking".into(),
            },
            Action::List { path: ".".into() },
            Action::Read {
                path: "a/b.rs".into(),
            },
            Action::Write {
                path: "a.txt".into(),
                content: "héllo ★".into(),
            },
            Action::Command {
                command: "cargo test".into(),
            },
            Action::Done {
                summary: "all good".into(),
            },
            Action::Fail {
                reason: "broken".into(),
            },
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
            mode: String::new(),
            crew: String::new(),
            stage_policy: String::new(),
            stage_max_steps: 0,
            allow_mcp: false,
            mcp_tools: String::new(),
            tokens: 0,
        };
        // Coding + every flag on: writes are allowed outright.
        assert!(matches!(
            PermissionGate::decides(
                &base,
                &Action::Write {
                    path: "a".into(),
                    content: "b".into()
                }
            ),
            GateDecision::Allowed
        ));
        // Planner/review are read-only even with every checkbox ticked.
        for profile in ["planner", "review"] {
            let run = gated_run(profile);
            assert!(
                matches!(
                    PermissionGate::decides(
                        &run,
                        &Action::Write {
                            path: "a".into(),
                            content: "b".into()
                        }
                    ),
                    GateDecision::Denied(_)
                ),
                "profile {profile} must not write"
            );
            assert!(
                matches!(
                    PermissionGate::decides(
                        &run,
                        &Action::Command {
                            command: "ls".into()
                        }
                    ),
                    GateDecision::Denied(_)
                ),
                "profile {profile} must not run commands"
            );
        }
        // Testing runs commands but never writes.
        let testing = gated_run("testing");
        assert!(matches!(
            PermissionGate::decides(
                &testing,
                &Action::Command {
                    command: "cargo test".into()
                }
            ),
            GateDecision::Allowed
        ));
        assert!(matches!(
            PermissionGate::decides(
                &testing,
                &Action::Write {
                    path: "a".into(),
                    content: "b".into()
                }
            ),
            GateDecision::Denied(_)
        ));

        // Debugger runs commands but never writes.
        let dbg = gated_run("debugger");
        assert!(matches!(
            PermissionGate::decides(
                &dbg,
                &Action::Command {
                    command: "ls".into()
                }
            ),
            GateDecision::Allowed
        ));
        assert!(matches!(
            PermissionGate::decides(
                &dbg,
                &Action::Write {
                    path: "a".into(),
                    content: "b".into()
                }
            ),
            GateDecision::Denied(_)
        ));
        // Builders get writes (frontend here).
        let fe = gated_run("frontend");
        assert!(matches!(
            PermissionGate::decides(
                &fe,
                &Action::Write {
                    path: "a".into(),
                    content: "b".into()
                }
            ),
            GateDecision::Allowed
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
            mode: String::new(),
            crew: String::new(),
            stage_policy: String::new(),
            stage_max_steps: 0,
            allow_mcp: false,
            mcp_tools: String::new(),
            tokens: 0,
        }
    }

    /// An all-allowed run whose MCP flag and catalogue are under test.
    fn mcp_run(allow_mcp: bool, snapshot: &str) -> RunRow {
        let mut run = gated_run("coding");
        run.allow_mcp = allow_mcp;
        run.mcp_tools = snapshot.to_string();
        run
    }

    const MCP_SNAPSHOT: &str = r#"{"tools":[{"server":"echo","name":"echo","description":"Echo text back","required":["text"]}],"errors":[]}"#;

    #[test]
    fn an_mcp_action_parses_with_and_without_arguments() {
        match parse_action(
            "{\"action\":\"mcp\",\"server\":\"echo\",\"tool\":\"echo\",\"arguments\":{\"text\":\"hi\"}}",
        ) {
            ParsedAction::Ok(Action::Mcp {
                server,
                tool,
                arguments,
            }) => {
                assert_eq!(server, "echo");
                assert_eq!(tool, "echo");
                assert_eq!(arguments["text"], json!("hi"));
            }
            other => panic!("expected an mcp action, got {other:?}"),
        }
        // Missing arguments means no arguments, not a malformed reply.
        match parse_action("{\"action\":\"mcp-call\",\"server\":\"echo\",\"tool\":\"ping\"}") {
            ParsedAction::Ok(Action::Mcp { arguments, .. }) => assert_eq!(arguments, json!({})),
            other => panic!("expected an mcp action, got {other:?}"),
        }
        // A non-object arguments value still parses: the tool's schema rejects
        // it as a step error the model can read and repair.
        match parse_action(
            "{\"action\":\"mcp\",\"server\":\"e\",\"tool\":\"t\",\"arguments\":\"x\"}",
        ) {
            ParsedAction::Ok(Action::Mcp { arguments, .. }) => assert_eq!(arguments, json!("x")),
            other => panic!("expected an mcp action, got {other:?}"),
        }
        // Naming the server or tool is not optional.
        match parse_action("{\"action\":\"mcp\",\"tool\":\"echo\"}") {
            ParsedAction::Malformed(error) => assert!(error.contains("server"), "{error}"),
            other => panic!("expected malformed, got {other:?}"),
        }
    }

    #[test]
    fn an_mcp_call_is_summarised_for_the_approval_row() {
        let bare = Action::Mcp {
            server: "echo".into(),
            tool: "echo".into(),
            arguments: json!({}),
        };
        assert_eq!(pending_summary(&bare), "call MCP tool echo.echo");
        assert_eq!(bare.tag(), "mcp");
        let with_args = Action::Mcp {
            server: "files".into(),
            tool: "read_file".into(),
            arguments: json!({ "path": "src/main.rs" }),
        };
        let summary = pending_summary(&with_args);
        assert!(
            summary.starts_with("call MCP tool files.read_file with {"),
            "{summary}"
        );
        // The pending action survives the round trip the approval route takes.
        assert_eq!(
            Action::from_json(&with_args.to_json()),
            Ok(Action::Mcp {
                server: "files".into(),
                tool: "read_file".into(),
                arguments: json!({ "path": "src/main.rs" }),
            })
        );
    }

    #[test]
    fn mcp_calls_are_gated_by_the_run_and_still_pause() {
        let call = Action::Mcp {
            server: "echo".into(),
            tool: "echo".into(),
            arguments: json!({ "text": "hi" }),
        };
        // A run started without MCP may never call a tool.
        assert!(matches!(
            PermissionGate::decides(&mcp_run(false, MCP_SNAPSHOT), &call),
            GateDecision::Denied(_)
        ));
        // Allowed but unapproved: it pauses for the user first.
        let mut run = mcp_run(true, MCP_SNAPSHOT);
        run.auto_approve = false;
        assert!(matches!(
            PermissionGate::decides(&run, &call),
            GateDecision::NeedsApproval
        ));
        // Allowed and auto-approved: it runs.
        assert!(matches!(
            PermissionGate::decides(&mcp_run(true, MCP_SNAPSHOT), &call),
            GateDecision::Allowed
        ));
    }

    #[test]
    fn a_snapshot_carries_its_refresh_allowance() {
        // A snapshot written before refreshes existed reads as none used.
        assert_eq!(mcp_refresh_count(MCP_SNAPSHOT), 0);
        assert_eq!(
            mcp_refresh_count(r#"{"tools":[],"errors":[],"refreshes":2}"#),
            2
        );
        // A corrupt snapshot is zero, never a panic in the run loop.
        assert_eq!(mcp_refresh_count("not json"), 0);
    }

    #[test]
    fn a_refreshed_catalogue_reaches_the_next_prompt() {
        let refreshed = r#"{"tools":[{"server":"echo","name":"ping","description":"Answer pong","required":[]}],"errors":[],"refreshes":1}"#;
        let prompt = build_messages(&mcp_run(true, refreshed), "(one failed step)")
            .iter()
            .map(|message| message["content"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>()
            .join("\n");
        // The tool the server offers after the refresh is what the model reads.
        assert!(prompt.contains("echo.ping"), "{prompt}");
        assert!(prompt.contains("\"action\":\"mcp\""), "{prompt}");
        assert_eq!(mcp_tool_count(refreshed), 1);
        assert_eq!(mcp_refresh_count(refreshed), 1);
    }

    #[test]
    fn the_mcp_action_is_advertised_only_when_a_tool_was_found() {
        let prompt = |run: &RunRow| {
            build_messages(run, "(no steps yet)")
                .iter()
                .map(|message| message["content"].as_str().unwrap_or("").to_string())
                .collect::<Vec<_>>()
                .join("\n")
        };
        // No flag: the action is forbidden and no catalogue rides along.
        let without = prompt(&mcp_run(false, MCP_SNAPSHOT));
        assert!(without.contains("mcp FORBIDDEN"), "{without}");
        assert!(!without.contains("echo.echo"), "{without}");
        // Flag on with a discovered tool: the action, the rule and the tool.
        let with = prompt(&mcp_run(true, MCP_SNAPSHOT));
        assert!(with.contains("\"action\":\"mcp\""), "{with}");
        assert!(with.contains("echo.echo"), "{with}");
        assert!(with.contains("required: text"), "{with}");
        // Flag on but nothing discovered: no action line, and the prompt says so.
        let empty = prompt(&mcp_run(
            true,
            "{\"tools\":[],\"errors\":[\"gone: no such file\"]}",
        ));
        assert!(!empty.contains("\"action\":\"mcp\""), "{empty}");
        assert!(empty.contains("no tool was discovered"), "{empty}");
        // A corrupt snapshot degrades to "no tools", never a panic.
        assert_eq!(mcp_tool_count("{not json"), 0);
        assert_eq!(mcp_tool_count(MCP_SNAPSHOT), 1);
        assert!(mcp_catalog_block("{not json").is_none());
    }

    #[test]
    fn unknown_profile_names_are_rejected() {
        assert_eq!(normalize_profile(""), "coding");
        assert_eq!(normalize_profile(" coding "), "coding");
        assert_eq!(normalize_profile("PLANNER"), "planner");
        assert_eq!(normalize_profile("Frontend"), "frontend");
        assert_eq!(normalize_profile("backend"), "backend");
        assert_eq!(normalize_profile(" database "), "database");
        assert_eq!(normalize_profile("Debugger"), "debugger");
        assert_eq!(normalize_profile("root"), "");
    }

    #[test]
    fn stage_policy_accepts_only_fail_skip_or_blank() {
        assert_eq!(normalize_stage_policy("").unwrap(), "");
        assert_eq!(normalize_stage_policy("fail").unwrap(), "fail");
        assert_eq!(normalize_stage_policy(" SKIP ").unwrap(), "skip");
        assert!(normalize_stage_policy("chew").is_err());
    }

    #[test]
    fn run_modes_accept_default_hybrid_and_team() {
        assert_eq!(normalize_mode(""), Some(String::new()));
        assert_eq!(normalize_mode(" Hybrid "), Some("hybrid".to_string()));
        assert_eq!(normalize_mode("team"), Some("team".to_string()));
        assert_eq!(normalize_mode(" Team "), Some("team".to_string()));
        assert_eq!(normalize_mode("cloud"), None);
        assert_eq!(normalize_mode("local"), None);
    }

    #[test]
    fn reviewer_verdict_requires_an_explicit_approve_word() {
        let (approved, findings) = interpret_verdict("APPROVE - the goal is met.");
        assert!(approved);
        assert!(findings.contains("APPROVE"));
        // Any formatting the models actually produce still counts.
        let (approved, _) = interpret_verdict("```\nAPPROVE\n```");
        assert!(approved);
        // The verdict word must come first: “I approve.” buried in a sentence
        // is not an approval the executor can rely on.
        let (approved, _) = interpret_verdict("I approve.");
        assert!(!approved);
        // Ambiguous or missing verdict is ISSUES: an unverified approval would
        // be a lie, an extra repair round is only a slower run.
        let (approved, _) = interpret_verdict("The file looks fine to me.");
        assert!(!approved);
        let (approved, _) = interpret_verdict("");
        assert!(!approved);
        // Findings survive verbatim for the handoff step.
        let (approved, findings) = interpret_verdict("ISSUES: test missing");
        assert!(!approved);
        assert_eq!(findings, "ISSUES: test missing");
    }

    #[test]
    fn crew_defaults_to_executor_then_reviewer() {
        // No roster sent: the default line-up executes and then reviews.
        assert_eq!(
            parse_crew(&json!({}), "crew", false).unwrap(),
            vec!["coding", "review"]
        );
        // Crew bodies only exist for crew-mode runs.
        assert_eq!(
            parse_crew(&json!({ "crew": ["coding"] }), "", false).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn crew_validates_roles_counts_and_writers() {
        let err = parse_crew(&json!({ "crew": ["chef", "review"] }), "crew", false);
        assert!(err.unwrap_err().contains("Known roles"));
        let err = parse_crew(&json!({ "crew": [] }), "crew", false);
        assert!(err.unwrap_err().contains("1–6"));
        let err = parse_crew(&json!({ "crew": { "0": "coding" } }), "crew", false);
        assert!(err.unwrap_err().contains("JSON array"));
        // Exact profile names survive case and spacing differences.
        assert_eq!(
            parse_crew(&json!({ "crew": ["  CODING "] }), "crew", false).unwrap(),
            vec!["coding"]
        );
        // Writes allowed but no builder role in the line-up is refused.
        let err = parse_crew(&json!({ "crew": ["planner", "review"] }), "crew", true);
        assert!(err.unwrap_err().contains("no building agent"));
    }

    #[test]
    fn crew_stage_index_clamps_to_the_last_stage() {
        assert_eq!(crew_stage_index(0, 3), 0);
        assert_eq!(crew_stage_index(2, 3), 2);
        assert_eq!(crew_stage_index(99, 3), 2);
        assert_eq!(crew_stage_index(0, 0), 0);
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
            mode: String::new(),
            crew: String::new(),
            stage_policy: String::new(),
            stage_max_steps: 0,
            allow_mcp: false,
            mcp_tools: String::new(),
            tokens: 0,
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
            mode: String::new(),
            crew: String::new(),
            stage_policy: String::new(),
            stage_max_steps: 0,
            allow_mcp: false,
            mcp_tools: String::new(),
            tokens: 0,
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
        for (id, expected) in [
            ("a", INTERRUPTED),
            ("b", INTERRUPTED),
            ("c", SUCCEEDED),
            ("d", FAILED),
            ("e", CANCELLED),
        ] {
            let status: String = conn
                .query_row("SELECT status FROM agent_runs WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(status, expected, "{id}");
        }
    }

    #[tokio::test]
    async fn run_command_captures_stdout_and_stderr() {
        let dir = std::env::temp_dir().join(format!("saarthi-agent-cmd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = FsPath::new(&dir);
        let output = run_command(root, "echo marker-stdout; echo marker-stderr 1>&2")
            .await
            .unwrap();
        assert!(output.contains("marker-stdout"), "{output}");
        assert!(output.contains("marker-stderr"), "{output}");
        assert!(output.contains("exit: 0"), "{output}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn run_command_reports_the_real_exit_code() {
        let dir = std::env::temp_dir().join(format!("saarthi-agent-cmd2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let output = run_command(FsPath::new(&dir), "exit 7").await.unwrap();
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

    #[tokio::test]
    async fn write_action_creates_a_missing_parent_folder_inside_the_workspace() {
        // The 1.5B executor kept writing into subfolders that do not exist;
        // the write action creates the parent instead of failing the step.
        let salt = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let dir = std::env::temp_dir().join(format!("saarthi-agent-wparent-{}", salt));
        std::fs::create_dir_all(&dir).unwrap();
        let root = FsPath::new(&dir);
        let action = Action::Write {
            path: "src/even_check.py".into(),
            content: "def is_even(n):\n    return n % 2 == 0\n".into(),
        };
        let output = dispatch_action(root, &action).await.unwrap();
        assert!(output.starts_with("Wrote "), "{output}");
        assert_eq!(
            std::fs::read_to_string(dir.join("src/even_check.py")).unwrap(),
            "def is_even(n):\n    return n % 2 == 0\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
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
        let (checkpoint, old) = capture_checkpoint(root, "missing.txt");
        assert!(checkpoint.is_empty() && old.is_none());

        std::fs::write(dir.join("real.txt"), "previous content").unwrap();
        let (checkpoint, old) = capture_checkpoint(root, "real.txt");
        assert_eq!(old.as_deref(), Some("previous content"));
        let parsed: Value = serde_json::from_str(&checkpoint).unwrap();
        assert_eq!(parsed["path"], "real.txt");
        assert_eq!(parsed["old"], "previous content");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn steps_persist_checkpoints_and_initialize_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();
        conn.execute(
            "INSERT INTO agent_runs(id,project_id,goal,status,allow_writes,allow_commands,auto_approve,max_steps,created_at,updated_at) VALUES('r','','t','running',0,0,0,12,0,0)",
            [],
        )
        .unwrap();
        insert_step(
            &mut conn,
            "r",
            "write",
            "w",
            "out",
            "{\"path\":\"a\",\"old\":\"b\"}",
        )
        .unwrap();
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
        initialize(&conn).unwrap();
    }
    // ---------------------------------------------------------------
    // End-to-end orchestration: the real run loop against a mocked
    // router, exercising the crew handoff and the team review round.

    use std::path::PathBuf;
    use tower::ServiceExt;

    struct RunFixture {
        state: AppState,
        app: Router,
        root: PathBuf,
    }

    fn run_fixture(tag: &str, base_url: &str) -> RunFixture {
        let root =
            std::env::temp_dir().join(format!("saarthi-agent-e2e-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let state = AppState {
            config_path: root.join("config.json"),
            database_path: root.join("studio.db"),
            static_dir: root.join("static"),
            workspace: crate::workspace::WorkspaceStore::load(root.join("workspace.json")),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap(),
            terminal: crate::terminal::hub(),
            events: crate::events::Hub::new(),
            models_cache: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        };
        let _ = crate::ensure_setup(&state);
        // Point every model call at the scripted mock; local AI stays off.
        let mut cfg = crate::load_config(&state);
        cfg.base_url = base_url.to_string();
        cfg.api_key = "test-key".to_string();
        cfg.local_chat = false;
        let _ = crate::save_config_file(&state.config_path, &cfg);
        state
            .workspace
            .open(ws.to_str().unwrap())
            .expect("open workspace root");
        let app = router().with_state(state.clone());
        RunFixture { state, app, root }
    }

    /// Spawns a router that answers /chat/completions with the scripted
    /// replies in order; an empty queue answers with an honest `fail` so a
    /// script mismatch shows up as a failed run instead of a hang.
    async fn scripted_router(replies: &[&str]) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let queue = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(
            replies.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
        )));
        let app = Router::new().route(
            "/chat/completions",
            axum::routing::post(move |body: axum::Json<Value>| async move {
                // Whatever string we return here becomes the parsed action.
                let _ = body;
                let reply = queue.lock().unwrap().pop_front().unwrap_or_else(|| {
                    "{\"action\":\"fail\",\"reason\":\"mock out of replies\"}".to_string()
                });
                axum::Json(json!({
                    "choices": [{ "message": { "role": "assistant", "content": reply } }]
                }))
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}")
    }

    async fn start_run(app: &Router, body: Value) -> String {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/runs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["ok"], json!(true), "{payload}");
        payload["run"]["id"]
            .as_str()
            .expect("create_run returns an id")
            .to_string()
    }

    async fn wait_for_terminal(state: &AppState, id: &str) -> (String, Vec<String>) {
        // `cancelled` is terminal too — the helper used to ignore it, so a
        // genuinely cancelled run looked like a hang.
        let terminal = [SUCCEEDED, FAILED, INTERRUPTED, CANCELLED];
        for _ in 0..250 {
            if let Ok(conn) = db(state) {
                if let Ok(status) = conn.query_row::<String, _, _>(
                    "SELECT status FROM agent_runs WHERE id=?1",
                    [id],
                    |r| r.get(0),
                ) {
                    if terminal.contains(&status.as_str()) {
                        let mut stmt = conn
                            .prepare("SELECT action FROM agent_steps WHERE run_id=?1 ORDER BY seq")
                            .unwrap();
                        let actions: Vec<String> = stmt
                            .query_map([id], |row| row.get::<_, String>(0))
                            .unwrap()
                            .map(|res| res.expect("step row"))
                            .collect();
                        return (status, actions);
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("the run never reached a terminal state: {id}");
    }

    async fn wait_until_status(state: &AppState, id: &str, wanted: &str) {
        for _ in 0..250 {
            if let Ok(conn) = db(state) {
                if let Ok(status) = conn.query_row::<String, _, _>(
                    "SELECT status FROM agent_runs WHERE id=?1",
                    [id],
                    |r| r.get(0),
                ) {
                    if status == wanted {
                        return;
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("the run never reached {wanted}: {id}");
    }

    async fn post_resume(app: &Router, id: &str) -> (StatusCode, String) {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/api/runs/{id}/resume"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    #[tokio::test]
    async fn crew_run_hands_off_between_stages_and_finishes() {
        let base = scripted_router(&[
            // Stage 1 (coding): write and declare the stage done.
            "{\"action\":\"write\",\"path\":\"hello.txt\",\"content\":\"hi from coding\"}",
            "{\"action\":\"done\",\"summary\":\"hello.txt is written\"}",
            // Stage 2 (review): inspect and finish the whole run.
            "{\"action\":\"read\",\"path\":\"hello.txt\"}",
            "{\"action\":\"done\",\"summary\":\"the file content checks out\"}",
        ])
        .await;
        let fixture = run_fixture("crew", &base);
        let id = start_run(
            &fixture.app,
            json!({
                "goal": "write hello.txt, then have a reviewer verify it",
                "allowWrites": true,
                "autoApprove": true,
                "mode": "crew"
            }),
        )
        .await;
        let (status, actions) = wait_for_terminal(&fixture.state, &id).await;
        assert_eq!(status, SUCCEEDED, "actions: {actions:?}");
        assert_eq!(actions, ["write", "handoff", "read", "done"]);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("ws/hello.txt")).unwrap(),
            "hi from coding"
        );
        let conn = db(&fixture.state).unwrap();
        let handoff: String = conn
            .query_row(
                "SELECT summary FROM agent_steps WHERE run_id=?1 AND action='handoff'",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(handoff.contains("review agent takes over"), "{handoff}");
    }

    #[tokio::test]
    async fn crew_stage_budget_skip_records_the_incomplete_stage() {
        let base = scripted_router(&[
            // The coding stage never finishes: it only thinks.
            "{\"action\":\"think\",\"summary\":\"looking at the goal\"}",
            // Budget (1 step) hits; skip policy hands off to review.
            "{\"action\":\"done\",\"summary\":\"the file content checks out\"}",
        ])
        .await;
        let fixture = run_fixture("crew-budget-skip", &base);
        let id = start_run(
            &fixture.app,
            json!({
                "goal": "write hello.txt, then have a reviewer verify it",
                "allowWrites": true,
                "autoApprove": true,
                "mode": "crew",
                "stageMaxSteps": 1,
                "stagePolicy": "skip"
            }),
        )
        .await;
        let (status, actions) = wait_for_terminal(&fixture.state, &id).await;
        assert_eq!(status, SUCCEEDED, "actions: {actions:?}");
        assert_eq!(actions, ["think", "handoff", "done"]);
        let conn = db(&fixture.state).unwrap();
        let handoff: String = conn
            .query_row(
                "SELECT summary FROM agent_steps WHERE run_id=?1 AND action='handoff'",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(handoff.contains("budget exhausted"), "{handoff}");
    }

    #[tokio::test]
    async fn crew_stage_budget_fail_stops_the_run_honestly() {
        let base =
            scripted_router(&["{\"action\":\"think\",\"summary\":\"looking at the goal\"}"]).await;
        let fixture = run_fixture("crew-budget-fail", &base);
        let id = start_run(
            &fixture.app,
            json!({
                "goal": "write hello.txt, then have a reviewer verify it",
                "allowWrites": true,
                "autoApprove": true,
                "mode": "crew",
                "stageMaxSteps": 1
            }),
        )
        .await;
        let (status, actions) = wait_for_terminal(&fixture.state, &id).await;
        assert_eq!(status, FAILED, "actions: {actions:?}");
        assert_eq!(actions, ["think", "fail"]);
        let conn = db(&fixture.state).unwrap();
        let fail: String = conn
            .query_row(
                "SELECT output FROM agent_steps WHERE run_id=?1 AND action='fail'",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(fail.contains("without finishing"), "{fail}");
    }

    #[tokio::test]
    async fn crew_run_resumes_from_stage_after_budget_fail() {
        let base = scripted_router(&[
            // Stage 1 (coding) burns its whole budget on a write, then fails.
            "{\"action\":\"write\",\"path\":\"hello.txt\",\"content\":\"hi from coding\"}",
            // Resumed coding stage finishes; the reviewer then verifies.
            "{\"action\":\"done\",\"summary\":\"hello.txt is written\"}",
            "{\"action\":\"done\",\"summary\":\"the file content checks out\"}",
        ])
        .await;
        let fixture = run_fixture("crew-resume", &base);
        let id = start_run(
            &fixture.app,
            json!({
                "goal": "write hello.txt, then have a reviewer verify it",
                "allowWrites": true,
                "autoApprove": true,
                "mode": "crew",
                "stageMaxSteps": 1
            }),
        )
        .await;
        let (status, actions) = wait_for_terminal(&fixture.state, &id).await;
        assert_eq!(status, FAILED, "actions: {actions:?}");
        assert_eq!(actions, ["write", "fail"]);

        // The step log is the orchestrator state: resuming re-activates THIS
        // run and the coding stage gets a fresh budget after the marker.
        let (code, body) = post_resume(&fixture.app, &id).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        let (resumed_status, resumed_actions) = wait_for_terminal(&fixture.state, &id).await;
        assert_eq!(resumed_status, SUCCEEDED, "actions: {resumed_actions:?}");
        assert_eq!(
            resumed_actions,
            ["write", "fail", "resume", "handoff", "done"]
        );

        // A finished run must not be resumed again.
        let (code, body) = post_resume(&fixture.app, &id).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("already delivered"), "{body}");
    }

    #[tokio::test]
    async fn resume_rejects_live_and_empty_runs() {
        // A write that waits for approval keeps the run live.
        let base =
            scripted_router(&["{\"action\":\"write\",\"path\":\"a.txt\",\"content\":\"x\"}"]).await;
        let fixture = run_fixture("resume-live", &base);
        let id = start_run(
            &fixture.app,
            json!({
                "goal": "write a.txt",
                "allowWrites": true,
                "autoApprove": false,
                "mode": "crew",
                "stageMaxSteps": 9
            }),
        )
        .await;
        wait_until_status(&fixture.state, &id, AWAITING_APPROVAL).await;
        let (code, body) = post_resume(&fixture.app, &id).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("still live"), "{body}");
        let _ = fixture
            .app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/api/runs/{id}/cancel"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // A recorded-but-empty run has nothing to resume.
        let base = scripted_router(&[]).await;
        let fixture = run_fixture("resume-empty", &base);
        let conn = db(&fixture.state).unwrap();
        conn.execute(
            "INSERT INTO agent_runs(id,project_id,goal,status,allow_writes,allow_commands,auto_approve,max_steps,pending_action,error,created_at,updated_at) \
             VALUES('r-empty', 'p', 'a goal that never ran', 'failed', 0, 0, 0, 5, '', '', 1.0, 1.0)",
            params![],
        )
        .unwrap();
        let (code, body) = post_resume(&fixture.app, "r-empty").await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body}");
        assert!(body.contains("no steps"), "{body}");
    }

    #[tokio::test]
    async fn team_run_repairs_after_review_findings() {
        let base = scripted_router(&[
            "{\"action\":\"write\",\"path\":\"hi.txt\",\"content\":\"helo\"}",
            "{\"action\":\"done\",\"summary\":\"wrote hi.txt\"}",
            // Reviewer round 1: findings go back to the executor as a handoff.
            "ISSUES: hi.txt reads helo; it should read hello.",
            // Repair round: real replaced content, then done again.
            "{\"action\":\"write\",\"path\":\"hi.txt\",\"content\":\"hello\"}",
            "{\"action\":\"done\",\"summary\":\"fixed the spelling\"}",
            // Reviewer round 2 approves.
            "APPROVE - the finding is fixed.",
        ])
        .await;
        let fixture = run_fixture("team", &base);
        let id = start_run(
            &fixture.app,
            json!({
                "goal": "create hi.txt containing the word hello",
                "allowWrites": true,
                "autoApprove": true,
                "mode": "team"
            }),
        )
        .await;
        let (status, actions) = wait_for_terminal(&fixture.state, &id).await;
        assert_eq!(status, SUCCEEDED, "actions: {actions:?}");
        assert_eq!(
            actions,
            ["write", "review", "handoff", "write", "review", "done"]
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("ws/hi.txt")).unwrap(),
            "hello"
        );
    }

    #[tokio::test]
    async fn the_emergency_stop_cancels_every_live_run() {
        // Enough no-op think steps to outlive the stop: the mock answers the
        // same harmless reply until the store says cancelled.
        let reply = "{\"action\":\"think\",\"summary\":\"waiting to be stopped\"}";
        let replies: Vec<String> = std::iter::repeat(reply.to_string()).take(120).collect();
        let refs: Vec<&str> = replies.iter().map(String::as_str).collect();
        let base = scripted_router(&refs).await;
        let fixture = run_fixture("emergency-stop", &base);
        let id = start_run(
            &fixture.app,
            json!({
                "goal": "work until the emergency stop",
                "autoApprove": true,
                "maxSteps": 120
            }),
        )
        .await;
        wait_until_status(&fixture.state, &id, RUNNING).await;

        // One stop accounts for every live run and flips its record on the
        // spot, so anything polling the store sees `cancelled`, never a stale
        // `running`.
        let stopped = cancel_all_active(&fixture.state, "Test emergency stop.");
        assert_eq!(stopped, vec![id.clone()], "{stopped:?}");
        let (status, actions) = wait_for_terminal(&fixture.state, &id).await;
        assert_eq!(status, CANCELLED, "actions: {actions:?}");
        assert!(
            actions.contains(&"emergency-stop".to_string()),
            "actions: {actions:?}"
        );

        // A stopped run is no longer live: a second stop finds nothing.
        let again = cancel_all_active(&fixture.state, "Second stop.");
        assert!(again.is_empty(), "{again:?}");
    }

    /// The list query and `row_to_run` are two separate declarations of one
    /// column order. When they drifted apart every row failed to map and the
    /// Runs panel read "No runs yet" while the runs existed.
    #[test]
    fn the_runs_list_query_maps_every_column() {
        let path =
            std::env::temp_dir().join(format!("saarthi-runs-list-{}.db", uuid::Uuid::new_v4()));
        let conn = crate::open_db(&path).expect("temp db");
        initialize(&conn).expect("schema");
        conn.execute(
            "INSERT INTO agent_runs(id,project_id,goal,status,allow_writes,allow_commands,auto_approve,max_steps,pending_action,error,created_at,updated_at,router_config,model,profile,mode,crew,stage_policy,stage_max_steps,allow_mcp,mcp_tools) \
             VALUES('run-1','proj','the goal','succeeded',1,1,0,7,'','',10.0,11.0,'cfg','the-model','review','crew','[\"coding\"]','skip',5,1,'{\"tools\":[],\"errors\":[]}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agent_steps(run_id,seq,action,created_at) VALUES('run-1',1,'think',1.0)",
            [],
        )
        .unwrap();
        let mut stmt = conn.prepare(RUN_LIST_SQL).expect("the list SQL prepares");
        let (run, steps) = stmt
            .query_row([], |row| {
                let run = row_to_run(row)?;
                let steps: i64 = row.get(22)?;
                Ok((run, steps))
            })
            .expect("every list row maps");
        assert_eq!(run.model, "the-model");
        assert_eq!(run.profile, "review");
        assert_eq!(run.mode, "crew");
        assert_eq!(run.crew, "[\"coding\"]");
        assert_eq!(run.stage_policy, "skip");
        assert_eq!(run.stage_max_steps, 5);
        assert!(run.allow_mcp);
        assert_eq!(mcp_tool_count(&run.mcp_tools), 0);
        assert_eq!(steps, 1);
        let _ = std::fs::remove_file(&path);
    }

    /// Writes a registry with one server whose command cannot start, so the
    /// wiring is tested without needing a real MCP binary on every platform.
    fn write_ghost_server(state: &crate::AppState) {
        let registry = state.config_path.with_file_name("mcp.json");
        std::fs::write(
            &registry,
            json!({
                "servers": [{
                    "name": "ghost",
                    "command": "saarthi-no-such-mcp-server",
                    "args": [],
                    "enabled": true
                }]
            })
            .to_string(),
        )
        .expect("write registry");
    }

    async fn post_approve(app: &Router, id: &str, approve: bool) -> (StatusCode, String) {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/api/runs/{id}/approve"))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        json!({ "approve": approve }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    #[tokio::test]
    async fn an_mcp_run_pauses_for_approval_then_attempts_the_call() {
        let base = scripted_router(&[
            "{\"action\":\"think\",\"summary\":\"use the echo tool\"}",
            "{\"action\":\"mcp\",\"server\":\"ghost\",\"tool\":\"echo\",\"arguments\":{\"text\":\"hi\"}}",
            "{\"action\":\"done\",\"summary\":\"attempted the MCP call\"}",
        ])
        .await;
        let fixture = run_fixture("mcp-approve", &base);
        write_ghost_server(&fixture.state);
        let id = start_run(
            &fixture.app,
            json!({
                "goal": "call the echo tool",
                "allowMcp": true,
                "autoApprove": false,
                "maxSteps": 6
            }),
        )
        .await;
        wait_until_status(&fixture.state, &id, AWAITING_APPROVAL).await;
        // The pause names the tool and the arguments it would send.
        let pending: String = db(&fixture.state)
            .unwrap()
            .query_row(
                "SELECT pending_action FROM agent_runs WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(pending.contains("\"action\":\"mcp\""), "{pending}");
        assert!(pending.contains("ghost"), "{pending}");

        // Approving runs the call; a server that cannot start fails the run
        // with its reason instead of pretending a tool answered.
        let (code, body) = post_approve(&fixture.app, &id, true).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        let error: String = db(&fixture.state)
            .unwrap()
            .query_row("SELECT error FROM agent_runs WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(
            error.contains("Could not start MCP server") || error.contains("ghost"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_run_started_without_mcp_cannot_call_a_tool() {
        let base = scripted_router(&[
            "{\"action\":\"mcp\",\"server\":\"ghost\",\"tool\":\"echo\",\"arguments\":{}}",
        ])
        .await;
        let fixture = run_fixture("mcp-denied", &base);
        write_ghost_server(&fixture.state);
        let id = start_run(
            &fixture.app,
            json!({
                "goal": "call a tool this run never asked for",
                "autoApprove": true,
                "maxSteps": 4
            }),
        )
        .await;
        let (status, actions) = wait_for_terminal(&fixture.state, &id).await;
        assert_eq!(status, FAILED, "actions: {actions:?}");
        assert_eq!(actions, ["mcp"]);
        let error: String = db(&fixture.state)
            .unwrap()
            .query_row("SELECT error FROM agent_runs WHERE id=?1", [&id], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(error.contains("MCP tool calls"), "{error}");
    }
}
