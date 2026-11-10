//! Phase 13: workflow graphs.
//!
//! A workflow is a directed acyclic graph of nodes stored in `workflows.json`
//! beside the config, so the existing saved prompts stay valid (a workflow with
//! no nodes is still a saved prompt). Each **run** is persisted in SQLite
//! (`workflow_runs` / `workflow_steps`) and executed by one spawned task that
//! walks the graph in dependency order, so a step's outcome is on disk before
//! the next step starts.
//!
//! Node kinds are the ones that execute against real state today:
//!
//! - `agent` starts a real agent run — the same `agent_runs` loop the Runs panel
//!   drives — and waits for it to finish, including waiting at its approval gate;
//! - `approval` parks the workflow until a person approves or rejects it;
//! - `condition` asserts what its dependencies ended as, which is what makes a
//!   `when: "failed"` branch meaningful;
//! - `task` records a note and succeeds — an explicit checkpoint in a graph.
//!
//! Edges are `dependsOn` entries: a string means "after it succeeded", an object
//! `{id, when}` can also require `when: "failed"`, which is how a repair branch
//! is expressed. Validation runs before a run row exists: unknown kinds,
//! duplicate or missing ids, self references, cycles, an agent node without a
//! prompt and an unknown profile are refused with the node named. Execution is
//! not a placeholder: every step's status is a stored row and every agent node
//! is a real run.

use axum::extract::{Path as AxumPath, Query as AxumQuery, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::AppState;

const MAX_NODES: usize = 50;
const EXPORT_VERSION: i64 = 1;
const MAX_PROMPT_CHARS: usize = 4_000;
const MAX_NOTE_CHARS: usize = 1_000;
const MAX_ERROR_CHARS: usize = 400;
const AGENT_POLL: Duration = Duration::from_millis(400);
const APPROVAL_POLL: Duration = Duration::from_millis(300);
/// One agent node may run for an hour before the workflow calls it stuck. An
/// approval waits the same ceiling: automation that waits forever is a hang.
const NODE_TIMEOUT_SECS: u64 = 60 * 60;

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn cap(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    value.chars().take(limit).collect()
}

pub(crate) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS workflow_runs (
            id TEXT PRIMARY KEY, workflow_id TEXT NOT NULL, name TEXT NOT NULL,
            status TEXT NOT NULL, error TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL, updated_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS workflow_steps (
            run_id TEXT NOT NULL, node_id TEXT NOT NULL, kind TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '', status TEXT NOT NULL,
            output TEXT NOT NULL DEFAULT '', agent_run_id TEXT NOT NULL DEFAULT '',
            started_at REAL NOT NULL DEFAULT 0, ended_at REAL NOT NULL DEFAULT 0,
            PRIMARY KEY(run_id, node_id)
        );
        CREATE TABLE IF NOT EXISTS workflow_schedule_claims (
            workflow_id TEXT PRIMARY KEY, window_at REAL NOT NULL
        );",
    )
}

/// A workflow run cannot survive the process that drives it: the graph lives in
/// the task's memory, so a row still marked live would otherwise claim work that
/// is not happening. Steps keep what they did.
pub(crate) fn mark_interrupted(state: &AppState) {
    if let Ok(conn) = crate::open_db(&state.database_path) {
        let _ = conn.execute(
            "UPDATE workflow_runs SET status='interrupted', error='The app closed while this workflow was running.' \
             WHERE status IN ('running','waiting')",
            [],
        );
    }
}

// ---------------------------------------------------------------------------
// Stored workflows

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Workflow {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub skill: String,
    /// The graph: a JSON array of nodes. Missing on every workflow saved before
    /// graphs existed, which is why it defaults to an empty array.
    #[serde(default)]
    pub nodes: Value,
    /// 0 means manual only; anything else is the interval in minutes.
    #[serde(default, rename = "everyMinutes")]
    pub every_minutes: i64,
    /// Start of the current schedule window (unix seconds). Advanced when a
    /// scheduled run starts, so a restart does not fire early.
    #[serde(default, rename = "scheduleSince")]
    pub schedule_since: f64,
}

fn path(state: &AppState) -> PathBuf {
    state.config_path.with_file_name("workflows.json")
}

fn load(state: &AppState) -> Vec<Workflow> {
    fs::read(path(state))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Workflow>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|workflow| !workflow.id.is_empty() && !workflow.name.is_empty())
        .collect()
}

fn store(state: &AppState, workflows: &[Workflow]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(workflows).map_err(|error| error.to_string())?;
    let temp = path(state).with_extension("tmp");
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    crate::set_private_permissions(&temp, false).map_err(|error| error.to_string())?;
    fs::rename(&temp, path(state)).map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// Graph validation

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Agent,
    Approval,
    Condition,
    Task,
}

