//! Phase 17 — observability: one timeline, one support bundle.
//!
//! The app already records what it did, but in four separate places:
//! `request_log` (provider calls), `permission_audit` (privileged decisions),
//! `agent_runs`/`agent_steps` (tasks) and the router install records. Each is
//! visible only in its own panel, so answering "why did that run fail" means
//! visiting four screens and remembering the order.
//!
//! This module is a **read-only projection** over those tables: one
//! time-ordered list, a level per row, and the id that ties the row to a run, a
//! project or a conversation. It also builds a support bundle that can be shared
//! without a prompt, a file body or a credential leaving the machine.
//!
//! It writes nothing, calls no provider and duplicates no table: if a panel
//! shows it, the timeline reads the same row.

use crate::{now_seconds, open_db, AppState};
use axum::{
    extract::{Query as AxumQuery, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Rows returned when the caller does not ask for a count.
const DEFAULT_LIMIT: usize = 100;
/// Ceiling per request: a diagnostic view is read by a person, not a machine.
const MAX_LIMIT: usize = 500;
/// Detail strings are clipped so one provider paragraph cannot fill the panel.
const DETAIL_CHARS: usize = 400;
/// Same window the budget and the provider health card use.
const WINDOW_SECONDS: f64 = 86_400.0;

/// Field names whose *value* must never leave the process.
fn sensitive_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "authorization",
        "cookie",
    ]
    .iter()
    .any(|part| name.contains(part))
}

/// Strip secret-shaped text out of anything that leaves the process.
///
/// This is deliberately a value redaction, not a schema: a provider error can
/// echo a URL, a header or a fragment of a request, and none of that belongs in
/// an exported bundle. It is conservative — redacting a public digest costs
/// nothing, printing a private key costs everything.
pub(crate) fn redact(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let mut after_bearer = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            push_redacted_word(&mut word, &mut out, &mut after_bearer);
            out.push(ch);
        } else {
            word.push(ch);
        }
    }
    push_redacted_word(&mut word, &mut out, &mut after_bearer);
    out
}

fn push_redacted_word(word: &mut String, out: &mut String, after_bearer: &mut bool) {
    if word.is_empty() {
        return;
    }
    let trimmed = word.trim_matches(|ch: char| matches!(ch, '"' | '\'' | ',' | ';' | '(' | ')'));
    let lower = trimmed.to_ascii_lowercase();
    // "Bearer x": keep the scheme readable, drop the credential behind it.
    let is_bearer = lower == "bearer" || lower.ends_with(":bearer");
    let secret = *after_bearer || looks_like_secret(trimmed) || pair_is_sensitive(trimmed);
    if secret && !is_bearer {
        out.push_str("[redacted]");
    } else {
        out.push_str(word);
    }
    *after_bearer = is_bearer;
    word.clear();
}

