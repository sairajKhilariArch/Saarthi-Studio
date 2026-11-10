//! Native OpenAI Platform provider — the Responses API, not the OpenAI-compatible
//! preset.
//!
//! The preset in Settings points at whatever gateway the user runs; this module
//! talks to the platform's own `/responses` endpoint with its own key, offers
//! its model list under the `openai:` prefix, and is the only place a Thinking
//! level becomes a request parameter.
//!
//! Two rules shape the code:
//!
//! * A reasoning level is only ever translated into the parameter the API
//!   documents (`reasoning.effort`). "Off" sends no parameter at all, because
//!   several models reject `effort: "none"` with HTTP 400.
//! * When a model still refuses that parameter, the request is retried once
//!   without it and the refusal is remembered for the life of the process, so
//!   picking Thinking on such a model costs one retry rather than every turn.
//!
//! Streaming is folded into the upstream SSE frame shapes the shared stream
//! machine in `main.rs` already reads (`choices[].delta.content`, `usage`,
//! `[DONE]`), plus one frame of its own for reasoning summaries.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::thinking::ThinkingLevel;
use crate::AppState;

/// Model ids the composer offers from this provider are prefixed so the chat
/// paths can tell a platform pick from a router pick without guessing.
pub(crate) const PREFIX: &str = "openai:";

const TIMEOUT: Duration = Duration::from_secs(600);

type ByteStream = futures_util::stream::BoxStream<'static, Result<bytes::Bytes, reqwest::Error>>;

/// `openai:gpt-…` → `gpt-…`. Only a non-empty suffix counts as a platform
/// pick; everything else belongs to the router path.
pub(crate) fn platform_model(chosen: &str) -> Option<String> {
    chosen
        .strip_prefix(PREFIX)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
}

pub(crate) fn responses_url(base_url: &str) -> String {
    format!("{}/responses", base_url.trim_end_matches('/'))
}

/// Text of a packed message. The packer writes strings; anything else is
/// reduced to its text parts so an unexpected shape can never send an invalid
/// content block to the API.
fn content_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    if let Some(parts) = content.as_array() {
        return parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.as_str())
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    content.to_string()
}

/// The packed chat transcript as a Responses request. The system prompt becomes
/// `instructions` (the API's own field) and every other message becomes one
/// `input` item, so nothing is stringified into a single blob.
pub(crate) fn request_body(
    model: &str,
    messages: &[Value],
    level: Option<ThinkingLevel>,
    stream: bool,
) -> Value {
    let mut instructions = String::new();
    let mut input: Vec<Value> = Vec::new();
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        let content = content_text(message.get("content").unwrap_or(&Value::Null));
        if role == "system" {
            if !instructions.is_empty() {
                instructions.push_str("\n\n");
            }
            instructions.push_str(&content);
            continue;
        }
        let role = if role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        input.push(json!({ "role": role, "content": content }));
    }
    let mut body = json!({ "model": model, "input": input, "stream": stream });
    if !instructions.is_empty() {
        body["instructions"] = json!(instructions);
    }
    if let Some(reasoning) = level.and_then(crate::thinking::responses_reasoning) {
        body["reasoning"] = reasoning;
    }
    body
}

/// Remove the reasoning block before a retry. Kept separate so both the
/// non-streaming and streaming paths strip exactly the same key.
fn strip_reasoning(body: &mut Value) {
    if let Some(object) = body.as_object_mut() {
        object.remove("reasoning");
    }
}

/// Visible answer text: the `output_text` parts of `message` items, falling
/// back to the convenience field the API also returns.
pub(crate) fn output_text(response: &Value) -> String {
    let mut out = String::new();
    if let Some(items) = response.get("output").and_then(Value::as_array) {
        for item in items {
            if item.get("type").and_then(Value::as_str) != Some("message") {
                continue;
            }
            for part in item
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if part.get("type").and_then(Value::as_str) == Some("output_text") {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        out.push_str(text);
                    }
                }
            }
        }
    }
    if out.is_empty() {
        if let Some(text) = response.get("output_text").and_then(Value::as_str) {
            out.push_str(text);
        }
    }
    out
}

