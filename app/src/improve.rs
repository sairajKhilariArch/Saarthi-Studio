//! Self-improvement: learn from missions that actually ran, measure what the
//! agents did, and improve the studio through the same pipelines the user has.
//!
//! Three rules decide every line here:
//!
//! 1. A learning source is a finished mission with a report. A model's opinion
//!    of itself is not evidence; neither is a run that failed halfway.
//! 2. A number is a `COUNT`, a `SUM` or an average over rows this app wrote.
//!    When a row is missing the value is `null` and the report says so.
//! 3. Self-modification is a Phase 14 proposal → pipeline → branch → diff →
//!    approval → commit → optional revert. This module never writes code itself
//!    and never touches permissions, credentials, approval policy or releases.
//!
//! Nothing here changes agent permissions, model pins, provider settings or
//! budgets without a person: it recommends, and a proposal needs approval.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path as FsPath, PathBuf};

use crate::AppState;

const WINDOW_DAYS: i64 = 30;
const MAX_EXPERIENCE_DETAIL: usize = 1_200;
const MAX_SOURCES: usize = 2_000;
const MAX_SOURCE_BYTES: u64 = 512 * 1024;
const MAX_DOC_BYTES: u64 = 256 * 1024;
const MAX_PROPOSALS: usize = 40;
const LEARN_WINDOW: i64 = 200;

const SKIP_DIRS: [&str; 18] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
    ".gradle",
    "coverage",
    ".turbo",
    ".cache",
    ".idea",
    "Pods",
    "vendor",
    "out",
    "bin",
];

fn now() -> f64 {
    crate::now_seconds()
}

fn db(state: &AppState) -> Result<Connection, String> {
    crate::open_db(&state.database_path).map_err(|error| format!("Database error: {error}"))
}

pub(crate) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS improve_settings(
            id INTEGER PRIMARY KEY CHECK(id = 1),
            enabled INTEGER NOT NULL DEFAULT 0,
            autonomy INTEGER NOT NULL DEFAULT 2,
            retention_days INTEGER NOT NULL DEFAULT 90,
            budget_tokens INTEGER NOT NULL DEFAULT 0,
            store_goals INTEGER NOT NULL DEFAULT 0,
            priority TEXT NOT NULL DEFAULT 'balanced',
            updated_at REAL NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS improve_experiences(
            id TEXT PRIMARY KEY,
            project TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL,
            key TEXT NOT NULL,
            title TEXT NOT NULL,
            detail TEXT NOT NULL DEFAULT '',
            evidence TEXT NOT NULL DEFAULT '',
            confidence REAL NOT NULL DEFAULT 0,
            uses INTEGER NOT NULL DEFAULT 0,
            validated INTEGER NOT NULL DEFAULT 0,
            source TEXT NOT NULL DEFAULT '',
            source_id TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL,
            updated_at REAL NOT NULL,
            expires_at REAL NOT NULL DEFAULT 0,
            UNIQUE(project, kind, key)
        );
        CREATE TABLE IF NOT EXISTS improve_proposals(
            id TEXT PRIMARY KEY,
            project TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            problem TEXT NOT NULL DEFAULT '',
            evidence TEXT NOT NULL DEFAULT '[]',
            benefit TEXT NOT NULL DEFAULT '',
            risk TEXT NOT NULL DEFAULT '',
            files TEXT NOT NULL DEFAULT '[]',
            testing TEXT NOT NULL DEFAULT '',
            rollback TEXT NOT NULL DEFAULT '',
            priority INTEGER NOT NULL DEFAULT 1,
            status TEXT NOT NULL DEFAULT 'proposed',
            pipeline TEXT NOT NULL DEFAULT '',
            rollback_commit TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL,
            updated_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS improve_experiments(
            id TEXT PRIMARY KEY,
            project TEXT NOT NULL DEFAULT '',
            name TEXT NOT NULL,
            goal TEXT NOT NULL,
            baseline TEXT NOT NULL DEFAULT 'baseline',
            variant TEXT NOT NULL DEFAULT 'lean',
            baseline_mission TEXT NOT NULL DEFAULT '',
            variant_mission TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'running',
            baseline_metrics TEXT NOT NULL DEFAULT '',
            variant_metrics TEXT NOT NULL DEFAULT '',
            verdict TEXT NOT NULL DEFAULT '{}',
            created_at REAL NOT NULL,
            updated_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS improve_events(
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            subject TEXT NOT NULL DEFAULT '',
            detail TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL
        );
        CREATE INDEX IF NOT EXISTS improve_experiences_by_project
            ON improve_experiences(project, kind, confidence);",
    )?;
    // Databases created by an earlier build have no priority column yet.
    let _ = conn.execute(
        "ALTER TABLE improve_settings ADD COLUMN priority TEXT NOT NULL DEFAULT 'balanced'",
        [],
    );
    Ok(())
}

// ---------------------------------------------------------------- settings

#[derive(Clone)]
pub(crate) struct Settings {
    pub(crate) enabled: bool,
    pub(crate) autonomy: i64,
    pub(crate) retention_days: i64,
    pub(crate) budget_tokens: i64,
    pub(crate) store_goals: bool,
    pub(crate) priority: String,
}

fn settings(state: &AppState) -> Settings {
    db(state)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT enabled, autonomy, retention_days, budget_tokens, store_goals, priority \
                 FROM improve_settings WHERE id=1",
                [],
                |row| {
                    Ok(Settings {
                        enabled: row.get::<_, i64>(0)? != 0,
                        autonomy: row.get::<_, i64>(1)?,
                        retention_days: row.get::<_, i64>(2)?,
                        budget_tokens: row.get::<_, i64>(3)?,
                        store_goals: row.get::<_, i64>(4)? != 0,
                        priority: row.get::<_, String>(5)?,
                    })
                },
            )
            .optional()
            .ok()
            .flatten()
        })
        .unwrap_or(Settings {
            enabled: false,
            autonomy: 2,
            retention_days: 90,
            budget_tokens: 0,
            store_goals: false,
            priority: "balanced".to_string(),
        })
}

fn save_settings(state: &AppState, input: &Value) -> Result<Settings, String> {
    let current = settings(state);
    let enabled = input
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(current.enabled);
    let autonomy = input
        .get("autonomy")
        .and_then(Value::as_i64)
        .unwrap_or(current.autonomy)
        .clamp(1, 3);
    let retention_days = input
        .get("retentionDays")
        .and_then(Value::as_i64)
        .unwrap_or(current.retention_days)
        .clamp(1, 3650);
    let budget_tokens = input
        .get("budgetTokens")
        .and_then(Value::as_i64)
        .unwrap_or(current.budget_tokens)
        .clamp(0, 100_000_000);
    let store_goals = input
        .get("storeGoals")
        .and_then(Value::as_bool)
        .unwrap_or(current.store_goals);
    let priority = input
        .get("priority")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| {
            matches!(
                value.as_str(),
                "balanced" | "reliability" | "speed" | "cost"
            )
        })
        .unwrap_or_else(|| current.priority.clone());
    let conn = db(state)?;
    conn.execute(
        "INSERT INTO improve_settings(id, enabled, autonomy, retention_days, budget_tokens, store_goals, priority, updated_at) \
         VALUES(1,?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET \
         enabled=excluded.enabled, autonomy=excluded.autonomy, retention_days=excluded.retention_days, \
         budget_tokens=excluded.budget_tokens, store_goals=excluded.store_goals, \
         priority=excluded.priority, updated_at=excluded.updated_at",
        params![
            i64::from(enabled),
            autonomy,
            retention_days,
            budget_tokens,
            i64::from(store_goals),
            priority,
            now()
        ],
    )
    .map_err(|error| error.to_string())?;
    event(
        state,
        "settings",
        "improve",
        &format!(
            "enabled={enabled} autonomy={autonomy} retentionDays={retention_days} budgetTokens={budget_tokens} storeGoals={store_goals} priority={priority}"
        ),
    );
    Ok(settings(state))
}

fn settings_json(settings: Settings) -> Value {
    json!({
        "enabled": settings.enabled,
        "autonomy": settings.autonomy,
        "retentionDays": settings.retention_days,
        "budgetTokens": settings.budget_tokens,
        "storeGoals": settings.store_goals,
        "priority": settings.priority,
        "note": "These settings control learning and experiments only. They cannot change permissions, credential storage, approval requirements or release credentials.",
    })
}

fn event(state: &AppState, kind: &str, subject: &str, detail: &str) {
    if let Ok(conn) = db(state) {
        let _ = conn.execute(
            "INSERT INTO improve_events(kind, subject, detail, created_at) VALUES(?1,?2,?3,?4)",
            params![kind, subject, detail, now()],
        );
    }
}

/// Tokens reported by runs created in the rolling 24 hours, for the optional
/// experiment budget. Zero means no limit.
fn tokens_today(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COALESCE(SUM(tokens), 0) FROM agent_runs WHERE created_at >= ?1",
        params![now() - 24.0 * 3600.0],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
}

fn budget_refusal(state: &AppState, s: Settings) -> Option<String> {
    if s.budget_tokens <= 0 {
        return None;
    }
    let used = db(state).map(|conn| tokens_today(&conn)).unwrap_or(0);
    (used >= s.budget_tokens).then(|| {
        format!(
            "The self-improvement budget is spent: {used} of {} tokens in the last 24 hours. Raise it in the Self-improvement settings.",
            s.budget_tokens
        )
    })
}

// ---------------------------------------------------------------- experiences

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Observation {
    pub(crate) kind: &'static str,
    pub(crate) key: String,
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) evidence: String,
    pub(crate) validated: bool,
    pub(crate) confidence: f64,
}

fn normalize_error(text: &str) -> String {
    // Two failures are "the same" when they differ only in paths, line numbers
    // and digits. Anything else stays distinct.
    let raw = text
        .chars()
        .take(400)
        .collect::<String>()
        .to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    for token in raw.split_whitespace() {
        let token = token
            .trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '_' | ':' | '-')));
        if token.is_empty() {
            continue;
        }
        let looks_like_file = token.contains('/')
            || [".rs", ".ts", ".tsx", ".js", ".jsx", ".py", ".go", ".md"]
                .iter()
                .any(|suffix| token.contains(suffix));
        let cleaned: String = if looks_like_file {
            "<file>".to_string()
        } else {
            token
                .chars()
                .map(|c| if c.is_ascii_digit() { '#' } else { c })
                .collect()
        };
        if out.last() == Some(&cleaned) {
            continue;
        }
        out.push(cleaned);
    }
    out.join(" ").chars().take(160).collect()
}

fn keywords(text: &str) -> Vec<String> {
    let mut words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= 4)
        .map(|word| word.to_ascii_lowercase())
        .filter(|word| {
            !matches!(
                word.as_str(),
                "with"
                    | "this"
                    | "that"
                    | "from"
                    | "into"
                    | "then"
                    | "than"
                    | "should"
                    | "would"
                    | "could"
                    | "have"
                    | "will"
                    | "make"
                    | "file"
                    | "files"
                    | "code"
                    | "change"
                    | "changes"
                    | "using"
                    | "your"
                    | "about"
                    | "when"
                    | "where"
                    | "which"
                    | "there"
                    | "their"
                    | "them"
                    | "also"
                    | "only"
                    | "some"
                    | "such"
            )
        })
        .collect();
    words.sort();
    words.dedup();
    words.truncate(6);
    words
}