impl Kind {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "agent" => Some(Self::Agent),
            "approval" => Some(Self::Approval),
            "condition" => Some(Self::Condition),
            "task" => Some(Self::Task),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Approval => "approval",
            Self::Condition => "condition",
            Self::Task => "task",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum When {
    Succeeded,
    Failed,
}

impl When {
    fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn matches(self, status: &str) -> bool {
        status == self.as_str()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Edge {
    id: String,
    when: When,
}

#[derive(Clone, Debug)]
struct ValidNode {
    id: String,
    kind: Kind,
    title: String,
    prompt: String,
    profile: String,
    depends_on: Vec<Edge>,
    auto_approve: bool,
    allow_writes: bool,
    allow_commands: bool,
    max_steps: i64,
    retries: i64,
    expect: When,
    note: String,
    model: String,
    /// Canvas coordinates, when the user has dragged the card. Absent means the
    /// editor lays the node out by dependency depth.
    x: Option<i64>,
    y: Option<i64>,
}

impl ValidNode {
    fn to_json(&self) -> Value {
        let mut stored = json!({
            "id": self.id,
            "kind": self.kind.as_str(),
            "title": self.title,
            "prompt": self.prompt,
            "profile": self.profile,
            "dependsOn": self.depends_on.iter().map(|edge| json!({
                "id": edge.id,
                "when": edge.when.as_str(),
            })).collect::<Vec<_>>(),
            "autoApprove": self.auto_approve,
            "allowWrites": self.allow_writes,
            "allowCommands": self.allow_commands,
            "maxSteps": self.max_steps,
            "retries": self.retries,
            "expect": self.expect.as_str(),
            "note": self.note,
            "model": self.model,
        });
        // A position is written only when the canvas gave the node one, so a graph
        // saved before positioning existed keeps its canonical form.
        if let (Some(x), Some(y)) = (self.x, self.y) {
            if let Some(object) = stored.as_object_mut() {
                object.insert("x".to_string(), json!(x));
                object.insert("y".to_string(), json!(y));
            }
        }
        stored
    }

    fn title_or_id(&self) -> String {
        if self.title.trim().is_empty() {
            self.id.clone()
        } else {
            self.title.trim().to_string()
        }
    }
}

fn clean_id(value: &str) -> Option<String> {
    let id = value.trim();
    if id.is_empty() || id.chars().count() > 40 {
        return None;
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(id.to_string())
}

fn string_field(node: &Value, key: &str) -> String {
    node.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn bool_field(node: &Value, key: &str) -> bool {
    node.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn int_field(node: &Value, key: &str, fallback: i64, low: i64, high: i64) -> i64 {
    node.get(key)
        .and_then(Value::as_i64)
        .unwrap_or(fallback)
        .clamp(low, high)
}

fn parse_edges(node_id: &str, raw: Option<&Value>) -> Result<Vec<Edge>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let Some(list) = raw.as_array() else {
        return Err(format!("Node \"{node_id}\": dependsOn must be a list."));
    };
    let mut edges: Vec<Edge> = Vec::new();
    for entry in list {
        let edge = match entry {
            Value::String(id) => Edge {
                id: clean_id(id).ok_or_else(|| {
                    format!("Node \"{node_id}\": a dependency id must be 1-40 letters, digits, - or _.")
                })?,
                when: When::Succeeded,
            },
            Value::Object(map) => {
                let id = map.get("id").and_then(Value::as_str).unwrap_or("");
                let id = clean_id(id).ok_or_else(|| {
                    format!("Node \"{node_id}\": a dependency needs a valid id.")
                })?;
                let when = match map.get("when").and_then(Value::as_str).unwrap_or("succeeded") {
                    "succeeded" => When::Succeeded,
                    "failed" => When::Failed,
                    other => {
                        return Err(format!(
                            "Node \"{node_id}\": when must be \"succeeded\" or \"failed\", not \"{other}\"."
                        ))
                    }
                };
                Edge { id, when }
            }
            _ => {
                return Err(format!(
                    "Node \"{node_id}\": each dependency is an id or an {{id, when}} object."
                ))
            }
        };
        if let Some(seen) = edges.iter().find(|seen| seen.id == edge.id) {
            if seen.when != edge.when {
                return Err(format!(
                    "Node \"{node_id}\" names dependency \"{}\" twice with different outcomes.",
                    edge.id
                ));
            }
            continue;
        }
        edges.push(edge);
    }
    Ok(edges)
}

/// Validate and normalise a stored or submitted graph. Every error names the
/// node so the editor can point at it rather than at the array.
/// A node's canvas position, when it carries one. Only a drag writes these, so a
/// value that is missing or not a number means "lay this node out by dependency
/// depth" rather than an error — a graph written before positioning still loads.
/// Out-of-range values are clamped instead of rejected; the editor bounds a drag.
fn position_field(entry: &Value, name: &str) -> Option<i64> {
    let value = entry.get(name)?.as_f64()?;
    if !value.is_finite() {
        return None;
    }
    Some(value.round().clamp(0.0, 20_000.0) as i64)
}

fn validate_nodes(raw: &Value) -> Result<Vec<ValidNode>, String> {
    let list = match raw {
        Value::Null => return Ok(Vec::new()),
        Value::Array(list) => list,
        _ => return Err("nodes must be a list.".to_string()),
    };
    if list.len() > MAX_NODES {
        return Err(format!(
            "A workflow can hold {MAX_NODES} nodes; this one has {}.",
            list.len()
        ));
    }
    let mut nodes: Vec<ValidNode> = Vec::new();
    for entry in list {
        if !entry.is_object() {
            return Err("Every node must be an object.".to_string());
        }
        let id = clean_id(&string_field(entry, "id"))
            .ok_or_else(|| "Every node needs an id of 1-40 letters, digits, - or _.".to_string())?;
        if nodes.iter().any(|node| node.id == id) {
            return Err(format!("Node id \"{id}\" is used twice."));
        }
        let kind_name = string_field(entry, "kind");
        let kind = Kind::parse(&kind_name).ok_or_else(|| {
            format!(
                "Node \"{id}\": unknown kind \"{kind_name}\" — use agent, approval, condition or task."
            )
        })?;
        let title = cap(&string_field(entry, "title"), 80);
        let prompt = string_field(entry, "prompt");
        let profile = if matches!(kind, Kind::Agent) {
            let requested = string_field(entry, "profile");
            let requested = if requested.is_empty() {
                "coding".to_string()
            } else {
                requested
            };
            crate::agent::checked_profile(&requested).map_err(|error| format!("Node \"{id}\": {error}"))?
        } else {
            String::new()
        };
        if matches!(kind, Kind::Agent) && prompt.is_empty() {
            return Err(format!("Node \"{id}\": an agent node needs a prompt."));
        }
        if prompt.chars().count() > MAX_PROMPT_CHARS {
            return Err(format!(
                "Node \"{id}\": the prompt is longer than {MAX_PROMPT_CHARS} characters."
            ));
        }
        let note = cap(&string_field(entry, "note"), MAX_NOTE_CHARS);
        if matches!(kind, Kind::Task) && note.is_empty() && prompt.is_empty() {
            return Err(format!("Node \"{id}\": a task node needs a note."));
        }
        let depends_on = parse_edges(&id, entry.get("dependsOn"))?;
        if matches!(kind, Kind::Condition) && depends_on.is_empty() {
            return Err(format!(
                "Node \"{id}\": a condition node needs at least one dependency to check."
            ));
        }
        let expect = match string_field(entry, "expect").as_str() {
            "" | "succeeded" => When::Succeeded,
            "failed" => When::Failed,
            other => {
                return Err(format!(
                    "Node \"{id}\": expect must be \"succeeded\" or \"failed\", not \"{other}\"."
                ))
            }
        };
        // A position is a pair: one coordinate without the other would pin the node
        // to an axis it was never dragged along, so a half position counts as none.
        let (x, y) = match (position_field(entry, "x"), position_field(entry, "y")) {
            (Some(x), Some(y)) => (Some(x), Some(y)),
            _ => (None, None),
        };
        nodes.push(ValidNode {
            id,
            kind,
            title,
            prompt,
            profile,
            depends_on,
            auto_approve: bool_field(entry, "autoApprove"),
            allow_writes: bool_field(entry, "allowWrites"),
            allow_commands: bool_field(entry, "allowCommands"),
            max_steps: int_field(entry, "maxSteps", 8, 1, 40),
            retries: int_field(entry, "retries", 0, 0, 3),
            expect,
            note,
            model: cap(&string_field(entry, "model"), 200),
            x,
            y,
        });
    }
    // Dependencies must exist and cannot point at the node itself.
    for node in &nodes {
        for edge in &node.depends_on {
            if edge.id == node.id {
                return Err(format!("Node \"{}\" depends on itself.", node.id));
            }
            if !nodes.iter().any(|other| other.id == edge.id) {
                return Err(format!(
                    "Node \"{}\" depends on \"{}\", which is not in this workflow.",
                    node.id, edge.id
                ));
            }
        }
    }
    // A cycle would leave every node in it permanently unready.
    fn visit(
        index: usize,
        nodes: &[ValidNode],
        marks: &mut [u8],
        trail: &mut Vec<String>,
    ) -> Result<(), String> {
        match marks[index] {
            2 => return Ok(()),
            1 => {
                trail.push(nodes[index].id.clone());
                return Err(format!("The graph has a cycle: {}.", trail.join(" -> ")));
            }
            _ => {}
        }
        marks[index] = 1;
        trail.push(nodes[index].id.clone());
        for edge in &nodes[index].depends_on {
            if let Some(next) = nodes.iter().position(|node| node.id == edge.id) {
                visit(next, nodes, marks, trail)?;
            }
        }
        trail.pop();
        marks[index] = 2;
        Ok(())
    }
    let mut marks = vec![0u8; nodes.len()];
    for index in 0..nodes.len() {
        let mut trail = Vec::new();
        visit(index, &nodes, &mut marks, &mut trail)?;
    }
    Ok(nodes)
}

// ---------------------------------------------------------------------------
// Scheduling (pure, so the order can be tested without a database)

fn is_settled(status: &str) -> bool {
    matches!(
        status,
        "succeeded" | "failed" | "skipped" | "cancelled" | "approved" | "rejected"
    )
}

fn status_of<'a>(statuses: &'a HashMap<String, String>, id: &str) -> &'a str {
    statuses.get(id).map(String::as_str).unwrap_or("pending")
}

/// The first node, in declaration order, whose every dependency has settled.
/// Whether the edge conditions are met is a separate question: an unmet branch
/// skips the node instead of blocking the graph.
fn next_ready(nodes: &[ValidNode], statuses: &HashMap<String, String>) -> Option<usize> {
    nodes
        .iter()
        .position(|node| {
            status_of(statuses, &node.id) == "pending"
                && node
                    .depends_on
                    .iter()
                    .all(|edge| is_settled(status_of(statuses, &edge.id)))
        })
}

fn edges_satisfied(node: &ValidNode, statuses: &HashMap<String, String>) -> bool {
    node.depends_on
        .iter()
        .all(|edge| edge.when.matches(status_of(statuses, &edge.id)))
}

/// A failure is handled only by a dependent that asked for it and then
/// succeeded; anything else leaves the run failed. The message names the nodes
/// that were not handled.
fn outcome(nodes: &[ValidNode], statuses: &HashMap<String, String>) -> (bool, String) {
    let mut unhandled = Vec::new();
    for node in nodes {
        if status_of(statuses, &node.id) != "failed" {
            continue;
        }
        let handled = nodes.iter().any(|other| {
            other
                .depends_on
                .iter()
                .any(|edge| edge.id == node.id && edge.when == When::Failed)
                && status_of(statuses, &other.id) == "succeeded"
        });
        if !handled {
            unhandled.push(node.id.clone());
        }
    }
    if unhandled.is_empty() {
        (true, String::new())
    } else {
        (
            false,
            format!("Failed and not handled: {}.", unhandled.join(", ")),
        )
    }
}

// ---------------------------------------------------------------------------
// Run storage

fn insert_run(state: &AppState, run_id: &str, workflow: &Workflow) -> Result<(), String> {
    let conn = crate::open_db(&state.database_path).map_err(|error| error.to_string())?;
    let stamp = now();
    conn.execute(
        "INSERT INTO workflow_runs(id,workflow_id,name,status,error,created_at,updated_at) \
         VALUES(?1,?2,?3,'running','',?4,?4)",
        params![run_id, workflow.id, workflow.name, stamp],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn insert_steps(state: &AppState, run_id: &str, nodes: &[ValidNode]) -> Result<(), String> {
    let conn = crate::open_db(&state.database_path).map_err(|error| error.to_string())?;
    for node in nodes {
        conn.execute(
            "INSERT INTO workflow_steps(run_id,node_id,kind,title,status,output,agent_run_id,started_at,ended_at) \
             VALUES(?1,?2,?3,?4,'pending','','',0,0)",
            params![run_id, node.id, node.kind.as_str(), node.title_or_id()],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn set_run(state: &AppState, run_id: &str, status: &str, error: &str) {
    if let Ok(conn) = crate::open_db(&state.database_path) {
        let _ = conn.execute(
            "UPDATE workflow_runs SET status=?2, error=?3, updated_at=?4 WHERE id=?1",
            params![run_id, status, cap(error, MAX_ERROR_CHARS), now()],
        );
    }
}

fn run_status(state: &AppState, run_id: &str) -> String {
    crate::open_db(&state.database_path)
        .ok()
        .and_then(|conn| {
            conn.query_row("SELECT status FROM workflow_runs WHERE id=?1", [run_id], |row| {
                row.get::<_, String>(0)
            })
            .ok()
        })
        .unwrap_or_default()
}

fn begin_step(state: &AppState, run_id: &str, node: &ValidNode, status: &str, output: &str) {
    if let Ok(conn) = crate::open_db(&state.database_path) {
        let _ = conn.execute(
            "UPDATE workflow_steps SET status=?3, output=?4, started_at=CASE WHEN started_at=0 THEN ?5 ELSE started_at END \
             WHERE run_id=?1 AND node_id=?2",
            params![run_id, node.id, status, cap(output, MAX_ERROR_CHARS), now()],
        );
    }
}

fn finish_step(state: &AppState, run_id: &str, node: &ValidNode, status: &str, output: &str) {
    if let Ok(conn) = crate::open_db(&state.database_path) {
        let _ = conn.execute(
            "UPDATE workflow_steps SET status=?3, output=?4, ended_at=?5 \
             WHERE run_id=?1 AND node_id=?2",
            params![run_id, node.id, status, cap(output, MAX_ERROR_CHARS), now()],
        );
    }
}

fn step_status(state: &AppState, run_id: &str, node_id: &str) -> String {
    crate::open_db(&state.database_path)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT status FROM workflow_steps WHERE run_id=?1 AND node_id=?2",
                params![run_id, node_id],
                |row| row.get::<_, String>(0),
            )
            .ok()
        })
        .unwrap_or_default()
}

fn step_rows(state: &AppState, run_id: &str) -> Vec<(String, String)> {
    let Ok(conn) = crate::open_db(&state.database_path) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT node_id, status FROM workflow_steps WHERE run_id=?1 ORDER BY rowid",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([run_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

/// Only a run the process abandoned can be resumed. A cancelled or failed run
/// ended on purpose, and a live one still has a task driving it.
fn resumable(status: &str) -> bool {
    status == "interrupted"
}

/// Steps that keep their result across a resume: everything that already ended,
/// including a branch that was skipped and an answered approval. Anything else —
/// pending, running, waiting, interrupted — goes back to pending, because the
/// task that was driving it is gone.
fn carries_over(status: &str) -> bool {
    matches!(
        status,
        "succeeded" | "failed" | "skipped" | "cancelled" | "approved" | "rejected"
    )
}

fn step_statuses(state: &AppState, run_id: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(conn) = crate::open_db(&state.database_path) else {
        return map;
    };
    let Ok(mut stmt) = conn.prepare("SELECT node_id, status FROM workflow_steps WHERE run_id=?1") else {
        return map;
    };
    let Ok(rows) = stmt.query_map([run_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) else {
        return map;
    };
    for row in rows.flatten() {
        map.insert(row.0, row.1);
    }
    map
}

fn cancelled(state: &AppState, run_id: &str) -> bool {
    matches!(run_status(state, run_id).as_str(), "cancelled" | "interrupted")
}

// ---------------------------------------------------------------------------
// Execution

enum StepEnd {
    Succeeded(String),
    Failed(String),
    Cancelled,
}

fn spawn(state: AppState, run_id: String, nodes: Vec<ValidNode>) {
    tokio::spawn(async move {
        execute(&state, &run_id, &nodes).await;
    });
}

async fn execute(state: &AppState, run_id: &str, nodes: &[ValidNode]) {
    loop {
        if cancelled(state, run_id) {
            return;
        }
        let statuses = step_statuses(state, run_id);
        if let Some(index) = next_ready(nodes, &statuses) {
            let node = &nodes[index];
            if !edges_satisfied(node, &statuses) {
                finish_step(
                    state,
                    run_id,
                    node,
                    "skipped",
                    "A branch condition was not met, so this node did not run.",
                );
                continue;
            }
            let mut attempt = 0;
            loop {
                let end = execute_node(state, run_id, node, attempt).await;
                match end {
                    StepEnd::Succeeded(output) => {
                        finish_step(state, run_id, node, "succeeded", &output);
                        set_run(state, run_id, "running", "");
                        break;
                    }
                    StepEnd::Cancelled => {
                        finish_step(state, run_id, node, "cancelled", "The run was cancelled.");
                        return;
                    }
                    StepEnd::Failed(error) => {
                        if attempt < node.retries {
                            attempt += 1;
                            begin_step(
                                state,
                                run_id,
                                node,
                                "running",
                                &format!("attempt {attempt} failed: {error} — retrying"),
                            );
                            continue;
                        }
                        finish_step(state, run_id, node, "failed", &error);
                        break;
                    }
                }
            }
            continue;
        }
        if nodes
            .iter()
            .all(|node| is_settled(status_of(&statuses, &node.id)))
        {
            let (ok, detail) = outcome(nodes, &statuses);
            if ok {
                set_run(state, run_id, "succeeded", "");
            } else {
                set_run(state, run_id, "failed", &detail);
            }
            return;
        }
        // Nothing is ready and nothing is settled: validation rules out a cycle,
        // so this is a defect worth reporting rather than waiting on.
        set_run(
            state,
            run_id,
            "failed",
            "The graph could not be scheduled; check its dependencies.",
        );
        return;
    }
}

async fn execute_node(
    state: &AppState,
    run_id: &str,
    node: &ValidNode,
    attempt: i64,
) -> StepEnd {
    match node.kind {
        Kind::Agent => execute_agent(state, run_id, node, attempt).await,
        Kind::Approval => execute_approval(state, run_id, node).await,
        Kind::Condition => execute_condition(state, run_id, node),
        Kind::Task => {
            begin_step(state, run_id, node, "running", "Recording the note.");
            let text = if node.note.is_empty() {
                node.prompt.clone()
            } else {
                node.note.clone()
            };
            StepEnd::Succeeded(text)
        }
    }
}

fn execute_condition(state: &AppState, run_id: &str, node: &ValidNode) -> StepEnd {
    begin_step(state, run_id, node, "running", "Checking its dependencies.");
    let statuses = step_statuses(state, run_id);
    let observed = node
        .depends_on
        .iter()
        .map(|edge| format!("{}={}", edge.id, status_of(&statuses, &edge.id)))
        .collect::<Vec<_>>()
        .join(", ");
    let matched = node.depends_on.iter().all(|edge| {
        status_of(&statuses, &edge.id) == node.expect.as_str()
    });
    if matched {
        StepEnd::Succeeded(format!("Every dependency ended {} ({observed}).", node.expect.as_str()))
    } else {
        StepEnd::Failed(format!(
            "Expected every dependency to end {} but found {observed}.",
            node.expect.as_str()
        ))
    }
}

async fn execute_approval(state: &AppState, run_id: &str, node: &ValidNode) -> StepEnd {
    begin_step(
        state,
        run_id,
        node,
        "waiting",
        "Waiting for a person to approve this step.",
    );
    set_run(state, run_id, "waiting", "");
    let deadline = Instant::now() + Duration::from_secs(NODE_TIMEOUT_SECS);
    loop {
        if cancelled(state, run_id) {
            return StepEnd::Cancelled;
        }
        match step_status(state, run_id, &node.id).as_str() {
            "approved" => {
                set_run(state, run_id, "running", "");
                return StepEnd::Succeeded("Approved by the user.".to_string());
            }
            "rejected" => return StepEnd::Failed("Rejected by the user.".to_string()),
            _ => {}
        }
        if Instant::now() > deadline {
            return StepEnd::Failed("Nobody approved this step within an hour.".to_string());
        }
        tokio::time::sleep(APPROVAL_POLL).await;
    }
}

fn insert_agent_run(
    state: &AppState,
    node: &ValidNode,
    prompt: &str,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let project = crate::active_project_id(state);
    let (snapshot, model) = crate::run_record_snapshot(
        state,
        if node.model.is_empty() {
            None
        } else {
            Some(node.model.as_str())
        },
    );
    let conn = crate::open_db(&state.database_path).map_err(|error| error.to_string())?;
    let stamp = now();
    conn.execute(
        "INSERT INTO agent_runs(id,project_id,goal,status,allow_writes,allow_commands,auto_approve,max_steps,pending_action,error,created_at,updated_at,router_config,model,profile,mode,crew,stage_policy,stage_max_steps,allow_mcp,priority,depends_on) \
         VALUES(?1,?2,?3,'queued',?4,?5,?6,?7,'','',?8,?8,?9,?10,?11,'','','',0,0,0,'')",
        params![
            id,
            project,
            cap(prompt, MAX_PROMPT_CHARS),
            node.allow_writes,
            node.allow_commands,
            node.auto_approve,
            node.max_steps,
            stamp,
            snapshot,
            model,
            node.profile,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(id)
}

fn agent_outcome(state: &AppState, run_id: &str) -> Option<(String, String)> {
    crate::open_db(&state.database_path)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT status, error FROM agent_runs WHERE id=?1",
                [run_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .ok()
        })
}

async fn execute_agent(
    state: &AppState,
    run_id: &str,
    node: &ValidNode,
    attempt: i64,
) -> StepEnd {
    if state.workspace.root().is_err() {
        return StepEnd::Failed(
            "Open a workspace folder first — agent nodes run inside it.".to_string(),
        );
    }
    let prompt = if attempt == 0 {
        node.prompt.clone()
    } else {
        format!(
            "{}\n\nThis is attempt {} after the previous run failed. Diagnose that failure and finish the task.",
            node.prompt,
            attempt + 1
        )
    };
    let agent_id = match insert_agent_run(state, node, &prompt) {
        Ok(id) => id,
        Err(error) => return StepEnd::Failed(error),
    };
    crate::agent::start_queued(state).await;
    let waiting = format!("Started agent run {agent_id}.");
    begin_step(state, run_id, node, "running", &waiting);
    if let Ok(conn) = crate::open_db(&state.database_path) {
        let _ = conn.execute(
            "UPDATE workflow_steps SET agent_run_id=?3 WHERE run_id=?1 AND node_id=?2",
            params![run_id, node.id, agent_id],
        );
    }
    let deadline = Instant::now() + Duration::from_secs(NODE_TIMEOUT_SECS);
    let mut last = String::new();
    loop {
        if cancelled(state, run_id) {
            return StepEnd::Cancelled;
        }
        let Some((status, error)) = agent_outcome(state, &agent_id) else {
            return StepEnd::Failed(format!("Agent run {agent_id} disappeared."));
        };
        if status != last {
            match status.as_str() {
                "awaiting_approval" => {
                    begin_step(
                        state,
                        run_id,
                        node,
                        "waiting",
                        &format!("Agent run {agent_id} is waiting for your approval."),
                    );
                    set_run(state, run_id, "waiting", "");
                }
                "running" => {
                    begin_step(
                        state,
                        run_id,
                        node,
                        "running",
                        &format!("Agent run {agent_id} is running."),
                    );
                    set_run(state, run_id, "running", "");
                }
                _ => {}
            }
            last = status.clone();
        }
        match status.as_str() {
            "succeeded" => {
                return StepEnd::Succeeded(format!("Agent run {agent_id} succeeded."))
            }
            "failed" | "cancelled" | "interrupted" => {
                let suffix = if error.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", cap(error.trim(), MAX_ERROR_CHARS))
                };
                return StepEnd::Failed(format!(
                    "Agent run {agent_id} ended {status}{suffix}"
                ));
            }
            _ => {}
        }
        if Instant::now() > deadline {
            return StepEnd::Failed(format!(
                "Agent run {agent_id} did not finish within an hour."
            ));
        }
        tokio::time::sleep(AGENT_POLL).await;
    }
}

// ---------------------------------------------------------------------------
// Routes

fn ok(payload: Value) -> Response {
    Json(payload).into_response()
}

fn bad(message: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message.to_string() })),
    )
        .into_response()
}

fn not_found(message: impl std::fmt::Display) -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": message.to_string() })),
    )
        .into_response()
}

fn server_error(message: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": message.to_string() })),
    )
        .into_response()
}

