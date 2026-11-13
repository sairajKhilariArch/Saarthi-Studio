//! Provider health for the usage page.
//!
//! Two different questions, kept separate on purpose:
//!
//! * what this process is refusing right now — the circuit breaker's live state,
//! * what actually happened — `request_log` over the last 24 hours.
//!
//! The panel shows both, because "open" with no history is a different situation
//! from "healthy but failing one time in three". A rate-limit count is read from
//! the provider's own stored error, never inferred from a latency or a status.

use crate::{breaker, breaker_scope, now_seconds, open_db, AppState};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use rusqlite::Connection;
use serde_json::{json, Value};

/// Same window the budget uses; two different notions of "recently" in one panel
/// would just confuse the reader.
const WINDOW_SECONDS: f64 = 86_400.0;

/// A provider's rate-limit refusal, as far as a stored sentence can tell. Both
/// the numeric status and the words providers actually use are accepted; nothing
/// is guessed from timing or from a generic failure.
const RATE_LIMITED: &str =
    "(LOWER(COALESCE(r.error,'')) LIKE '%429%' OR LOWER(COALESCE(r.error,'')) LIKE '%rate limit%')";

fn provider_sql() -> String {
    format!(
        "SELECT COALESCE(model,'(unknown)') AS model, COUNT(*), SUM(status='ok'),
                COALESCE(AVG(latency_ms),0),
                (SELECT error FROM request_log e
                   WHERE COALESCE(e.model,'(unknown)')=COALESCE(r.model,'(unknown)')
                     AND e.error IS NOT NULL AND e.error<>''
                   ORDER BY e.id DESC LIMIT 1),
                COALESCE(SUM(CASE WHEN {RATE_LIMITED} THEN 1 ELSE 0 END),0),
                MAX(CASE WHEN {RATE_LIMITED} THEN created_at END)
         FROM request_log r WHERE created_at>=?1 GROUP BY model ORDER BY COUNT(*) DESC LIMIT 50"
    )
}

/// Per-model counters. A separate function so the SQL is exercised against a
/// real in-memory table rather than only through the route.
fn provider_rows(conn: &Connection, since: f64) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare(&provider_sql())?;
    let rows = stmt.query_map([since], |row| {
        Ok(json!({
            "model": row.get::<_, String>(0)?,
            "requests": row.get::<_, i64>(1)?,
            "ok": row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            "avg_latency_ms": row.get::<_, f64>(3)?,
            "last_error": row.get::<_, Option<String>>(4)?,
            "rate_limited": row.get::<_, Option<i64>>(5)?.unwrap_or(0),
            "last_rate_limit_at": row.get::<_, Option<f64>>(6)?,
        }))
    })?;
    rows.collect()
}

/// `GET /api/health/providers`: per-model requests, failures, latency, rate-limit
/// refusals and the breaker's state. No secrets and no prompts: only counters, a
/// latency and the last error string the provider itself returned.
pub(crate) async fn providers(State(state): State<AppState>) -> axum::response::Response {
    let query = (|| -> rusqlite::Result<Vec<Value>> {
        let conn = open_db(&state.database_path)?;
        provider_rows(&conn, now_seconds() - WINDOW_SECONDS)
    })();

    let mut rows = match query {
        Ok(rows) => rows,
        Err(error) => {
            return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
    };
    let scope = breaker_scope(&state);
    let circuits = breaker::snapshot(&scope);
    let open = circuits.iter().filter(|row| row.state == "open").count();

    // Merge the two views. A model can be in the breaker with no log rows yet
    // (every attempt failed before anything was recorded) or logged without ever
    // reaching the breaker (a request that succeeded once).
    for circuit in &circuits {
        let breaker_view = json!({
            "state": circuit.state,
            "consecutive_failures": circuit.consecutive_failures,
            "opens": circuit.opens,
            "retry_in_seconds": circuit.retry_in_seconds,
            "breaker_error": circuit.last_error,
        });
        match rows
            .iter_mut()
            .find(|row| row["model"].as_str() == Some(circuit.model.as_str()))
        {
            Some(row) => row["breaker"] = breaker_view,
            None => rows.push(json!({
                "model": circuit.model,
                "requests": 0,
                "ok": 0,
                "avg_latency_ms": 0.0,
                "last_error": Value::Null,
                "rate_limited": 0,
                "last_rate_limit_at": Value::Null,
                "breaker": breaker_view,
            })),
        }
    }

    Json(json!({
        "ok": true,
        "window_hours": 24,
        "circuits_open": open,
        // Said in the payload so the UI does not have to invent the rule.
        "rule": "three consecutive unavailable answers (429, 5xx, timeout, unreachable) open a 45-second circuit; one probe is then allowed through",
        "models": rows,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table shaped like the migrated `request_log`, without the rest of the app.
    fn log_table() -> Connection {
        let conn = Connection::open_in_memory().expect("db");
        conn.execute_batch(
            "CREATE TABLE request_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT, created_at REAL NOT NULL,
                conversation_id TEXT, model TEXT, provider TEXT, status TEXT NOT NULL,
                latency_ms INTEGER, prompt_tokens INTEGER, completion_tokens INTEGER,
                total_tokens INTEGER, fallback_attempts TEXT, error TEXT
             );",
        )
        .expect("schema");
        conn
    }

    fn insert(conn: &Connection, at: f64, model: &str, status: &str, error: Option<&str>) {
        conn.execute(
            "INSERT INTO request_log(created_at,model,status,latency_ms,error) \
             VALUES(?1,?2,?3,100,?4)",
            rusqlite::params![at, model, status, error],
        )
        .expect("row");
    }

    #[test]
    fn rate_limits_are_counted_from_the_provider_sentence() {
        let conn = log_table();
        insert(&conn, 10.0, "alpha", "error", Some("429 Too Many Requests"));
        insert(&conn, 20.0, "alpha", "ok", None);
        insert(&conn, 30.0, "beta", "error", Some("502 Bad Gateway"));
        insert(&conn, 40.0, "gamma", "error", Some("Rate limit exceeded"));
        insert(&conn, 1.0, "old", "error", Some("429 outside the window"));

        let rows = provider_rows(&conn, 5.0).expect("query");
        let by_model = |name: &str| {
            rows.iter()
                .find(|row| row["model"] == name)
                .unwrap_or_else(|| panic!("{name} missing"))
        };
        let alpha = by_model("alpha");
        assert_eq!(alpha["requests"], 2);
        assert_eq!(alpha["ok"], 1);
        assert_eq!(alpha["rate_limited"], 1);
        assert_eq!(alpha["last_rate_limit_at"], 10.0);
        assert_eq!(by_model("beta")["rate_limited"], 0);
        assert_eq!(by_model("gamma")["rate_limited"], 1);
        // The old row is outside the window, so it is neither a request nor a limit.
        assert!(rows.iter().all(|row| row["model"] != "old"));
    }
}