fn goal_digest(goal: &str) -> String {
    // A stable, non-reversible key for privacy mode: the text itself is never
    // stored, but similar goals still collapse into one experience.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in goal.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

fn confidence_for(kind: &str, uses: i64) -> f64 {
    let uses = uses.max(1) as f64;
    match kind {
        "strategy" => (0.25 + 0.15 * uses).min(0.9),
        "failure" => (0.4 + 0.05 * uses).min(0.8),
        "preference" => 0.9,
        _ => (0.3 + 0.1 * uses).min(0.85),
    }
}

/// Truthful set of observations from a finished mission. The summary a task
/// reported is quoted, never embellished; a strategy is only `validated` when
/// the mission succeeded *and* left a verifiable artifact (a write, a command or
/// a verifier check).
pub(crate) fn observations_from(
    mission: &Value,
    store_goals: bool,
    created_at: f64,
) -> Vec<Observation> {
    let mut out: Vec<Observation> = Vec::new();
    let status = mission["status"].as_str().unwrap_or("");
    if !matches!(status, "succeeded" | "failed") {
        return out;
    }
    let report = mission.get("report").cloned().unwrap_or(Value::Null);
    let files = report["filesWritten"].as_array().map(Vec::len).unwrap_or(0);
    let commands = report["commandsRun"].as_array().map(Vec::len).unwrap_or(0);
    let checks = report["checks"].as_array().map(Vec::len).unwrap_or(0);
    let tokens = report["tokens"].as_i64().unwrap_or(0);
    let elapsed = report["elapsedSeconds"].as_f64().unwrap_or(0.0);
    let goal = mission["goal"].as_str().unwrap_or("");
    let words = keywords(goal);
    let goal_key = if store_goals {
        words.join("-")
    } else {
        goal_digest(goal)
    };
    let goal_label = if store_goals && !words.is_empty() {
        words.iter().take(4).cloned().collect::<Vec<_>>().join(" ")
    } else {
        "this project".to_string()
    };
    let tasks: Vec<Value> = mission["tasks"].as_array().cloned().unwrap_or_default();
    let roles: Vec<String> = tasks
        .iter()
        .filter_map(|task| task["role"].as_str().map(str::to_string))
        .collect();
    let mut successful: Vec<String> = Vec::new();
    for task in &tasks {
        if task["status"] != json!("done") {
            continue;
        }
        let summary = task["summary"].as_str().unwrap_or("").trim();
        if summary.is_empty() {
            continue;
        }
        successful.push(format!(
            "{}: {}",
            task["roleName"].as_str().unwrap_or("agent"),
            summary.chars().take(220).collect::<String>()
        ));
    }
    let has_artifact = files > 0 || commands > 0 || checks > 0;
    if status == "succeeded" && has_artifact && !successful.is_empty() {
        out.push(Observation {
            kind: "strategy",
            key: format!("strategy:{}:{goal_key}", roles.join(">")),
            title: format!("Strategy for {goal_label}"),
            detail: successful.join("\n").chars().take(MAX_EXPERIENCE_DETAIL).collect(),
            evidence: format!(
                "Mission {} succeeded: {} task(s) done, {} file(s) written, {} command(s), {} check(s), {tokens} tokens, {:.1}s.",
                mission["id"].as_str().unwrap_or(""),
                tasks.len(),
                files,
                commands,
                checks,
                elapsed
            ),
            validated: true,
            confidence: confidence_for("strategy", 1),
        });
    }
    if status == "failed" {
        for task in &tasks {
            if !matches!(task["status"].as_str(), Some("failed") | Some("blocked")) {
                continue;
            }
            let error = task["error"].as_str().unwrap_or("").trim();
            if error.is_empty() {
                continue;
            }
            let normalized = normalize_error(error);
            if normalized.is_empty() {
                continue;
            }
            out.push(Observation {
                kind: "failure",
                key: format!(
                    "failure:{}:{}",
                    task["role"].as_str().unwrap_or("agent"),
                    normalized.chars().take(120).collect::<String>()
                ),
                title: format!(
                    "{} failed for {goal_label}",
                    task["roleName"].as_str().unwrap_or("agent")
                ),
                detail: format!(
                    "The task {} reported: {}",
                    task["id"].as_str().unwrap_or(""),
                    error.chars().take(400).collect::<String>()
                ),
                evidence: format!(
                    "Mission {} failed after {} attempt(s) on this task; the mission report records it.",
                    mission["id"].as_str().unwrap_or(""),
                    task["attempts"].as_i64().unwrap_or(0)
                ),
                validated: true,
                confidence: confidence_for("failure", 1),
            });
        }
    }
    let _ = created_at;
    out
}

fn record_experience(
    state: &AppState,
    project: &str,
    observation: &Observation,
    retention_days: i64,
) -> Result<(), String> {
    let conn = db(state)?;
    let existing: Option<(String, i64, f64, f64)> = conn
        .query_row(
            "SELECT id, uses, confidence, created_at FROM improve_experiences \
             WHERE project=?1 AND kind=?2 AND key=?3",
            params![project, observation.kind, observation.key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let expires_at = now() + retention_days as f64 * 86_400.0;
    if let Some((id, uses, confidence, created_at)) = existing {
        let uses = uses + 1;
        let confidence = confidence_for(observation.kind, uses).max(confidence.min(0.9));
        conn.execute(
            "UPDATE improve_experiences SET uses=?2, confidence=?3, updated_at=?4, expires_at=?5, \
             validated=MAX(validated, ?6) WHERE id=?1",
            params![
                id,
                uses,
                confidence,
                now(),
                expires_at,
                i64::from(observation.validated),
            ],
        )
        .map_err(|error| error.to_string())?;
        let _ = created_at;
        return Ok(());
    }
    conn.execute(
        "INSERT INTO improve_experiences(id, project, kind, key, title, detail, evidence, confidence, \
         uses, validated, source, source_id, created_at, updated_at, expires_at) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1,?9,'mission','',?10,?10,?11)",
        params![
            uuid::Uuid::new_v4().to_string(),
            project,
            observation.kind,
            observation.key,
            observation.title,
            observation.detail,
            observation.evidence,
            confidence_for(observation.kind, 1),
            i64::from(observation.validated),
            now(),
            expires_at
        ],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn project_key(state: &AppState) -> String {
    state
        .workspace
        .root()
        .map(|root| root.to_string_lossy().to_string())
        .unwrap_or_else(|_| "no-workspace".to_string())
}

/// Called by the mission supervisor when a mission settles. It is idempotent by
/// `(project, kind, key)`, so a restart that replays a mission increments the
/// use count instead of duplicating the lesson.
pub(crate) fn observe_mission(state: &AppState, mission_id: &str) -> Result<usize, String> {
    let s = settings(state);
    if !s.enabled {
        return Ok(0);
    }
    let Some(mission) = crate::multi_agent::mission_json_for(state, mission_id) else {
        return Ok(0);
    };
    let created_at = mission["createdAt"].as_f64().unwrap_or_else(now);
    let observations = observations_from(&mission, s.store_goals, created_at);
    // Experiences belong to the workspace, not to the transient mission
    // project id, so retrieval can scope to the folder the user has open.
    let project = project_key(state);
    let mut written = 0;
    for observation in &observations {
        if record_experience(state, &project, observation, s.retention_days).is_ok() {
            written += 1;
        }
    }
    if written > 0 {
        event(
            state,
            "learned",
            mission_id,
            &format!("{written} observation(s) from mission {mission_id}"),
        );
    }
    Ok(written)
}

fn experience_json(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "kind": row.get::<_, String>(1)?,
        "title": row.get::<_, String>(2)?,
        "detail": row.get::<_, String>(3)?,
        "evidence": row.get::<_, String>(4)?,
        "confidence": row.get::<_, f64>(5)?,
        "uses": row.get::<_, i64>(6)?,
        "validated": row.get::<_, i64>(7)? != 0,
        "createdAt": row.get::<_, f64>(8)?,
        "updatedAt": row.get::<_, f64>(9)?,
        "expiresAt": row.get::<_, f64>(10)?,
    }))
}

/// Lexical relevance without a model call: overlap of query keywords with the
/// record, then confidence, then a 30-day recency half-life.
pub(crate) fn relevance(
    query: &[String],
    title: &str,
    detail: &str,
    confidence: f64,
    age_days: f64,
) -> f64 {
    let haystack: BTreeSet<String> = keywords(&format!("{title} {detail}")).into_iter().collect();
    let lexical = if query.is_empty() {
        0.0
    } else {
        let hits = query.iter().filter(|word| haystack.contains(*word)).count();
        hits as f64 / query.len() as f64
    };
    let recency = 0.5_f64.powf((age_days.max(0.0)) / 30.0);
    let base = 0.6 * lexical + 0.3 * confidence.clamp(0.0, 1.0) + 0.1 * recency;
    ((base * 1000.0).round()) / 1000.0
}

fn retrieve(
    state: &AppState,
    query: &str,
    kind: &str,
    include_unverified: bool,
    limit: usize,
) -> Result<Vec<Value>, String> {
    let conn = db(state)?;
    // Records belong to the open workspace. A lesson learned in another folder
    // is not this project's knowledge.
    let sql = "SELECT id, kind, title, detail, evidence, confidence, uses, validated, created_at, updated_at, expires_at \
               FROM improve_experiences WHERE expires_at > ?1 AND project = ?2 \
               AND (?3 = '' OR kind = ?3) AND (?4 = 1 OR validated = 1) \
               ORDER BY confidence DESC, updated_at DESC LIMIT 400";
    let mut stmt = conn.prepare(sql).map_err(|error| error.to_string())?;
    let terms = keywords(query);
    let rows = stmt
        .query_map(
            params![
                now(),
                project_key(state),
                kind,
                i64::from(include_unverified)
            ],
            experience_json,
        )
        .map_err(|error| error.to_string())?;
    let mut scored: Vec<Value> = rows
        .flatten()
        .map(|mut row| {
            let age_days = (now() - row["createdAt"].as_f64().unwrap_or(now())) / 86_400.0;
            let score = relevance(
                &terms,
                row["title"].as_str().unwrap_or(""),
                row["detail"].as_str().unwrap_or(""),
                row["confidence"].as_f64().unwrap_or(0.0),
                age_days,
            );
            row["relevance"] = json!(score);
            row
        })
        .collect();
    scored.sort_by(|a, b| {
        b["relevance"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&a["relevance"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit.clamp(1, 50));
    Ok(scored)
}

fn add_preference(state: &AppState, input: &Value) -> Result<Value, (StatusCode, String)> {
    let text = input
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if text.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            "A preference needs a sentence a future run can follow.".to_string(),
        ));
    }
    let s = settings(state);
    let observation = Observation {
        kind: "preference",
        key: format!("preference:{}", normalize_error(&text)),
        title: text.chars().take(80).collect(),
        detail: text.chars().take(MAX_EXPERIENCE_DETAIL).collect(),
        evidence: "Recorded by the user; user-authored preferences are the only records trusted without a mission.".to_string(),
        validated: true,
        confidence: 0.9,
    };
    record_experience(state, &project_key(state), &observation, s.retention_days)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    event(state, "preference", "", &observation.key);
    Ok(json!({ "ok": true }))
}

fn forget(state: &AppState, id: &str) -> Result<bool, String> {
    let conn = db(state)?;
    let removed = conn
        .execute("DELETE FROM improve_experiences WHERE id=?1", [id])
        .map_err(|error| error.to_string())?;
    if removed > 0 {
        event(state, "forget", id, "experience removed by the user");
    }
    Ok(removed > 0)
}

fn purge_expired(state: &AppState) -> Result<usize, String> {
    let conn = db(state)?;
    let expired = conn
        .execute(
            "DELETE FROM improve_experiences WHERE expires_at > 0 AND expires_at <= ?1",
            params![now()],
        )
        .map_err(|error| error.to_string())?;
    if expired > 0 {
        event(
            state,
            "purge",
            "",
            &format!("{expired} expired experience(s) removed"),
        );
    }
    Ok(expired)
}

// ---------------------------------------------------------------- metrics

fn rate(numerator: i64, denominator: i64) -> Value {
    if denominator <= 0 {
        Value::Null
    } else {
        json!(((numerator as f64 / denominator as f64) * 1000.0).round() / 10.0)
    }
}

fn agent_metrics(state: &AppState) -> Value {
    let since = now() - WINDOW_DAYS as f64 * 86_400.0;
    let Ok(conn) = db(state) else {
        return json!([]);
    };
    let sql = "SELECT mt.role, COUNT(*) AS total, \
        SUM(CASE WHEN mt.status='done' THEN 1 ELSE 0 END) AS done, \
        SUM(CASE WHEN mt.status IN ('failed','blocked') THEN 1 ELSE 0 END) AS failed, \
        SUM(CASE WHEN mt.attempts>0 THEN 1 ELSE 0 END) AS retried, \
        SUM(CASE WHEN mt.attempts>0 AND mt.status='done' THEN 1 ELSE 0 END) AS recovered, \
        COALESCE(SUM(r.tokens), 0) AS tokens, \
        AVG(CASE WHEN r.started_at>0 THEN r.updated_at - r.started_at END) AS avg_age \
        FROM mission_tasks mt JOIN missions m ON m.id = mt.mission_id \
        LEFT JOIN agent_runs r ON r.id = mt.run_id \
        WHERE m.created_at >= ?1 GROUP BY mt.role ORDER BY total DESC";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return json!([]);
    };
    let Ok(rows) = stmt.query_map(params![since], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, Option<f64>>(7)?,
        ))
    }) else {
        return json!([]);
    };
    rows.flatten()
        .map(
            |(role, total, done, failed, retried, recovered, tokens, avg_age)| {
                let review_rejected = conn
                .query_row(
                    "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id = mt.mission_id \
                     WHERE m.created_at >= ?1 AND mt.role='review' AND mt.status='failed'",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
                json!({
                    "role": role,
                    "total": total,
                    "done": done,
                    "failed": failed,
                    "successRate": rate(done, total),
                    "retried": retried,
                    "retryRate": rate(retried, total),
                    "recoveryRate": rate(recovered, retried),
                    "reviewRejections": if role == "review" { review_rejected } else { 0 },
                    "avgDurationSeconds": avg_age.map(|value| (value * 10.0).round() / 10.0),
                    "tokens": tokens,
                })
            },
        )
        .collect()
}