pub(crate) async fn list(State(state): State<AppState>) -> Response {
    let workflows = load(&state)
        .iter()
        .map(|workflow| workflow_json(&state, workflow))
        .collect::<Vec<_>>();
    ok(json!({ "ok": true, "workflows": workflows }))
}

fn last_run_at(state: &AppState, workflow_id: &str) -> f64 {
    crate::open_db(&state.database_path)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT COALESCE(MAX(created_at),0) FROM workflow_runs WHERE workflow_id=?1",
                [workflow_id],
                |row| row.get::<_, f64>(0),
            )
            .ok()
        })
        .unwrap_or(0.0)
}

fn live_run(state: &AppState, workflow_id: &str) -> bool {
    crate::open_db(&state.database_path)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM workflow_runs WHERE workflow_id=?1 AND status IN ('running','waiting')",
                [workflow_id],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        })
        .unwrap_or(0)
        > 0
}

/// When the next scheduled run is due. `None` means the workflow is manual. The
/// later of the last run and the window start is the anchor, so a long run does
/// not stack up missed intervals, and a restart cannot fire early.
fn due_at(last_run: f64, since: f64, every_minutes: i64) -> Option<f64> {
    if every_minutes <= 0 {
        return None;
    }
    let anchor = last_run.max(since);
    Some(if anchor <= 0.0 {
        0.0
    } else {
        anchor + (every_minutes as f64) * 60.0
    })
}

