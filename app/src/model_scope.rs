//! Model scope: which model a turn uses, and which models it may fall through
//! to when that one is unavailable.
//!
//! Precedence mirrors Thinking and is fixed: task > agent > project > global >
//! `auto`. Nothing is inferred from a model's name or price — an empty scope
//! inherits from the next one, and `auto` is only what remains when every scope
//! is unset.
//!
//! The fallback chain is ordered, deduplicated and bounded. It is never used for
//! a failure that says the *request* was wrong (a 4xx refusal would be refused
//! identically by every model), only for ones that say the *provider* was
//! unavailable: rate limits, server errors, timeouts and transport failures.

use crate::thinking::Scope;

/// How many fallbacks a chain may carry. A short chain bounds both the worst
/// case latency of one turn and the number of provider calls it can cost.
pub(crate) const MAX_FALLBACKS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub(crate) model: String,
    pub(crate) scope: Scope,
}

/// First configured scope wins, exactly as the specification orders it. The
/// task scope is the composer's pick or a run's own model.
pub(crate) fn resolve(
    task: Option<&str>,
    agent: Option<&str>,
    project: Option<&str>,
    global: Option<&str>,
) -> Resolved {
    for (value, scope) in [
        (task, Scope::Message),
        (agent, Scope::Agent),
        (project, Scope::Project),
        (global, Scope::Global),
    ] {
        if let Some(model) = value.map(str::trim).filter(|model| !model.is_empty()) {
            return Resolved {
                model: model.to_string(),
                scope,
            };
        }
    }
    Resolved {
        model: "auto".to_string(),
        scope: Scope::Global,
    }
}

/// The configured fallbacks, trimmed, deduplicated case-insensitively, and
/// capped. A `local:` entry is dropped: local routing is decided by local mode,
/// never by a fallback, and sending `local:x` to the router would only produce a
/// confusing "unknown model" refusal.
pub(crate) fn normalised(fallbacks: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for candidate in fallbacks {
        let candidate = candidate.trim();
        if candidate.is_empty() || candidate.to_ascii_lowercase().starts_with("local:") {
            continue;
        }
        if out.iter().any(|seen| seen.eq_ignore_ascii_case(candidate)) {
            continue;
        }
        out.push(candidate.to_string());
        if out.len() == MAX_FALLBACKS {
            break;
        }
    }
    out
}

/// The models to try, in order: the chosen one first, then the configured
/// fallbacks that are not the chosen one already.
pub(crate) fn chain(chosen: &str, fallbacks: &[String]) -> Vec<String> {
    let chosen = chosen.trim();
    let chosen = if chosen.is_empty() { "auto" } else { chosen };
    let mut chain = vec![chosen.to_string()];
    for candidate in normalised(fallbacks) {
        if chain
            .iter()
            .any(|seen| seen.eq_ignore_ascii_case(&candidate))
        {
            continue;
        }
        chain.push(candidate);
        // `normalised` is already capped; the chosen model may add one more.
        if chain.len() > MAX_FALLBACKS {
            break;
        }
    }
    chain
}

/// Whether another model is worth trying after this failure. A rejected request
/// (400/401/403/404, a missing key, a Thinking refusal) fails the same way
/// everywhere; a rate limit, a server error, a timeout or an unreachable
/// provider says nothing about the next candidate.
pub(crate) fn retryable(error: &str) -> bool {
    let lowered = error.to_ascii_lowercase();
    if let Some(status) = http_status(&lowered) {
        return status == 429 || (500..600).contains(&status);
    }
    const TRANSPORT: [&str; 7] = [
        "timed out",
        "timeout",
        "could not reach",
        "unreachable",
        "error sending request",
        "connection reset",
        "connection refused",
    ];
    TRANSPORT.iter().any(|needle| lowered.contains(needle))
}

/// The status in a message like "Router returned HTTP 503: ..." or "OpenAI
/// Platform rate limit (HTTP 429)". `None` for messages that carry no status.
fn http_status(lowered: &str) -> Option<u16> {
    let start = lowered.find("http ")? + "http ".len();
    let digits: String = lowered[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_specific_scope_wins_and_reports_itself() {
        let message = Some("composer-pick");
        let agent = Some("agent-default");
        let project = Some("project-default");
        let global = Some("global-default");
        assert_eq!(
            resolve(message, agent, project, global).model,
            "composer-pick"
        );
        assert_eq!(
            resolve(message, agent, project, global).scope,
            Scope::Message
        );

        let resolved = resolve(None, agent, project, global);
        assert_eq!(resolved.model, "agent-default");
        assert_eq!(resolved.scope, Scope::Agent);

        let resolved = resolve(None, None, project, global);
        assert_eq!(resolved.model, "project-default");
        assert_eq!(resolved.scope, Scope::Project);

        let resolved = resolve(None, None, None, global);
        assert_eq!(resolved.model, "global-default");
        assert_eq!(resolved.scope, Scope::Global);
    }

    #[test]
    fn a_blank_scope_inherits_instead_of_selecting_an_empty_model() {
        assert_eq!(
            resolve(Some("   "), Some(""), Some("  "), Some("global-x")).model,
            "global-x"
        );
        // Nothing configured anywhere is the router's `auto`, never an empty id.
        let resolved = resolve(None, None, None, None);
        assert_eq!(resolved.model, "auto");
        assert_eq!(resolved.scope, Scope::Global);
    }

    #[test]
    fn the_chain_keeps_order_drops_duplicates_and_skips_local_picks() {
        let fallbacks = vec![
            "model-b".to_string(),
            "MODEL-B".to_string(),
            "".to_string(),
            "local:llama3".to_string(),
            "model-c".to_string(),
        ];
        assert_eq!(
            chain("model-a", &fallbacks),
            ["model-a", "model-b", "model-c"]
        );
        // The chosen model is never tried twice, whatever its case.
        assert_eq!(chain("Model-B", &fallbacks), ["Model-B", "model-c"]);
        // An empty pick still starts from `auto`.
        assert_eq!(chain("", &fallbacks)[0], "auto");
    }

    #[test]
    fn the_chain_is_bounded() {
        let many: Vec<String> = (0..20).map(|index| format!("model-{index}")).collect();
        let built = chain("chosen", &many);
        assert_eq!(built.len(), MAX_FALLBACKS + 1);
        assert_eq!(normalised(&many).len(), MAX_FALLBACKS);
    }

    #[test]
    fn only_an_unavailable_provider_is_worth_the_next_model() {
        // Unavailable: another model may answer.
        for error in [
            "Router returned HTTP 429: slow down",
            "Router returned HTTP 503: upstream unavailable",
            "OpenAI Platform rate limit (HTTP 429). Wait, then retry.",
            "OpenAI Platform returned HTTP 500: boom",
            "Could not reach router at http://localhost:3001/v1: error sending request",
            "The OpenAI Platform is unreachable: connection reset",
            "The operation timed out",
        ] {
            assert!(retryable(error), "{error}");
        }
        // Rejected: every model would refuse the same request.
        for error in [
            "Router returned HTTP 400: unknown field",
            "Router returned HTTP 401: no key",
            "Router returned HTTP 404: model not found",
            "Add your router's unified API key in Settings first.",
            "gpt-x does not advertise reasoning, so it cannot run with Thinking high.",
        ] {
            assert!(!retryable(error), "{error}");
        }
    }
}