fn model_metrics(state: &AppState) -> (Value, Vec<String>) {
    let since = now() - WINDOW_DAYS as f64 * 86_400.0;
    let Ok(conn) = db(state) else {
        return (
            json!([]),
            vec!["model history (database unavailable)".to_string()],
        );
    };
    let sql = "SELECT CASE WHEN model='' THEN '(routed default)' ELSE model END AS label, \
        COUNT(*), SUM(CASE WHEN status='succeeded' THEN 1 ELSE 0 END), \
        SUM(CASE WHEN status IN ('failed','interrupted') THEN 1 ELSE 0 END), \
        AVG(CASE WHEN tokens>0 THEN tokens END), \
        AVG(CASE WHEN started_at>0 THEN updated_at - started_at END) \
        FROM agent_runs WHERE created_at >= ?1 GROUP BY label ORDER BY COUNT(*) DESC";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return (json!([]), vec![]);
    };
    let Ok(rows) = stmt.query_map(params![since], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<f64>>(4)?,
            row.get::<_, Option<f64>>(5)?,
        ))
    }) else {
        return (json!([]), vec![]);
    };
    let mut unpriced = 0;
    let models: Vec<Value> = rows
        .flatten()
        .map(|(label, total, succeeded, failed, avg_tokens, avg_age)| {
            // A cost exists only from the provider's published price and this
            // app's own token count; the output share is unknown, so the basis
            // says so.
            let cost = crate::openai::published_price(&label).and_then(|price| {
                avg_tokens.map(|tokens| {
                    ((tokens / 1_000_000.0 * price.input_per_million) * 100_000.0).round()
                        / 100_000.0
                })
            });
            if cost.is_none() {
                unpriced += 1;
            }
            json!({
                "model": label,
                "runs": total,
                "succeeded": succeeded,
                "failed": failed,
                "successRate": rate(succeeded, total),
                "avgTokens": avg_tokens.map(|value| value.round() as i64),
                "avgDurationSeconds": avg_age.map(|value| (value * 10.0).round() / 10.0),
                "estimatedCostPerRunUsd": cost,
                "costBasis": if cost.is_some() { json!("published price × average total tokens; output split unknown") } else { Value::Null },
                "failovers": conn.query_row(
                    "SELECT COUNT(*) FROM agent_steps s JOIN agent_runs r ON r.id=s.run_id \
                     WHERE r.created_at >= ?1 AND s.action IN ('failover','escalate')",
                    params![since],
                    |row| row.get::<_, i64>(0),
                ).unwrap_or(0),
            })
        })
        .collect();
    let mut unavailable = Vec::new();
    if unpriced > 0 {
        unavailable.push(format!(
            "cost for {unpriced} model line(s) with no published price"
        ));
    }
    (json!(models), unavailable)
}

/// Rank measured model lines for a chosen optimisation priority. This only
/// orders evidence: it never pins a model, changes a provider or spends money.
pub(crate) fn rank_models(models: &[Value], priority: &str) -> Vec<Value> {
    let mut ranked: Vec<(f64, Value)> = models
        .iter()
        .filter(|model| model["runs"].as_i64().unwrap_or(0) >= 2)
        .map(|model| {
            let success = model["successRate"].as_f64().unwrap_or(0.0);
            let seconds = model["avgDurationSeconds"].as_f64().unwrap_or(0.0);
            let cost = model["estimatedCostPerRunUsd"].as_f64();
            let speed = 1.0 / (1.0 + seconds / 30.0);
            let cost_score = cost.map(|value| 1.0 / (1.0 + value * 100.0)).unwrap_or(0.5);
            let (score, reason) = match priority {
                "reliability" => (success, "ordered by measured success rate"),
                "speed" => (speed * 100.0, "ordered by measured average duration"),
                "cost" => (
                    cost_score * 100.0,
                    "ordered by published price × reported tokens; unpriced lines score neutrally",
                ),
                _ => (
                    success * 0.6 + speed * 25.0 + cost_score * 15.0,
                    "balanced: success rate first, then speed and estimated cost",
                ),
            };
            (
                (score * 100.0).round() / 100.0,
                json!({
                    "model": model["model"],
                    "runs": model["runs"],
                    "successRate": model["successRate"],
                    "avgDurationSeconds": model["avgDurationSeconds"],
                    "estimatedCostPerRunUsd": model["estimatedCostPerRunUsd"],
                    "score": (score * 100.0).round() / 100.0,
                    "reason": reason,
                }),
            )
        })
        .collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    ranked.into_iter().take(5).map(|(_, value)| value).collect()
}

fn workflow_metrics(state: &AppState) -> (Value, Vec<String>) {
    let since = now() - WINDOW_DAYS as f64 * 86_400.0;
    let Ok(conn) = db(state) else {
        return (
            json!({}),
            vec!["workflow history (database unavailable)".to_string()],
        );
    };
    let missions = conn
        .query_row(
            "SELECT COUNT(*) FROM missions WHERE created_at >= ?1",
            params![since],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let tasks = conn
        .query_row(
            "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id=mt.mission_id WHERE m.created_at >= ?1",
            params![since],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let retried = conn
        .query_row(
            "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id=mt.mission_id \
             WHERE m.created_at >= ?1 AND mt.attempts>0",
            params![since],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let succeeded = conn
        .query_row(
            "SELECT COUNT(*) FROM missions WHERE created_at >= ?1 AND status='succeeded'",
            params![since],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let verified = conn
        .query_row(
            "SELECT COUNT(DISTINCT m.id) FROM missions m JOIN mission_tasks mt ON mt.mission_id=m.id \
             JOIN agent_steps s ON s.run_id=mt.run_id \
             WHERE m.created_at >= ?1 AND (s.action='verify' OR s.action='command')",
            params![since],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let mut duplicate_commands: Vec<Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT s.summary, COUNT(*) c FROM agent_steps s JOIN mission_tasks mt ON mt.run_id=s.run_id \
         JOIN missions m ON m.id=mt.mission_id WHERE m.created_at >= ?1 AND s.action='command' \
         GROUP BY s.summary HAVING c > 1 ORDER BY c DESC LIMIT 5",
    ) {
        if let Ok(rows) = stmt.query_map(params![since], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }) {
            duplicate_commands = rows
                .flatten()
                .map(|(command, count)| json!({ "command": command, "count": count }))
                .collect();
        }
    }
    let repeated_reads = conn
        .query_row(
            "SELECT COALESCE(SUM(c - 1), 0) FROM (SELECT s.summary, COUNT(*) c FROM agent_steps s \
             JOIN mission_tasks mt ON mt.run_id=s.run_id JOIN missions m ON m.id=mt.mission_id \
             WHERE m.created_at >= ?1 AND s.action='read' GROUP BY s.summary HAVING c > 1)",
            params![since],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let mut recommendations: Vec<Value> = Vec::new();
    if retried > 0 {
        recommendations.push(json!({
            "finding": format!("{retried} task(s) needed a retry"),
            "evidence": "mission_tasks.attempts > 0 in the window",
            "suggestion": "A retry is a fresh run with the failure in its goal. If the same role retries repeatedly, inspect that role's instructions or its model before adding another stage.",
        }));
    }
    if let Some(first) = duplicate_commands.first() {
        recommendations.push(json!({
            "finding": format!("The same command ran {} times: {}", first["count"], first["command"]),
            "evidence": "agent_steps rows with action=command and the same summary",
            "suggestion": "Cache the command result by command + file fingerprint for the rest of the mission; keep the check itself.",
        }));
    }
    if repeated_reads > 0 {
        recommendations.push(json!({
            "finding": format!("{repeated_reads} repeated file read(s)"),
            "evidence": "agent_steps rows with action=read and the same summary",
            "suggestion": "Include a short summary of already-read files in the dependent task's goal instead of re-reading them.",
        }));
    }
    if succeeded > verified {
        recommendations.push(json!({
            "finding": format!("{} succeeded mission(s) recorded no command or verify step", succeeded - verified),
            "evidence": "missions joined to agent_steps",
            "suggestion": "A success without a check is an unverified success. Require a check or review stage before delivery; do not drop verification to save a step.",
        }));
    }
    let unavailable = if missions == 0 {
        vec!["workflow comparison (no missions in the window)".to_string()]
    } else {
        Vec::new()
    };
    (
        json!({
            "windowDays": WINDOW_DAYS,
            "missions": missions,
            "tasks": tasks,
            "avgTasksPerMission": if missions > 0 { json!(((tasks as f64 / missions as f64) * 10.0).round() / 10.0) } else { Value::Null },
            "retriedTasks": retried,
            "verifiedMissions": verified,
            "duplicateCommands": duplicate_commands,
            "repeatedReads": repeated_reads,
            "recommendations": recommendations,
        }),
        unavailable,
    )
}

fn recurring_failures(state: &AppState) -> Value {
    let since = now() - WINDOW_DAYS as f64 * 86_400.0;
    let Ok(conn) = db(state) else {
        return json!([]);
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT mt.error, COUNT(*) c, MAX(mt.updated_at) last FROM mission_tasks mt \
         JOIN missions m ON m.id=mt.mission_id WHERE m.created_at >= ?1 AND mt.error <> '' \
         GROUP BY mt.error ORDER BY c DESC LIMIT 8",
    ) else {
        return json!([]);
    };
    let Ok(rows) = stmt.query_map(params![since], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, f64>(2)?,
        ))
    }) else {
        return json!([]);
    };
    rows.flatten()
        .map(|(error, count, last)| {
            json!({
                "error": error.chars().take(240).collect::<String>(),
                "count": count,
                "lastSeen": last,
            })
        })
        .collect()
}

// ---------------------------------------------------------------- quality scan

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Finding {
    pub(crate) kind: &'static str,
    pub(crate) title: String,
    pub(crate) problem: String,
    pub(crate) evidence: Vec<String>,
    pub(crate) benefit: String,
    pub(crate) risk: String,
    pub(crate) files: Vec<String>,
    pub(crate) testing: String,
    pub(crate) rollback: String,
    pub(crate) priority: i64,
}

fn walk_files(root: &FsPath, predicate: &dyn Fn(&FsPath) -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 10 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if found.len() >= MAX_SOURCES {
                return found;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if kind.is_dir() {
                if SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.') && name != ".github"
                {
                    continue;
                }
                stack.push((entry.path(), depth + 1));
                continue;
            }
            if predicate(&entry.path()) {
                found.push(entry.path());
            }
        }
    }
    found
}

