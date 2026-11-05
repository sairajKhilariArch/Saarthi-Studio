//! Provider health for the usage page.
//!
//! Two different questions, kept separate on purpose:
//!
//! * what this process is refusing right now — the circuit breaker's live state,
//! * what actually happened — `request_log` over the last 24 hours.
//!
//! The panel shows both, because "open" with no history is a different situation
//! from "healthy but failing one time in three".

use crate::{breaker, breaker_scope, now_seconds, open_db, AppState};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Value};

/// Same window the budget uses; two different notions of "recently" in one panel
/// would just confuse the reader.
const WINDOW_SECONDS: f64 = 86_400.0;

/// `GET /api/health/providers`: per-model requests, failures, latency and the
/// breaker's state. No secrets and no prompts: only counters, a latency and the
/// last error string the provider itself returned.
pub(crate) async fn providers(State(state): State<AppState>) -> axum::response::Response {
    let query = (|| -> rusqlite::Result<Vec<Value>> {
        let conn = open_db(&state.database_path)?;
        let since = now_seconds() - WINDOW_SECONDS;
        let mut stmt = conn.prepare(
            "SELECT COALESCE(model,'(unknown)') AS model, COUNT(*), SUM(status='ok'),
                    COALESCE(AVG(latency_ms),0),
                    (SELECT error FROM request_log e
                       WHERE COALESCE(e.model,'(unknown)')=COALESCE(r.model,'(unknown)')
                         AND e.error IS NOT NULL AND e.error<>''
                       ORDER BY e.id DESC LIMIT 1)
             FROM request_log r WHERE created_at>=?1 GROUP BY model ORDER BY COUNT(*) DESC LIMIT 50",
        )?;
        let rows = stmt.query_map([since], |row| {
            Ok(json!({
                "model": row.get::<_, String>(0)?,
                "requests": row.get::<_, i64>(1)?,
                "ok": row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                "avg_latency_ms": row.get::<_, f64>(3)?,
                "last_error": row.get::<_, Option<String>>(4)?,
            }))
        })?;
        rows.collect()
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