/// Whatever concise reasoning summaries the model chose to publish. This is the
/// API's own summary channel — never presented as hidden chain-of-thought.
pub(crate) fn reasoning_summary(response: &Value) -> Option<String> {
    let items = response.get("output")?.as_array()?;
    let mut parts = Vec::new();
    for item in items {
        if item.get("type").and_then(Value::as_str) != Some("reasoning") {
            continue;
        }
        for part in item
            .get("summary")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

/// Usage, mapped onto the names the app's `request_log` already stores.
/// `reasoning_tokens` is carried separately because it is the part of the
/// completion the user paid for but never sees.
pub(crate) fn usage_of(usage: &Value) -> Value {
    json!({
        "prompt_tokens": usage.get("input_tokens").and_then(Value::as_i64),
        "completion_tokens": usage.get("output_tokens").and_then(Value::as_i64),
        "total_tokens": usage.get("total_tokens").and_then(Value::as_i64),
        "reasoning_tokens": usage
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(Value::as_i64),
    })
}

/// One parsed Responses SSE payload.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Event {
    pub text: Option<String>,
    pub summary: Option<String>,
    pub usage: Option<Value>,
    pub done: bool,
    pub error: Option<String>,
}

pub(crate) fn parse_event(payload: &str) -> Event {
    let payload = payload.trim();
    if payload.is_empty() {
        return Event::default();
    }
    if payload == "[DONE]" {
        return Event {
            done: true,
            ..Event::default()
        };
    }
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return Event::default();
    };
    let non_empty = |text: Option<&Value>| {
        text.and_then(Value::as_str)
            .map(str::to_string)
            .filter(|text| !text.is_empty())
    };
    match value.get("type").and_then(Value::as_str).unwrap_or("") {
        "response.output_text.delta" => Event {
            text: non_empty(value.get("delta")),
            ..Event::default()
        },
        "response.reasoning_summary_text.delta" => Event {
            summary: non_empty(value.get("delta")),
            ..Event::default()
        },
        "response.completed" => Event {
            done: true,
            usage: value
                .pointer("/response/usage")
                .filter(|usage| !usage.is_null())
                .cloned(),
            ..Event::default()
        },
        "response.incomplete" => Event {
            done: true,
            usage: value
                .pointer("/response/usage")
                .filter(|usage| !usage.is_null())
                .cloned(),
            error: value
                .pointer("/response/incomplete_details/reason")
                .and_then(Value::as_str)
                .map(|reason| format!("The response was incomplete: {reason}."))
                .or_else(|| Some("The response was incomplete.".to_string())),
            ..Event::default()
        },
        "response.failed" => Event {
            done: true,
            error: value
                .pointer("/response/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| Some("The OpenAI Platform reported a failed response.".to_string())),
            ..Event::default()
        },
        "error" => Event {
            done: true,
            error: value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| Some("The OpenAI Platform reported an error.".to_string())),
            ..Event::default()
        },
        _ => Event::default(),
    }
}

/// Fold one Responses payload into the upstream frames the shared stream
/// machine reads: text deltas, `usage`, `[DONE]`, inline errors — and a
/// `reasoning` frame the client renders in its own collapsible block.
pub(crate) fn upstream_frames(payload: &str) -> String {
    let event = parse_event(payload);
    let mut out = String::new();
    if let Some(text) = event.text {
        out.push_str(&format!(
            "data: {}\n\n",
            json!({ "choices": [{ "delta": { "content": text } }] })
        ));
    }
    if let Some(summary) = event.summary {
        out.push_str(&format!(
            "data: {}\n\n",
            json!({ "reasoning": { "summary": summary } })
        ));
    }
    if let Some(usage) = event.usage {
        out.push_str(&format!(
            "data: {}\n\n",
            json!({ "usage": usage_of(&usage) })
        ));
    }
    if let Some(error) = event.error {
        out.push_str(&format!(
            "data: {}\n\n",
            json!({ "error": { "message": error } })
        ));
    }
    if event.done {
        out.push_str("data: [DONE]\n\n");
    }
    out
}

fn rejected() -> &'static Mutex<HashSet<String>> {
    static REJECTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REJECTED.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(crate) fn remembers_rejection(model: &str) -> bool {
    rejected()
        .lock()
        .map(|set| set.contains(model))
        .unwrap_or(false)
}

fn remember_rejection(model: &str) {
    if let Ok(mut set) = rejected().lock() {
        set.insert(model.to_string());
    }
}

/// A 400 whose body names reasoning is the model refusing the parameter, which
/// is the one case worth retrying without it.
pub(crate) fn is_reasoning_rejection(status: u16, body: &str) -> bool {
    status == 400 && body.to_ascii_lowercase().contains("reasoning")
}