fn function_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines().take(20_000) {
        let line = line.trim();
        let candidate = if let Some(at) = line.find("fn ") {
            if line[..at].trim().is_empty()
                || line.starts_with("pub ")
                || line.starts_with("async ")
                || line.starts_with("pub(crate) ")
                || line.starts_with("pub async ")
            {
                line[at + 3..].split(['(', '<', ' ']).next()
            } else {
                None
            }
        } else if let Some(at) = line.find("function ") {
            line[at + 9..].split(['(', '<', ' ']).next()
        } else {
            None
        };
        let Some(name) = candidate else { continue };
        let name = name.trim();
        if name.len() >= 5
            && !matches!(
                name,
                "main" | "render" | "format" | "default" | "new" | "from" | "into"
            )
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            names.push(name.to_string());
        }
    }
    names
}

fn docs_referencing_missing_paths(root: &FsPath) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let docs = walk_files(root, &|path| {
        matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("md") | Some("mdx")
        ) && !path.to_string_lossy().contains("/node_modules/")
    });
    for path in docs.iter().take(200) {
        if std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) > MAX_DOC_BYTES {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let relative = path
            .strip_prefix(root)
            .map(|rest| rest.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if relative.contains("/docs/release-history/")
            || relative.starts_with("docs/release-history")
        {
            continue;
        }
        let mut missing: Vec<String> = Vec::new();
        for token in text.split('`').skip(1).step_by(2) {
            let token = token.trim();
            if token.len() < 4
                || token.len() > 160
                || token.contains(char::is_whitespace)
                || token.starts_with("http")
                || !token.contains('/')
                || !token.contains('.')
                || token.starts_with("~")
                || token.starts_with('/')
            {
                continue;
            }
            if !root.join(token).exists() && !missing.iter().any(|known| known == token) {
                missing.push(token.to_string());
            }
        }
        if !missing.is_empty() {
            out.push((relative, missing));
        }
    }
    out
}

/// Deterministic proposals from the workspace's own files. There is no model
/// call here: every finding quotes a count and a path.
pub(crate) fn quality_findings(root: &FsPath, profile: &Value) -> Vec<Finding> {
    let mut findings: Vec<Finding> = Vec::new();
    let source_paths = walk_files(root, &|path| {
        matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("rs") | Some("ts") | Some("tsx") | Some("js") | Some("jsx") | Some("py")
        )
    });
    let mut definitions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut long_files: Vec<(String, usize)> = Vec::new();
    for path in source_paths.iter().take(MAX_SOURCES) {
        if std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) > MAX_SOURCE_BYTES {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let relative = path
            .strip_prefix(root)
            .map(|rest| rest.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let lines = text.lines().count();
        if lines > 1_200 {
            long_files.push((relative.clone(), lines));
        }
        for name in function_names(&text) {
            definitions
                .entry(name)
                .or_default()
                .insert(relative.clone());
        }
    }
    long_files.sort_by_key(|(_, lines)| std::cmp::Reverse(*lines));
    let mut duplicates: Vec<(String, Vec<String>)> = definitions
        .into_iter()
        .filter(|(_, files)| files.len() >= 2)
        .map(|(name, files)| (name, files.into_iter().collect()))
        .collect();
    duplicates.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    for (name, files) in duplicates.into_iter().take(8) {
        findings.push(Finding {
            kind: "duplication",
            title: format!("`{name}` is defined in {} files", files.len()),
            problem: format!(
                "The function `{name}` appears in {} files. Two implementations of one behaviour drift: a fix lands in one and the other keeps the bug.",
                files.len()
            ),
            evidence: files
                .iter()
                .map(|file| format!("definition of `{name}` in {file}"))
                .collect(),
            benefit: "One definition removes the drift risk and the duplicate review surface.".to_string(),
            risk: "The two implementations may differ subtly; merging them changes call sites, so the diff needs review and the callers' tests.".to_string(),
            files: files.clone(),
            testing: "Run the project's test command and any test that names these files before and after the change.".to_string(),
            rollback: "The work lands on an isolated branch; revert the delivery commit to restore both implementations.".to_string(),
            priority: 2,
        });
    }
    for (file, lines) in long_files.into_iter().take(4) {
        findings.push(Finding {
            kind: "maintainability",
            title: format!("{file} is {lines} lines"),
            problem: format!(
                "{file} holds {lines} lines. A file that size makes review, testing and conflict resolution slower; it is a maintainability risk, not a proven performance problem."
            ),
            evidence: vec![format!("{lines} lines in {file}")],
            benefit: "Splitting by responsibility reduces the review surface and the chance of two agents editing one file.".to_string(),
            risk: "A split moves code; module paths and imports can break, and the change is large.".to_string(),
            files: vec![file],
            testing: "Compile and run the project's tests after each move; keep the split mechanical.".to_string(),
            rollback: "Deliver on a branch and revert the commit if the split breaks imports.".to_string(),
            priority: 1,
        });
    }
    for (doc, missing) in docs_referencing_missing_paths(root).into_iter().take(6) {
        let count = missing.len();
        findings.push(Finding {
            kind: "documentation",
            title: format!("{doc} references {count} missing path(s)"),
            problem: format!(
                "{doc} names paths that no longer exist, so a reader following it lands nowhere."
            ),
            evidence: missing
                .iter()
                .take(6)
                .map(|path| format!("{doc} mentions {path}, which does not exist"))
                .collect(),
            benefit: "Docs that resolve keep setup and API instructions usable.".to_string(),
            risk: "A path may be generated at build time; check before deleting the reference."
                .to_string(),
            files: vec![doc],
            testing: "Re-read the page and follow every command and path after the edit."
                .to_string(),
            rollback: "Revert the documentation commit; no runtime behaviour is involved."
                .to_string(),
            priority: 2,
        });
    }
    let tests = profile["testFiles"].as_u64().unwrap_or(0);
    let sources = profile["sourceFiles"].as_u64().unwrap_or(0);
    if tests == 0 && sources > 0 {
        findings.push(Finding {
            kind: "tests",
            title: "No test files were detected".to_string(),
            problem: format!(
                "The scan found {sources} source file(s) and no test file. Verification for every change currently depends on a build check and a review, not on a test."
            ),
            evidence: vec![
                format!("{sources} source file(s)"),
                "0 files matching test paths or test file names".to_string(),
            ],
            benefit: "One test per changed behaviour turns a review opinion into a repeatable check.".to_string(),
            risk: "Writing tests is new code; a wrong test can pass while the feature is broken, so tests need review too.".to_string(),
            files: Vec::new(),
            testing: "The new test must fail before the implementation exists and pass after it.".to_string(),
            rollback: "Revert the test commit; production behaviour is untouched.".to_string(),
            priority: 3,
        });
    }
    findings.truncate(MAX_PROPOSALS);
    findings
}

fn proposal_json(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "kind": row.get::<_, String>(1)?,
        "title": row.get::<_, String>(2)?,
        "problem": row.get::<_, String>(3)?,
        "evidence": serde_json::from_str::<Value>(&row.get::<_, String>(4)?).unwrap_or(json!([])),
        "benefit": row.get::<_, String>(5)?,
        "risk": row.get::<_, String>(6)?,
        "files": serde_json::from_str::<Value>(&row.get::<_, String>(7)?).unwrap_or(json!([])),
        "testing": row.get::<_, String>(8)?,
        "rollback": row.get::<_, String>(9)?,
        "priority": row.get::<_, i64>(10)?,
        "status": row.get::<_, String>(11)?,
        "pipeline": row.get::<_, String>(12)?,
        "rollbackCommit": row.get::<_, String>(13)?,
        "createdAt": row.get::<_, f64>(14)?,
        "updatedAt": row.get::<_, f64>(15)?,
    }))
}

