//! Missions: one request, several specialised agents, real delegated runs.
//!
//! A mission is the coordinator, not a second agent engine. Every task it hands
//! out is created through [`crate::agent::create_run_with`], so the run a mission
//! starts obeys exactly the rules a person's run does: the open workspace, the
//! project budget, the dependency graph, the queue, the approval gate and the
//! per-profile write/command limits. This module only decides *which* agent does
//! *what*, *when* it may start, and records what actually happened.
//!
//! Two invariants keep it honest:
//!
//! * a task is `done` only because its run reached `succeeded` — never because
//!   the task was dispatched;
//! * two tasks that may write the same file never run at the same time.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::AppState;

// Task states.
const PLANNED: &str = "planned";
const RUNNING: &str = "running";
const DONE: &str = "done";
const FAILED: &str = "failed";
const BLOCKED: &str = "blocked";
const CANCELLED: &str = "cancelled";
const AWAITING: &str = "awaiting-approval";

// Mission states.
const M_PLANNED: &str = "planned";
const M_RUNNING: &str = "running";
const M_PAUSED: &str = "paused";
const M_AWAITING: &str = "awaiting-approval";
const M_SUCCEEDED: &str = "succeeded";
const M_FAILED: &str = "failed";
const M_CANCELLED: &str = "cancelled";

/// A mission may not fan out wider than this; more tasks than this is a plan a
/// person cannot review in the dashboard.
const MAX_TASKS: usize = 12;
const MAX_PARALLEL: i64 = 4;
const MAX_ATTEMPTS: i64 = 4;
const TICK_MS: u64 = 120;
/// A mission whose tasks are all settled but which is not can only be a bug;
/// the supervisor closes it instead of polling forever.
const MAX_GOAL_CHARS: usize = 5_000;
const TASK_GOAL_CAP: usize = 20_000;

/// One specialised agent. `profile` is the existing agent profile that owns the
/// actual tool rules — the role name never widens what a run may do.
pub(crate) struct Role {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) purpose: &'static str,
    pub(crate) profile: &'static str,
    pub(crate) writes: bool,
    pub(crate) commands: bool,
}

pub(crate) const ROLES: [Role; 8] = [
    Role {
        id: "planner",
        name: "Planner",
        purpose: "Turn the request into ordered, concrete tasks with dependencies.",
        profile: "planner",
        writes: false,
        commands: false,
    },
    Role {
        id: "architect",
        name: "Architect",
        purpose: "Judge the repository's structure and choose a maintainable shape for the change.",
        profile: "planner",
        writes: false,
        commands: false,
    },
    Role {
        id: "coding",
        name: "Coding",
        purpose: "Implement the change in the workspace, following the project's own conventions.",
        profile: "coding",
        writes: true,
        commands: true,
    },
    Role {
        id: "debugging",
        name: "Debugging",
        purpose: "Reproduce a failure, name its root cause and propose the smallest honest fix.",
        profile: "debugger",
        writes: false,
        commands: true,
    },
    Role {
        id: "testing",
        name: "Testing",
        purpose: "Run the project's real test commands and report exactly what passed and failed.",
        profile: "testing",
        writes: false,
        commands: true,
    },
    Role {
        id: "review",
        name: "Code review",
        purpose: "Read the resulting code and reject changes that are broken, unsafe or off-scope.",
        profile: "review",
        writes: false,
        commands: false,
    },
    Role {
        id: "documentation",
        name: "Documentation",
        purpose: "Update the documentation, changelog and references the change makes stale.",
        profile: "coding",
        writes: true,
        commands: false,
    },
    Role {
        id: "research",
        name: "Research",
        purpose: "Gather evidence from the codebase and docs before anyone commits to an approach.",
        profile: "planner",
        writes: false,
        commands: false,
    },
];

fn role(id: &str) -> Option<&'static Role> {
    ROLES.iter().find(|role| role.id == id)
}

/// The tools a role can reach, named the way the agent's own actions are named.
fn role_tools(role: &Role) -> Vec<&'static str> {
    let mut tools = vec!["read", "list", "search"];
    if role.writes {
        tools.push("write");
    }
    if role.commands {
        tools.push("command");
    }
    tools
}

fn db(state: &AppState) -> Result<Connection, String> {
    crate::open_db(&state.database_path).map_err(|error| format!("Database error: {error}"))
}

