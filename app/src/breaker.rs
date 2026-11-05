//! Circuit breaker for model attempts.
//!
//! A provider that has failed three times in a row does not deserve a fourth
//! request on the next turn: each try costs a full timeout and hides the outage
//! behind a slow reply. The breaker remembers that per (workspace, model) and
//! refuses the call until a cooldown has passed, then lets exactly one probe
//! through — the half-open state. A successful probe closes the circuit, a failed
//! one re-opens it for another cooldown.
//!
//! This state is process-local on purpose: it describes a live condition, not
//! history. The durable record of what actually happened is `request_log`, which
//! the health panel reads; nothing here is presented as history.
//!
//! A 4xx refusal never reaches the breaker — the caller only reports failures the
//! fallback chain already treats as "provider unavailable" (429, 5xx, timeout,
//! transport). Breaking a circuit because the user asked for an unknown model
//! would hide the real mistake behind a cooldown.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Consecutive unavailable responses before the circuit opens.
const FAILURE_THRESHOLD: u32 = 3;
/// How long an open circuit refuses calls before allowing one probe.
const COOLDOWN: Duration = Duration::from_secs(45);

#[derive(Clone, Debug, Default)]
struct Circuit {
    consecutive: u32,
    /// When the circuit opened, as a monotonic mark (not wall-clock: a clock
    /// change must not extend or shorten an outage).
    opened_at: Option<Instant>,
    last_error: String,
    opens: u32,
}

fn circuits() -> &'static Mutex<HashMap<String, Circuit>> {
    static CIRCUITS: OnceLock<Mutex<HashMap<String, Circuit>>> = OnceLock::new();
    CIRCUITS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(scope: &str, model: &str) -> String {
    format!("{scope}\u{1f}{model}")
}

/// What the breaker currently thinks about one model.
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub model: String,
    /// `closed` (healthy), `open` (refusing) or `half-open` (next call is a probe).
    pub state: &'static str,
    pub consecutive_failures: u32,
    pub opens: u32,
    /// Seconds until the circuit will accept a probe; zero when it is not open.
    pub retry_in_seconds: f64,
    pub last_error: String,
}

/// Whether `model` may be asked right now.
pub(crate) fn allow(scope: &str, model: &str) -> bool {
    let Ok(map) = circuits().lock() else {
        // A poisoned lock must not take the app down with it: allow the call and
        // let the provider answer for itself.
        return true;
    };
    match map.get(&key(scope, model)) {
        Some(circuit) => match circuit.opened_at {
            None => true,
            Some(opened) => opened.elapsed() >= COOLDOWN,
        },
        None => true,
    }
}

/// A model answered: the circuit closes and its failure streak ends.
pub(crate) fn success(scope: &str, model: &str) {
    let Ok(mut map) = circuits().lock() else {
        return;
    };
    if let Some(circuit) = map.get_mut(&key(scope, model)) {
        circuit.consecutive = 0;
        circuit.opened_at = None;
    }
}

/// A model was unavailable. Three in a row opens the circuit; a failed probe
/// re-opens it with a fresh cooldown.
pub(crate) fn failure(scope: &str, model: &str, error: &str) {
    let Ok(mut map) = circuits().lock() else {
        return;
    };
    let circuit = map.entry(key(scope, model)).or_default();
    circuit.consecutive = circuit.consecutive.saturating_add(1);
    circuit.last_error = error.chars().take(400).collect();
    if circuit.consecutive >= FAILURE_THRESHOLD {
        if circuit.opened_at.is_none() {
            circuit.opens = circuit.opens.saturating_add(1);
        }
        circuit.opened_at = Some(Instant::now());
    }
}