/// Errors that say what to do next instead of echoing a bare status code.
fn api_error(status: StatusCode, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(600).collect());
    match status.as_u16() {
        401 | 403 => format!(
            "The OpenAI Platform rejected the key (HTTP {}). Save a valid key in Settings. {detail}",
            status.as_u16()
        ),
        404 => format!(
            "The OpenAI Platform does not offer this model (HTTP 404). Pick one from its model list. {detail}"
        ),
        429 => format!("OpenAI Platform rate limit (HTTP 429). Wait, then retry. {detail}"),
        _ => format!("OpenAI Platform returned HTTP {}: {detail}", status.as_u16()),
    }
}

/// One non-streaming turn. `answers`/`meta` share the shape the router path
/// returns, so callers cannot tell the two apart.
pub(crate) async fn responses_once(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: Value,
) -> Result<(String, Value), String> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut body = body;
    let mut dropped = false;
    if remembers_rejection(&model) {
        strip_reasoning(&mut body);
        dropped = true;
    }
    loop {
        let response = client
            .post(responses_url(base_url))
            .bearer_auth(api_key)
            .timeout(TIMEOUT)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("The OpenAI Platform is unreachable: {e}"))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if status.is_success() {
            let parsed: Value = serde_json::from_str(&text)
                .map_err(|e| format!("The platform answered with unreadable JSON: {e}"))?;
            let answer = output_text(&parsed);
            if answer.trim().is_empty() {
                return Err("The OpenAI Platform returned an empty answer.".to_string());
            }
            let mut meta = usage_of(parsed.get("usage").unwrap_or(&Value::Null));
            meta["model"] = parsed
                .get("model")
                .cloned()
                .unwrap_or_else(|| json!(model.clone()));
            meta["provider"] = json!("openai-platform");
            if let Some(summary) = reasoning_summary(&parsed) {
                meta["reasoning_summary"] = json!(summary);
            }
            meta["reasoning_dropped"] = json!(dropped);
            return Ok((answer, meta));
        }
        if !dropped
            && body.get("reasoning").is_some()
            && is_reasoning_rejection(status.as_u16(), &text)
        {
            remember_rejection(&model);
            strip_reasoning(&mut body);
            dropped = true;
            continue;
        }
        return Err(api_error(status, &text));
    }
}

/// Streaming turn, translated into upstream frames as they arrive. A failure
/// before the stream starts is a plain error; one after it becomes an error
/// frame, so the reply saved so far is still kept by the shared recorder.
pub(crate) async fn responses_stream_bytes(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    body: Value,
) -> Result<ByteStream, String> {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut body = body;
    let mut dropped = false;
    if remembers_rejection(&model) {
        strip_reasoning(&mut body);
        dropped = true;
    }
    let response = loop {
        let response = client
            .post(responses_url(base_url))
            .bearer_auth(api_key)
            .header("accept", "text/event-stream")
            .timeout(TIMEOUT)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("The OpenAI Platform is unreachable: {e}"))?;
        if response.status().is_success() {
            break response;
        }
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !dropped
            && body.get("reasoning").is_some()
            && is_reasoning_rejection(status.as_u16(), &text)
        {
            remember_rejection(&model);
            strip_reasoning(&mut body);
            dropped = true;
            continue;
        }
        return Err(api_error(status, &text));
    };
    let upstream: ByteStream = response.bytes_stream().boxed();
    Ok(futures_util::stream::unfold(
        (upstream, String::new()),
        |(mut upstream, mut carry)| async move {
            loop {
                if let Some(index) = carry.find('\n') {
                    let line: String = carry.drain(..=index).collect();
                    let Some(payload) = line.trim().strip_prefix("data:") else {
                        continue;
                    };
                    let frames = upstream_frames(payload);
                    if !frames.is_empty() {
                        return Some((Ok(bytes::Bytes::from(frames)), (upstream, carry)));
                    }
                    continue;
                }
                match upstream.next().await {
                    Some(Ok(chunk)) => carry.push_str(&String::from_utf8_lossy(&chunk)),
                    Some(Err(error)) => {
                        let payload = format!(
                            "data: {}\n\n",
                            json!({ "error": { "message": format!("The OpenAI Platform stream failed: {error}") } })
                        );
                        return Some((Ok(bytes::Bytes::from(payload)), (upstream, carry)));
                    }
                    // Ended: the shared stream machine sends the done frame.
                    None => return None,
                }
            }
        },
    )
    .boxed())
}