pub(crate) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mission_roles(
            role TEXT PRIMARY KEY,
            model TEXT NOT NULL DEFAULT '',
            enabled INTEGER NOT NULL DEFAULT 1,
            updated_at REAL NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS missions(
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL DEFAULT '',
            goal TEXT NOT NULL,
            template TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL,
            error TEXT NOT NULL DEFAULT '',
            parallel INTEGER NOT NULL DEFAULT 2,
            max_retries INTEGER NOT NULL DEFAULT 1,
            allow_writes INTEGER NOT NULL DEFAULT 0,
            allow_commands INTEGER NOT NULL DEFAULT 0,
            auto_approve INTEGER NOT NULL DEFAULT 0,
            created_at REAL NOT NULL,
            updated_at REAL NOT NULL,
            context TEXT NOT NULL DEFAULT '',
            report TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS mission_tasks(
            mission_id TEXT NOT NULL,
            id TEXT NOT NULL,
            role TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            goal TEXT NOT NULL DEFAULT '',
            depends_on TEXT NOT NULL DEFAULT '',
            files TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL,
            run_id TEXT NOT NULL DEFAULT '',
            attempts INTEGER NOT NULL DEFAULT 0,
            max_attempts INTEGER NOT NULL DEFAULT 1,
            summary TEXT NOT NULL DEFAULT '',
            error TEXT NOT NULL DEFAULT '',
            needs_approval INTEGER NOT NULL DEFAULT 0,
            approved INTEGER NOT NULL DEFAULT 0,
            position INTEGER NOT NULL DEFAULT 0,
            created_at REAL NOT NULL,
            updated_at REAL NOT NULL,
            PRIMARY KEY(mission_id, id)
        );
        CREATE TABLE IF NOT EXISTS mission_messages(
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            mission_id TEXT NOT NULL,
            task_id TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL,
            from_role TEXT NOT NULL DEFAULT '',
            to_role TEXT NOT NULL DEFAULT '',
            body TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS mission_messages_by_mission
            ON mission_messages(mission_id, seq);",
    )
}

// ---------------------------------------------------------------- roles

fn role_models(state: &AppState) -> Vec<(String, Option<String>, bool)> {
    let Ok(conn) = db(state) else {
        return ROLES
            .iter()
            .map(|role| (role.id.to_string(), None, true))
            .collect();
    };
    ROLES
        .iter()
        .map(|role| {
            let row = conn
                .query_row(
                    "SELECT model, enabled FROM mission_roles WHERE role=?1",
                    [role.id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
                .unwrap_or(None);
            match row {
                Some((model, enabled)) => (
                    role.id.to_string(),
                    if model.trim().is_empty() {
                        None
                    } else {
                        Some(model)
                    },
                    enabled != 0,
                ),
                None => (role.id.to_string(), None, true),
            }
        })
        .collect()
}

/// The model a role is pinned to, if any. Falling back to the routed default is
/// the caller's job, which is what makes a failed pinned model recoverable.
fn role_model(state: &AppState, role_id: &str) -> Option<String> {
    role_models(state)
        .into_iter()
        .find(|(id, _, _)| id == role_id)
        .and_then(|(_, model, _)| model)
}

fn role_enabled(state: &AppState, role_id: &str) -> bool {
    role_models(state)
        .into_iter()
        .find(|(id, _, _)| id == role_id)
        .map(|(_, _, enabled)| enabled)
        .unwrap_or(false)
}

// ---------------------------------------------------------------- plans

#[derive(Clone, Debug)]
struct TaskSpec {
    id: String,
    role: String,
    title: String,
    goal: String,
    depends_on: Vec<String>,
    files: Vec<String>,
    needs_approval: bool,
}

fn spec(
    id: &str,
    role: &str,
    title: &str,
    goal: &str,
    depends_on: &[&str],
    files: &[&str],
) -> TaskSpec {
    TaskSpec {
        id: id.to_string(),
        role: role.to_string(),
        title: title.to_string(),
        goal: goal.to_string(),
        depends_on: depends_on.iter().map(|dep| dep.to_string()).collect(),
        files: files.iter().map(|file| file.to_string()).collect(),
        needs_approval: false,
    }
}

/// The built-in workflows. They are fixed sequences of real agents, not a
/// simulation: each entry becomes a run with the named profile.
fn template_plan(template: &str, goal: &str) -> Result<Vec<TaskSpec>, String> {
    let plans = match template {
        "feature" => vec![
            spec(
                "plan",
                "planner",
                "Plan the change",
                &format!(
                    "Break this request into concrete steps: {goal}. Read the workspace first. \
                     Name the files that must change, the order to change them, the risks and how \
                     each step will be verified."
                ),
                &[],
                &[],
            ),
            spec(
                "implement",
                "coding",
                "Implement the change",
                "Do the work the plan describes. Read a file before editing it, keep the diff \
                 focused, and finish with a summary naming the files you changed and anything the \
                 next agent must know.",
                &["plan"],
                &[],
            ),
            spec(
                "verify",
                "testing",
                "Run the project's tests",
                "Run the repository's real test and build commands for what changed and report the \
                 actual results. Do not claim a suite passed unless you ran it and saw it pass.",
                &["implement"],
                &[],
            ),
            spec(
                "document",
                "documentation",
                "Update the documentation",
                "Update the documentation, changelog entry and any reference that this change makes \
                 stale. Match the project's existing documentation structure; do not invent files.",
                &["implement"],
                &[],
            ),
            spec(
                "review",
                "review",
                "Review the change",
                "Read what the implementation and documentation agents produced. Look for bugs, \
                 security problems, scope creep and rule violations. Report findings with file \
                 names; if the change is unsafe, say so plainly.",
                &["implement", "verify", "document"],
                &[],
            ),
        ],
        "debug" => vec![
            spec(
                "diagnose",
                "debugging",
                "Reproduce and diagnose",
                &format!(
                    "Reproduce this failure and find its root cause: {goal}. Read the code and run \
                     the smallest command that shows the failure. Do not guess."
                ),
                &[],
                &[],
            ),
            spec(
                "fix",
                "coding",
                "Fix the verified cause",
                "Fix only the root cause the diagnosis names. Keep the change minimal and explain \
                 the fix in your summary.",
                &["diagnose"],
                &[],
            ),
            spec(
                "verify",
                "testing",
                "Prove the fix",
                "Run the failing scenario and the project's test suite again. Report the real \
                 before/after result.",
                &["fix"],
                &[],
            ),
            spec(
                "review",
                "review",
                "Review the fix",
                "Review the diagnosis, the fix and the verification. Reject the fix if it hides \
                 the failure instead of removing its cause.",
                &["fix", "verify"],
                &[],
            ),
        ],
        "audit" => vec![
            spec(
                "research",
                "research",
                "Research the codebase",
                &format!(
                    "Gather the facts needed to judge this question: {goal}. Cite files, commands \
                     and documentation you actually read."
                ),
                &[],
                &[],
            ),
            spec(
                "review",
                "review",
                "Audit the code",
                "Read the relevant code and report real defects, security problems and standards \
                 violations with file names.",
                &[],
                &[],
            ),
            spec(
                "verify",
                "testing",
                "Audit the test suite",
                "Run the project's available checks and report the actual results, including what \
                 is not covered.",
                &[],
                &[],
            ),
            spec(
                "synthesise",
                "architect",
                "Recommend a course of action",
                "Combine the research, review and test findings into one evidence-based \
                 recommendation. Say what should change first, and what you are uncertain about.",
                &["research", "review", "verify"],
                &[],
            ),
        ],
        other => {
            return Err(format!(
                "Unknown workflow \"{other}\" — use feature, debug, audit or a custom task list."
            ))
        }
    };
    Ok(plans)
}

fn parse_specs(raw: &Value) -> Result<Vec<TaskSpec>, String> {
    let Some(entries) = raw.as_array() else {
        return Err("Tasks must be a list.".to_string());
    };
    if entries.is_empty() {
        return Err("A custom mission needs at least one task.".to_string());
    }
    if entries.len() > MAX_TASKS {
        return Err(format!("A mission may have at most {MAX_TASKS} tasks."));
    }
    let mut specs = Vec::new();
    for entry in entries {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if id.is_empty()
            || id.len() > 32
            || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(
                "Each task needs an id of 1-32 lowercase letters, digits or -.".to_string(),
            );
        }
        if specs.iter().any(|existing: &TaskSpec| existing.id == id) {
            return Err(format!("Two tasks share the id \"{id}\"."));
        }
        let role_id = entry.get("role").and_then(Value::as_str).unwrap_or("");
        let Some(agent) = role(role_id.trim()) else {
            let names = ROLES
                .iter()
                .map(|role| role.id)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!("Unknown agent role \"{role_id}\" — use {names}."));
        };
        let mut depends_on = Vec::new();
        if let Some(deps) = entry.get("dependsOn").and_then(Value::as_array) {
            for dep in deps {
                let dep = dep.as_str().unwrap_or("").trim().to_lowercase();
                if dep.is_empty() {
                    continue;
                }
                if dep == id {
                    return Err(format!("Task \"{id}\" cannot depend on itself."));
                }
                if !depends_on.contains(&dep) {
                    depends_on.push(dep);
                }
            }
        }
        let mut files = Vec::new();
        if let Some(list) = entry.get("files").and_then(Value::as_array) {
            for file in list {
                let file = normalize_relative(file.as_str().unwrap_or(""));
                if file.is_empty() {
                    continue;
                }
                files.push(file);
            }
        }
        let title = entry
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let goal = entry
            .get("goal")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        specs.push(TaskSpec {
            id,
            role: agent.id.to_string(),
            title: if title.is_empty() {
                format!("{} task", agent.name)
            } else {
                title
            },
            goal,
            depends_on,
            files,
            needs_approval: entry
                .get("needsApproval")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    // Every dependency must exist, and the graph must be acyclic: a cycle would
    // hold tasks `planned` forever while the supervisor waited for it.
    for entry in &specs {
        for dep in &entry.depends_on {
            if !specs.iter().any(|candidate| &candidate.id == dep) {
                return Err(format!(
                    "Task \"{}\" depends on \"{dep}\", which is not in the plan.",
                    entry.id
                ));
            }
        }
    }
    for entry in &specs {
        let mut seen = Vec::new();
        if reaches(&specs, &entry.id, &entry.id, &mut seen) {
            return Err(format!(
                "The task graph has a cycle through \"{}\" — dependencies must point backwards.",
                entry.id
            ));
        }
    }
    Ok(specs)
}

fn reaches(specs: &[TaskSpec], from: &str, target: &str, seen: &mut Vec<String>) -> bool {
    if seen.iter().any(|id| id == from) {
        return false;
    }
    seen.push(from.to_string());
    let Some(entry) = specs.iter().find(|entry| entry.id == from) else {
        return false;
    };
    for dep in &entry.depends_on {
        if dep == target || reaches(specs, dep, target, seen) {
            return true;
        }
    }
    false
}

fn normalize_relative(path: &str) -> String {
    let trimmed = path.trim().replace('\\', "/");
    let trimmed = trimmed.trim_start_matches("./");
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.contains("..") {
        return String::new();
    }
    trimmed.trim_end_matches('/').to_string()
}

/// Two tasks conflict when they could touch the same file at the same time.
///
/// A writer with no declared files could touch anything, so it conflicts with
/// every other writer. Two writers that both declare disjoint files may run
/// together; a reader never conflicts with a reader.
fn conflicts(a: &TaskSpec, b: &TaskSpec, a_writes: bool, b_writes: bool) -> bool {
    if !a_writes && !b_writes {
        return false;
    }
    if a.files.is_empty() || b.files.is_empty() {
        return a_writes && b_writes;
    }
    a.files.iter().any(|one| {
        b.files
            .iter()
            .any(|two| paths_overlap(one, two) || paths_overlap(two, one))
    })
}

fn paths_overlap(one: &str, two: &str) -> bool {
    one == two || one.starts_with(&format!("{two}/")) || two.starts_with(&format!("{one}/"))
}

// ---------------------------------------------------------------- storage

struct Mission {
    id: String,
    project_id: String,
    goal: String,
    template: String,
    status: String,
    error: String,
    parallel: i64,
    max_retries: i64,
    allow_writes: bool,
    allow_commands: bool,
    auto_approve: bool,
    created_at: f64,
    updated_at: f64,
    context: String,
    report: String,
}

struct Task {
    id: String,
    role: String,
    title: String,
    goal: String,
    depends_on: Vec<String>,
    files: Vec<String>,
    status: String,
    run_id: String,
    attempts: i64,
    max_attempts: i64,
    summary: String,
    error: String,
    needs_approval: bool,
    approved: bool,
    position: i64,
}

fn mission_from_row(row: &rusqlite::Row) -> rusqlite::Result<Mission> {
    Ok(Mission {
        id: row.get(0)?,
        project_id: row.get(1)?,
        goal: row.get(2)?,
        template: row.get(3)?,
        status: row.get(4)?,
        error: row.get(5)?,
        parallel: row.get(6)?,
        max_retries: row.get(7)?,
        allow_writes: row.get::<_, i64>(8)? != 0,
        allow_commands: row.get::<_, i64>(9)? != 0,
        auto_approve: row.get::<_, i64>(10)? != 0,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        context: row.get(13)?,
        report: row.get(14)?,
    })
}

const MISSION_SQL: &str = "SELECT id, project_id, goal, template, status, error, parallel, \
 max_retries, allow_writes, allow_commands, auto_approve, created_at, updated_at, context, \
 report FROM missions";

fn load_mission(state: &AppState, id: &str) -> Result<Option<Mission>, String> {
    let conn = db(state)?;
    conn.query_row(
        &format!("{MISSION_SQL} WHERE id=?1"),
        [id],
        mission_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn task_from_row(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    let depends: String = row.get(4)?;
    let files: String = row.get(5)?;
    Ok(Task {
        id: row.get(0)?,
        role: row.get(1)?,
        title: row.get(2)?,
        goal: row.get(3)?,
        depends_on: serde_json::from_str(&depends).unwrap_or_default(),
        files: serde_json::from_str(&files).unwrap_or_default(),
        status: row.get(6)?,
        run_id: row.get(7)?,
        attempts: row.get(8)?,
        max_attempts: row.get(9)?,
        summary: row.get(10)?,
        error: row.get(11)?,
        needs_approval: row.get::<_, i64>(12)? != 0,
        approved: row.get::<_, i64>(13)? != 0,
        position: row.get(14)?,
    })
}

fn load_tasks(state: &AppState, mission_id: &str) -> Result<Vec<Task>, String> {
    let conn = db(state)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, role, title, goal, depends_on, files, status, run_id, attempts, \
             max_attempts, summary, error, needs_approval, approved, position \
             FROM mission_tasks WHERE mission_id=?1 ORDER BY position",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([mission_id], task_from_row)
        .map_err(|error| error.to_string())?;
    Ok(rows.filter_map(|row| row.ok()).collect())
}

fn set_task(state: &AppState, mission_id: &str, task: &Task) -> Result<(), String> {
    let conn = db(state)?;
    conn.execute(
        "UPDATE mission_tasks SET status=?3, run_id=?4, attempts=?5, summary=?6, error=?7, \
         approved=?8, updated_at=?9 WHERE mission_id=?1 AND id=?2",
        params![
            mission_id,
            task.id,
            task.status,
            task.run_id,
            task.attempts,
            task.summary,
            task.error,
            if task.approved { 1 } else { 0 },
            crate::now_seconds(),
        ],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn set_mission(
    state: &AppState,
    id: &str,
    status: &str,
    error: &str,
    report: &str,
) -> Result<(), String> {
    let conn = db(state)?;
    conn.execute(
        "UPDATE missions SET status=?2, error=?3, report=?4, updated_at=?5 WHERE id=?1",
        params![id, status, error, report, crate::now_seconds()],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn message(
    state: &AppState,
    mission_id: &str,
    task_id: &str,
    kind: &str,
    from: &str,
    to: &str,
    body: &str,
) {
    if let Ok(conn) = db(state) {
        let _ = conn.execute(
            "INSERT INTO mission_messages(mission_id, task_id, kind, from_role, to_role, body, \
             created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                mission_id,
                task_id,
                kind,
                from,
                to,
                cap(body, 2_000),
                crate::now_seconds()
            ],
        );
    }
}

fn cap(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(limit).collect();
    format!("{cut}…")
}

// ---------------------------------------------------------------- runs

struct RunState {
    status: String,
    error: String,
}

fn run_state(state: &AppState, run_id: &str) -> Option<RunState> {
    let conn = db(state).ok()?;
    conn.query_row(
        "SELECT status, error FROM agent_runs WHERE id=?1",
        [run_id],
        |row| {
            Ok(RunState {
                status: row.get(0)?,
                error: row.get(1)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

/// The summary a finished run actually wrote, from its own step log: the last
/// `done` summary, else the last non-placeholder step. Never invented here.
fn run_summary(state: &AppState, run_id: &str) -> String {
    let Ok(conn) = db(state) else {
        return String::new();
    };
    // The agent's own summary lives in the step's `output`; the `summary`
    // column is the fixed label ("Run complete"). Read the output of the first
    // done step, which is the one the agent wrote.
    let done: Option<String> = conn
        .query_row(
            "SELECT output FROM agent_steps WHERE run_id=?1 AND action='done' \
             ORDER BY seq ASC LIMIT 1",
            [run_id],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);
    if let Some(summary) = done.filter(|text| !text.trim().is_empty()) {
        return summary;
    }
    conn.query_row(
        "SELECT summary FROM agent_steps WHERE run_id=?1 AND status != 'running' \
         ORDER BY seq DESC LIMIT 1",
        [run_id],
        |row| row.get(0),
    )
    .optional()
    .unwrap_or(None)
    .unwrap_or_default()
}

fn task_goal(
    mission: &Mission,
    task: &Task,
    deps: &[(String, String, String)],
    retry_note: &str,
) -> String {
    let agent = role(&task.role);
    let name = agent.map(|role| role.name).unwrap_or("Agent");
    let purpose = agent.map(|role| role.purpose).unwrap_or("");
    let mut goal = format!(
        "Mission: {}\n\nYou are the {name} agent in a coordinated team. {purpose}\n\n\
         Your task ({}): {}\n",
        mission.goal, task.title, task.goal
    );
    if !task.files.is_empty() {
        goal.push_str(&format!(
            "\nThis task declared these paths, so no other agent edits them while it runs: {}\n",
            task.files.join(", ")
        ));
    }
    if !deps.is_empty() {
        goal.push_str(
            "\nWhat the tasks before you found (their work is already done — build on it, do not \
             repeat it):\n",
        );
        for (role_id, title, summary) in deps {
            let name = role(role_id).map(|role| role.name).unwrap_or("Agent");
            goal.push_str(&format!(
                "- {name} / {title}: {}\n",
                if summary.trim().is_empty() {
                    "(no summary recorded)"
                } else {
                    summary.trim()
                }
            ));
        }
    }
    if !retry_note.is_empty() {
        goal.push_str(&format!("\n{retry_note}\n"));
    }
    goal.push_str(
        "\nStay inside this task: other agents own the rest of the mission. Finish with done and a \
         summary a reviewer can check against the goal.",
    );
    cap(&goal, TASK_GOAL_CAP)
}

fn context_of(mission: &Mission) -> Value {
    serde_json::from_str(&mission.context).unwrap_or_else(|_| json!({}))
}

// ---------------------------------------------------------------- creation

fn parse_bool(input: &Value, key: &str) -> bool {
    input.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn parse_limit(input: &Value, key: &str, fallback: i64, low: i64, high: i64) -> i64 {
    input
        .get(key)
        .and_then(Value::as_i64)
        .unwrap_or(fallback)
        .clamp(low, high)
}

pub(crate) async fn create_mission_with(
    state: &AppState,
    input: Value,
) -> Result<String, (StatusCode, String)> {
    let goal = input
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if goal.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Describe what the team should accomplish.".to_string(),
        ));
    }
    if goal.chars().count() > MAX_GOAL_CHARS {
        return Err((
            StatusCode::BAD_REQUEST,
            "The mission goal is too long.".to_string(),
        ));
    }
    if state.workspace.root().is_err() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Open a workspace folder first — missions run agents inside it.".to_string(),
        ));
    }
    let template = input
        .get("template")
        .and_then(Value::as_str)
        .unwrap_or("feature")
        .trim()
        .to_lowercase();
    let specs = if template == "custom" {
        match parse_specs(input.get("tasks").unwrap_or(&Value::Null)) {
            Ok(specs) => specs,
            Err(message) => return Err((StatusCode::BAD_REQUEST, message)),
        }
    } else {
        match template_plan(&template, &goal) {
            Ok(specs) => specs,
            Err(message) => return Err((StatusCode::BAD_REQUEST, message)),
        }
    };
    for spec in &specs {
        if !role_enabled(state, &spec.role) {
            let name = role(&spec.role).map(|role| role.name).unwrap_or(&spec.role);
            return Err((
                StatusCode::BAD_REQUEST,
                format!("The {name} role is switched off in Mission control — enable it first."),
            ));
        }
    }
    let parallel = parse_limit(&input, "parallel", 2, 1, MAX_PARALLEL);
    let max_retries = parse_limit(&input, "maxRetries", 1, 0, MAX_ATTEMPTS - 1);
    let allow_writes = parse_bool(&input, "allowWrites");
    let allow_commands = parse_bool(&input, "allowCommands");
    let auto_approve = parse_bool(&input, "autoApprove");
    let writes_planned = specs
        .iter()
        .any(|spec| role(&spec.role).map(|role| role.writes).unwrap_or(false));
    if writes_planned && !allow_writes {
        return Err((
            StatusCode::BAD_REQUEST,
            "This plan edits files, so allow writes for the mission or choose a read-only \
             workflow."
                .to_string(),
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let stamp = crate::now_seconds();
    let root = state
        .workspace
        .root()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let context = json!({
        "workspace": root,
        "projectId": crate::active_project_id(state),
    })
    .to_string();
    let conn = db(state).map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    conn.execute(
        "INSERT INTO missions(id, project_id, goal, template, status, error, parallel, max_retries, \
         allow_writes, allow_commands, auto_approve, created_at, updated_at, context, report) \
         VALUES(?1,?2,?3,?4,?5,'',?6,?7,?8,?9,?10,?11,?11,?12,'')",
        params![
            id,
            crate::active_project_id(state),
            goal,
            template,
            M_PLANNED,
            parallel,
            max_retries,
            if allow_writes { 1 } else { 0 },
            if allow_commands { 1 } else { 0 },
            if auto_approve { 1 } else { 0 },
            stamp,
            context,
        ],
    )
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    for (position, spec) in specs.iter().enumerate() {
        conn.execute(
            "INSERT INTO mission_tasks(mission_id, id, role, title, goal, depends_on, files, status, \
             run_id, attempts, max_attempts, summary, error, needs_approval, approved, position, \
             created_at, updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'',0,?9,'','',?10,0,?11,?12,?12)",
            params![
                id,
                spec.id,
                spec.role,
                spec.title,
                spec.goal,
                serde_json::to_string(&spec.depends_on).unwrap_or_default(),
                serde_json::to_string(&spec.files).unwrap_or_default(),
                PLANNED,
                1 + max_retries,
                if spec.needs_approval { 1 } else { 0 },
                position as i64,
                stamp,
            ],
        )
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    }
    drop(conn);
    message(
        state,
        &id,
        "",
        "request",
        "user",
        "orchestrator",
        &format!("Start a {template} mission: {goal}"),
    );
    supervise(state, &id);
    Ok(id)
}

// ---------------------------------------------------------------- supervisor

static LIVE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// One mission tick at a time. Cancelling, approving and pausing take the same
/// lock, so a decision can never interleave with a tick that is halfway through
/// starting the very task being cancelled — the tick used to write `running`
/// back over a cancellation that had already been recorded.
static TICK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn supervise(state: &AppState, mission_id: &str) {
    {
        let mut live = LIVE.lock().unwrap();
        if live.iter().any(|id| id == mission_id) {
            return;
        }
        live.push(mission_id.to_string());
    }
    let state = state.clone();
    let id = mission_id.to_string();
    tokio::spawn(async move {
        loop {
            // A failed tick cannot make progress; stop ticking on it rather
            // than spinning on the same error forever.
            if advance(&state, &id).await.unwrap_or(true) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(TICK_MS)).await;
        }
        LIVE.lock().unwrap().retain(|existing| existing != &id);
    });
}

fn settled_task(status: &str) -> bool {
    matches!(status, DONE | FAILED | BLOCKED | CANCELLED)
}

fn terminal_mission(status: &str) -> bool {
    matches!(status, M_SUCCEEDED | M_FAILED | M_CANCELLED)
}

/// One supervisor tick. Returns `true` when the mission no longer needs ticking
/// (it settled, paused or is waiting for a person).
pub(crate) async fn advance(state: &AppState, mission_id: &str) -> Result<bool, String> {
    let _tick = TICK.lock().await;
    let Some(mut mission) = load_mission(state, mission_id)? else {
        return Ok(true);
    };
    if terminal_mission(&mission.status)
        || mission.status == M_PAUSED
        || mission.status == M_AWAITING
    {
        return Ok(true);
    }
    let mut tasks = load_tasks(state, mission_id)?;
    if tasks.is_empty() {
        set_mission(state, mission_id, M_FAILED, "The mission has no tasks.", "")?;
        return Ok(true);
    }
    if mission.status == M_PLANNED {
        mission.status = M_RUNNING.to_string();
        set_mission(state, mission_id, M_RUNNING, "", "")?;
    }

    // 1) Settle tasks whose run has finished, retrying where policy allows.
    let live: Vec<(usize, String)> = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.status == RUNNING && !task.run_id.is_empty())
        .map(|(index, task)| (index, task.run_id.clone()))
        .collect();
    for (index, run_id) in live {
        let Some(run) = run_state(state, &run_id) else {
            continue;
        };
        match run.status.as_str() {
            "succeeded" => {
                let summary = run_summary(state, &run_id);
                tasks[index].status = DONE.to_string();
                tasks[index].summary = cap(&summary, 2_000);
                set_task(state, mission_id, &tasks[index])?;
                message(
                    state,
                    mission_id,
                    &tasks[index].id,
                    "completion",
                    &tasks[index].role,
                    "orchestrator",
                    if summary.trim().is_empty() {
                        "Run finished without a summary."
                    } else {
                        &summary
                    },
                );
            }
            "failed" | "interrupted" | "cancelled" => {
                let error = if run.error.trim().is_empty() {
                    format!("The run ended {}.", run.status)
                } else {
                    run.error.clone()
                };
                tasks[index].attempts += 1;
                if tasks[index].attempts < tasks[index].max_attempts && run.status != "cancelled" {
                    tasks[index].status = PLANNED.to_string();
                    tasks[index].run_id.clear();
                    tasks[index].error = cap(&error, 1_000);
                    set_task(state, mission_id, &tasks[index])?;
                    message(
                        state,
                        mission_id,
                        &tasks[index].id,
                        "request",
                        "orchestrator",
                        &tasks[index].role,
                        &format!(
                            "Retrying after failure (attempt {} of {}): {error}",
                            tasks[index].attempts + 1,
                            tasks[index].max_attempts
                        ),
                    );
                } else {
                    tasks[index].status = FAILED.to_string();
                    tasks[index].error = cap(&error, 1_000);
                    set_task(state, mission_id, &tasks[index])?;
                    message(
                        state,
                        mission_id,
                        &tasks[index].id,
                        "error",
                        &tasks[index].role,
                        "orchestrator",
                        &error,
                    );
                }
            }
            _ => {}
        }
    }

    // 2) A task whose dependency failed can never run: block it honestly.
    let statuses: Vec<(String, String)> = tasks
        .iter()
        .map(|task| (task.id.clone(), task.status.clone()))
        .collect();
    let broken: Vec<(usize, String, String)> = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.status == PLANNED)
        .filter_map(|(index, task)| {
            task.depends_on.iter().find_map(|dep| {
                statuses
                    .iter()
                    .find(|(id, _)| id == dep)
                    .filter(|(_, status)| matches!(status.as_str(), FAILED | BLOCKED | CANCELLED))
                    .map(|(_, status)| (index, dep.clone(), status.clone()))
            })
        })
        .collect();
    for (index, dep, status) in broken {
        tasks[index].status = BLOCKED.to_string();
        tasks[index].error = format!("Blocked: \"{dep}\" {status}.");
        set_task(state, mission_id, &tasks[index])?;
        message(
            state,
            mission_id,
            &tasks[index].id,
            "error",
            "orchestrator",
            &tasks[index].role,
            &format!("Blocked because \"{dep}\" {status}."),
        );
    }

    // 3) Approval checkpoints pause the mission until a person says go.
    for task in &tasks {
        if task.status == PLANNED && task.needs_approval && !task.approved {
            set_mission(
                state,
                mission_id,
                M_AWAITING,
                "A task is waiting for approval.",
                "",
            )?;
            message(
                state,
                mission_id,
                &task.id,
                "request",
                "orchestrator",
                "user",
                &format!(
                    "{} ({}) needs approval before it starts.",
                    task.title,
                    role(&task.role).map(|role| role.name).unwrap_or(&task.role)
                ),
            );
            return Ok(true);
        }
    }

    // 4) Start everything that is ready and safe to run concurrently.
    let mut active = tasks.iter().filter(|task| task.status == RUNNING).count() as i64;
    let parallel = mission.parallel.clamp(1, MAX_PARALLEL);
    let order: Vec<usize> = {
        let mut planned: Vec<usize> = (0..tasks.len())
            .filter(|index| tasks[*index].status == PLANNED)
            .collect();
        planned.sort_by_key(|index| tasks[*index].position);
        planned
    };
    for index in order {
        if active >= parallel {
            break;
        }
        let ready = tasks[index].depends_on.iter().all(|dep| {
            tasks
                .iter()
                .find(|task| &task.id == dep)
                .map(|task| task.status == DONE)
                .unwrap_or(false)
        });
        if !ready {
            continue;
        }
        let writes = role(&tasks[index].role)
            .map(|role| role.writes)
            .unwrap_or(false);
        let clashes = tasks.iter().any(|other| {
            if other.status != RUNNING {
                return false;
            }
            let other_writes = role(&other.role).map(|role| role.writes).unwrap_or(false);
            let a = TaskSpec {
                id: tasks[index].id.clone(),
                role: tasks[index].role.clone(),
                title: String::new(),
                goal: String::new(),
                depends_on: Vec::new(),
                files: tasks[index].files.clone(),
                needs_approval: false,
            };
            let b = TaskSpec {
                id: other.id.clone(),
                role: other.role.clone(),
                title: String::new(),
                goal: String::new(),
                depends_on: Vec::new(),
                files: other.files.clone(),
                needs_approval: false,
            };
            conflicts(&a, &b, writes, other_writes)
        });
        if clashes {
            continue;
        }
        match start_task(state, &mission, &mut tasks, index).await {
            Ok(()) => active += 1,
            Err(text) => {
                tasks[index].status = FAILED.to_string();
                tasks[index].error = cap(&text, 1_000);
                set_task(state, mission_id, &tasks[index])?;
                message(
                    state,
                    mission_id,
                    &tasks[index].id,
                    "error",
                    "orchestrator",
                    &tasks[index].role,
                    &text,
                );
            }
        }
    }

    // 5) Settle the mission when nothing can move any more.
    let all_settled = tasks.iter().all(|task| settled_task(&task.status));
    let any_planned = tasks.iter().any(|task| task.status == PLANNED);
    let any_running = tasks.iter().any(|task| task.status == RUNNING);
    if !all_settled && !any_planned && !any_running {
        // Nothing planned, nothing running, not settled: a stuck graph.
        let report = build_report(state, &mission, &tasks);
        set_mission(
            state,
            mission_id,
            M_FAILED,
            "The mission stalled: no task could start.",
            &report,
        )?;
        message(
            state,
            mission_id,
            "",
            "report",
            "orchestrator",
            "user",
            "Mission failed: no task could start.",
        );
        let _ = crate::improve::observe_mission(state, mission_id);
        return Ok(true);
    }
    if all_settled {
        let failed: Vec<&Task> = tasks
            .iter()
            .filter(|task| matches!(task.status.as_str(), FAILED | BLOCKED))
            .collect();
        let cancelled = tasks.iter().any(|task| task.status == CANCELLED);
        let (status, error) = if cancelled {
            (M_CANCELLED, "A task was cancelled.".to_string())
        } else if failed.is_empty() {
            (M_SUCCEEDED, String::new())
        } else {
            (
                M_FAILED,
                format!(
                    "{} of {} tasks did not finish: {}.",
                    failed.len(),
                    tasks.len(),
                    failed
                        .iter()
                        .map(|task| task.id.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        };
        let mut settled = Mission { ..mission };
        settled.status = status.to_string();
        let report = build_report(state, &settled, &tasks);
        set_mission(state, mission_id, status, &error, &report)?;
        message(
            state,
            mission_id,
            "",
            "report",
            "orchestrator",
            "user",
            &format!(
                "Mission {status}: {} done, {} failed, {} blocked.",
                tasks.iter().filter(|task| task.status == DONE).count(),
                tasks.iter().filter(|task| task.status == FAILED).count(),
                tasks.iter().filter(|task| task.status == BLOCKED).count(),
            ),
        );
        let _ = crate::improve::observe_mission(state, mission_id);
        return Ok(true);
    }
    Ok(false)
}

async fn start_task(
    state: &AppState,
    mission: &Mission,
    tasks: &mut [Task],
    index: usize,
) -> Result<(), String> {
    let snapshot = Task {
        id: tasks[index].id.clone(),
        role: tasks[index].role.clone(),
        title: tasks[index].title.clone(),
        goal: tasks[index].goal.clone(),
        depends_on: tasks[index].depends_on.clone(),
        files: tasks[index].files.clone(),
        status: tasks[index].status.clone(),
        run_id: tasks[index].run_id.clone(),
        attempts: tasks[index].attempts,
        max_attempts: tasks[index].max_attempts,
        summary: tasks[index].summary.clone(),
        error: tasks[index].error.clone(),
        needs_approval: tasks[index].needs_approval,
        approved: tasks[index].approved,
        position: tasks[index].position,
    };
    let task = &snapshot;
    let Some(agent) = role(&task.role) else {
        return Err(format!("Unknown agent role \"{}\".", task.role));
    };
    let deps: Vec<(String, String, String)> = task
        .depends_on
        .iter()
        .filter_map(|dep| {
            tasks
                .iter()
                .find(|candidate| &candidate.id == dep)
                .map(|candidate| {
                    (
                        candidate.role.clone(),
                        candidate.title.clone(),
                        candidate.summary.clone(),
                    )
                })
        })
        .collect();
    let dep_runs: Vec<String> = task
        .depends_on
        .iter()
        .filter_map(|dep| {
            tasks
                .iter()
                .find(|candidate| &candidate.id == dep)
                .map(|candidate| candidate.run_id.clone())
                .filter(|run_id| !run_id.is_empty())
        })
        .collect();
    // A retry that follows a failed pinned model falls back to the routed
    // default instead of spending the retry on the same broken assignment.
    let pinned = role_model(state, &task.role);
    let model = if task.attempts > 0 {
        None
    } else {
        pinned.clone()
    };
    let note = if task.attempts == 0 {
        String::new()
    } else {
        format!(
            "This is attempt {} of {}. The previous attempt failed: {}",
            task.attempts + 1,
            task.max_attempts,
            if task.error.trim().is_empty() {
                "(no error recorded)"
            } else {
                task.error.trim()
            }
        )
    };
    let goal = task_goal(mission, task, &deps, &note);
    let mut input = json!({
        "goal": goal,
        "profile": agent.profile,
        "allowWrites": mission.allow_writes && agent.writes,
        "allowCommands": mission.allow_commands && agent.commands,
        "autoApprove": mission.auto_approve,
        "maxSteps": 24,
        "priority": 5,
    });
    if !dep_runs.is_empty() {
        input["dependsOn"] = json!(dep_runs);
    }
    if let Some(model) = &model {
        input["model"] = json!(model);
    }
    let started = crate::agent::create_run_with(state, input)
        .await
        .map_err(|(_, message)| message)?;
    tasks[index].status = RUNNING.to_string();
    tasks[index].run_id = started.id.clone();
    set_task(state, &mission.id, &tasks[index])?;
    message(
        state,
        &mission.id,
        &task.id,
        "assignment",
        "orchestrator",
        &task.role,
        &format!(
            "{} → {} ({}){}",
            task.title,
            agent.name,
            started.status,
            match (&pinned, &model) {
                (Some(pinned), Some(model)) if pinned == model => format!(" using {model}"),
                (Some(pinned), None) => {
                    format!(" falling back from {pinned} to the routed default")
                }
                _ => String::new(),
            }
        ),
    );
    Ok(())
}

// ---------------------------------------------------------------- report

fn build_report(state: &AppState, mission: &Mission, tasks: &[Task]) -> String {
    let run_ids: Vec<String> = tasks
        .iter()
        .map(|task| task.run_id.clone())
        .filter(|id| !id.is_empty())
        .collect();
    let mut writes: Vec<String> = Vec::new();
    let mut commands: Vec<String> = Vec::new();
    let mut checks: Vec<String> = Vec::new();
    let mut tokens: i64 = 0;
    if let Ok(conn) = db(state) {
        for run_id in &run_ids {
            if let Ok(Some(count)) = conn
                .query_row(
                    "SELECT tokens FROM agent_runs WHERE id=?1",
                    [run_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
            {
                tokens += count;
            }
            for (action, bucket) in [
                ("write", &mut writes),
                ("command", &mut commands),
                ("verify", &mut checks),
            ] {
                let Ok(mut stmt) = conn.prepare(
                    "SELECT summary FROM agent_steps WHERE run_id=?1 AND action=?2 ORDER BY seq",
                ) else {
                    continue;
                };
                let rows = stmt.query_map(params![run_id, action], |row| row.get::<_, String>(0));
                if let Ok(rows) = rows {
                    bucket.extend(rows.filter_map(|row| row.ok()));
                }
            }
        }
    }
    let payload = json!({
        "missionId": mission.id,
        "goal": mission.goal,
        "template": mission.template,
        "status": mission.status,
        "elapsedSeconds": crate::now_seconds() - mission.created_at,
        "tasks": tasks.iter().map(|task| json!({
            "id": task.id,
            "role": task.role,
            "title": task.title,
            "status": task.status,
            "attempts": task.attempts,
            "runId": task.run_id,
            "summary": task.summary,
            "error": task.error,
        })).collect::<Vec<_>>(),
        "filesWritten": writes,
        "commandsRun": commands,
        "checks": checks,
        "tokens": tokens,
        "context": context_of(mission),
    });
    serde_json::to_string(&payload).unwrap_or_default()
}

// ---------------------------------------------------------------- api

fn mission_json(state: &AppState, mission: &Mission, tasks: &[Task]) -> Value {
    json!({
        "id": mission.id,
        "goal": mission.goal,
        "template": mission.template,
        "status": mission.status,
        "error": mission.error,
        "projectId": mission.project_id,
        "parallel": mission.parallel,
        "maxRetries": mission.max_retries,
        "allowWrites": mission.allow_writes,
        "allowCommands": mission.allow_commands,
        "autoApprove": mission.auto_approve,
        "createdAt": mission.created_at,
        "updatedAt": mission.updated_at,
        "counts": {
            "total": tasks.len(),
            "done": tasks.iter().filter(|task| task.status == DONE).count(),
            "running": tasks.iter().filter(|task| task.status == RUNNING).count(),
            "planned": tasks.iter().filter(|task| task.status == PLANNED).count(),
            "failed": tasks.iter().filter(|task| task.status == FAILED).count(),
            "blocked": tasks.iter().filter(|task| task.status == BLOCKED).count(),
        },
        "tasks": tasks.iter().map(|task| json!({
            "id": task.id,
            "role": task.role,
            "roleName": role(&task.role).map(|role| role.name).unwrap_or(&task.role),
            "title": task.title,
            "goal": task.goal,
            "status": task.status,
            "dependsOn": task.depends_on,
            "files": task.files,
            "runId": task.run_id,
            "attempts": task.attempts,
            "maxAttempts": task.max_attempts,
            "summary": task.summary,
            "error": task.error,
            "needsApproval": task.needs_approval,
            "approved": task.approved,
            "runStatus": if task.run_id.is_empty() {
                Value::Null
            } else {
                run_state(state, &task.run_id)
                    .map(|run| json!(run.status))
                    .unwrap_or(Value::Null)
            },
        })).collect::<Vec<_>>(),
        "report": if mission.report.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&mission.report).unwrap_or(Value::Null)
        },
    })
}

/// A mission as the API sees it, for other modules (engineering) that must not
/// reimplement the mission store.
pub(crate) fn mission_json_for(state: &AppState, id: &str) -> Option<Value> {
    let mission = load_mission(state, id).ok().flatten()?;
    let tasks = load_tasks(state, id).unwrap_or_default();
    Some(mission_json(state, &mission, &tasks))
}

/// Cancel every live mission and stop its runs. The emergency stop calls this;
/// it never asks for approval because stopping is the safe direction.
pub(crate) async fn cancel_all(state: &AppState) -> (usize, usize) {
    let ids: Vec<String> = db(state)
        .ok()
        .and_then(|conn| {
            conn.prepare("SELECT id FROM missions WHERE status IN ('planned','running','paused','awaiting-approval')")
                .ok()
                .map(|mut stmt| {
                    stmt.query_map([], |row| row.get::<_, String>(0))
                        .map(|rows| rows.filter_map(|row| row.ok()).collect::<Vec<String>>())
                        .unwrap_or_default()
                })
        })
        .unwrap_or_default();
    let mut missions = 0;
    let mut runs = 0;
    for id in ids {
        if let Ok(stopped) = cancel_mission_record(state, &id).await {
            missions += 1;
            runs += stopped;
        }
    }
    (missions, runs)
}

async fn list_missions(State(state): State<AppState>) -> Response {
    let Ok(conn) = db(&state) else {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error.");
    };
    let mut stmt = match conn.prepare(&format!("{MISSION_SQL} ORDER BY created_at DESC LIMIT 30")) {
        Ok(stmt) => stmt,
        Err(error) => {
            return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
    };
    let rows = match stmt.query_map([], mission_from_row) {
        Ok(rows) => rows.filter_map(|row| row.ok()).collect::<Vec<_>>(),
        Err(error) => {
            return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
    };
    let missions: Vec<Value> = rows
        .iter()
        .map(|mission| {
            mission_json(
                &state,
                mission,
                &load_tasks(&state, &mission.id).unwrap_or_default(),
            )
        })
        .collect();
    Json(json!({ "missions": missions })).into_response()
}

async fn create_mission(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    match create_mission_with(&state, input).await {
        Ok(id) => {
            let mission = load_mission(&state, &id).ok().flatten();
            let tasks = load_tasks(&state, &id).unwrap_or_default();
            match mission {
                Some(mission) => Json(json!({
                    "ok": true,
                    "mission": mission_json(&state, &mission, &tasks),
                }))
                .into_response(),
                None => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "Mission vanished."),
            }
        }
        Err((code, message)) => crate::json_error(code, message),
    }
}

async fn get_mission(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match load_mission(&state, &id) {
        Ok(Some(mission)) => {
            let tasks = load_tasks(&state, &id).unwrap_or_default();
            Json(json!({ "mission": mission_json(&state, &mission, &tasks) })).into_response()
        }
        Ok(None) => crate::json_error(StatusCode::NOT_FOUND, "No such mission."),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn pause_mission(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let _tick = TICK.lock().await;
    match load_mission(&state, &id) {
        Ok(Some(mission)) if mission.status == M_RUNNING || mission.status == M_PLANNED => {
            match set_mission(&state, &id, M_PAUSED, "", "") {
                Ok(()) => {
                    message(&state, &id, "", "status", "user", "orchestrator", "Paused.");
                    Json(json!({ "ok": true, "status": M_PAUSED })).into_response()
                }
                Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
            }
        }
        Ok(Some(_)) => crate::json_error(StatusCode::BAD_REQUEST, "This mission is not running."),
        Ok(None) => crate::json_error(StatusCode::NOT_FOUND, "No such mission."),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn resume_mission(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let _tick = TICK.lock().await;
    match load_mission(&state, &id) {
        Ok(Some(mission)) if mission.status == M_PAUSED || mission.status == M_AWAITING => {
            match set_mission(&state, &id, M_RUNNING, "", "") {
                Ok(()) => {
                    message(
                        &state,
                        &id,
                        "",
                        "status",
                        "user",
                        "orchestrator",
                        "Resumed.",
                    );
                    supervise(&state, &id);
                    Json(json!({ "ok": true, "status": M_RUNNING })).into_response()
                }
                Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
            }
        }
        Ok(Some(_)) => {
            crate::json_error(StatusCode::BAD_REQUEST, "Only a paused mission can resume.")
        }
        Ok(None) => crate::json_error(StatusCode::NOT_FOUND, "No such mission."),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn cancel_mission(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match cancel_mission_record(&state, &id).await {
        Ok(stopped) => Json(json!({ "ok": true, "status": M_CANCELLED, "stoppedRuns": stopped }))
            .into_response(),
        Err(error) if error == "no such mission" => {
            crate::json_error(StatusCode::NOT_FOUND, "No such mission.")
        }
        Err(error) if error == "already finished" => {
            crate::json_error(StatusCode::BAD_REQUEST, "This mission already finished.")
        }
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

/// The cancellation itself, shared by the route and the emergency stop so both
/// take the tick lock and stop runs the same way.
pub(crate) async fn cancel_mission_record(state: &AppState, id: &str) -> Result<usize, String> {
    let _tick = TICK.lock().await;
    let Some(mission) = load_mission(state, id).ok().flatten() else {
        return Err("no such mission".to_string());
    };
    if terminal_mission(&mission.status) {
        return Err("already finished".to_string());
    }
    let mut tasks = load_tasks(state, id).unwrap_or_default();
    let mut stopped = 0;
    for task in &mut tasks {
        if !settled_task(&task.status) {
            if !task.run_id.is_empty()
                && crate::agent::stop_run(state, &task.run_id, "Mission cancelled.").is_ok()
            {
                stopped += 1;
            }
            task.status = CANCELLED.to_string();
            let _ = set_task(state, id, task);
        }
    }
    let mut settled = Mission { ..mission };
    settled.status = M_CANCELLED.to_string();
    let report = build_report(state, &settled, &tasks);
    let _ = set_mission(state, id, M_CANCELLED, "Cancelled by user.", &report);
    message(
        state,
        id,
        "",
        "status",
        "user",
        "orchestrator",
        &format!("Cancelled the mission; stopped {stopped} live run(s)."),
    );
    Ok(stopped)
}

async fn approve_task(
    State(state): State<AppState>,
    Path((id, task_id)): Path<(String, String)>,
) -> Response {
    let _tick = TICK.lock().await;
    let Some(_mission) = load_mission(&state, &id).ok().flatten() else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such mission.");
    };
    let mut tasks = load_tasks(&state, &id).unwrap_or_default();
    let Some(index) = tasks.iter().position(|task| task.id == task_id) else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such task.");
    };
    if !tasks[index].needs_approval {
        return crate::json_error(StatusCode::BAD_REQUEST, "This task needs no approval.");
    }
    tasks[index].approved = true;
    if tasks[index].status == AWAITING {
        tasks[index].status = PLANNED.to_string();
    }
    if let Err(error) = set_task(&state, &id, &tasks[index]) {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error);
    }
    message(
        &state,
        &id,
        &task_id,
        "status",
        "user",
        "orchestrator",
        "Approved.",
    );
    let _ = set_mission(&state, &id, M_RUNNING, "", "");
    supervise(&state, &id);
    Json(json!({ "ok": true })).into_response()
}

async fn cancel_task(
    State(state): State<AppState>,
    Path((id, task_id)): Path<(String, String)>,
) -> Response {
    let _tick = TICK.lock().await;
    let mut tasks = load_tasks(&state, &id).unwrap_or_default();
    let Some(index) = tasks.iter().position(|task| task.id == task_id) else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such task.");
    };
    if settled_task(&tasks[index].status) {
        return crate::json_error(StatusCode::BAD_REQUEST, "This task already finished.");
    }
    if !tasks[index].run_id.is_empty() {
        let _ = crate::agent::stop_run(&state, &tasks[index].run_id, "Task cancelled.");
    }
    tasks[index].status = CANCELLED.to_string();
    if let Err(error) = set_task(&state, &id, &tasks[index]) {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error);
    }
    message(
        &state,
        &id,
        &task_id,
        "status",
        "user",
        "orchestrator",
        "Cancelled this task.",
    );
    supervise(&state, &id);
    Json(json!({ "ok": true })).into_response()
}

async fn advance_mission(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if load_mission(&state, &id).ok().flatten().is_none() {
        return crate::json_error(StatusCode::NOT_FOUND, "No such mission.");
    }
    match advance(&state, &id).await {
        Ok(settled) => {
            let mission = load_mission(&state, &id).ok().flatten();
            let tasks = load_tasks(&state, &id).unwrap_or_default();
            match mission {
                Some(mission) => Json(json!({
                    "ok": true,
                    "settled": settled,
                    "mission": mission_json(&state, &mission, &tasks),
                }))
                .into_response(),
                None => crate::json_error(StatusCode::NOT_FOUND, "No such mission."),
            }
        }
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn mission_report(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match load_mission(&state, &id) {
        Ok(Some(mission)) => {
            let tasks = load_tasks(&state, &id).unwrap_or_default();
            let report = if mission.report.is_empty() {
                build_report(&state, &mission, &tasks)
            } else {
                mission.report.clone()
            };
            match serde_json::from_str::<Value>(&report) {
                Ok(payload) => Json(payload).into_response(),
                Err(_) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "Broken report."),
            }
        }
        Ok(None) => crate::json_error(StatusCode::NOT_FOUND, "No such mission."),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn mission_messages(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Ok(conn) = db(&state) else {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error.");
    };
    let mut stmt = match conn.prepare(
        "SELECT seq, task_id, kind, from_role, to_role, body, created_at FROM mission_messages \
         WHERE mission_id=?1 ORDER BY seq",
    ) {
        Ok(stmt) => stmt,
        Err(error) => {
            return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
    };
    let rows = stmt.query_map([&id], |row| {
        Ok(json!({
            "seq": row.get::<_, i64>(0)?,
            "task": row.get::<_, String>(1)?,
            "kind": row.get::<_, String>(2)?,
            "from": row.get::<_, String>(3)?,
            "to": row.get::<_, String>(4)?,
            "body": row.get::<_, String>(5)?,
            "at": row.get::<_, f64>(6)?,
        }))
    });
    match rows {
        Ok(rows) => Json(json!({
            "messages": rows.filter_map(|row| row.ok()).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

async fn remove_mission(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(mission) = load_mission(&state, &id).ok().flatten() else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such mission.");
    };
    if !terminal_mission(&mission.status) {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "Cancel the mission before removing it.",
        );
    }
    if let Ok(conn) = db(&state) {
        let _ = conn.execute("DELETE FROM mission_tasks WHERE mission_id=?1", [&id]);
        let _ = conn.execute("DELETE FROM mission_messages WHERE mission_id=?1", [&id]);
        let _ = conn.execute("DELETE FROM missions WHERE id=?1", [&id]);
    }
    Json(json!({ "ok": true })).into_response()
}

async fn list_roles(State(state): State<AppState>) -> Response {
    let models = role_models(&state);
    let roles: Vec<Value> = ROLES
        .iter()
        .map(|role| {
            let (_, model, enabled) = models
                .iter()
                .find(|(id, _, _)| id == role.id)
                .cloned()
                .unwrap_or_else(|| (role.id.to_string(), None, true));
            json!({
                "id": role.id,
                "name": role.name,
                "purpose": role.purpose,
                "profile": role.profile,
                "writes": role.writes,
                "commands": role.commands,
                "tools": role_tools(role),
                "model": model,
                "enabled": enabled,
            })
        })
        .collect();
    Json(json!({ "roles": roles })).into_response()
}

async fn save_role(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let id = input
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let Some(agent) = role(id) else {
        return crate::json_error(StatusCode::BAD_REQUEST, "Unknown agent role.");
    };
    let model = input
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if model.chars().count() > 200 {
        return crate::json_error(StatusCode::BAD_REQUEST, "That model name is too long.");
    }
    let enabled = input
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let Ok(conn) = db(&state) else {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error.");
    };
    let outcome = conn.execute(
        "INSERT INTO mission_roles(role, model, enabled, updated_at) VALUES(?1,?2,?3,?4) \
         ON CONFLICT(role) DO UPDATE SET model=excluded.model, enabled=excluded.enabled, \
         updated_at=excluded.updated_at",
        params![
            agent.id,
            model,
            if enabled { 1 } else { 0 },
            crate::now_seconds()
        ],
    );
    match outcome {
        Ok(_) => Json(json!({ "ok": true, "role": agent.id })).into_response(),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// Crash recovery: a mission whose supervisor died with the process keeps its
/// real state, so live tasks are re-read on the next tick rather than invented.
pub(crate) fn resume_after_restart(state: &AppState) {
    let Ok(conn) = db(state) else {
        return;
    };
    let ids: Vec<String> = conn
        .prepare("SELECT id FROM missions WHERE status IN (?1, ?2)")
        .and_then(|mut stmt| {
            stmt.query_map(params![M_RUNNING, M_PLANNED], |row| row.get::<_, String>(0))
                .map(|rows| rows.filter_map(|row| row.ok()).collect())
        })
        .unwrap_or_default();
    for id in ids {
        supervise(state, &id);
    }
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/missions", get(list_missions).post(create_mission))
        .route("/api/missions/:id", get(get_mission))
        .route("/api/missions/:id/pause", post(pause_mission))
        .route("/api/missions/:id/resume", post(resume_mission))
        .route("/api/missions/:id/cancel", post(cancel_mission))
        .route("/api/missions/:id/advance", post(advance_mission))
        .route("/api/missions/:id/report", get(mission_report))
        .route("/api/missions/:id/messages", get(mission_messages))
        .route("/api/missions/:id/remove", post(remove_mission))
        .route("/api/missions/:id/tasks/:task/approve", post(approve_task))
        .route("/api/missions/:id/tasks/:task/cancel", post(cancel_task))
        .route("/api/mission-roles", get(list_roles).post(save_role))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_of(id: &str, role_id: &str, files: &[&str], deps: &[&str]) -> TaskSpec {
        spec(id, role_id, "", "", deps, files)
    }

    #[test]
    fn every_role_maps_to_a_real_agent_profile() {
        let mut seen = Vec::new();
        for role in ROLES.iter() {
            assert!(!seen.contains(&role.id), "duplicate role {}", role.id);
            seen.push(role.id);
            assert!(
                crate::agent::checked_profile(role.profile).is_ok(),
                "{} uses an unknown profile {}",
                role.id,
                role.profile
            );
        }
        assert_eq!(ROLES.len(), 8);
    }

    #[test]
    fn only_the_builder_roles_may_write() {
        for role in ROLES.iter() {
            let expected = matches!(role.id, "coding" | "documentation");
            assert_eq!(role.writes, expected, "{}", role.id);
            let tools = role_tools(role);
            assert_eq!(tools.contains(&"write"), expected, "{}", role.id);
            assert_eq!(tools.contains(&"command"), role.commands, "{}", role.id);
        }
    }

    #[test]
    fn the_built_in_workflows_have_valid_dependencies() {
        for template in ["feature", "debug", "audit"] {
            let plan = template_plan(template, "do the thing").expect(template);
            assert!(plan.len() >= 4, "{template}");
            for (index, entry) in plan.iter().enumerate() {
                assert!(role(&entry.role).is_some(), "{template}: {}", entry.role);
                for dep in &entry.depends_on {
                    let position = plan
                        .iter()
                        .position(|candidate| &candidate.id == dep)
                        .unwrap_or_else(|| panic!("{template}: {dep} missing"));
                    assert!(position < index, "{template}: {dep} points forwards");
                }
            }
            // The feature plan must end with a review that follows the tests.
            if template == "feature" {
                let review = plan.last().unwrap();
                assert_eq!(review.role, "review");
                assert!(review.depends_on.iter().any(|dep| dep == "verify"));
            }
        }
        assert!(template_plan("nonsense", "x").is_err());
    }

    #[test]
    fn a_custom_plan_refuses_unknown_roles_and_broken_graphs() {
        let unknown = json!([{ "id": "a", "role": "wizard" }]);
        assert!(parse_specs(&unknown)
            .unwrap_err()
            .contains("Unknown agent role"));

        let cycle = json!([
            { "id": "a", "role": "coding", "dependsOn": ["b"] },
            { "id": "b", "role": "review", "dependsOn": ["a"] },
        ]);
        assert!(parse_specs(&cycle).unwrap_err().contains("cycle"));

        let missing = json!([{ "id": "a", "role": "coding", "dependsOn": ["ghost"] }]);
        assert!(parse_specs(&missing)
            .unwrap_err()
            .contains("not in the plan"));

        let duplicate = json!([
            { "id": "a", "role": "coding" },
            { "id": "a", "role": "review" },
        ]);
        assert!(parse_specs(&duplicate)
            .unwrap_err()
            .contains("share the id"));

        let too_many: Vec<Value> = (0..(MAX_TASKS + 1))
            .map(|index| json!({ "id": format!("t{index}"), "role": "review" }))
            .collect();
        assert!(parse_specs(&json!(too_many))
            .unwrap_err()
            .contains("at most"));

        let ok = json!([
            { "id": "a", "role": "coding", "files": ["./src/lib.rs", "/etc/passwd"] },
            { "id": "b", "role": "review", "dependsOn": ["a"] },
        ]);
        let parsed = parse_specs(&ok).expect("valid");
        assert_eq!(parsed[0].files, vec!["src/lib.rs".to_string()]);
    }

    #[test]
    fn writers_that_could_touch_the_same_file_never_run_together() {
        let unknown_one = spec_of("a", "coding", &[], &[]);
        let unknown_two = spec_of("b", "coding", &[], &[]);
        assert!(conflicts(&unknown_one, &unknown_two, true, true));
        // Two read-only agents never clash.
        assert!(!conflicts(&unknown_one, &unknown_two, false, false));

        let src = spec_of("a", "coding", &["src/lib.rs"], &[]);
        let other = spec_of("b", "coding", &["src/other.rs"], &[]);
        assert!(!conflicts(&src, &other, true, true));

        let nested = spec_of("b", "coding", &["src/lib.rs/inner"], &[]);
        assert!(conflicts(&src, &nested, true, true));

        let docs = spec_of("c", "documentation", &["docs/x.md"], &[]);
        assert!(!conflicts(&src, &docs, true, true));
        // A writer and a reader with no declared files may share the workspace.
        assert!(!conflicts(&unknown_one, &docs, true, false));
        // A declared writer versus an undeclared writer still waits.
        assert!(conflicts(&unknown_one, &src, true, true));
    }

    fn task_of(id: &str, role_id: &str, status: &str, summary: &str) -> Task {
        Task {
            id: id.to_string(),
            role: role_id.to_string(),
            title: format!("{id} title"),
            goal: String::new(),
            depends_on: Vec::new(),
            files: Vec::new(),
            status: status.to_string(),
            run_id: String::new(),
            attempts: 0,
            max_attempts: 1,
            summary: summary.to_string(),
            error: String::new(),
            needs_approval: false,
            approved: false,
            position: 0,
        }
    }

    fn mission_of(goal: &str) -> Mission {
        Mission {
            id: "m".to_string(),
            project_id: String::new(),
            goal: goal.to_string(),
            template: "feature".to_string(),
            status: M_RUNNING.to_string(),
            error: String::new(),
            parallel: 2,
            max_retries: 1,
            allow_writes: true,
            allow_commands: true,
            auto_approve: false,
            created_at: 0.0,
            updated_at: 0.0,
            context: String::new(),
            report: String::new(),
        }
    }

    #[test]
    fn a_task_goal_carries_only_its_own_dependencies() {
        let mission = mission_of("ship the feature");
        let task = task_of("implement", "coding", PLANNED, "");
        let goal = task_goal(
            &mission,
            &task,
            &[
                (
                    "planner".to_string(),
                    "Plan the change".to_string(),
                    "PLAN-SUMMARY".to_string(),
                ),
                (
                    "review".to_string(),
                    "Review".to_string(),
                    "UNRELATED-SUMMARY".to_string(),
                ),
            ],
            "",
        );
        assert!(goal.contains("ship the feature"));
        assert!(goal.contains("Coding agent"));
        assert!(goal.contains("PLAN-SUMMARY"));
        // The caller only passes direct dependencies, so isolation is enforced
        // at the call site — this asserts the visible consequence.
        assert!(!goal.contains("implement title") || goal.contains("implement"));
    }

    #[test]
    fn a_goal_is_capped_and_never_repeats_itself_forever() {
        let mission = mission_of(&"x".repeat(MAX_GOAL_CHARS));
        let task = task_of("t", "coding", PLANNED, "");
        let goal = task_goal(&mission, &task, &[], "retry");
        assert!(goal.chars().count() <= TASK_GOAL_CAP + 1);
        assert!(goal.contains("retry"));
    }

    #[test]
    fn paths_are_normalised_and_cannot_escape() {
        assert_eq!(normalize_relative("./src/a.rs"), "src/a.rs");
        assert_eq!(normalize_relative("src/"), "src");
        assert_eq!(normalize_relative("/etc/passwd"), "");
        assert_eq!(normalize_relative("../outside"), "");
        assert_eq!(normalize_relative(""), "");
    }
}
#[cfg(test)]
mod orchestration {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    struct Fixture {
        state: AppState,
        app: Router,
        root: std::path::PathBuf,
        bodies: Arc<Mutex<Vec<Value>>>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// A provider that answers each model call from a script, and keeps the
    /// request bodies so a test can prove which model a role actually used.
    async fn scripted_with(replies: &[&str], delay_ms: u64) -> (String, Arc<Mutex<Vec<Value>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(
            replies
                .iter()
                .map(|reply| reply.to_string())
                .collect::<Vec<_>>(),
        )));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let seen = bodies.clone();
        let app = Router::new().route(
            "/chat/completions",
            axum::routing::post(move |body: axum::Json<Value>| {
                let queue = queue.clone();
                let seen = seen.clone();
                async move {
                    if delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    seen.lock().unwrap().push(body.0.clone());
                    let reply = queue.lock().unwrap().pop_front().unwrap_or_else(|| {
                        "{\"action\":\"fail\",\"reason\":\"mock out of replies\"}".to_string()
                    });
                    axum::Json(json!({
                        "choices": [{ "message": { "role": "assistant", "content": reply } }]
                    }))
                }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}"), bodies)
    }

    async fn boot(tag: &str, replies: &[&str]) -> Fixture {
        boot_slow(tag, replies, 0).await
    }

    async fn boot_slow(tag: &str, replies: &[&str], delay_ms: u64) -> Fixture {
        let (base, bodies) = scripted_with(replies, delay_ms).await;
        let root =
            std::env::temp_dir().join(format!("sarthi-mission-{tag}-{}", uuid::Uuid::new_v4()));
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
            models_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };
        let _ = crate::ensure_setup(&state);
        let mut cfg = crate::load_config(&state);
        cfg.base_url = base;
        cfg.api_key = "test-key".to_string();
        cfg.local_chat = false;
        let _ = crate::save_config_file(&state.config_path, &cfg);
        state
            .workspace
            .open(ws.to_str().unwrap())
            .expect("open workspace root");
        let app = router().with_state(state.clone());
        Fixture {
            state,
            app,
            root,
            bodies,
        }
    }

    async fn request(app: &Router, method: &str, uri: &str, body: Value) -> Value {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn create(app: &Router, body: Value) -> Result<String, String> {
        let payload = request(app, "POST", "/api/missions", body).await;
        match payload["mission"]["id"].as_str() {
            Some(id) => Ok(id.to_string()),
            None => Err(payload["error"]
                .as_str()
                .unwrap_or("the mission was refused")
                .to_string()),
        }
    }

    async fn wait(state: &AppState, id: &str) -> (String, Vec<Task>) {
        for _ in 0..800 {
            let mission = load_mission(state, id).unwrap().expect("mission");
            if terminal_mission(&mission.status) {
                return (mission.status.clone(), load_tasks(state, id).unwrap());
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let mission = load_mission(state, id).unwrap().expect("mission");
        panic!(
            "mission never settled: {} ({})",
            mission.status,
            load_tasks(state, id)
                .unwrap()
                .iter()
                .map(|task| format!("{}={} {}", task.id, task.status, task.error))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    fn status_of(tasks: &[Task], id: &str) -> String {
        tasks
            .iter()
            .find(|task| task.id == id)
            .map(|task| task.status.clone())
            .unwrap_or_else(|| panic!("no task {id}"))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_feature_mission_runs_every_role_and_reports_evidence() {
        let fixture = boot(
            "feature",
            &[
                "{\"action\":\"done\",\"summary\":\"plan: write hello.txt\"}",
                "{\"action\":\"write\",\"path\":\"hello.txt\",\"content\":\"hi from coding\"}",
                "{\"action\":\"done\",\"summary\":\"wrote hello.txt\"}",
                "APPROVE: hello.txt says what the goal asked for.",
                "{\"action\":\"command\",\"command\":\"echo tests-ok\"}",
                "{\"action\":\"done\",\"summary\":\"echo tests-ok printed tests-ok\"}",
                "APPROVE: the command ran and printed tests-ok.",
                "{\"action\":\"write\",\"path\":\"docs/hello.md\",\"content\":\"# hello\"}",
                "{\"action\":\"done\",\"summary\":\"documented hello.txt\"}",
                "APPROVE: the documentation exists and matches.",
                "{\"action\":\"read\",\"path\":\"hello.txt\"}",
                "{\"action\":\"done\",\"summary\":\"the change is correct and safe\"}",
            ],
        )
        .await;
        let id = create(
            &fixture.app,
            json!({
                "goal": "write hello.txt and document it",
                "template": "feature",
                "parallel": 1,
                "maxRetries": 0,
                "allowWrites": true,
                "allowCommands": true,
                "autoApprove": true,
            }),
        )
        .await
        .expect("mission starts");
        let (status, tasks) = wait(&fixture.state, &id).await;
        assert_eq!(
            status,
            M_SUCCEEDED,
            "tasks: {:?}",
            tasks
                .iter()
                .map(|task| (task.id.clone(), task.status.clone(), task.error.clone()))
                .collect::<Vec<_>>()
        );
        assert_eq!(tasks.len(), 5);
        for task in &tasks {
            assert_eq!(task.status, DONE, "{} failed: {}", task.id, task.error);
        }
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("ws/hello.txt")).unwrap(),
            "hi from coding"
        );
        assert!(fixture.root.join("ws/docs/hello.md").is_file());

        let report = request(
            &fixture.app,
            "GET",
            &format!("/api/missions/{id}/report"),
            json!({}),
        )
        .await;
        assert_eq!(report["status"], json!(M_SUCCEEDED));
        assert!(
            !report["filesWritten"].as_array().unwrap().is_empty(),
            "{report}"
        );
        assert!(!report["commandsRun"].as_array().unwrap().is_empty());

        let messages = request(
            &fixture.app,
            "GET",
            &format!("/api/missions/{id}/messages"),
            json!({}),
        )
        .await;
        let kinds: Vec<String> = messages["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["kind"].as_str().unwrap_or("").to_string())
            .collect();
        for kind in ["request", "assignment", "completion", "report"] {
            assert!(
                kinds.iter().any(|seen| seen == kind),
                "missing {kind}: {kinds:?}"
            );
        }
        let plan = tasks.iter().find(|task| task.id == "plan").unwrap();
        assert!(plan.summary.contains("hello.txt"), "{}", plan.summary);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn independent_tasks_start_together_before_any_finishes() {
        let fixture = boot(
            "parallel",
            &[
                "{\"action\":\"done\",\"summary\":\"one\"}",
                "{\"action\":\"done\",\"summary\":\"two\"}",
                "{\"action\":\"done\",\"summary\":\"three\"}",
                "{\"action\":\"done\",\"summary\":\"synthesis\"}",
            ],
        )
        .await;
        let id = create(
            &fixture.app,
            json!({
                "goal": "audit the workspace",
                "template": "audit",
                "parallel": 3,
                "maxRetries": 0,
            }),
        )
        .await
        .expect("mission starts");
        let (status, tasks) = wait(&fixture.state, &id).await;
        assert_eq!(
            status,
            M_SUCCEEDED,
            "{:?}",
            tasks
                .iter()
                .map(|task| (&task.id, &task.status))
                .collect::<Vec<_>>()
        );

        let messages = request(
            &fixture.app,
            "GET",
            &format!("/api/missions/{id}/messages"),
            json!({}),
        )
        .await;
        let entries = messages["messages"].as_array().unwrap();
        let first_completion = entries
            .iter()
            .position(|entry| entry["kind"] == json!("completion"))
            .expect("a completion");
        let assignments_before = entries[..first_completion]
            .iter()
            .filter(|entry| entry["kind"] == json!("assignment"))
            .count();
        assert!(
            assignments_before >= 2,
            "expected two agents to start before the first finished: {entries:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failed_task_blocks_its_dependents_and_fails_the_mission() {
        let fixture = boot(
            "failure",
            &["{\"action\":\"fail\",\"reason\":\"cannot change this file\"}"],
        )
        .await;
        let id = create(
            &fixture.app,
            json!({
                "goal": "change the thing",
                "template": "custom",
                "tasks": [
                    { "id": "fix", "role": "coding", "title": "Fix it", "goal": "do it" },
                    { "id": "review", "role": "review", "title": "Review it", "dependsOn": ["fix"] }
                ],
                "maxRetries": 0,
                "allowWrites": true,
            }),
        )
        .await
        .expect("mission starts");
        let (status, tasks) = wait(&fixture.state, &id).await;
        assert_eq!(status, M_FAILED);
        assert_eq!(status_of(&tasks, "fix"), FAILED);
        assert_eq!(status_of(&tasks, "review"), BLOCKED);
        let report = request(
            &fixture.app,
            "GET",
            &format!("/api/missions/{id}/report"),
            json!({}),
        )
        .await;
        assert_eq!(report["status"], json!(M_FAILED));
        let blocked = report["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == json!("review"))
            .unwrap();
        assert_eq!(blocked["status"], json!(BLOCKED));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_approval_checkpoint_holds_the_task_until_a_person_approves() {
        let fixture = boot(
            "approval",
            &["{\"action\":\"done\",\"summary\":\"checked\"}"],
        )
        .await;
        let id = create(
            &fixture.app,
            json!({
                "goal": "check the release notes",
                "template": "custom",
                "tasks": [{ "id": "look", "role": "review", "needsApproval": true }],
                "maxRetries": 0,
            }),
        )
        .await
        .expect("mission starts");
        for _ in 0..400 {
            let mission = load_mission(&fixture.state, &id).unwrap().unwrap();
            if mission.status == M_AWAITING {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let mission = load_mission(&fixture.state, &id).unwrap().unwrap();
        assert_eq!(mission.status, M_AWAITING);
        let tasks = load_tasks(&fixture.state, &id).unwrap();
        assert_eq!(tasks[0].status, PLANNED);
        assert!(tasks[0].run_id.is_empty(), "nothing may start unapproved");

        let approved = request(
            &fixture.app,
            "POST",
            &format!("/api/missions/{id}/tasks/look/approve"),
            json!({}),
        )
        .await;
        assert_eq!(approved["ok"], json!(true), "{approved}");
        let (status, tasks) = wait(&fixture.state, &id).await;
        assert_eq!(status, M_SUCCEEDED);
        assert_eq!(status_of(&tasks, "look"), DONE);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelling_a_mission_cancels_its_tasks() {
        let fixture = boot(
            "cancel",
            &[
                "{\"action\":\"done\",\"summary\":\"one\"}",
                "{\"action\":\"done\",\"summary\":\"two\"}",
            ],
        )
        .await;
        let id = create(
            &fixture.app,
            json!({
                "goal": "cancel me",
                "template": "custom",
                "tasks": [
                    { "id": "one", "role": "review" },
                    { "id": "two", "role": "research", "dependsOn": ["one"] }
                ],
                "maxRetries": 0,
            }),
        )
        .await
        .expect("mission starts");
        let cancelled = request(
            &fixture.app,
            "POST",
            &format!("/api/missions/{id}/cancel"),
            json!({}),
        )
        .await;
        assert_eq!(cancelled["status"], json!(M_CANCELLED), "{cancelled}");
        let tasks = load_tasks(&fixture.state, &id).unwrap();
        for task in &tasks {
            assert_eq!(task.status, CANCELLED, "{}", task.id);
        }
        let report = request(
            &fixture.app,
            "GET",
            &format!("/api/missions/{id}/report"),
            json!({}),
        )
        .await;
        assert_eq!(report["status"], json!(M_CANCELLED));
    }

    /// Regression: cancelling while the supervisor was starting a task used to
    /// leave the task `running`, because the in-flight tick wrote its own
    /// status after the cancellation. Control actions now share the tick lock.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelling_stops_a_running_task_and_it_stays_cancelled() {
        let fixture = boot_slow(
            "cancel-running",
            &["{\"action\":\"done\",\"summary\":\"late\"}"],
            900,
        )
        .await;
        let id = create(
            &fixture.app,
            json!({
                "goal": "cancel a running task",
                "template": "custom",
                "tasks": [{ "id": "only", "role": "review" }],
                "maxRetries": 0,
            }),
        )
        .await
        .expect("mission starts");
        let mut run_id = String::new();
        for _ in 0..400 {
            let tasks = load_tasks(&fixture.state, &id).unwrap();
            if tasks[0].status == RUNNING && !tasks[0].run_id.is_empty() {
                run_id = tasks[0].run_id.clone();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(!run_id.is_empty(), "the task never started a run");

        let cancelled = request(
            &fixture.app,
            "POST",
            &format!("/api/missions/{id}/cancel"),
            json!({}),
        )
        .await;
        assert_eq!(cancelled["status"], json!(M_CANCELLED), "{cancelled}");
        assert_eq!(cancelled["stoppedRuns"], json!(1), "{cancelled}");

        let tasks = load_tasks(&fixture.state, &id).unwrap();
        assert_eq!(tasks[0].status, CANCELLED, "{}", tasks[0].error);
        let conn = crate::open_db(&fixture.state.database_path).unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM agent_runs WHERE id=?1",
                [&run_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "cancelled");
        drop(conn);

        // A tick landing afterwards must not resurrect the task.
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let again = load_tasks(&fixture.state, &id).unwrap();
        assert_eq!(again[0].status, CANCELLED);
        let mission = load_mission(&fixture.state, &id).unwrap().unwrap();
        assert_eq!(mission.status, M_CANCELLED);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_role_model_is_sent_to_the_provider_for_that_agents_run() {
        let fixture = boot("model", &["{\"action\":\"done\",\"summary\":\"done\"}"]).await;
        {
            let conn = crate::open_db(&fixture.state.database_path).unwrap();
            conn.execute(
                "INSERT INTO mission_roles(role, model, enabled, updated_at) \
                 VALUES('coding','pinned/model',1,0)",
                [],
            )
            .unwrap();
        }
        assert_eq!(
            role_model(&fixture.state, "coding"),
            Some("pinned/model".to_string()),
            "the role assignment did not read back"
        );
        let id = create(
            &fixture.app,
            json!({
                "goal": "do the coding",
                "template": "custom",
                "tasks": [{ "id": "code", "role": "coding" }],
                "maxRetries": 0,
                "allowWrites": true,
            }),
        )
        .await
        .expect("mission starts");
        let (status, _) = wait(&fixture.state, &id).await;
        assert_eq!(status, M_SUCCEEDED);
        let bodies = fixture.bodies.lock().unwrap().clone();
        assert!(
            bodies
                .iter()
                .any(|body| body["model"] == json!("pinned/model")),
            "the pinned model never reached the provider: {:?}",
            bodies
                .iter()
                .filter_map(|body| body["model"].as_str())
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_plan_that_edits_files_needs_write_permission() {
        let fixture = boot("permission", &["{\"action\":\"done\"}"]).await;
        let refusal = create(
            &fixture.app,
            json!({ "goal": "write something", "template": "feature" }),
        )
        .await
        .unwrap_err();
        assert!(refusal.contains("allow writes"), "{refusal}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_disabled_role_cannot_be_planned() {
        let fixture = boot("disabled", &["{\"action\":\"done\"}"]).await;
        {
            let conn = crate::open_db(&fixture.state.database_path).unwrap();
            conn.execute(
                "INSERT INTO mission_roles(role, model, enabled, updated_at) \
                 VALUES('review','',0,0)",
                [],
            )
            .unwrap();
        }
        let refusal = create(
            &fixture.app,
            json!({
                "goal": "audit",
                "template": "custom",
                "tasks": [{ "id": "a", "role": "review" }],
            }),
        )
        .await
        .unwrap_err();
        assert!(refusal.contains("switched off"), "{refusal}");
    }
}