fn parse_interval(minutes: i64) -> Result<i64, String> {
    if minutes == 0 {
        return Ok(0);
    }
    if (5..=10_080).contains(&minutes) {
        return Ok(minutes);
    }
    Err("A schedule runs every 5 minutes to 7 days, or 0 for manual only.".to_string())
}

fn workflow_json(state: &AppState, workflow: &Workflow) -> Value {
    let mut value = serde_json::to_value(workflow).unwrap_or_else(|_| json!({}));
    let last = last_run_at(state, &workflow.id);
    value["lastRunAt"] = json!(last);
    value["nextRunAt"] = match due_at(last, workflow.schedule_since, workflow.every_minutes) {
        Some(next) => json!(next),
        None => Value::Null,
    };
    value
}

/// A file name that survives every platform: lowercase ascii, single dashes,
/// never empty.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "workflow".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

/// Importing twice must not overwrite the first copy: names are the handle a
/// person sees, ids are internal.
fn unique_name(base: &str, workflows: &[Workflow]) -> String {
    let base = if base.trim().is_empty() {
        "Imported workflow"
    } else {
        base.trim()
    };
    if !workflows
        .iter()
        .any(|workflow| workflow.name.eq_ignore_ascii_case(base))
    {
        return base.to_string();
    }
    for suffix in 2..1000 {
        let candidate = format!("{base} ({suffix})");
        if !workflows
            .iter()
            .any(|workflow| workflow.name.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
    }
    format!("{base} ({})", uuid::Uuid::new_v4().simple())
}

struct ImportedGraph {
    name: String,
    prompt: String,
    skill: String,
    every_minutes: i64,
    nodes: Vec<Value>,
}

/// Validate a document before it can touch `workflows.json`. The same rules as
/// the save route apply, so an imported graph cannot be a way around them.
fn parse_import(document: &Value) -> Result<ImportedGraph, String> {
    if !document.is_object() {
        return Err("An imported workflow must be a JSON object.".to_string());
    }
    let name = clipped(document.get("name").and_then(Value::as_str), 64)
        .trim()
        .to_string();
    if name.is_empty() {
        return Err("An imported workflow needs a name.".to_string());
    }
    let prompt = clipped(
        document.get("prompt").and_then(Value::as_str),
        MAX_PROMPT_CHARS,
    );
    let skill = clipped(document.get("skill").and_then(Value::as_str), 160);
    let every_minutes = parse_interval(
        document.get("everyMinutes").and_then(Value::as_i64).unwrap_or(0),
    )?;
    let raw = document.get("nodes").cloned().unwrap_or_else(|| json!([]));
    let nodes = if matches!(raw, Value::Null) {
        Vec::new()
    } else {
        validate_nodes(&raw)?
            .iter()
            .map(ValidNode::to_json)
            .collect::<Vec<_>>()
    };
    if prompt.trim().is_empty() && nodes.is_empty() {
        return Err("An imported workflow needs a prompt or at least one node.".to_string());
    }
    if every_minutes > 0 && nodes.is_empty() {
        return Err("A scheduled workflow needs at least one node.".to_string());
    }
    Ok(ImportedGraph {
        name,
        prompt,
        skill,
        every_minutes,
        nodes,
    })
}

fn export_document(workflow: &Workflow) -> Value {
    json!({
        "format": "sarthi.workflow",
        "version": EXPORT_VERSION,
        "name": workflow.name,
        "prompt": workflow.prompt,
        "skill": workflow.skill,
        "everyMinutes": workflow.every_minutes,
        "nodes": workflow.nodes.clone(),
    })
}

pub(crate) async fn export(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let Some(workflow) = load(&state)
        .into_iter()
        .find(|workflow| workflow.id == id)
    else {
        return not_found("No such workflow.");
    };
    let body = match serde_json::to_string_pretty(&export_document(&workflow)) {
        Ok(body) => body,
        Err(error) => return server_error(error),
    };
    (
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}.json\"", slug(&workflow.name)),
            ),
        ],
        body,
    )
        .into_response()
}