/// A token that is shaped like a credential rather than like prose.
fn looks_like_secret(word: &str) -> bool {
    if word.len() < 8 {
        return false;
    }
    let lower = word.to_ascii_lowercase();
    const PREFIXES: [&str; 6] = ["sk-", "sk_", "ghp_", "gho_", "xoxb-", "aiza"];
    if PREFIXES.iter().any(|prefix| lower.starts_with(prefix)) {
        return true;
    }
    // A long unbroken run of credential-ish characters is treated as a secret
    // instead of being printed. Digests fall in here too, which is harmless.
    word.len() >= 40
        && word
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

/// `name=value` / `name: value` where the name says the value is a secret.
fn pair_is_sensitive(word: &str) -> bool {
    word.char_indices()
        .filter(|(_, ch)| *ch == '=' || *ch == ':')
        .any(|(index, ch)| {
            let name = &word[..index];
            let rest = &word[index + ch.len_utf8()..];
            sensitive_name(name) && !rest.trim().is_empty()
        })
}

pub(crate) fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

/// One row of the merged timeline, before it is turned into JSON.
struct Entry {
    at: f64,
    area: &'static str,
    event: String,
    level: &'static str,
    correlation: String,
    detail: String,
}

/// Merge the existing records into one newest-last list.
///
/// `include_goal` is false for the support bundle: a run's goal is the user's
/// own prompt, useful on screen and out of place in a file meant for sharing.
pub(crate) fn timeline_value(
    state: &AppState,
    limit: usize,
    area: Option<&str>,
    include_goal: bool,
) -> Result<Value, String> {
    let limit = limit.clamp(1, MAX_LIMIT);
    let conn = open_db(&state.database_path).map_err(|error| error.to_string())?;
    let mut entries: Vec<Entry> = Vec::new();

    // Provider requests: the model, the provider's own latency and error text.
    {
        let mut stmt = conn
            .prepare(
                "SELECT created_at, COALESCE(conversation_id,''), COALESCE(model,''),
                        COALESCE(provider,''), status, COALESCE(latency_ms,0),
                        COALESCE(total_tokens,0), COALESCE(error,'')
                   FROM request_log ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok((
                    row.get::<_, f64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (at, conversation, model, provider, status, latency, tokens, error) =
                row.map_err(|error| error.to_string())?;
            let ok = status == "ok";
            let mut detail = format!("{model} · {provider} · {latency} ms");
            if tokens > 0 {
                detail.push_str(&format!(" · {tokens} tok"));
            }
            if !error.is_empty() {
                detail.push_str(&format!(" — {error}"));
            }
            entries.push(Entry {
                at,
                area: "provider",
                event: if ok {
                    "request ok".to_string()
                } else {
                    "request failed".to_string()
                },
                level: if ok { "info" } else { "error" },
                correlation: conversation,
                detail,
            });
        }
    }

    // Permission decisions: policy working (an "ask") is not a failure.
    {
        let mut stmt = conn
            .prepare(
                "SELECT created_at, action, target, decision, scope, project, detail
                   FROM permission_audit ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok((
                    row.get::<_, f64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (at, action, target, decision, scope, project, detail) =
                row.map_err(|error| error.to_string())?;
            let mut summary = format!("{target} · scope {scope}");
            if !detail.is_empty() {
                summary.push_str(&format!(" — {detail}"));
            }
            entries.push(Entry {
                at,
                area: "permission",
                event: format!("{action} {decision}"),
                level: if decision == "allow" { "info" } else { "warn" },
                correlation: project,
                detail: summary,
            });
        }
    }

    // Agent runs: the run id is the correlation key everything else shares.
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, COALESCE(goal,''), status, COALESCE(error,''),
                        updated_at, COALESCE(tokens,0)
                   FROM agent_runs ORDER BY updated_at DESC LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (id, goal, status, error, at, tokens) = row.map_err(|error| error.to_string())?;
            let mut detail = if include_goal {
                clip(&goal, 160)
            } else {
                String::new()
            };
            if tokens > 0 {
                if !detail.is_empty() {
                    detail.push_str(" · ");
                }
                detail.push_str(&format!("{tokens} tokens"));
            }
            if !error.is_empty() {
                if !detail.is_empty() {
                    detail.push_str(" · ");
                }
                detail.push_str(&error);
            }
            entries.push(Entry {
                at,
                area: "agent",
                event: format!("run {status}"),
                level: match status.as_str() {
                    "failed" => "error",
                    "cancelled" | "interrupted" => "warn",
                    _ => "info",
                },
                correlation: id,
                detail,
            });
        }
    }

    // Steps that went wrong are the ones worth surfacing; a successful read is
    // already implied by the run that contains it.
    {
        let mut stmt = conn
            .prepare(
                "SELECT run_id, action, summary, created_at FROM agent_steps
                  WHERE action IN ('fail','mcp-error','write-conflict')
                  ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (run_id, action, summary, at) = row.map_err(|error| error.to_string())?;
            entries.push(Entry {
                at,
                area: "agent",
                event: format!("step {action}"),
                level: if action == "fail" { "error" } else { "warn" },
                correlation: run_id,
                detail: summary,
            });
        }
    }

    if let Some(area) = area {
        entries.retain(|entry| entry.area == area);
    }
    entries.sort_by(|left, right| {
        left.at
            .partial_cmp(&right.at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if entries.len() > limit {
        let excess = entries.len() - limit;
        entries.drain(0..excess);
    }

    let events: Vec<Value> = entries
        .iter()
        .map(|entry| {
            json!({
                "at": entry.at,
                "area": entry.area,
                "event": redact(&entry.event),
                "level": entry.level,
                "correlation": entry.correlation,
                "detail": clip(&redact(&entry.detail), DETAIL_CHARS),
            })
        })
        .collect();

    Ok(json!({
        "ok": true,
        "limit": limit,
        "areas": ["provider", "permission", "agent"],
        "events": events,
    }))
}

/// Counts per area, from the same rows the timeline reads. Status and decision
/// names come out of the database rather than a hardcoded list, so a new state
/// appears here the moment it is written.
pub(crate) fn summary_value(state: &AppState) -> Result<Value, String> {
    let conn = open_db(&state.database_path).map_err(|error| error.to_string())?;
    let since = now_seconds() - WINDOW_SECONDS;

    let (requests, ok): (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(status='ok'),0) FROM request_log WHERE created_at>=?1",
            [since],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;

    let mut decisions = serde_json::Map::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT decision, COUNT(*) FROM permission_audit
                  WHERE created_at>=?1 GROUP BY decision ORDER BY COUNT(*) DESC",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([since], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (decision, count) = row.map_err(|error| error.to_string())?;
            decisions.insert(decision, json!(count));
        }
    }

    let mut statuses = serde_json::Map::new();
    {
        let mut stmt = conn
            .prepare("SELECT status, COUNT(*) FROM agent_runs GROUP BY status")
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (status, count) = row.map_err(|error| error.to_string())?;
            statuses.insert(status, json!(count));
        }
    }

    let live: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_runs
              WHERE status IN ('queued','running','paused','awaiting_approval')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;

    Ok(json!({
        "window_hours": 24,
        "provider": {
            "requests": requests,
            "ok": ok,
            "errors": requests - ok,
        },
        "permission_decisions": Value::Object(decisions),
        "agent": {
            "by_status": Value::Object(statuses),
            "live": live,
        },
    }))
}

/// A file the user can attach to a bug report. Shapes come from the same
/// projection the panel reads; the only difference is that run goals are left
/// out and every string is redacted and clipped.
pub(crate) fn bundle_value(state: &AppState) -> Result<Value, String> {
    let cfg = crate::load_config(state);
    let timeline = timeline_value(state, DEFAULT_LIMIT, None, false)?;
    let events = timeline.get("events").cloned().unwrap_or(Value::Null);
    Ok(json!({
        "ok": true,
        "generated_at": now_seconds(),
        "app": {
            "name": crate::APP_NAME,
            "version": env!("CARGO_PKG_VERSION"),
        },
        "platform": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "family": std::env::consts::FAMILY,
        },
        "config": {
            "model": cfg.model,
            "base_url": redact(&cfg.base_url),
            "openai_base_url": redact(&cfg.openai_base_url),
            "local_base_url": redact(&cfg.local_base_url),
            "embedding_model": cfg.embedding_model,
            "thinking_level": cfg.thinking_level,
            "run_thinking_level": cfg.run_thinking_level,
            "agent_model": cfg.agent_model,
            "fallback_models": cfg.fallback_models,
            "daily_budget_usd": cfg.daily_budget_usd,
            "max_concurrent_runs": cfg.max_concurrent_runs,
            "context_budget_tokens": cfg.context_budget_tokens,
            "history_messages": cfg.history_messages,
            "auto_select_skills": cfg.auto_select_skills,
            "adaptive_routing": cfg.adaptive_routing,
            "privacy_exclusions": cfg.privacy_exclusions,
            "credentials": {
                "router_key_saved": !cfg.api_key.trim().is_empty(),
                "openai_key_saved": !cfg.openai_api_key.trim().is_empty(),
                "search_key_saved": !cfg.search_api_key.trim().is_empty(),
            },
        },
        "summary": summary_value(state)?,
        "events": events,
        "notes": [
            "No prompt text, file body or credential value is included.",
            "Credentials appear only as whether one is saved.",
            "Provider errors are clipped and redacted before they leave the process.",
        ],
    }))
}

/// `GET /api/diagnostics?limit=&area=` — the merged timeline plus counts.
async fn timeline_route(
    State(state): State<AppState>,
    AxumQuery(params): AxumQuery<HashMap<String, String>>,
) -> Response {
    let limit = params
        .get("limit")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    let area = params
        .get("area")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(named) = area.as_deref() {
        if !matches!(named, "provider" | "permission" | "agent") {
            return crate::json_error(
                StatusCode::BAD_REQUEST,
                "area must be provider, permission or agent",
            );
        }
    }
    let built = timeline_value(&state, limit, area.as_deref(), true).and_then(|mut payload| {
        payload["summary"] = summary_value(&state)?;
        Ok(payload)
    });
    match built {
        Ok(payload) => Json(payload).into_response(),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

/// `GET /api/diagnostics/log` — the process log, which is the one record that
/// exists when nothing was committed to the database.
///
/// It reads the bounded in-memory ring, not the file: the ring is already
/// redacted and clipped exactly as the file is, and reading it cannot fail on a
/// locked or missing file.
async fn log_route(AxumQuery(params): AxumQuery<HashMap<String, String>>) -> Response {
    let limit = params
        .get("limit")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    let records = crate::oplog::tail(limit);
    Json(json!({
        "ok": true,
        "path": crate::oplog::path(),
        "count": records.len(),
        "records": records,
    }))
    .into_response()
}

/// `GET /api/diagnostics/bundle` — a redacted JSON file for a bug report.
async fn bundle_route(State(state): State<AppState>) -> Response {
    match bundle_value(&state) {
        Ok(bundle) => Json(bundle).into_response(),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

/// Both routes read this machine's own records, so they keep the same loopback
/// `Host`/`Origin` guard every other local read uses.
pub(crate) fn router(port: u16) -> Router<AppState> {
    Router::new()
        .route("/api/diagnostics", get(timeline_route))
        .route("/api/diagnostics/bundle", get(bundle_route))
        .route("/api/diagnostics/log", get(log_route))
        .route_layer(axum::middleware::from_fn_with_state(
            crate::workspace::LocalOriginGuard::new(port),
            crate::workspace::guard_local_origin,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn fixture(name: &str) -> AppState {
        let dir =
            std::env::temp_dir().join(format!("saarthi-diagnostics-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("fixture dir");
        let state = AppState {
            config_path: dir.join("config.json"),
            database_path: dir.join("studio.db"),
            static_dir: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static"),
            workspace: crate::workspace::WorkspaceStore::load(dir.join("workspace.json")),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("test client"),
            terminal: crate::terminal::hub(),
            events: crate::events::Hub::new(),
            models_cache: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        };
        crate::initialize_database(&state.database_path).expect("database");
        state
    }

    fn seed_provider(state: &AppState, at: f64, status: &str, error: &str) {
        let conn = open_db(&state.database_path).expect("open");
        conn.execute(
            "INSERT INTO request_log(created_at,conversation_id,model,provider,status,latency_ms,total_tokens,error)
             VALUES(?1,'conv-1','model-a','router',?2,120,42,?3)",
            params![at, status, error],
        )
        .expect("seed request");
    }

    fn seed_permission(state: &AppState, at: f64, decision: &str) {
        let conn = open_db(&state.database_path).expect("open");
        conn.execute(
            "INSERT INTO permission_audit(created_at,action,target,decision,scope,project,detail)
             VALUES(?1,'fs_write','src/main.rs',?2,'project','proj-1','refused by a saved rule')",
            params![at, decision],
        )
        .expect("seed audit");
    }

    fn seed_run(state: &AppState, id: &str, at: f64, status: &str, error: &str) {
        let conn = open_db(&state.database_path).expect("open");
        conn.execute(
            "INSERT INTO agent_runs(id,project_id,goal,status,allow_writes,allow_commands,
                                    auto_approve,max_steps,created_at,updated_at,error,tokens)
             VALUES(?1,'proj-1','plan for the secret launch',?2,1,0,0,10,?3,?3,?4,17)",
            params![id, status, at, error],
        )
        .expect("seed run");
        conn.execute(
            "INSERT INTO agent_steps(run_id,seq,action,summary,created_at)
             VALUES(?1,1,'write-conflict','src/main.rs is claimed by another run',?2)",
            params![id, at],
        )
        .expect("seed step");
    }

    #[test]
    fn redaction_strips_credentials_and_keeps_prose() {
        let key = redact("the request failed with sk-live-abcdefghijklmnop returned");
        assert!(!key.contains("sk-live"), "a provider key leaked: {key}");
        assert!(key.contains("[redacted]"));
        assert!(key.contains("request failed"));

        let bearer = redact("Authorization: Bearer abc123 in the header");
        assert!(
            !bearer.contains("abc123"),
            "a bearer token leaked: {bearer}"
        );
        assert!(bearer.contains("Bearer [redacted]"));

        let pair = redact("GET http://localhost:3001/v1?api_key=supersecret now");
        assert!(!pair.contains("supersecret"), "a query key leaked: {pair}");
        assert!(pair.contains("[redacted]"));

        // A long opaque run is treated as a credential, digest or not.
        let digest = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        assert!(!redact(digest).contains(digest));

        // Ordinary prose is untouched, including numbers and short words.
        let prose = "the build failed on line 12 after 3 attempts";
        assert_eq!(redact(prose), prose);
    }

    #[test]
    fn timeline_merges_areas_in_time_order_and_honours_the_limit() {
        let state = fixture("timeline");
        let now = now_seconds();
        seed_provider(&state, now - 30.0, "ok", "");
        seed_permission(&state, now - 20.0, "deny");
        seed_run(&state, "run-1", now - 10.0, "failed", "the model gave up");

        let payload = timeline_value(&state, 50, None, true).expect("timeline");
        let events = payload["events"].as_array().expect("events");
        let areas: Vec<&str> = events
            .iter()
            .filter_map(|event| event["area"].as_str())
            .collect();
        assert_eq!(areas, vec!["provider", "permission", "agent", "agent"]);
        assert_eq!(events[0]["level"], "info");
        assert_eq!(events[1]["level"], "warn");
        assert_eq!(
            events[3]["level"], "warn",
            "a refused write is not a failure"
        );
        assert_eq!(events[2]["level"], "error");
        assert_eq!(events[2]["correlation"], "run-1");
        assert_eq!(events[2]["event"], "run failed");
        // The goal is the user's prompt: on screen, before a share decision.
        assert!(events[2]["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("plan for the secret launch"));

        let newest = timeline_value(&state, 2, None, true).expect("timeline");
        assert_eq!(newest["events"].as_array().expect("events").len(), 2);

        let only_agent = timeline_value(&state, 50, Some("agent"), true).expect("timeline");
        assert_eq!(only_agent["events"].as_array().expect("events").len(), 2);

        let clamped = timeline_value(&state, 0, None, true).expect("timeline");
        assert_eq!(clamped["limit"], 1);
    }

    #[test]
    fn timeline_redacts_what_the_provider_returned() {
        let state = fixture("redact-events");
        seed_provider(
            &state,
            now_seconds(),
            "error",
            "401 for Bearer sk-live-abcdefghijklmnop",
        );
        let payload = timeline_value(&state, 10, None, true).expect("timeline");
        let detail = payload["events"][0]["detail"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(
            !detail.contains("sk-live"),
            "a key reached the timeline: {detail}"
        );
        assert!(detail.contains("401"));
    }

    #[test]
    fn summary_counts_every_source_from_the_rows_themselves() {
        let state = fixture("summary");
        let now = now_seconds();
        seed_provider(&state, now - 5.0, "ok", "");
        seed_provider(&state, now - 4.0, "error", "502");
        seed_permission(&state, now - 3.0, "deny");
        seed_permission(&state, now - 2.0, "allow");
        seed_run(&state, "run-ok", now - 1.0, "succeeded", "");

        let summary = summary_value(&state).expect("summary");
        assert_eq!(summary["provider"]["requests"], 2);
        assert_eq!(summary["provider"]["errors"], 1);
        assert_eq!(summary["permission_decisions"]["deny"], 1);
        assert_eq!(summary["permission_decisions"]["allow"], 1);
        assert_eq!(summary["agent"]["by_status"]["succeeded"], 1);
        assert_eq!(summary["agent"]["live"], 0);

        seed_run(&state, "run-live", now, "running", "");
        let summary = summary_value(&state).expect("summary");
        assert_eq!(summary["agent"]["live"], 1);
    }

    #[test]
    fn bundle_carries_counts_and_no_secret_or_prompt() {
        let state = fixture("bundle");
        let now = now_seconds();
        seed_provider(&state, now, "ok", "");
        seed_run(&state, "run-1", now, "failed", "Bearer abc123 rejected");

        let mut cfg = crate::Config::defaults();
        cfg.api_key = "sk-live-router-secret".to_string();
        cfg.openai_api_key = "sk-openai-secret-value".to_string();
        cfg.base_url = "http://localhost:3001/v1?api_key=url-secret-value".to_string();
        std::fs::write(
            &state.config_path,
            serde_json::to_vec(&cfg).expect("serialise config"),
        )
        .expect("write config");

        let bundle = bundle_value(&state).expect("bundle");
        let text = serde_json::to_string(&bundle).expect("serialise bundle");
        assert!(!text.contains("sk-live-router-secret"), "a key leaked");
        assert!(!text.contains("sk-openai-secret-value"), "a key leaked");
        assert!(!text.contains("url-secret-value"), "a key in a URL leaked");
        assert!(
            !text.contains("plan for the secret launch"),
            "a prompt leaked"
        );
        assert!(!text.contains("abc123"), "a bearer token leaked");
        assert_eq!(bundle["config"]["credentials"]["router_key_saved"], true);
        assert_eq!(bundle["config"]["credentials"]["openai_key_saved"], true);
        assert_eq!(bundle["app"]["version"], env!("CARGO_PKG_VERSION"));
        assert!(bundle["events"].as_array().expect("events").len() >= 2);
        assert!(!bundle["notes"].as_array().expect("notes").is_empty());
    }
}