fn save_finding(state: &AppState, project: &str, finding: &Finding) -> Result<bool, String> {
    let conn = db(state)?;
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM improve_proposals WHERE project=?1 AND title=?2 AND status IN ('proposed','approved','in-progress')",
            params![project, finding.title],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if existing.is_some() {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO improve_proposals(id, project, kind, title, problem, evidence, benefit, risk, \
         files, testing, rollback, priority, status, pipeline, rollback_commit, created_at, updated_at) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'proposed','','',?13,?13)",
        params![
            uuid::Uuid::new_v4().to_string(),
            project,
            finding.kind,
            finding.title,
            finding.problem,
            json!(finding.evidence).to_string(),
            finding.benefit,
            finding.risk,
            json!(finding.files).to_string(),
            finding.testing,
            finding.rollback,
            finding.priority,
            now()
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(true)
}

fn proposals(state: &AppState, limit: i64) -> Result<Vec<Value>, String> {
    let conn = db(state)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, title, problem, evidence, benefit, risk, files, testing, rollback, \
             priority, status, pipeline, rollback_commit, created_at, updated_at \
             FROM improve_proposals ORDER BY priority DESC, created_at DESC LIMIT ?1",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![limit.clamp(1, 100)], proposal_json)
        .map_err(|error| error.to_string())?;
    Ok(rows.flatten().collect())
}

fn load_proposal(state: &AppState, id: &str) -> Result<Option<Value>, String> {
    let conn = db(state)?;
    conn.query_row(
        "SELECT id, kind, title, problem, evidence, benefit, risk, files, testing, rollback, \
         priority, status, pipeline, rollback_commit, created_at, updated_at FROM improve_proposals WHERE id=?1",
        [id],
        proposal_json,
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn set_proposal_status(
    state: &AppState,
    id: &str,
    status: &str,
    pipeline: Option<&str>,
    rollback_commit: Option<&str>,
) -> Result<(), String> {
    let conn = db(state)?;
    conn.execute(
        "UPDATE improve_proposals SET status=?2, updated_at=?3, \
         pipeline=CASE WHEN ?4 <> '' THEN ?4 ELSE pipeline END, \
         rollback_commit=CASE WHEN ?5 <> '' THEN ?5 ELSE rollback_commit END WHERE id=?1",
        params![
            id,
            status,
            now(),
            pipeline.unwrap_or(""),
            rollback_commit.unwrap_or("")
        ],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------- experiments

fn outcome(mission: &Value) -> Value {
    let report = mission.get("report").cloned().unwrap_or(Value::Null);
    json!({
        "missionId": mission["id"],
        "status": mission["status"],
        "succeeded": mission["status"] == json!("succeeded"),
        "tasks": mission["tasks"].as_array().map(Vec::len).unwrap_or(0),
        "retries": mission["tasks"].as_array().map(|tasks| tasks.iter().map(|task| task["attempts"].as_i64().unwrap_or(0)).sum::<i64>()).unwrap_or(0),
        "tokens": report["tokens"].as_i64().unwrap_or(0),
        "elapsedSeconds": report["elapsedSeconds"].as_f64().unwrap_or(0.0),
        "checks": report["checks"].as_array().map(Vec::len).unwrap_or(0),
        "commands": report["commandsRun"].as_array().map(Vec::len).unwrap_or(0),
        "wroteFiles": report["filesWritten"].as_array().map(Vec::len).unwrap_or(0),
    })
}

/// Compare two arms on success first, then cost, and never call a smaller run
/// better when it dropped a verification stage. `verificationKept` comes from
/// the plan, not from the measurement.
pub(crate) fn compare(
    baseline: &Value,
    variant: &Value,
    variant_keeps_verification: bool,
) -> Value {
    let base_ok = baseline["succeeded"].as_bool().unwrap_or(false);
    let var_ok = variant["succeeded"].as_bool().unwrap_or(false);
    let base_tokens = baseline["tokens"].as_i64().unwrap_or(0);
    let var_tokens = variant["tokens"].as_i64().unwrap_or(0);
    let base_time = baseline["elapsedSeconds"].as_f64().unwrap_or(0.0);
    let var_time = variant["elapsedSeconds"].as_f64().unwrap_or(0.0);
    let (verdict, reason) = if base_ok && !var_ok {
        (
            "baseline",
            "The variant failed while the baseline succeeded.",
        )
    } else if !base_ok && var_ok {
        (
            "variant",
            "The baseline failed while the variant succeeded.",
        )
    } else if !base_ok && !var_ok {
        (
            "neither",
            "Both arms failed; there is no improvement to claim.",
        )
    } else if !variant_keeps_verification {
        (
            "inconclusive",
            "The variant drops a verification stage, so a faster result is not comparable.",
        )
    } else if var_tokens < base_tokens && var_time <= base_time * 1.2 {
        (
            "variant",
            "Both succeeded and the variant used fewer tokens without running materially longer.",
        )
    } else if base_tokens < var_tokens && base_time <= var_time * 1.2 {
        (
            "baseline",
            "Both succeeded and the baseline was cheaper without running materially longer.",
        )
    } else {
        (
            "tie",
            "Both succeeded with comparable cost; this sample does not separate them.",
        )
    };
    json!({
        "verdict": verdict,
        "reason": reason,
        "sampleSize": 1,
        "caveat": "One run per arm. This is a measurement, not a statistically meaningful result; repeat before trusting a small difference.",
        "baseline": baseline,
        "variant": variant,
        "deltas": {
            "tokens": var_tokens - base_tokens,
            "elapsedSeconds": ((var_time - base_time) * 100.0).round() / 100.0,
            "tasks": variant["tasks"].as_i64().unwrap_or(0) - baseline["tasks"].as_i64().unwrap_or(0),
        },
    })
}

fn experiment_plan(lean: bool) -> Value {
    if lean {
        json!([
            { "id": "check", "role": "testing", "title": "Check the current behaviour" },
            { "id": "review", "role": "review", "title": "Review the finding", "dependsOn": ["check"] }
        ])
    } else {
        json!([
            { "id": "analyse", "role": "architect", "title": "Analyse the project" },
            { "id": "check", "role": "testing", "title": "Check the current behaviour", "dependsOn": ["analyse"] },
            { "id": "review", "role": "review", "title": "Review the finding", "dependsOn": ["check"] }
        ])
    }
}

// ---------------------------------------------------------------- prediction

/// A risk score with its observations attached. This is a weighted count, not a
/// calibrated probability, and the response says so.
pub(crate) fn risk_score(
    failed_missions: i64,
    check_failures: i64,
    has_tests: bool,
    dirty: bool,
) -> Value {
    let mut score = 0.1;
    let mut evidence: Vec<String> = Vec::new();
    if failed_missions > 0 {
        score += (0.15 * failed_missions as f64).min(0.45);
        evidence.push(format!(
            "{failed_missions} matching mission(s) failed in the last {WINDOW_DAYS} days"
        ));
    }
    if check_failures > 0 {
        score += (0.2 * check_failures as f64).min(0.3);
        evidence.push(format!(
            "{check_failures} failed project check(s) in the last 7 days"
        ));
    }
    if !has_tests {
        score += 0.15;
        evidence.push("no test files were detected".to_string());
    }
    if dirty {
        score += 0.1;
        evidence.push("the worktree has uncommitted changes".to_string());
    }
    let score = ((score.min(0.95)) * 100.0).round() / 100.0;
    let band = if score >= 0.6 {
        "high"
    } else if score >= 0.35 {
        "medium"
    } else {
        "low"
    };
    json!({
        "score": score,
        "band": band,
        "label": "heuristic risk score, not a calibrated probability",
        "evidence": evidence,
        "observations": evidence.len(),
    })
}

// ---------------------------------------------------------------- report

fn maintenance(state: &AppState) -> (Value, Vec<String>) {
    let profile = crate::engineering::profile_view(state).unwrap_or(Value::Null);
    let checks: Value = db(state)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT kind, status, summary, created_at FROM engineering_checks ORDER BY seq DESC LIMIT 1",
                [],
                |row| {
                    Ok(json!({
                        "kind": row.get::<_, String>(0)?,
                        "status": row.get::<_, String>(1)?,
                        "summary": row.get::<_, String>(2)?,
                        "at": row.get::<_, f64>(3)?,
                    }))
                },
            )
            .optional()
            .ok()
            .flatten()
        })
        .unwrap_or(Value::Null);
    let expired = db(state)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM improve_experiences WHERE expires_at > 0 AND expires_at <= ?1",
                params![now()],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        })
        .unwrap_or(0);
    let open = db(state)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM improve_proposals WHERE status IN ('proposed','approved','in-progress')",
                [],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        })
        .unwrap_or(0);
    let mut unavailable = vec![
        "outdated dependencies (no registry query is made; the app is offline-first)".to_string(),
        "vulnerability scan (no security scanner is configured)".to_string(),
        "dead-code analysis (the index reports references per symbol, but a whole-program reachability pass is not implemented)".to_string(),
    ];
    if profile.is_null() {
        unavailable.push("project profile (open a workspace)".to_string());
    }
    (
        json!({
            "branch": profile["branch"],
            "dirty": profile["dirty"],
            "manifests": profile["manifests"],
            "declaredDependencies": profile["dependencies"],
            "sourceFiles": profile["sourceFiles"],
            "testFiles": profile["testFiles"],
            "docFiles": profile["docFiles"],
            "lastCheck": checks,
            "openProposals": open,
            "expiredExperiences": expired,
            "note": "Repository health is what the scan and the last check recorded. Registry freshness, vulnerabilities and whole-program dead code are not measured and stay unavailable.",
        }),
        unavailable,
    )
}