pub(crate) async fn import_graph(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response {
    let document = input.get("workflow").cloned().unwrap_or(input);
    let parsed = match parse_import(&document) {
        Ok(parsed) => parsed,
        Err(error) => return bad(error),
    };
    let mut workflows = load(&state);
    let workflow = Workflow {
        id: uuid::Uuid::new_v4().to_string(),
        name: unique_name(&parsed.name, &workflows),
        prompt: parsed.prompt,
        skill: parsed.skill,
        nodes: Value::Array(parsed.nodes),
        every_minutes: parsed.every_minutes,
        // A fresh window, so importing a nightly workflow does not start it at once.
        schedule_since: if parsed.every_minutes > 0 { now() } else { 0.0 },
    };
    workflows.push(workflow.clone());
    workflows.sort_by_key(|workflow| workflow.name.to_lowercase());
    if let Err(error) = store(&state, &workflows) {
        return server_error(error);
    }
    ok(json!({ "ok": true, "workflow": workflow_json(&state, &workflow) }))
}

fn clipped(value: Option<&str>, max: usize) -> String {
    value.unwrap_or("").chars().take(max).collect::<String>()
}

pub(crate) async fn save(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let name = clipped(input.get("name").and_then(Value::as_str), 64)
        .trim()
        .to_string();
    let prompt = clipped(input.get("prompt").and_then(Value::as_str), MAX_PROMPT_CHARS);
    let skill = clipped(input.get("skill").and_then(Value::as_str), 160);
    if name.is_empty() {
        return bad("A workflow needs a name.");
    }
    let raw_nodes = input.get("nodes").cloned().unwrap_or_else(|| json!([]));
    let canonical = if matches!(raw_nodes, Value::Null) {
        Vec::new()
    } else {
        match validate_nodes(&raw_nodes) {
            Ok(nodes) => nodes.iter().map(ValidNode::to_json).collect::<Vec<_>>(),
            Err(error) => return bad(error),
        }
    };
    if prompt.trim().is_empty() && canonical.is_empty() {
        return bad("A workflow needs a prompt or at least one node.");
    }

    let every_minutes = match parse_interval(
        input.get("everyMinutes").and_then(Value::as_i64).unwrap_or(0),
    ) {
        Ok(minutes) => minutes,
        Err(error) => return bad(error),
    };
    if every_minutes > 0 && canonical.is_empty() {
        return bad("A scheduled workflow needs at least one node.");
    }

    let mut workflows = load(&state);
    let requested_id = input.get("id").and_then(Value::as_str).unwrap_or("");
    let known = workflows
        .iter()
        .any(|workflow| !requested_id.is_empty() && workflow.id == requested_id);
    let id = if known {
        requested_id.to_string()
    } else {
        uuid::Uuid::new_v4().to_string()
    };
    let previous = workflows.iter().find(|workflow| workflow.id == id).cloned();
    // Keeping the same interval keeps its window; any change starts a fresh one,
    // so saving a workflow never makes it due immediately by accident.
    let schedule_since = match (&previous, every_minutes) {
        (_, 0) => 0.0,
        (Some(previous), minutes) if previous.every_minutes == minutes && previous.schedule_since > 0.0 => {
            previous.schedule_since
        }
        _ => now(),
    };
    workflows.retain(|workflow| workflow.id != id);
    workflows.push(Workflow {
        id: id.clone(),
        name,
        prompt,
        skill,
        nodes: Value::Array(canonical),
        every_minutes,
        schedule_since,
    });
    workflows.sort_by_key(|workflow| workflow.name.to_lowercase());
    if let Err(error) = store(&state, &workflows) {
        return server_error(error);
    }
    ok(json!({ "ok": true, "id": id }))
}

pub(crate) async fn delete(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let mut workflows = load(&state);
    let before = workflows.len();
    workflows.retain(|workflow| workflow.id != id);
    if workflows.len() == before {
        return not_found("No such workflow.");
    }
    if let Err(error) = store(&state, &workflows) {
        return server_error(error);
    }
    // A claim would otherwise outlive its workflow, and because a claim only moves
    // forward it could suppress a later workflow that reuses the id.
    if let Ok(conn) = crate::open_db(&state.database_path) {
        let _ = conn.execute(
            "DELETE FROM workflow_schedule_claims WHERE workflow_id=?1",
            [id.as_str()],
        );
    }
    ok(json!({ "ok": true }))
}

pub(crate) async fn run(State(state): State<AppState>, AxumPath(id): AxumPath<String>) -> Response {
    let Some(workflow) = load(&state).into_iter().find(|workflow| workflow.id == id) else {
        return not_found("No such workflow.");
    };
    let nodes = match validate_nodes(&workflow.nodes) {
        Ok(nodes) => nodes,
        Err(error) => return bad(error),
    };
    if nodes.is_empty() {
        return bad("This workflow has no nodes yet. Add a graph, or load its prompt instead.");
    }
    match begin_run(&state, &workflow, &nodes) {
        Ok(run_id) => ok(json!({
            "ok": true,
            "run": { "id": run_id, "status": "running" }
        })),
        Err(error) => server_error(error),
    }
}

fn begin_run(
    state: &AppState,
    workflow: &Workflow,
    nodes: &[ValidNode],
) -> Result<String, String> {
    let run_id = uuid::Uuid::new_v4().to_string();
    insert_run(state, &run_id, workflow)?;
    if let Err(error) = insert_steps(state, &run_id, nodes) {
        set_run(state, &run_id, "failed", "Could not record the steps.");
        return Err(error);
    }
    spawn(state.clone(), run_id.clone(), nodes.to_vec());
    Ok(run_id)
}

/// Claim a due time for a workflow, across processes.
///
/// Every process runs its own timer, so a desktop app and a headless server would
/// otherwise both see the same workflow as due and start two runs for one window.
/// The database is the only place both write, so the claim is one conditional
/// upsert: it succeeds only when it moves the stored window forward, which means
/// exactly one process can own a given due time. A clock running backwards cannot
/// reopen a window either, because an older due time never satisfies the predicate.
fn claim_window(conn: &Connection, workflow_id: &str, window: f64) -> bool {
    matches!(
        conn.execute(
            "INSERT INTO workflow_schedule_claims(workflow_id, window_at) VALUES(?1, ?2) \
             ON CONFLICT(workflow_id) DO UPDATE SET window_at=excluded.window_at \
             WHERE excluded.window_at > workflow_schedule_claims.window_at",
            rusqlite::params![workflow_id, window],
        ),
        Ok(1)
    )
}

/// Hand a claimed window back when the run could not start, so the next tick
/// retries instead of waiting out the whole interval.
fn release_window(conn: &Connection, workflow_id: &str, window: f64) {
    let _ = conn.execute(
        "DELETE FROM workflow_schedule_claims WHERE workflow_id=?1 AND window_at<=?2",
        rusqlite::params![workflow_id, window],
    );
}

/// Scheduled workflows wake on a timer. The first tick is one interval after
/// boot, so a restart never runs a schedule early; a due workflow with a live
/// run is skipped rather than stacked.
pub(crate) fn start_scheduler(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            tick(&state).await;
        }
    });
}

