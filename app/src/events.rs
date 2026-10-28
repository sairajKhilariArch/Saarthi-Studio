//! Live thread mirroring.
//!
//! A small in-process broadcast bus publishes `conversation-changed` events
//! whenever a conversation is created, deleted, gains a message, starts or
//! finishes a stream, or fails. The `/api/events` handler streams those events
//! as SSE so a second browser tab (or the open web UI) can refetch the affected
//! thread instead of requiring a manual reload. Events carry no content: clients
//! swap IDs for fresh reads, so nothing leaks across threads and the payload
//! stays tiny.

use axum::response::sse::{Event, KeepAlive, Sse};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Fire-and-forget mirror bus. A paused SSE client or closed stream simply
/// drops events — mirroring is a live refresh hint, never a durable record.
#[derive(Clone)]
pub(crate) struct Hub {
    sender: Arc<broadcast::Sender<String>>,
}

const CAPACITY: usize = 256;

impl Hub {
    pub(crate) fn new() -> Self {
        let (sender, _) = broadcast::channel(CAPACITY);
        Self {
            sender: Arc::new(sender),
        }
    }

    pub(crate) fn publish(&self, event: Value) {
        let _ = self.sender.send(event.to_string());
    }

    pub(crate) fn subscribe(
        &self,
    ) -> impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>> {
        use futures_util::StreamExt;
        tokio_stream::wrappers::BroadcastStream::new(self.sender.subscribe())
            .filter_map(|item| async move {
                match item {
                    Ok(payload) => serde_json::from_str::<Value>(&payload)
                        .ok()
                        .and_then(|event| {
                            Event::default()
                                .event("conversation-changed")
                                .json_data(event)
                                .ok()
                        }),
                    Err(_) => None, // Lagged subscriber: skip Lagged and resync.
                }
            })
            .map(Ok)
    }
}

/// One refresh hint for every subscriber.
pub(crate) fn notify(hub: &Hub, conversation_id: &str, reason: &str) {
    hub.publish(json!({
        "type": "conversation-changed",
        "conversation_id": conversation_id,
        "reason": reason,
        "at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
    }));
}

/// SSE endpoint. Heartbeats keep proxies from closing idle connections.
pub(crate) async fn events(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(state.events.subscribe()).keep_alive(KeepAlive::default())
}