/// The platform's own model list, with the `openai:` prefix and whatever
/// capability metadata its rows carry (its `/models` rows carry none, so the
/// flags stay `null` and the UI says unknown rather than promising reasoning).
pub(crate) async fn list_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<Value>, String> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let response = client
        .get(&url)
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("The OpenAI Platform is unreachable: {e}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(api_error(status, &text));
    }
    let parsed: Value = serde_json::from_str(&text).map_err(|_| {
        "The platform answered /models with something that is not JSON.".to_string()
    })?;
    let rows = parsed
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut models: Vec<Value> = rows
        .iter()
        .filter_map(|row| {
            let id = row.get("id").and_then(Value::as_str)?;
            let caps = crate::capabilities::from_catalog_row(row);
            Some(json!({
                "id": format!("{PREFIX}{id}"),
                "label": id,
                "owner": row.get("owned_by").and_then(Value::as_str).unwrap_or("openai"),
                "context_window": caps.context_limit,
                "capabilities": caps,
            }))
        })
        .collect();
    models.sort_by(|a, b| {
        a.get("label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(b.get("label").and_then(Value::as_str).unwrap_or(""))
    });
    Ok(models)
}

pub(crate) fn read_routes() -> Router<AppState> {
    Router::new()
        .route("/api/openai/models", get(openai_models))
        .route("/api/openai/test", post(openai_test))
}

/// The platform's catalog for the composer. A missing key is a 400 that says
/// where to put it, not an empty list that looks like "no models".
async fn openai_models(State(state): State<AppState>) -> Response {
    let cfg = crate::load_config(&state);
    if cfg.openai_api_key.trim().is_empty() {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "Save an OpenAI Platform key in Settings to list its models.",
        );
    }
    match list_models(&state.client, &cfg.openai_base_url, &cfg.openai_api_key).await {
        Ok(models) => Json(json!({ "ok": true, "models": models })).into_response(),
        Err(error) => crate::json_error(StatusCode::BAD_GATEWAY, error),
    }
}