async fn tick(state: &AppState) {
    let mut workflows = load(state);
    // Without the database there is nothing to arbitrate with and nothing to
    // record a run in, so this process does not start scheduled work at all.
    let Ok(conn) = crate::open_db(&state.database_path) else {
        return;
    };
    let mut changed = false;
    for workflow in workflows.iter_mut() {
        if workflow.every_minutes <= 0 || live_run(state, &workflow.id) {
            continue;
        }
        let last = last_run_at(state, &workflow.id);
        let Some(next) = due_at(last, workflow.schedule_since, workflow.every_minutes) else {
            continue;
        };
        if now() < next {
            continue;
        }
        let Ok(nodes) = validate_nodes(&workflow.nodes) else {
            continue;
        };
        if nodes.is_empty() {
            continue;
        }
        // One due time, one run: another process ticking at the same moment wins
        // the claim first and this one leaves the window alone.
        if !claim_window(&conn, &workflow.id, next) {
            continue;
        }
        match begin_run(state, workflow, &nodes) {
            Ok(run_id) => {
                // The new row is already on disk; moving the window keeps this
                // workflow from firing again on the next tick. If the write
                // below fails, the run's own created_at is still the anchor.
                workflow.schedule_since = now();
                changed = true;
                crate::oplog::record(
                    "info",
                    "workflow",
                    "scheduled",
                    format!("{} started run {run_id} (every {} min)", workflow.name, workflow.every_minutes),
                );
            }
            Err(error) => {
                // Nothing started, so the window goes back and the next tick can
                // try again rather than waiting out the interval.
                release_window(&conn, &workflow.id, next);
                crate::oplog::record(
                    "error",
                    "workflow",
                    "schedule-failed",
                    format!("{} could not start: {error}", workflow.name),
                );
            }
        }
    }
    if changed {
        let _ = store(state, &workflows);
    }
}

fn run_rows(state: &AppState, id: Option<&str>, limit: usize) -> Vec<Value> {
    let Ok(conn) = crate::open_db(&state.database_path) else {
        return Vec::new();
    };
    let sql = if id.is_some() {
        "SELECT r.id, r.workflow_id, r.name, r.status, r.error, r.created_at, r.updated_at, \
         (SELECT COUNT(*) FROM workflow_steps s WHERE s.run_id=r.id) AS steps, \
         (SELECT COUNT(*) FROM workflow_steps s WHERE s.run_id=r.id AND s.status='succeeded') AS done \
         FROM workflow_runs r WHERE r.id=?1"
    } else {
        "SELECT r.id, r.workflow_id, r.name, r.status, r.error, r.created_at, r.updated_at, \
         (SELECT COUNT(*) FROM workflow_steps s WHERE s.run_id=r.id) AS steps, \
         (SELECT COUNT(*) FROM workflow_steps s WHERE s.run_id=r.id AND s.status='succeeded') AS done \
         FROM workflow_runs r ORDER BY r.created_at DESC LIMIT ?1"
    };
    let map = |row: &rusqlite::Row<'_>| {
        Ok(json!({
            "id": row.get::<_, String>(0)?,
            "workflowId": row.get::<_, String>(1)?,
            "name": row.get::<_, String>(2)?,
            "status": row.get::<_, String>(3)?,
            "error": row.get::<_, String>(4)?,
            "createdAt": row.get::<_, f64>(5)?,
            "updatedAt": row.get::<_, f64>(6)?,
            "steps": row.get::<_, i64>(7)?,
            "succeededSteps": row.get::<_, i64>(8)?,
        }))
    };
    let mut stmt = match conn.prepare(sql) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let rows = match id {
        Some(id) => stmt.query_map([id], map),
        None => stmt.query_map([limit as i64], map),
    };
    match rows {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

pub(crate) async fn runs(
    State(state): State<AppState>,
    AxumQuery(params): AxumQuery<HashMap<String, String>>,
) -> Response {
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20)
        .clamp(1, 100);
    ok(json!({ "ok": true, "runs": run_rows(&state, None, limit) }))
}

pub(crate) async fn run_detail(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let mut rows = run_rows(&state, Some(&id), 1);
    let Some(mut run) = rows.pop() else {
        return not_found("No such workflow run.");
    };
    let steps = crate::open_db(&state.database_path)
        .ok()
        .and_then(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT node_id, kind, title, status, output, agent_run_id, started_at, ended_at \
                     FROM workflow_steps WHERE run_id=?1 ORDER BY rowid",
                )
                .ok()?;
            let rows = stmt
                .query_map([&id], |row| {
                    Ok(json!({
                        "nodeId": row.get::<_, String>(0)?,
                        "kind": row.get::<_, String>(1)?,
                        "title": row.get::<_, String>(2)?,
                        "status": row.get::<_, String>(3)?,
                        "output": row.get::<_, String>(4)?,
                        "agentRunId": row.get::<_, String>(5)?,
                        "startedAt": row.get::<_, f64>(6)?,
                        "endedAt": row.get::<_, f64>(7)?,
                    }))
                })
                .ok()?;
            Some(rows.flatten().collect::<Vec<_>>())
        })
        .unwrap_or_default();
    run["stepList"] = json!(steps);
    ok(json!({ "ok": true, "run": run }))
}

