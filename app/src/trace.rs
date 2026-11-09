//! A correlation id for the operations that are not agent runs.
//!
//! An agent run already has a key: the run id, which ties its steps, tokens and
//! files together. A terminal open, a Git stage, a settings save or a router
//! install has no such thing — the audit row says *what* was decided, not which
//! HTTP call asked for it, so two refusals a second apart cannot be told apart in
//! the timeline.
//!
//! This module gives every local API request an id, keeps it in a task-local for
//! the duration of the handler, echoes it in the `x-request-id` response header,
//! and exposes it through [`current`] so the audit writer can store it. A caller
//! that already has an id may send one; it is accepted only if it is short and
//! made of boring characters, because it ends up in a database and on screen.
//!
//! Deliberately *not* a tracing span: no timing, no parent/child tree, no
//! sampling. The id is a label the panels and the log can quote, nothing more.

use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use std::sync::atomic::{AtomicU64, Ordering};

/// Both the accepted request header and the echoed response header.
pub(crate) const HEADER: &str = "x-request-id";
/// Shortest and longest id accepted from a client. Long enough for a UUID,
/// short enough that it cannot be used to push text into a panel.
const MIN_LEN: usize = 8;
const MAX_LEN: usize = 64;

static COUNTER: AtomicU64 = AtomicU64::new(0);

tokio::task_local! {
    /// The id of the request the current task is serving. Empty outside one.
    static REQUEST: String;
}

/// The id of the request being served, or `""` when this task is not serving
/// one — a background sweep, a queue drain, a unit test.
pub(crate) fn current() -> String {
    REQUEST.try_with(|id| id.clone()).unwrap_or_default()
}

/// A client-supplied id, if it is safe to store and show.
fn accept(candidate: &str) -> Option<String> {
    let trimmed = candidate.trim();
    let acceptable = (MIN_LEN..=MAX_LEN).contains(&trimmed.len())
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    acceptable.then(|| trimmed.to_string())
}

/// Milliseconds since the epoch plus a counter, so two requests in the same
/// millisecond still differ and a restart cannot reuse an id it already wrote.
fn mint() -> String {
    let stamp = (crate::now_seconds() * 1000.0) as u64;
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{stamp:x}-{sequence:x}")
}

/// Axum middleware: assign the id, run the handler inside it, echo it back.
pub(crate) async fn assign(request: Request, next: Next) -> Response {
    let id = request
        .headers()
        .get(HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(accept)
        .unwrap_or_else(mint);
    let echoed = id.clone();
    let mut response = REQUEST.scope(id, next.run(request)).await;
    // The header is what lets a person paste "look at request X" into a report
    // and have it match a row.
    if let Ok(value) = HeaderValue::from_str(&echoed) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(HEADER), value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_distinct_ids() {
        let first = mint();
        let second = mint();
        assert_ne!(first, second);
        assert!(accept(&first).is_some(), "a minted id must be storable");
    }

    #[test]
    fn accepts_a_reasonable_caller_id() {
        assert_eq!(
            accept("0f8b1c4a-7a4e-4f47-9f0e-5b1c2d3e4f50").as_deref(),
            Some("0f8b1c4a-7a4e-4f47-9f0e-5b1c2d3e4f50")
        );
        assert_eq!(accept("  build-2026-11-09  ").as_deref(), Some("build-2026-11-09"));
    }

    #[test]
    fn rejects_ids_that_would_reach_a_panel_as_text() {
        assert_eq!(accept("short"), None, "too short to be unique");
        assert_eq!(accept(&"a".repeat(MAX_LEN + 1)), None, "too long");
        assert_eq!(accept("has space"), None);
        assert_eq!(accept("<script>alert(1)</script>"), None);
        assert_eq!(accept("semi;colon"), None);
    }

    #[test]
    fn current_is_empty_outside_a_request() {
        assert_eq!(current(), "");
    }

    #[tokio::test]
    async fn current_reads_the_scoped_id() {
        let seen = REQUEST
            .scope("req-abc-123".to_string(), async { current() })
            .await;
        assert_eq!(seen, "req-abc-123");
        assert_eq!(current(), "", "the scope must not leak out");
    }
}