/// The breaker's view for the health panel, most-impaired first.
pub(crate) fn snapshot(scope: &str) -> Vec<Snapshot> {
    let Ok(map) = circuits().lock() else {
        return Vec::new();
    };
    let prefix = format!("{scope}\u{1f}");
    let mut rows: Vec<Snapshot> = map
        .iter()
        .filter_map(|(key, circuit)| {
            let model = key.strip_prefix(&prefix)?.to_string();
            let (state, retry_in_seconds) = match circuit.opened_at {
                None => ("closed", 0.0),
                Some(opened) => {
                    let elapsed = opened.elapsed();
                    if elapsed >= COOLDOWN {
                        ("half-open", 0.0)
                    } else {
                        ("open", (COOLDOWN - elapsed).as_secs_f64())
                    }
                }
            };
            Some(Snapshot {
                model,
                state,
                consecutive_failures: circuit.consecutive,
                opens: circuit.opens,
                retry_in_seconds,
                last_error: circuit.last_error.clone(),
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        let rank = |state: &str| match state {
            "open" => 0,
            "half-open" => 1,
            _ => 2,
        };
        rank(a.state)
            .cmp(&rank(b.state))
            .then_with(|| a.model.cmp(&b.model))
    });
    rows
}

/// The refusal when every candidate in a chain is cooling down. Names what is
/// open and for how long, because "unavailable" with no explanation reads as a
/// bug.
pub(crate) fn all_open_message(scope: &str, chain: &[String]) -> String {
    let open: Vec<String> = chain
        .iter()
        .map(|model| {
            let retry = snapshot(scope)
                .into_iter()
                .find(|row| &row.model == model)
                .map(|row| row.retry_in_seconds)
                .unwrap_or(0.0);
            format!("{model} (retry in {}s)", retry.ceil() as i64)
        })
        .collect();
    format!(
        "Every model in the chain is cooling down after repeated failures: {}. Nothing was sent; try again shortly or raise the fallback list.",
        open.join(", ")
    )
}

/// Clear every circuit for a scope. Only tests use this: one scenario must not
/// leak a cooldown into the next.
#[cfg(test)]
pub(crate) fn reset(scope: &str) {
    if let Ok(mut map) = circuits().lock() {
        let prefix = format!("{scope}\u{1f}");
        map.retain(|key, _| !key.starts_with(&prefix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_model_is_allowed_and_reports_closed() {
        let scope = "test-fresh";
        reset(scope);
        assert!(allow(scope, "model-a"));
        assert!(snapshot(scope).is_empty(), "no history, nothing to report");
    }

    #[test]
    fn three_unavailable_answers_open_the_circuit_and_refuse_calls() {
        let scope = "test-open";
        reset(scope);
        failure(scope, "model-a", "HTTP 503");
        failure(scope, "model-a", "HTTP 503");
        assert!(allow(scope, "model-a"), "still under the threshold");
        failure(scope, "model-a", "HTTP 503");
        assert!(!allow(scope, "model-a"), "three in a row opens it");

        let rows = snapshot(scope);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, "open");
        assert_eq!(rows[0].consecutive_failures, 3);
        assert_eq!(rows[0].opens, 1);
        assert!(rows[0].retry_in_seconds > 0.0);
        assert_eq!(rows[0].last_error, "HTTP 503");
    }

    #[test]
    fn a_success_closes_the_circuit_and_clears_the_streak() {
        let scope = "test-success";
        reset(scope);
        failure(scope, "model-a", "timeout");
        failure(scope, "model-a", "timeout");
        failure(scope, "model-a", "timeout");
        assert!(!allow(scope, "model-a"));
        success(scope, "model-a");
        assert!(allow(scope, "model-a"));
        let rows = snapshot(scope);
        assert_eq!(rows[0].state, "closed");
        assert_eq!(rows[0].consecutive_failures, 0);
        assert_eq!(
            rows[0].opens, 1,
            "the outage still happened, and is recorded"
        );
    }

    #[test]
    fn scopes_do_not_see_each_other() {
        let one = "test-scope-one";
        let two = "test-scope-two";
        reset(one);
        reset(two);
        failure(one, "model-a", "503");
        failure(one, "model-a", "503");
        failure(one, "model-a", "503");
        assert!(!allow(one, "model-a"));
        assert!(
            allow(two, "model-a"),
            "a different workspace's circuit stays closed"
        );
    }

    #[test]
    fn the_refusal_names_every_cooling_down_model() {
        let scope = "test-message";
        reset(scope);
        for _ in 0..3 {
            failure(scope, "primary-model", "HTTP 503");
        }
        let chain = vec!["primary-model".to_string(), "backup-model".to_string()];
        let message = all_open_message(scope, &chain);
        assert!(message.contains("primary-model (retry in"), "{message}");
        // A model with no failures is named without a misleading cooldown.
        assert!(message.contains("backup-model (retry in 0s)"), "{message}");
    }
}