pub(crate) async fn approve(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<Value>,
) -> Response {
    let Some(approved) = input.get("approve").and_then(Value::as_bool) else {
        return bad("Provide approve: true or false.");
    };
    let status = run_status(&state, &id);
    if status.is_empty() {
        return not_found("No such workflow run.");
    }
    if status != "waiting" {
        return bad("This workflow run is not waiting for approval.");
    }
    let next = if approved { "approved" } else { "rejected" };
    let changed = crate::open_db(&state.database_path)
        .ok()
        .and_then(|conn| {
            conn.execute(
                "UPDATE workflow_steps SET status=?2 WHERE run_id=?1 AND kind='approval' AND status='waiting'",
                params![id, next],
            )
            .ok()
        })
        .unwrap_or(0);
    if changed == 0 {
        return bad(
            "The waiting step is an agent run, not an approval node — answer it in the Runs panel.",
        );
    }
    ok(json!({ "ok": true, "status": next }))
}

pub(crate) async fn resume(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let status = run_status(&state, &id);
    if status.is_empty() {
        return not_found("No such workflow run.");
    }
    if !resumable(&status) {
        return bad(format!(
            "This workflow run is {status}; only an interrupted run can be resumed."
        ));
    }
    let workflow_id = crate::open_db(&state.database_path)
        .ok()
        .and_then(|conn| {
            conn.query_row("SELECT workflow_id FROM workflow_runs WHERE id=?1", [&id], |row| {
                row.get::<_, String>(0)
            })
            .ok()
        })
        .unwrap_or_default();
    let Some(workflow) = load(&state)
        .into_iter()
        .find(|workflow| workflow.id == workflow_id)
    else {
        return bad("The workflow behind this run was deleted, so it cannot be resumed.");
    };
    let nodes = match validate_nodes(&workflow.nodes) {
        Ok(nodes) => nodes,
        Err(error) => return bad(error),
    };
    let steps = step_rows(&state, &id);
    if steps.is_empty() {
        return bad("This run has no recorded steps to resume from.");
    }
    let mut stored = steps.iter().map(|(node, _)| node.as_str()).collect::<Vec<_>>();
    stored.sort_unstable();
    let mut expected = nodes.iter().map(|node| node.id.as_str()).collect::<Vec<_>>();
    expected.sort_unstable();
    if stored != expected {
        return bad(
            "The workflow changed after this run started, so it cannot be resumed. Start a new run instead.",
        );
    }
    if let Ok(conn) = crate::open_db(&state.database_path) {
        for (node_id, step_status) in &steps {
            if carries_over(step_status) {
                continue;
            }
            let _ = conn.execute(
                "UPDATE workflow_steps SET status='pending', output='', started_at=0, ended_at=0 \
                 WHERE run_id=?1 AND node_id=?2",
                params![id.as_str(), node_id.as_str()],
            );
        }
    }
    set_run(&state, &id, "running", "");
    spawn(state.clone(), id.clone(), nodes);
    ok(json!({
        "ok": true,
        "run": { "id": id, "status": "running" }
    }))
}

pub(crate) async fn cancel(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let status = run_status(&state, &id);
    if status.is_empty() {
        return not_found("No such workflow run.");
    }
    if matches!(status.as_str(), "succeeded" | "failed" | "cancelled" | "interrupted") {
        return bad(format!("This workflow run already ended {status}."));
    }
    set_run(&state, &id, "cancelled", "Cancelled by the user.");
    ok(json!({ "ok": true, "status": "cancelled" }))
}