fn report(state: &AppState) -> Value {
    let s = settings(state);
    let since = now() - WINDOW_DAYS as f64 * 86_400.0;
    let mut unavailable: Vec<String> = Vec::new();
    let agents = agent_metrics(state);
    let (models, model_unavailable) = model_metrics(state);
    unavailable.extend(model_unavailable);
    let model_advice = rank_models(
        models.as_array().map(Vec::as_slice).unwrap_or(&[]),
        &s.priority,
    );
    if model_advice.is_empty() {
        unavailable.push("model advice (no model has two recorded runs in the window)".to_string());
    }
    let (workflow, workflow_unavailable) = workflow_metrics(state);
    unavailable.extend(workflow_unavailable);
    let (maintenance, maintenance_unavailable) = maintenance(state);
    unavailable.extend(maintenance_unavailable);
    let (quality, tasks_done, tokens, missions) = db(state)
        .map(|conn| {
            let missions = conn
                .query_row(
                    "SELECT COUNT(*) FROM missions WHERE created_at >= ?1",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let done = conn
                .query_row(
                    "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id=mt.mission_id WHERE m.created_at >= ?1 AND mt.status='done'",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let tasks = conn
                .query_row(
                    "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id=mt.mission_id WHERE m.created_at >= ?1",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let succeeded = conn
                .query_row(
                    "SELECT COUNT(*) FROM missions WHERE created_at >= ?1 AND status='succeeded'",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let retried = conn
                .query_row(
                    "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id=mt.mission_id WHERE m.created_at >= ?1 AND mt.attempts>0",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let recovered = conn
                .query_row(
                    "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id=mt.mission_id WHERE m.created_at >= ?1 AND mt.attempts>0 AND mt.status='done'",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let first_attempt = conn
                .query_row(
                    "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id=mt.mission_id WHERE m.created_at >= ?1 AND mt.status='done' AND mt.attempts=0",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let tokens = conn
                .query_row(
                    "SELECT COALESCE(SUM(r.tokens),0) FROM agent_runs r WHERE r.created_at >= ?1",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let review_failed = conn
                .query_row(
                    "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id=mt.mission_id WHERE m.created_at >= ?1 AND mt.role='review'",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let review_rejected = conn
                .query_row(
                    "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id=mt.mission_id WHERE m.created_at >= ?1 AND mt.role='review' AND mt.status='failed'",
                    params![since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            (
                json!({
                    "windowDays": WINDOW_DAYS,
                    "missions": missions,
                    "missionsSucceeded": succeeded,
                    "missionSuccessRate": rate(succeeded, missions),
                    "tasks": tasks,
                    "tasksDone": done,
                    "taskSuccessRate": rate(done, tasks),
                    "firstAttemptRate": rate(first_attempt, done),
                    "retries": retried,
                    "recoveryRate": rate(recovered, retried),
                    "reviewRejectionRate": rate(review_rejected, review_failed),
                    "tokens": tokens,
                }),
                done,
                tokens,
                missions,
            )
        })
        .unwrap_or((json!({}), 0, 0, 0));
    let _ = (tasks_done, tokens);
    if missions == 0 {
        unavailable.push("agent and workflow trends (no missions in the window yet)".to_string());
    }
    let experience_counts = db(state)
        .ok()
        .map(|conn| {
            json!({
                "total": conn.query_row("SELECT COUNT(*) FROM improve_experiences", [], |row| row.get::<_, i64>(0)).unwrap_or(0),
                "validated": conn.query_row("SELECT COUNT(*) FROM improve_experiences WHERE validated=1", [], |row| row.get::<_, i64>(0)).unwrap_or(0),
                "unvalidated": conn.query_row("SELECT COUNT(*) FROM improve_experiences WHERE validated=0", [], |row| row.get::<_, i64>(0)).unwrap_or(0),
                "expired": conn.query_row("SELECT COUNT(*) FROM improve_experiences WHERE expires_at > 0 AND expires_at <= ?1", params![now()], |row| row.get::<_, i64>(0)).unwrap_or(0),
                "preferences": conn.query_row("SELECT COUNT(*) FROM improve_experiences WHERE kind='preference'", [], |row| row.get::<_, i64>(0)).unwrap_or(0),
            })
        })
        .unwrap_or(json!({}));
    let proposal_counts = db(state)
        .ok()
        .map(|conn| {
            json!({
                "proposed": conn.query_row("SELECT COUNT(*) FROM improve_proposals WHERE status='proposed'", [], |row| row.get::<_, i64>(0)).unwrap_or(0),
                "inProgress": conn.query_row("SELECT COUNT(*) FROM improve_proposals WHERE status='in-progress'", [], |row| row.get::<_, i64>(0)).unwrap_or(0),
                "rejected": conn.query_row("SELECT COUNT(*) FROM improve_proposals WHERE status='rejected'", [], |row| row.get::<_, i64>(0)).unwrap_or(0),
                "rolledBack": conn.query_row("SELECT COUNT(*) FROM improve_proposals WHERE status='rolled-back'", [], |row| row.get::<_, i64>(0)).unwrap_or(0),
            })
        })
        .unwrap_or(json!({}));
    let experiments = db(state)
        .ok()
        .and_then(|conn| {
            conn.query_row("SELECT COUNT(*) FROM improve_experiments", [], |row| {
                row.get::<_, i64>(0)
            })
            .ok()
        })
        .unwrap_or(0);
    let events: Vec<Value> = db(state)
        .ok()
        .map(|conn| {
            let mut stmt = match conn.prepare(
                "SELECT seq, kind, subject, detail, created_at FROM improve_events ORDER BY seq DESC LIMIT 15",
            ) {
                Ok(stmt) => stmt,
                Err(_) => return Vec::new(),
            };
            stmt.query_map([], |row| {
                Ok(json!({
                    "seq": row.get::<_, i64>(0)?,
                    "kind": row.get::<_, String>(1)?,
                    "subject": row.get::<_, String>(2)?,
                    "detail": row.get::<_, String>(3)?,
                    "at": row.get::<_, f64>(4)?,
                }))
            })
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
        })
        .unwrap_or_default();
    json!({
        "settings": settings_json(s.clone()),
        "quality": quality,
        "agents": agents,
        "models": models,
        "modelAdvice": model_advice,
        "modelPriority": s.priority,
        "workflow": workflow,
        "failures": recurring_failures(state),
        "maintenance": maintenance,
        "experiences": experience_counts,
        "proposals": proposal_counts,
        "experimentCount": experiments,
        "events": events,
        "unavailable": unavailable,
    })
}

// ---------------------------------------------------------------- api

fn bad(message: impl Into<String>) -> Response {
    crate::json_error(StatusCode::BAD_REQUEST, message)
}

fn disabled(state: &AppState) -> Option<Response> {
    (!settings(state).enabled).then(|| {
        crate::json_error(
            StatusCode::CONFLICT,
            "Self-improvement is disabled. Enable it in the Self-improvement settings before learning or proposing.",
        )
    })
}

async fn get_settings(State(state): State<AppState>) -> Response {
    Json(json!({ "settings": settings_json(settings(&state)) })).into_response()
}

async fn put_settings(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    match save_settings(&state, &input) {
        Ok(saved) => Json(json!({ "ok": true, "settings": settings_json(saved) })).into_response(),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn report_route(State(state): State<AppState>) -> Response {
    Json(report(&state)).into_response()
}

async fn learn(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    if let Some(response) = disabled(&state) {
        return response;
    }
    if let Some(id) = input.get("missionId").and_then(Value::as_str) {
        return match observe_mission(&state, id) {
            Ok(0) => Json(json!({
                "ok": true,
                "learned": 0,
                "note": "Nothing was recorded: the mission is not finished, or it produced no verifiable result.",
            }))
            .into_response(),
            Ok(count) => Json(json!({ "ok": true, "learned": count })).into_response(),
            Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
        };
    }
    // Backfill the newest finished missions; the hook already ran for new ones.
    let Ok(conn) = db(&state) else {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error.");
    };
    let ids: Vec<String> = conn
        .prepare(
            "SELECT id FROM missions WHERE status IN ('succeeded','failed') ORDER BY updated_at DESC LIMIT ?1",
        )
        .and_then(|mut stmt| {
            stmt.query_map(params![LEARN_WINDOW], |row| row.get::<_, String>(0))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();
    let mut learned = 0;
    for id in ids {
        learned += observe_mission(&state, &id).unwrap_or(0);
    }
    Json(json!({ "ok": true, "learned": learned })).into_response()
}

#[derive(serde::Deserialize)]
struct ExperienceQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    include_unverified: Option<bool>,
}

async fn experiences(
    State(state): State<AppState>,
    Query(query): Query<ExperienceQuery>,
) -> Response {
    match retrieve(
        &state,
        &query.q,
        &query.kind,
        query.include_unverified.unwrap_or(false),
        query.limit.unwrap_or(10),
    ) {
        Ok(rows) => Json(json!({ "experiences": rows })).into_response(),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn preference(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    match add_preference(&state, &input) {
        Ok(payload) => Json(payload).into_response(),
        Err((code, message)) => crate::json_error(code, message),
    }
}

async fn forget_experience(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match forget(&state, &id) {
        Ok(true) => Json(json!({ "ok": true })).into_response(),
        Ok(false) => crate::json_error(StatusCode::NOT_FOUND, "No such experience."),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn purge(State(state): State<AppState>) -> Response {
    match purge_expired(&state) {
        Ok(count) => Json(json!({ "ok": true, "removed": count })).into_response(),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn scan(State(state): State<AppState>) -> Response {
    if let Some(response) = disabled(&state) {
        return response;
    }
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(_) => return bad("Open a workspace folder first."),
    };
    let profile = crate::engineering::profile_view(&state).unwrap_or(Value::Null);
    let project = project_key(&state);
    let findings = quality_findings(&root, &profile);
    let mut created = 0;
    let mut skipped = 0;
    for finding in &findings {
        match save_finding(&state, &project, finding) {
            Ok(true) => created += 1,
            Ok(false) => skipped += 1,
            Err(error) => return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
        }
    }
    event(
        &state,
        "scan",
        &project,
        &format!("{created} proposal(s) created, {skipped} already open"),
    );
    Json(json!({
        "ok": true,
        "findings": findings.len(),
        "created": created,
        "alreadyOpen": skipped,
        "proposals": proposals(&state, 100).unwrap_or_default(),
    }))
    .into_response()
}

async fn list_proposals(State(state): State<AppState>) -> Response {
    match proposals(&state, 100) {
        Ok(rows) => Json(json!({ "proposals": rows })).into_response(),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn approve_proposal(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    if let Some(response) = disabled(&state) {
        return response;
    }
    let Some(proposal) = load_proposal(&state, &id).ok().flatten() else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such proposal.");
    };
    if proposal["status"] != json!("proposed") {
        return bad("Only a proposed improvement can be approved.");
    }
    if let Some(refusal) = budget_refusal(&state, settings(&state)) {
        return crate::json_error(StatusCode::CONFLICT, refusal);
    }
    let goal = format!(
        "Improve the project: {title}\n\nProblem: {problem}\n\nEvidence:\n{evidence}\n\nExpected benefit: {benefit}\n\nRisk: {risk}\n\nTesting: {testing}\n\nRollback: {rollback}",
        title = proposal["title"].as_str().unwrap_or(""),
        problem = proposal["problem"].as_str().unwrap_or(""),
        evidence = proposal["evidence"]
            .as_array()
            .map(|items| items
                .iter()
                .filter_map(|item| item.as_str())
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n"))
            .unwrap_or_default(),
        benefit = proposal["benefit"].as_str().unwrap_or(""),
        risk = proposal["risk"].as_str().unwrap_or(""),
        testing = proposal["testing"].as_str().unwrap_or(""),
        rollback = proposal["rollback"].as_str().unwrap_or(""),
    );
    let autonomy = settings(&state).autonomy;
    match crate::engineering::create_for_goal(&state, &goal, autonomy).await {
        Ok((pipeline, needs_input, _)) => {
            let pipeline_id = pipeline["id"].as_str().unwrap_or("").to_string();
            let _ = set_proposal_status(
                &state,
                &id,
                if needs_input {
                    "needs-input"
                } else {
                    "in-progress"
                },
                Some(&pipeline_id),
                None,
            );
            event(
                &state,
                "approve",
                &id,
                &format!("pipeline {pipeline_id} started at autonomy {autonomy}"),
            );
            Json(json!({
                "ok": true,
                "needsInput": needs_input,
                "proposal": load_proposal(&state, &id).ok().flatten(),
                "pipeline": pipeline,
                "note": "The change is implemented on an isolated branch by an engineering pipeline. Approval started it; the commit still goes through the git approval policy.",
            }))
            .into_response()
        }
        Err((code, message)) => crate::json_error(code, message),
    }
}

async fn reject_proposal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    let Some(proposal) = load_proposal(&state, &id).ok().flatten() else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such proposal.");
    };
    if !matches!(
        proposal["status"].as_str(),
        Some("proposed") | Some("needs-input")
    ) {
        return bad("Only a proposal that has not started can be rejected.");
    }
    let reason = input
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if let Err(error) = set_proposal_status(&state, &id, "rejected", None, None) {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error);
    }
    event(&state, "reject", &id, reason);
    Json(json!({ "ok": true, "proposal": load_proposal(&state, &id).ok().flatten() }))
        .into_response()
}

async fn rollback_proposal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    let Some(proposal) = load_proposal(&state, &id).ok().flatten() else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such proposal.");
    };
    let pipeline_id = proposal["pipeline"].as_str().unwrap_or("");
    let Some(pipeline) = crate::engineering::pipeline_view(&state, pipeline_id) else {
        return bad("This proposal has no pipeline to roll back.");
    };
    let Some((commit, _branch)) = crate::engineering::delivery_of(&pipeline) else {
        return bad(
            "The pipeline has not delivered a commit; reject or abandon the proposal instead.",
        );
    };
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !approve {
        return crate::json_error(
            StatusCode::CONFLICT,
            "Rolling back rewrites history by adding a revert commit: approve it.",
        );
    }
    if let Some(response) = crate::permissions::guard_user(
        &state,
        crate::permissions::Action::Git,
        &commit,
        "",
        true,
        "revert a self-improvement commit",
    ) {
        let _ = response;
        return crate::json_error(
            StatusCode::CONFLICT,
            "The git permission refused the rollback.",
        );
    }
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(_) => return bad("Open a workspace folder first."),
    };
    match crate::git::revert_checked(&root, &commit) {
        Ok(result) => {
            let revert = result["commit"].as_str().unwrap_or("").to_string();
            let _ = set_proposal_status(&state, &id, "rolled-back", None, Some(&revert));
            event(
                &state,
                "rollback",
                &id,
                &format!("reverted {commit} as {revert}"),
            );
            Json(json!({
                "ok": true,
                "reverted": commit,
                "revertCommit": revert,
                "proposal": load_proposal(&state, &id).ok().flatten(),
            }))
            .into_response()
        }
        Err(error) => crate::json_error(StatusCode::CONFLICT, error),
    }
}

async fn predict(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let goal = input
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if goal.is_empty() {
        return bad("Describe the change to assess.");
    }
    let terms = keywords(goal);
    let profile = crate::engineering::profile_view(&state).unwrap_or(Value::Null);
    let since = now() - WINDOW_DAYS as f64 * 86_400.0;
    let check_since = now() - 7.0 * 86_400.0;
    let (failed_missions, check_failures) = db(&state)
        .map(|conn| {
            let mut failed = 0;
            if let Ok(mut stmt) = conn.prepare(
                "SELECT goal FROM missions WHERE created_at >= ?1 AND status='failed'",
            ) {
                if let Ok(rows) = stmt.query_map(params![since], |row| row.get::<_, String>(0)) {
                    for mission_goal in rows.flatten() {
                        let mission_terms = keywords(&mission_goal);
                        let overlap = terms
                            .iter()
                            .filter(|word| mission_terms.contains(*word))
                            .count();
                        if overlap >= 2 {
                            failed += 1;
                        }
                    }
                }
            }
            let checks = conn
                .query_row(
                    "SELECT COUNT(*) FROM engineering_checks WHERE status='failed' AND created_at >= ?1",
                    params![check_since],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            (failed, checks)
        })
        .unwrap_or((0, 0));
    let risk = risk_score(
        failed_missions,
        check_failures,
        profile["testFiles"].as_u64().unwrap_or(0) > 0,
        profile["dirty"].as_bool().unwrap_or(false),
    );
    Json(json!({
        "goal": goal,
        "risk": risk,
        "baseCommit": profile["commit"],
        "note": "The score is a weighted count of recorded failures and gaps. It is a risk assessment with its evidence, never a prediction of a guaranteed outcome.",
    }))
    .into_response()
}

fn experiment_json(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "name": row.get::<_, String>(1)?,
        "goal": row.get::<_, String>(2)?,
        "baseline": row.get::<_, String>(3)?,
        "variant": row.get::<_, String>(4)?,
        "baselineMission": row.get::<_, String>(5)?,
        "variantMission": row.get::<_, String>(6)?,
        "status": row.get::<_, String>(7)?,
        "baselineMetrics": serde_json::from_str::<Value>(&row.get::<_, String>(8)?).unwrap_or(Value::Null),
        "variantMetrics": serde_json::from_str::<Value>(&row.get::<_, String>(9)?).unwrap_or(Value::Null),
        "verdict": serde_json::from_str::<Value>(&row.get::<_, String>(10)?).unwrap_or(Value::Null),
        "createdAt": row.get::<_, f64>(11)?,
        "updatedAt": row.get::<_, f64>(12)?,
    }))
}

const EXPERIMENT_SQL: &str = "SELECT id, name, goal, baseline, variant, baseline_mission, variant_mission, \
 status, baseline_metrics, variant_metrics, verdict, created_at, updated_at FROM improve_experiments";

async fn create_experiment(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    if let Some(response) = disabled(&state) {
        return response;
    }
    let goal = input
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if goal.split_whitespace().count() < 3 {
        return bad("An experiment needs a concrete goal so both arms do the same work.");
    }
    if let Some(refusal) = budget_refusal(&state, settings(&state)) {
        return crate::json_error(StatusCode::CONFLICT, refusal);
    }
    let s = settings(&state);
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(|name| name.trim().chars().take(80).collect::<String>())
        .unwrap_or_else(|| "Decomposition: analysis+check+review vs check+review".to_string());
    let mission = |tasks: Value, label: &str| {
        json!({
            "goal": format!("{goal}\n\nExperiment arm: {label}"),
            "template": "custom",
            "tasks": tasks,
            "parallel": 2,
            "maxRetries": 0,
            "allowWrites": false,
            "allowCommands": true,
            "autoApprove": s.autonomy >= 3,
        })
    };
    let baseline = match crate::multi_agent::create_mission_with(
        &state,
        mission(experiment_plan(false), "baseline"),
    )
    .await
    {
        Ok(id) => id,
        Err((code, message)) => return crate::json_error(code, message),
    };
    let variant = match crate::multi_agent::create_mission_with(
        &state,
        mission(experiment_plan(true), "variant"),
    )
    .await
    {
        Ok(id) => id,
        Err((code, message)) => {
            let _ = crate::multi_agent::cancel_mission_record(&state, &baseline).await;
            return crate::json_error(code, message);
        }
    };
    let id = uuid::Uuid::new_v4().to_string();
    let Ok(conn) = db(&state) else {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error.");
    };
    if conn
        .execute(
            "INSERT INTO improve_experiments(id, project, name, goal, baseline, variant, \
             baseline_mission, variant_mission, status, baseline_metrics, variant_metrics, verdict, created_at, updated_at) \
             VALUES(?1,?2,?3,?4,'baseline','lean',?5,?6,'running','','','{}',?7,?7)",
            params![id, project_key(&state), name, goal, baseline, variant, now()],
        )
        .is_err()
    {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "The experiment could not be recorded.");
    }
    event(&state, "experiment", &id, "two read-only missions created");
    Json(json!({
        "ok": true,
        "experiment": load_experiment(&state, &id).ok().flatten(),
        "note": "Both arms are read-only missions over the same goal; they cannot write files. One run per arm, so the verdict is a measurement, not a statistical result.",
    }))
    .into_response()
}

fn load_experiment(state: &AppState, id: &str) -> Result<Option<Value>, String> {
    let conn = db(state)?;
    conn.query_row(
        &format!("{EXPERIMENT_SQL} WHERE id=?1"),
        [id],
        experiment_json,
    )
    .optional()
    .map_err(|error| error.to_string())
}

async fn list_experiments(State(state): State<AppState>) -> Response {
    let Ok(conn) = db(&state) else {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error.");
    };
    let rows: Vec<Value> = conn
        .prepare(&format!(
            "{EXPERIMENT_SQL} ORDER BY created_at DESC LIMIT 30"
        ))
        .and_then(|mut stmt| {
            stmt.query_map([], experiment_json)
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();
    Json(json!({ "experiments": rows })).into_response()
}

async fn complete_experiment(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(experiment) = load_experiment(&state, &id).ok().flatten() else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such experiment.");
    };
    if experiment["status"] == json!("completed") {
        return Json(json!({ "ok": true, "experiment": experiment })).into_response();
    }
    let baseline_id = experiment["baselineMission"].as_str().unwrap_or("");
    let variant_id = experiment["variantMission"].as_str().unwrap_or("");
    let Some(baseline_mission) = crate::multi_agent::mission_json_for(&state, baseline_id) else {
        return bad("The baseline mission is missing.");
    };
    let Some(variant_mission) = crate::multi_agent::mission_json_for(&state, variant_id) else {
        return bad("The variant mission is missing.");
    };
    let terminal = |mission: &Value| {
        matches!(
            mission["status"].as_str(),
            Some("succeeded") | Some("failed") | Some("cancelled")
        )
    };
    if !terminal(&baseline_mission) || !terminal(&variant_mission) {
        return crate::json_error(
            StatusCode::CONFLICT,
            "One arm is still running; complete the experiment after both finish.",
        );
    }
    let baseline_metrics = outcome(&baseline_mission);
    let variant_metrics = outcome(&variant_mission);
    let verdict = compare(&baseline_metrics, &variant_metrics, true);
    let Ok(conn) = db(&state) else {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error.");
    };
    if conn
        .execute(
            "UPDATE improve_experiments SET status='completed', baseline_metrics=?2, variant_metrics=?3, \
             verdict=?4, updated_at=?5 WHERE id=?1",
            params![
                id,
                baseline_metrics.to_string(),
                variant_metrics.to_string(),
                verdict.to_string(),
                now()
            ],
        )
        .is_err()
    {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "The result could not be stored.");
    }
    event(
        &state,
        "experiment-result",
        &id,
        verdict["verdict"].as_str().unwrap_or(""),
    );
    Json(json!({
        "ok": true,
        "experiment": load_experiment(&state, &id).ok().flatten(),
    }))
    .into_response()
}

async fn stop(State(state): State<AppState>) -> Response {
    let (missions, runs) = crate::multi_agent::cancel_all(&state).await;
    let detached = crate::agent::cancel_all_active(&state, "Emergency stop from Self-improvement.");
    Json(json!({
        "ok": true,
        "missionsCancelled": missions,
        "runsStopped": runs.max(detached.len()),
    }))
    .into_response()
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/improve/settings",
            get(get_settings).post(put_settings),
        )
        .route("/api/improve/report", get(report_route))
        .route("/api/improve/learn", post(learn))
        .route(
            "/api/improve/experiences",
            get(experiences).post(preference),
        )
        .route(
            "/api/improve/experiences/:id",
            axum::routing::delete(forget_experience),
        )
        .route("/api/improve/purge", post(purge))
        .route("/api/improve/scan", post(scan))
        .route("/api/improve/proposals", get(list_proposals))
        .route("/api/improve/proposals/:id/approve", post(approve_proposal))
        .route("/api/improve/proposals/:id/reject", post(reject_proposal))
        .route(
            "/api/improve/proposals/:id/rollback",
            post(rollback_proposal),
        )
        .route(
            "/api/improve/experiments",
            get(list_experiments).post(create_experiment),
        )
        .route(
            "/api/improve/experiments/:id/complete",
            post(complete_experiment),
        )
        .route("/api/improve/predict", post(predict))
        .route("/api/improve/stop", post(stop))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mission(status: &str, tasks: Value, report: Value) -> Value {
        json!({
            "id": "m1",
            "goal": "Add a CSV export to the reports page",
            "status": status,
            "tasks": tasks,
            "report": report,
        })
    }

    fn task(role: &str, status: &str, summary: &str, error: &str, attempts: i64) -> Value {
        json!({
            "id": role,
            "role": role,
            "roleName": role,
            "status": status,
            "summary": summary,
            "error": error,
            "attempts": attempts,
        })
    }

    #[test]
    fn a_successful_mission_only_teaches_when_it_left_an_artifact() {
        let without = mission(
            "succeeded",
            json!([task("coding", "done", "explained, wrote nothing", "", 0)]),
            json!({ "filesWritten": [], "commandsRun": [], "checks": [] }),
        );
        let observations = observations_from(&without, false, now());
        assert!(observations.is_empty(), "a summary alone is not a strategy");

        let with = mission(
            "succeeded",
            json!([task("coding", "done", "wrote src/export.rs", "", 0)]),
            json!({ "filesWritten": ["write src/export.rs (10 bytes)"], "commandsRun": [], "checks": [] }),
        );
        let observations = observations_from(&with, false, now());
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].kind, "strategy");
        assert!(observations[0].validated);
        assert!(observations[0].evidence.contains("1 file(s) written"));
        // Privacy mode: the goal text is not stored, only a digest key.
        assert!(!observations[0].key.contains("csv"));
        assert!(!observations[0].title.contains("CSV"));
    }

    #[test]
    fn the_goal_is_kept_only_when_the_user_opted_in() {
        let done = mission(
            "succeeded",
            json!([task("coding", "done", "wrote a file", "", 0)]),
            json!({ "filesWritten": ["write a.txt (1 bytes)"] }),
        );
        let opt_in = observations_from(&done, true, now());
        assert!(opt_in[0].key.contains("export"));
        assert!(
            opt_in[0].title.to_lowercase().contains("csv") || opt_in[0].title.contains("reports")
        );
    }

    #[test]
    fn a_failed_mission_records_a_failure_and_never_a_strategy() {
        let failed = mission(
            "failed",
            json!([task(
                "coding",
                "failed",
                "",
                "error[E0308]: mismatched types",
                2
            )]),
            json!({ "filesWritten": [] }),
        );
        let observations = observations_from(&failed, false, now());
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].kind, "failure");
        assert!(observations[0].validated);
        assert!(observations[0].key.starts_with("failure:coding:"));
        // The same error normalises to the same key so repeats are counted.
        let again = observations_from(&failed, false, now());
        assert_eq!(again[0].key, observations[0].key);
    }

    #[test]
    fn a_running_or_cancelled_mission_teaches_nothing() {
        for status in ["running", "planned", "cancelled", "awaiting-approval"] {
            let value = mission(status, json!([]), json!({}));
            assert!(
                observations_from(&value, true, now()).is_empty(),
                "{status}"
            );
        }
    }

    #[test]
    fn confidence_grows_with_use_but_stays_below_certainty() {
        assert!(confidence_for("strategy", 1) < confidence_for("strategy", 3));
        assert!(confidence_for("strategy", 100) <= 0.9);
        assert!(confidence_for("failure", 100) <= 0.8);
        assert_eq!(confidence_for("preference", 1), 0.9);
    }

    #[test]
    fn model_advice_orders_measurements_and_ignores_single_runs() {
        let models = vec![
            json!({ "model": "fast", "runs": 4, "successRate": 50.0, "avgDurationSeconds": 4.0, "estimatedCostPerRunUsd": 0.01 }),
            json!({ "model": "steady", "runs": 6, "successRate": 100.0, "avgDurationSeconds": 40.0, "estimatedCostPerRunUsd": 0.5 }),
            json!({ "model": "sample", "runs": 1, "successRate": 100.0, "avgDurationSeconds": 1.0, "estimatedCostPerRunUsd": 0.0 }),
            json!({ "model": "unpriced", "runs": 3, "successRate": 100.0, "avgDurationSeconds": 4.0, "estimatedCostPerRunUsd": null }),
        ];
        let by_reliability = rank_models(&models, "reliability");
        assert_eq!(by_reliability[0]["model"], "steady");
        assert!(by_reliability
            .iter()
            .all(|row| row["runs"].as_i64() >= Some(2)));
        let by_speed = rank_models(&models, "speed");
        assert!(by_speed[0]["avgDurationSeconds"].as_f64() <= Some(4.0));
        let by_cost = rank_models(&models, "cost");
        assert_eq!(by_cost[0]["model"], "fast");
        // Advice is ordered evidence only; every row carries its own reason.
        assert!(by_cost[0]["reason"].as_str().unwrap().contains("price"));
        assert!(rank_models(&models, "balanced").len() <= 5);
    }

    #[test]
    fn relevance_prefers_the_record_that_matches_and_then_confidence() {
        let query = keywords("csv export reports");
        let matching = relevance(
            &query,
            "CSV export strategy",
            "wrote the export path",
            0.5,
            1.0,
        );
        let unrelated = relevance(&query, "Database migration", "renamed a table", 0.5, 1.0);
        assert!(matching > unrelated);
        let old = relevance(
            &query,
            "CSV export strategy",
            "wrote the export path",
            0.5,
            200.0,
        );
        assert!(matching > old);
        // No query means confidence and recency only.
        assert!(
            relevance(&[], "anything", "", 0.8, 0.0) > relevance(&[], "anything", "", 0.2, 0.0)
        );
    }

    #[test]
    fn an_error_key_ignores_paths_numbers_and_case() {
        let a = normalize_error("error[E0308] at /tmp/x/src/a.rs:12: mismatched types");
        let b = normalize_error("ERROR[E0308] at /home/y/src/b.rs:99: mismatched types");
        assert_eq!(a, b);
    }

    #[test]
    fn a_duplicate_definition_becomes_a_prioritised_proposal_with_all_fields() {
        let root = std::env::temp_dir().join(format!("sarthi-improve-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "pub fn build_export() {}\n").unwrap();
        std::fs::write(root.join("src/b.rs"), "fn build_export() {}\n").unwrap();
        std::fs::write(root.join("src/short.rs"), "fn main() {}\n").unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join("docs/guide.md"),
            "See `app/src/missing.rs` for details.\n",
        )
        .unwrap();
        let profile = json!({ "sourceFiles": 3, "testFiles": 0 });
        let findings = quality_findings(&root, &profile);
        let duplicate = findings
            .iter()
            .find(|finding| finding.title.contains("build_export"))
            .expect("duplicate finding");
        assert_eq!(duplicate.kind, "duplication");
        assert_eq!(duplicate.files.len(), 2);
        assert!(!duplicate.problem.is_empty());
        assert!(!duplicate.benefit.is_empty());
        assert!(!duplicate.risk.is_empty());
        assert!(!duplicate.testing.is_empty());
        assert!(!duplicate.rollback.is_empty());
        assert!(findings
            .iter()
            .any(|finding| finding.kind == "documentation"));
        assert!(findings
            .iter()
            .any(|finding| finding.kind == "tests" && finding.priority == 3));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_split_run_is_never_declared_better_when_it_drops_a_check() {
        let baseline =
            json!({ "succeeded": true, "tokens": 900, "elapsedSeconds": 10.0, "tasks": 3 });
        let variant =
            json!({ "succeeded": true, "tokens": 400, "elapsedSeconds": 9.0, "tasks": 2 });
        assert_eq!(
            compare(&baseline, &variant, false)["verdict"],
            json!("inconclusive")
        );
        let kept = compare(&baseline, &variant, true);
        assert_eq!(kept["verdict"], json!("variant"));
        assert_eq!(kept["sampleSize"], json!(1));
        assert!(kept["caveat"].as_str().unwrap().contains("One run per arm"));
    }

    #[test]
    fn success_outranks_cost_in_a_comparison() {
        let failed = json!({ "succeeded": false, "tokens": 10, "elapsedSeconds": 1.0, "tasks": 2 });
        let ok = json!({ "succeeded": true, "tokens": 5000, "elapsedSeconds": 40.0, "tasks": 3 });
        assert_eq!(compare(&failed, &ok, true)["verdict"], json!("variant"));
        assert_eq!(compare(&ok, &failed, true)["verdict"], json!("baseline"));
        let neither = compare(&failed, &failed, true);
        assert_eq!(neither["verdict"], json!("neither"));
    }

    #[test]
    fn a_risk_score_carries_its_evidence_and_never_claims_certainty() {
        let low = risk_score(0, 0, true, false);
        assert_eq!(low["band"], json!("low"));
        let high = risk_score(3, 2, false, true);
        assert_eq!(high["band"], json!("high"));
        assert!(high["score"].as_f64().unwrap() <= 0.95);
        assert_eq!(high["observations"], json!(4));
        assert!(high["label"]
            .as_str()
            .unwrap()
            .contains("not a calibrated probability"));
    }

    #[test]
    fn the_experiment_plans_are_read_only_and_differ_by_one_stage() {
        let baseline = experiment_plan(false);
        let variant = experiment_plan(true);
        assert_eq!(baseline.as_array().unwrap().len(), 3);
        assert_eq!(variant.as_array().unwrap().len(), 2);
        for plan in [&baseline, &variant] {
            for task in plan.as_array().unwrap() {
                let role = task["role"].as_str().unwrap();
                assert!(matches!(role, "architect" | "testing" | "review"), "{role}");
            }
        }
    }
}

#[cfg(test)]
mod flow {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    struct Fixture {
        state: AppState,
        app: Router,
        root: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    async fn scripted(replies: &[&str]) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(
            replies
                .iter()
                .map(|reply| reply.to_string())
                .collect::<Vec<_>>(),
        )));
        let app = Router::new().route(
            "/chat/completions",
            axum::routing::post(move |_body: axum::Json<Value>| {
                let queue = queue.clone();
                async move {
                    let reply =
                        queue.lock().unwrap().pop_front().unwrap_or_else(|| {
                            "{\"action\":\"done\",\"summary\":\"ok\"}".to_string()
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
        format!("http://{addr}")
    }

    async fn boot(tag: &str, replies: &[&str]) -> Fixture {
        let base = scripted(replies).await;
        let root = std::env::temp_dir().join(format!(
            "sarthi-improve-flow-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let ws = root.join("ws");
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/a.rs"), "pub fn build_export() {}\n").unwrap();
        std::fs::write(ws.join("src/b.rs"), "fn build_export() {}\n").unwrap();
        std::fs::write(ws.join("README.md"), "# demo\n").unwrap();
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
            .expect("open workspace");
        let app = crate::multi_agent::router()
            .merge(router())
            .with_state(state.clone());
        Fixture { state, app, root }
    }

    async fn request(app: &Router, method: &str, uri: &str, body: Value) -> (u16, Value) {
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
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_finished_mission_becomes_an_experience_without_being_asked() {
        let fixture = boot(
            "learn",
            &[
                "{\"action\":\"write\",\"path\":\"src/export.rs\",\"content\":\"pub fn export() {}\"}",
                "{\"action\":\"done\",\"summary\":\"wrote the export helper\"}",
                "APPROVE: the file exists and matches the request",
            ],
        )
        .await;
        // Learning is off by default; this test is about the automatic hook, so
        // it opts in explicitly, exactly as a user would in the dashboard.
        let (status, _) = request(
            &fixture.app,
            "POST",
            "/api/improve/settings",
            json!({ "enabled": true }),
        )
        .await;
        assert_eq!(status, 200);
        let (status, created) = request(
            &fixture.app,
            "POST",
            "/api/missions",
            json!({
                "goal": "Add a CSV export helper for the reports page",
                "template": "custom",
                "tasks": [{ "id": "implement", "role": "coding" }],
                "allowWrites": true,
                "autoApprove": true,
                "maxRetries": 0,
            }),
        )
        .await;
        assert_eq!(status, 200, "{created}");
        let mission_id = created["mission"]["id"].as_str().unwrap().to_string();
        for _ in 0..400 {
            if let Some(mission) = crate::multi_agent::mission_json_for(&fixture.state, &mission_id)
            {
                if matches!(
                    mission["status"].as_str(),
                    Some("succeeded") | Some("failed")
                ) {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let mission = crate::multi_agent::mission_json_for(&fixture.state, &mission_id).unwrap();
        assert_eq!(mission["status"], json!("succeeded"), "{mission}");

        // The supervisor's hook recorded it; no learn call was needed.
        let (status, report) = request(&fixture.app, "GET", "/api/improve/report", json!({})).await;
        assert_eq!(status, 200);
        assert!(
            report["experiences"]["total"].as_i64().unwrap_or(0) >= 1,
            "{report}"
        );
        assert_eq!(report["experiences"]["validated"], json!(1));
        assert_eq!(report["quality"]["missions"], json!(1));
        assert_eq!(report["quality"]["missionsSucceeded"], json!(1));

        let (_, found) = request(
            &fixture.app,
            "GET",
            "/api/improve/experiences?q=export",
            json!({}),
        )
        .await;
        let lessons = found["experiences"].as_array().unwrap();
        assert!(!lessons.is_empty(), "{found}");
        assert_eq!(lessons[0]["kind"], json!("strategy"));
        assert!(lessons[0]["detail"]
            .as_str()
            .unwrap()
            .contains("wrote the export helper"));
        assert!(lessons[0]["evidence"]
            .as_str()
            .unwrap()
            .contains("succeeded"));
        // Privacy default: the goal sentence is not stored.
        assert!(!lessons[0]["detail"]
            .as_str()
            .unwrap()
            .contains("CSV export helper"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn disabling_improvement_blocks_scanning_and_a_proposal_needs_approval() {
        let fixture = boot("governance", &[]).await;
        let (status, _) = request(
            &fixture.app,
            "POST",
            "/api/improve/settings",
            json!({ "enabled": false }),
        )
        .await;
        assert_eq!(status, 200);
        let (status, refused) = request(&fixture.app, "POST", "/api/improve/scan", json!({})).await;
        assert_eq!(status, 409, "{refused}");

        let (status, _) = request(
            &fixture.app,
            "POST",
            "/api/improve/settings",
            json!({ "enabled": true, "autonomy": 2 }),
        )
        .await;
        assert_eq!(status, 200);
        let (status, scanned) = request(&fixture.app, "POST", "/api/improve/scan", json!({})).await;
        assert_eq!(status, 200, "{scanned}");
        let proposals = scanned["proposals"].as_array().unwrap();
        let duplicate = proposals
            .iter()
            .find(|proposal| proposal["kind"] == json!("duplication"))
            .expect("A duplicate function is a deterministic finding");
        for field in ["problem", "benefit", "risk", "testing", "rollback"] {
            assert!(
                !duplicate[field].as_str().unwrap_or("").is_empty(),
                "{field} must be present"
            );
        }
        assert!(duplicate["evidence"].as_array().unwrap().len() >= 2);
        assert_eq!(scanned["alreadyOpen"], json!(0));

        // A second scan does not duplicate the same open proposal.
        let (_, again) = request(&fixture.app, "POST", "/api/improve/scan", json!({})).await;
        assert_eq!(again["created"], json!(0));
        assert!(again["alreadyOpen"].as_i64().unwrap_or(0) >= 1);

        // Approving starts a real pipeline on a branch, at the configured level.
        let id = duplicate["id"].as_str().unwrap();
        let (status, approved) = request(
            &fixture.app,
            "POST",
            &format!("/api/improve/proposals/{id}/approve"),
            json!({}),
        )
        .await;
        assert_eq!(status, 200, "{approved}");
        assert!(!approved["pipeline"]["id"].as_str().unwrap_or("").is_empty());
        assert_eq!(approved["proposal"]["status"], json!("in-progress"));

        // A proposal cannot be approved twice.
        let (status, refused) = request(
            &fixture.app,
            "POST",
            &format!("/api/improve/proposals/{id}/approve"),
            json!({}),
        )
        .await;
        assert_eq!(status, 400);
        assert!(refused["error"].as_str().unwrap_or("").contains("proposed"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn preferences_are_stored_and_can_be_forgotten() {
        let fixture = boot("preference", &[]).await;
        let (status, _) = request(
            &fixture.app,
            "POST",
            "/api/improve/experiences",
            json!({ "text": "Always run the frontend tests before a release commit." }),
        )
        .await;
        assert_eq!(status, 200);
        let (_, found) = request(
            &fixture.app,
            "GET",
            "/api/improve/experiences?q=release%20commit",
            json!({}),
        )
        .await;
        let lessons = found["experiences"].as_array().unwrap();
        let preference = lessons
            .iter()
            .find(|lesson| lesson["kind"] == json!("preference"))
            .expect("preference");
        assert!(preference["validated"].as_bool().unwrap());
        let id = preference["id"].as_str().unwrap();
        let (status, _) = request(
            &fixture.app,
            "DELETE",
            &format!("/api/improve/experiences/{id}"),
            json!({}),
        )
        .await;
        assert_eq!(status, 200);
        let (_, after) = request(
            &fixture.app,
            "GET",
            "/api/improve/experiences?q=release%20commit",
            json!({}),
        )
        .await;
        assert!(after["experiences"].as_array().unwrap().is_empty());
    }
}