/// Key check that spends no tokens: the model list is the cheapest call the
/// platform offers.
async fn openai_test(State(state): State<AppState>) -> Response {
    let cfg = crate::load_config(&state);
    if cfg.openai_api_key.trim().is_empty() {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "Save an OpenAI Platform key in Settings first.",
        );
    }
    let started = std::time::Instant::now();
    match list_models(&state.client, &cfg.openai_base_url, &cfg.openai_api_key).await {
        Ok(models) => Json(json!({
            "ok": true,
            "models": models.len(),
            "latency_ms": started.elapsed().as_millis() as i64,
            "message": format!("OpenAI Platform answered with {} models.", models.len()),
        }))
        .into_response(),
        Err(error) => crate::json_error(StatusCode::BAD_GATEWAY, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript() -> Vec<Value> {
        vec![
            json!({ "role": "system", "content": "You are Sarthi." }),
            json!({ "role": "user", "content": "Add a test." }),
            json!({ "role": "assistant", "content": "Which file?" }),
            json!({ "role": "user", "content": "lib.rs" }),
        ]
    }

    #[test]
    fn a_platform_pick_needs_a_model_name() {
        assert_eq!(platform_model("openai:gpt-5"), Some("gpt-5".to_string()));
        assert_eq!(platform_model("openai: gpt-5 "), Some("gpt-5".to_string()));
        assert_eq!(platform_model("openai:"), None);
        assert_eq!(platform_model("gpt-5"), None);
        assert_eq!(platform_model("local:qwen"), None);
    }

    #[test]
    fn the_request_moves_the_system_prompt_into_instructions() {
        let body = request_body("gpt-5", &transcript(), None, false);
        assert_eq!(body["model"], "gpt-5");
        assert_eq!(body["instructions"], "You are Sarthi.");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 3);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[2]["content"], "lib.rs");
        // No level, no reasoning parameter.
        assert!(body.get("reasoning").is_none());
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn a_level_becomes_the_documented_reasoning_block() {
        let body = request_body("gpt-5", &transcript(), Some(ThinkingLevel::High), true);
        assert_eq!(body["reasoning"]["effort"], "high");
        assert_eq!(body["reasoning"]["summary"], "auto");
        assert_eq!(body["stream"], true);
        let off = request_body("gpt-5", &transcript(), Some(ThinkingLevel::Off), false);
        assert!(off.get("reasoning").is_none());
    }

    #[test]
    fn a_stubbed_out_transcript_never_produces_an_invalid_content_block() {
        let messages = vec![json!({
            "role": "user",
            "content": [{ "type": "text", "text": "line one" }, { "type": "text", "text": "line two" }]
        })];
        let body = request_body("gpt-5", &messages, None, false);
        assert_eq!(body["input"][0]["content"], "line one\nline two");
    }

    #[test]
    fn the_answer_is_read_from_the_message_items() {
        let response = json!({
            "output": [
                { "type": "reasoning", "summary": [{ "type": "summary_text", "text": "Considered the file." }] },
                { "type": "message", "content": [
                    { "type": "output_text", "text": "Here is the " },
                    { "type": "output_text", "text": "test." }
                ]}
            ]
        });
        assert_eq!(output_text(&response), "Here is the test.");
        assert_eq!(
            reasoning_summary(&response),
            Some("Considered the file.".to_string())
        );
    }

    #[test]
    fn a_summary_is_never_invented_when_the_model_published_none() {
        let response = json!({ "output": [{ "type": "message", "content": [] }] });
        assert_eq!(reasoning_summary(&response), None);
        assert_eq!(output_text(&response), "");
    }

    #[test]
    fn usage_is_mapped_onto_the_names_the_usage_log_stores() {
        let usage = usage_of(&json!({
            "input_tokens": 120,
            "output_tokens": 40,
            "total_tokens": 160,
            "output_tokens_details": { "reasoning_tokens": 24 }
        }));
        assert_eq!(usage["prompt_tokens"], 120);
        assert_eq!(usage["completion_tokens"], 40);
        assert_eq!(usage["total_tokens"], 160);
        assert_eq!(usage["reasoning_tokens"], 24);
    }

    #[test]
    fn stream_events_are_read_by_type() {
        let text = parse_event(r#"{"type":"response.output_text.delta","delta":"hi"}"#);
        assert_eq!(text.text.as_deref(), Some("hi"));
        assert!(text.summary.is_none());

        let summary =
            parse_event(r#"{"type":"response.reasoning_summary_text.delta","delta":"checked"}"#);
        assert_eq!(summary.summary.as_deref(), Some("checked"));
        assert!(summary.text.is_none());

        let completed = parse_event(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":5,"output_tokens":7,"total_tokens":12}}}"#,
        );
        assert!(completed.done);
        assert_eq!(completed.usage.as_ref().unwrap()["total_tokens"], 12);

        let failed = parse_event(
            r#"{"type":"response.failed","response":{"error":{"message":"bad model"}}}"#,
        );
        assert!(failed.done);
        assert_eq!(failed.error.as_deref(), Some("bad model"));

        let unknown = parse_event(r#"{"type":"response.created"}"#);
        assert_eq!(unknown, Event::default());
    }

    #[test]
    fn upstream_frames_carry_text_summary_usage_and_done() {
        let text = upstream_frames(r#"{"type":"response.output_text.delta","delta":"hi"}"#);
        assert!(text.contains(r#""content":"hi""#));

        let summary = upstream_frames(
            r#"{"type":"response.reasoning_summary_text.delta","delta":"thinking"}"#,
        );
        assert!(summary.contains(r#""summary":"thinking""#));

        let done = upstream_frames(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":1,"output_tokens":2,"total_tokens":3}}}"#,
        );
        assert!(done.contains(r#""total_tokens":3"#));
        assert!(done.contains("[DONE]"));

        // An event nobody needs produces no frame at all.
        assert!(upstream_frames(r#"{"type":"response.created"}"#).is_empty());
    }

    #[test]
    fn only_a_400_that_names_reasoning_is_a_reasoning_rejection() {
        assert!(is_reasoning_rejection(
            400,
            r#"{"error":{"message":"Unsupported parameter: 'reasoning'"}}"#
        ));
        assert!(!is_reasoning_rejection(
            400,
            r#"{"error":{"message":"Unknown model"}}"#
        ));
        assert!(!is_reasoning_rejection(429, "reasoning"));
    }

    #[test]
    fn errors_name_the_next_action() {
        assert!(api_error(StatusCode::UNAUTHORIZED, "{}").contains("Save a valid key"));
        assert!(api_error(StatusCode::TOO_MANY_REQUESTS, "{}").contains("rate limit"));
        let other = api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"boom"}}"#,
        );
        assert!(other.contains("boom"));
    }
}