// ---------------------------------------------------------------------------
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(raw: Value) -> Vec<ValidNode> {
        validate_nodes(&raw).expect("valid graph")
    }

    fn statuses(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(id, status)| (id.to_string(), status.to_string()))
            .collect()
    }

    #[test]
    fn a_graph_with_edges_and_branches_normalises() {
        let nodes = graph(json!([
            { "id": "plan", "kind": "agent", "prompt": "plan it", "profile": "planner" },
            { "id": "gate", "kind": "approval", "dependsOn": ["plan"] },
            {
                "id": "build", "kind": "agent", "prompt": "build it",
                "dependsOn": [{ "id": "gate", "when": "succeeded" }],
                "allowWrites": true, "maxSteps": 12, "retries": 1
            },
            {
                "id": "repair", "kind": "agent", "prompt": "fix it",
                "dependsOn": [{ "id": "build", "when": "failed" }]
            },
            { "id": "check", "kind": "condition", "dependsOn": ["build"], "expect": "succeeded" },
            { "id": "note", "kind": "task", "note": "done" }
        ]));
        assert_eq!(nodes.len(), 6);
        assert_eq!(nodes[0].profile, "planner");
        assert_eq!(nodes[1].kind, Kind::Approval);
        assert_eq!(nodes[2].max_steps, 12);
        assert_eq!(nodes[2].retries, 1);
        assert_eq!(nodes[3].depends_on[0].when, When::Failed);
        assert_eq!(nodes[4].expect, When::Succeeded);
        // Canonical output keeps the edges and the kind names.
        let stored = nodes[2].to_json();
        assert_eq!(stored["kind"], json!("agent"));
        assert_eq!(stored["dependsOn"][0]["id"], json!("gate"));
    }

    #[test]
    fn a_node_keeps_its_canvas_position_and_old_graphs_still_load() {
        let nodes = graph(json!([
            { "id": "a", "kind": "task", "note": "x", "x": 240, "y": 96 },
            { "id": "b", "kind": "task", "note": "y" },
            { "id": "c", "kind": "task", "note": "z", "x": -40, "y": 99999 },
            { "id": "d", "kind": "task", "note": "w", "x": "left" }
        ]));
        let stored = nodes[0].to_json();
        assert_eq!(stored["x"], json!(240));
        assert_eq!(stored["y"], json!(96));
        // A graph saved before positioning existed has no coordinates and its
        // canonical form does not invent them.
        assert!(nodes[1].to_json().get("x").is_none());
        // Out-of-range coordinates are clamped to the canvas, not rejected.
        assert_eq!(nodes[2].to_json()["x"], json!(0));
        assert_eq!(nodes[2].to_json()["y"], json!(20_000));
        // A non-numeric coordinate is treated as absent.
        assert!(nodes[3].to_json().get("x").is_none());
        // A half position is dropped as a pair.
        let partial = graph(json!([{ "id": "e", "kind": "task", "note": "n", "x": 40 }]));
        assert!(partial[0].to_json().get("x").is_none());
        // What is stored validates again, so a reload keeps the card in place.
        let reloaded = graph(json!([stored]));
        assert_eq!(reloaded[0].to_json()["x"], json!(240));
    }

    #[test]
    fn invalid_graphs_name_the_node_they_reject() {
        let cases = [
            (
                json!([{ "id": "a", "kind": "agent", "prompt": "x" }, { "id": "a", "kind": "task", "note": "y" }]),
                "used twice",
            ),
            (
                json!([{ "id": "a", "kind": "agent", "prompt": "x", "dependsOn": ["missing"] }]),
                "not in this workflow",
            ),
            (
                json!([{ "id": "a", "kind": "agent", "prompt": "x", "dependsOn": ["a"] }]),
                "depends on itself",
            ),
            (
                json!([
                    { "id": "a", "kind": "agent", "prompt": "x", "dependsOn": ["b"] },
                    { "id": "b", "kind": "agent", "prompt": "y", "dependsOn": ["a"] }
                ]),
                "cycle",
            ),
            (
                json!([{ "id": "a", "kind": "agent" }]),
                "needs a prompt",
            ),
            (
                json!([{ "id": "a", "kind": "teleport" }]),
                "unknown kind",
            ),
            (
                json!([{ "id": "a", "kind": "condition" }]),
                "at least one dependency",
            ),
            (
                json!([{ "id": "a", "kind": "task", "note": "x", "dependsOn": [{ "id": "missing", "when": "failed" }] }]),
                "not in this workflow",
            ),
            (
                json!([{ "id": "a", "kind": "agent", "prompt": "x", "profile": "wizard" }]),
                "Unknown agent profile",
            ),
            (
                json!([{ "id": "a", "kind": "agent", "prompt": "x", "dependsOn": [{ "id": "a", "when": "sometimes" }] }]),
                "when must be",
            ),
        ];
        for (raw, expected) in cases {
            let error = validate_nodes(&raw).expect_err("must be refused");
            assert!(
                error.contains(expected),
                "expected {expected:?} in {error:?}"
            );
        }
    }

    #[test]
    fn a_workflow_without_nodes_stays_a_saved_prompt() {
        assert!(validate_nodes(&json!([])).expect("empty").is_empty());
        assert!(validate_nodes(&Value::Null).expect("null").is_empty());
        assert!(validate_nodes(&json!({})).is_err());
    }

    #[test]
    fn scheduling_waits_for_dependencies_and_skips_unmet_branches() {
        let nodes = graph(json!([
            { "id": "first", "kind": "task", "note": "one" },
            { "id": "second", "kind": "task", "note": "two", "dependsOn": ["first"] },
            {
                "id": "repair", "kind": "task", "note": "fix",
                "dependsOn": [{ "id": "second", "when": "failed" }]
            },
            { "id": "after", "kind": "task", "note": "next", "dependsOn": ["second"] }
        ]));
        let mut states = statuses(&[
            ("first", "pending"),
            ("second", "pending"),
            ("repair", "pending"),
            ("after", "pending"),
        ]);
        assert_eq!(next_ready(&nodes, &states), Some(0));
        states.insert("first".to_string(), "succeeded".to_string());
        assert_eq!(next_ready(&nodes, &states), Some(1));
        states.insert("second".to_string(), "failed".to_string());
        assert_eq!(next_ready(&nodes, &states), Some(2));
        assert!(edges_satisfied(&nodes[2], &states));
        // The branch that asked for a success is not satisfied by the failure.
        assert!(!edges_satisfied(&nodes[3], &states));
    }

    #[test]
    fn a_failure_is_handled_only_by_a_branch_that_succeeded() {
        let nodes = graph(json!([
            { "id": "build", "kind": "task", "note": "build" },
            {
                "id": "repair", "kind": "task", "note": "repair",
                "dependsOn": [{ "id": "build", "when": "failed" }]
            }
        ]));
        let mut states = statuses(&[("build", "failed"), ("repair", "pending")]);
        let (ok, detail) = outcome(&nodes, &states);
        assert!(!ok);
        assert!(detail.contains("build"));
        states.insert("repair".to_string(), "succeeded".to_string());
        let (ok, _) = outcome(&nodes, &states);
        assert!(ok, "a successful repair branch handles the failure");
    }

    #[test]
    fn an_import_document_is_validated_and_normalised() {
        let parsed = parse_import(&json!({
            "format": "sarthi.workflow",
            "version": 1,
            "name": "  Nightly  ",
            "nodes": [{ "id": "tidy", "kind": "task", "note": "x" }]
        }))
        .unwrap();
        assert_eq!(parsed.name, "Nightly");
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.every_minutes, 0);
        assert!(parse_import(&json!({
            "name": "x",
            "nodes": [{ "id": "a", "kind": "nope" }]
        }))
        .is_err());
        assert!(parse_import(&json!({ "name": "x", "everyMinutes": 5, "nodes": [] })).is_err());
        assert!(parse_import(&json!({ "prompt": "x" })).is_err());
        assert!(parse_import(&json!([1, 2, 3])).is_err());
    }

    #[test]
    fn exported_files_get_a_safe_unique_name() {
        assert_eq!(slug("Nightly Review!"), "nightly-review");
        assert_eq!(slug("***"), "workflow");
        let existing = vec![Workflow {
            id: "1".to_string(),
            name: "Review".to_string(),
            prompt: String::new(),
            skill: String::new(),
            nodes: json!([]),
            every_minutes: 0,
            schedule_since: 0.0,
        }];
        assert_eq!(unique_name("Review", &existing), "Review (2)");
        assert_eq!(unique_name("Fresh", &existing), "Fresh");
    }

    #[test]
    fn a_resume_reopens_only_what_never_finished() {
        assert!(resumable("interrupted"));
        assert!(!resumable("running"));
        assert!(!resumable("waiting"));
        assert!(!resumable("succeeded"));
        assert!(carries_over("succeeded"));
        assert!(carries_over("failed"));
        assert!(carries_over("skipped"));
        assert!(carries_over("approved"));
        assert!(!carries_over("waiting"));
        assert!(!carries_over("running"));
        assert!(!carries_over("interrupted"));
        assert!(!carries_over("pending"));
    }

    #[test]
    fn a_schedule_is_due_after_its_interval_from_the_later_anchor() {
        assert_eq!(due_at(0.0, 0.0, 0), None);
        assert_eq!(due_at(1_000.0, 0.0, 5), Some(1_300.0));
        // A window that started later than the last run wins.
        assert_eq!(due_at(1_000.0, 1_200.0, 5), Some(1_500.0));
        // A run that outlasted its interval anchors on the run, not the window.
        assert_eq!(due_at(2_000.0, 1_200.0, 5), Some(2_300.0));
        // An empty workflow schedule is manual, never due.
        assert_eq!(due_at(0.0, 0.0, -5), None);
    }

    #[test]
    fn schedule_intervals_are_bounded() {
        assert_eq!(parse_interval(0), Ok(0));
        assert_eq!(parse_interval(5), Ok(5));
        assert_eq!(parse_interval(10_080), Ok(10_080));
        assert!(parse_interval(4).is_err());
        assert!(parse_interval(10_081).is_err());
        assert!(parse_interval(-1).is_err());
    }

    #[test]
    fn run_tables_are_created_idempotently() {
        let conn = Connection::open_in_memory().expect("db");
        initialize(&conn).expect("first");
        initialize(&conn).expect("second");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('workflow_runs','workflow_steps','workflow_schedule_claims')",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 3);
    }

    #[test]
    fn a_due_time_is_claimed_once_and_only_forward() {
        let conn = Connection::open_in_memory().expect("db");
        initialize(&conn).expect("schema");
        // The first process to see the window owns it.
        assert!(claim_window(&conn, "nightly", 1_700.0));
        // A second process computing the same due time loses, which is the whole
        // point: one window starts one run, not one per process.
        assert!(!claim_window(&conn, "nightly", 1_700.0));
        // A later window is a new claim.
        assert!(claim_window(&conn, "nightly", 2_000.0));
        // An older due time — a clock that went backwards — cannot reopen one.
        assert!(!claim_window(&conn, "nightly", 1_800.0));
        // Workflows claim independently of each other.
        assert!(claim_window(&conn, "weekly", 1_700.0));
        assert!(!claim_window(&conn, "weekly", 1_700.0));
    }

    #[test]
    fn a_window_that_could_not_start_is_released_for_the_next_tick() {
        let conn = Connection::open_in_memory().expect("db");
        initialize(&conn).expect("schema");
        assert!(claim_window(&conn, "nightly", 1_700.0));
        release_window(&conn, "nightly", 1_700.0);
        // A failed start is not a skipped interval: the same window is available to
        // the next tick.
        assert!(claim_window(&conn, "nightly", 1_700.0));
        // Releasing a window that is no longer current changes nothing, so a late
        // failure cannot free a window another tick already moved past.
        release_window(&conn, "nightly", 1_500.0);
        assert!(!claim_window(&conn, "nightly", 1_700.0));
    }
}
