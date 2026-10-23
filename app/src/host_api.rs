//! Authenticated, restricted router for the Android companion.
//!
//! This router is intentionally constructed separately from the local desktop UI
//! router and is not mounted on a network listener until the transport guardrails
//! in the next implementation step are in place.

use super::{chat, create_conversation, load_config, open_db, router_url, AppState};
use crate::host_auth::HostTokenStore;
use axum::{
    body::{to_bytes, Body},
    extract::{DefaultBodyLimit, Path, RawQuery, State},
    http::{header, HeaderValue, Request, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path as FsPath,
};
use uuid::Uuid;

const API_VERSION: &str = "1";
const MAX_REQUEST_BODY_BYTES: usize = 512 * 1024;
const MAX_CONVERSATION_PAGE_SIZE: usize = 100;
const DEFAULT_CONVERSATION_PAGE_SIZE: usize = 50;
const MAX_MESSAGE_PAGE_SIZE: usize = 150;
const DEFAULT_MESSAGE_PAGE_SIZE: usize = 100;
const INTERNAL_ERROR_MARKER: &str = "x-saarthi-internal-host-error";

/// Construct the remote-only router. Step 4 will mount this router on a separately
/// configured listener; do not merge it into the local UI router.
#[allow(dead_code)]
pub(super) fn build_router(state: AppState, token_store: HostTokenStore) -> Router {
    Router::new()
        .route("/api/host/v1/health", get(host_health))
        .route("/api/host/v1/capabilities", get(host_capabilities))
        .route("/api/host/v1/models", get(host_models))
        .route(
            "/api/host/v1/conversations",
            get(list_host_conversations).post(create_host_conversation),
        )
        .route(
            "/api/host/v1/conversations/:conversation_id/messages",
            get(list_host_messages),
        )
        .route("/api/host/v1/chat", post(host_chat))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .route_layer(from_fn_with_state(token_store, require_host_bearer))
        .with_state(state)
}

async fn require_host_bearer(
    State(token_store): State<HostTokenStore>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    let candidate = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_bearer);

    // Any missing, malformed, wrong, revoked, or unreadable token fails closed and
    // produces the same public response. Never log the presented credential.
    let authorized = candidate
        .map(|token| token_store.verify(token).unwrap_or(false))
        .unwrap_or(false);
    if !authorized {
        let mut response = public_host_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Authentication required.",
        );
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        return secure_host_response(response);
    }

    // STEP3_REQUEST_BODY_GUARD_V2: enforce the limit before extractors run.
    // In particular, Option<Json<T>> can swallow extractor failures and turn an
    // oversized body into an apparently empty request. Buffer only up to the
    // API's explicit limit, then restore the bounded body for the handler.
    let (request_parts, request_body) = request.into_parts();
    let request_body = match to_bytes(request_body, MAX_REQUEST_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return secure_host_response(public_host_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "JSON request body exceeds 512 KiB.",
            ));
        }
    };
    if !request_body.is_empty() {
        let content_type = request_parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let media_type = content_type.split(';').next().unwrap_or("").trim();
        if !media_type.eq_ignore_ascii_case("application/json") {
            return secure_host_response(public_host_error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                "Requests with a body must use application/json.",
            ));
        }
        if serde_json::from_slice::<Value>(&request_body).is_err() {
            return secure_host_response(public_host_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The request body must be valid JSON.",
            ));
        }
    }
    let request = Request::from_parts(request_parts, Body::from(request_body));
    let mut response = next.run(request).await;
    if response
        .headers_mut()
        .remove(INTERNAL_ERROR_MARKER)
        .is_some()
    {
        return secure_host_response(response);
    }
    let response = match response.status() {
        StatusCode::PAYLOAD_TOO_LARGE => public_host_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "JSON request body exceeds 512 KiB.",
        ),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => public_host_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "Requests with a body must use application/json.",
        ),
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => public_host_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The request is malformed or contains invalid fields.",
        ),
        _ => response,
    };
    secure_host_response(response)
}

fn secure_host_response(mut response: Response<Body>) -> Response<Body> {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn parse_bearer(value: &str) -> Option<&str> {
    let mut parts = value.split_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    if parts.next().is_some() || !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return None;
    }
    Some(token)
}

fn host_error(status: StatusCode, code: &str, message: &str) -> Response<Body> {
    let request_id = Uuid::new_v4().to_string();
    let mut response = (
        status,
        Json(json!({
            "error": {
                "code": code,
                "message": message,
                "request_id": request_id
            }
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(INTERNAL_ERROR_MARKER, HeaderValue::from_static("1"));
    response
}

fn public_host_error(status: StatusCode, code: &str, message: &str) -> Response<Body> {
    let mut response = host_error(status, code, message);
    response.headers_mut().remove(INTERNAL_ERROR_MARKER);
    response
}

async fn host_health(State(state): State<AppState>) -> Response<Body> {
    match load_or_create_host_id(&state.config_path) {
        Ok(host_id) => Json(json!({
            "ok": true,
            "product": "Saarthi Studio",
            "api_version": API_VERSION,
            "host_id": host_id.to_string()
        }))
        .into_response(),
        Err(_) => host_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "The desktop host could not read its identity.",
        ),
    }
}

async fn host_capabilities() -> Json<Value> {
    Json(json!({
        "api_version": API_VERSION,
        "model_policy": "desktop_configured",
        "features": {
            "conversation_list": true,
            "conversation_create": true,
            "message_history": true,
            "non_streaming_chat": true,
            "model_list": true,
            "streaming": false,
            "attachments": false,
            "conversation_delete": false,
            "usage_diagnostics": false
        },
        "limits": {
            "request_body_bytes": MAX_REQUEST_BODY_BYTES,
            "message_chars": 50000,
            "code_chars": 50000,
            "language_chars": 40,
            "conversation_page_size": MAX_CONVERSATION_PAGE_SIZE,
            "message_page_size": MAX_MESSAGE_PAGE_SIZE
        }
    }))
}

async fn host_models(State(state): State<AppState>) -> Response<Body> {
    let cfg = load_config(&state);
    if cfg.api_key.trim().is_empty() {
        return host_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "host_not_configured",
            "The desktop host's AI router is not configured.",
        );
    }

    let response = match state
        .client
        .get(router_url(&cfg, "models"))
        .bearer_auth(&cfg.api_key)
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response,
        Ok(_) => {
            return host_error(
                StatusCode::BAD_GATEWAY,
                "models_unavailable",
                "The desktop host could not retrieve its configured model list.",
            );
        }
        Err(_) => {
            return host_error(
                StatusCode::BAD_GATEWAY,
                "models_unavailable",
                "The desktop host could not retrieve its configured model list.",
            );
        }
    };

    let payload = match response.json::<Value>().await {
        Ok(payload) => payload,
        Err(_) => {
            return host_error(
                StatusCode::BAD_GATEWAY,
                "invalid_models_response",
                "The configured AI router returned an invalid model list.",
            );
        }
    };
    let models = match sanitize_model_list(&payload) {
        Some(models) => models,
        None => {
            return host_error(
                StatusCode::BAD_GATEWAY,
                "invalid_models_response",
                "The configured AI router returned an invalid model list.",
            );
        }
    };

    Json(json!({
        "api_version": API_VERSION,
        "object": "list",
        "count": models.len(),
        "data": models
    }))
    .into_response()
}

/// Remove upstream metadata and credentials from the public model list. The host API
/// intentionally exposes no provider ownership fields or raw router response objects.
fn sanitize_model_list(payload: &Value) -> Option<Vec<Value>> {
    const MAX_PUBLIC_MODELS: usize = 200;
    let items = payload.get("data")?.as_array()?;
    let mut seen = std::collections::HashSet::<String>::new();
    let mut models = Vec::new();
    for item in items {
        let id = item.get("id").and_then(Value::as_str).unwrap_or("").trim();
        if id.is_empty() || id.chars().count() > 256 || !seen.insert(id.to_owned()) {
            continue;
        }
        let raw_name = item
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(id);
        let name = raw_name.chars().take(128).collect::<String>();
        models.push(json!({ "id": id, "name": name }));
        if models.len() >= MAX_PUBLIC_MODELS {
            break;
        }
    }
    Some(models)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ConversationCursor {
    updated_at_bits: u64,
    id: String,
}

#[derive(Debug, Serialize)]
struct HostConversationSummary {
    id: String,
    title: String,
    created_at: f64,
    updated_at: f64,
}

async fn list_host_conversations(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> Response<Body> {
    let parameters = match parse_host_query(raw_query.as_deref()) {
        Ok(parameters) => parameters,
        Err(()) => {
            return host_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The conversation query is invalid.",
            )
        }
    };
    let limit = match parameters.get("limit") {
        Some(value) => match value.parse::<usize>() {
            Ok(value) => value,
            Err(_) => {
                return host_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Conversation page size must be an integer.",
                )
            }
        },
        None => DEFAULT_CONVERSATION_PAGE_SIZE,
    };
    if !(1..=MAX_CONVERSATION_PAGE_SIZE).contains(&limit) {
        return host_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Conversation page size must be between 1 and 100.",
        );
    }

    let cursor = match parameters.get("cursor").map(String::as_str) {
        Some(value) if value.len() > 512 => {
            return host_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The conversation cursor is invalid.",
            );
        }
        Some(value) => match decode_conversation_cursor(value) {
            Some(cursor) => Some(cursor),
            None => {
                return host_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "The conversation cursor is invalid.",
                );
            }
        },
        None => None,
    };

    let result = (|| -> rusqlite::Result<Vec<HostConversationSummary>> {
        let conn = open_db(&state.database_path)?;
        let cursor_updated_at = cursor
            .as_ref()
            .map(|item| f64::from_bits(item.updated_at_bits));
        let cursor_id = cursor.as_ref().map(|item| item.id.as_str());
        let mut statement = conn.prepare(
            "SELECT id, title, created_at, updated_at
             FROM conversations
             WHERE (?1 IS NULL OR updated_at < ?1 OR (updated_at = ?1 AND id > ?2))
             ORDER BY updated_at DESC, id ASC
             LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![cursor_updated_at, cursor_id, (limit + 1) as i64],
            |row| {
                Ok(HostConversationSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        )?;
        rows.collect()
    })();

    let mut items = match result {
        Ok(items) => items,
        Err(_) => {
            return host_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "The desktop host could not list conversations.",
            );
        }
    };
    let has_more = items.len() > limit;
    if has_more {
        items.pop();
    }
    let next_cursor = if has_more {
        items.last().and_then(encode_conversation_cursor)
    } else {
        None
    };

    Json(json!({ "items": items, "next_cursor": next_cursor })).into_response()
}

fn parse_host_query(
    raw_query: Option<&str>,
) -> Result<std::collections::HashMap<String, String>, ()> {
    let mut parameters = std::collections::HashMap::new();
    let Some(raw_query) = raw_query else {
        return Ok(parameters);
    };
    if raw_query.len() > 1024 {
        return Err(());
    }
    for pair in raw_query.split('&') {
        if pair.is_empty() {
            return Err(());
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if !matches!(key, "limit" | "cursor" | "before_id") || value.is_empty() {
            return Err(());
        }
        if parameters
            .insert(key.to_string(), value.to_string())
            .is_some()
        {
            return Err(());
        }
    }
    Ok(parameters)
}

fn encode_conversation_cursor(item: &HostConversationSummary) -> Option<String> {
    let cursor = ConversationCursor {
        updated_at_bits: item.updated_at.to_bits(),
        id: item.id.clone(),
    };
    let bytes = serde_json::to_vec(&cursor).ok()?;
    Some(encode_hex(&bytes))
}

fn decode_conversation_cursor(value: &str) -> Option<ConversationCursor> {
    if value.is_empty()
        || value.len() % 2 != 0
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    let bytes = decode_hex(value)?;
    let cursor: ConversationCursor = serde_json::from_slice(&bytes).ok()?;
    let updated_at = f64::from_bits(cursor.updated_at_bits);
    if !updated_at.is_finite() || Uuid::parse_str(&cursor.id).is_err() {
        return None;
    }
    Some(cursor)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        result.push((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?);
    }
    Some(result)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

async fn create_host_conversation(
    State(state): State<AppState>,
    payload: Option<Json<Value>>,
) -> Response<Body> {
    let input = payload
        .map(|Json(value)| value)
        .unwrap_or_else(|| json!({}));
    let Some(object) = input.as_object() else {
        return host_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The conversation request must be a JSON object.",
        );
    };
    if object.keys().any(|key| key != "title")
        || object.get("title").is_some_and(|value| !value.is_string())
        || object
            .get("title")
            .and_then(Value::as_str)
            .is_some_and(|value| value.chars().count() > 256)
    {
        return host_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The conversation request contains unsupported or invalid fields.",
        );
    }
    let response = create_conversation(State(state), Json(input)).await;
    if response.status().is_success() {
        return response;
    }
    host_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "The desktop host could not create a conversation.",
    )
}

#[derive(Debug, Serialize)]
struct HostMessage {
    id: i64,
    role: String,
    content: String,
    created_at: f64,
}

async fn list_host_messages(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    RawQuery(raw_query): RawQuery,
) -> Response<Body> {
    let parameters = match parse_host_query(raw_query.as_deref()) {
        Ok(parameters) => parameters,
        Err(()) => {
            return host_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The message query is invalid.",
            )
        }
    };
    if Uuid::parse_str(&conversation_id).is_err() {
        return host_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The conversation ID is invalid.",
        );
    }
    let limit = match parameters.get("limit") {
        Some(value) => match value.parse::<usize>() {
            Ok(value) => value,
            Err(_) => {
                return host_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Message page size must be an integer.",
                )
            }
        },
        None => DEFAULT_MESSAGE_PAGE_SIZE,
    };
    if !(1..=MAX_MESSAGE_PAGE_SIZE).contains(&limit) {
        return host_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Message page size must be between 1 and 150.",
        );
    }
    let before_id = match parameters.get("before_id") {
        Some(value) => match value.parse::<i64>() {
            Ok(value) if value > 0 => Some(value),
            _ => {
                return host_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "before_id must be a positive message ID.",
                )
            }
        },
        None => None,
    };

    let result = (|| -> rusqlite::Result<Option<Vec<HostMessage>>> {
        let conn = open_db(&state.database_path)?;
        let exists = conn
            .query_row(
                "SELECT id FROM conversations WHERE id=?1",
                [&conversation_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if exists.is_none() {
            return Ok(None);
        }

        let mut statement = conn.prepare(
            "SELECT id, role, content, created_at
             FROM messages
             WHERE conversation_id=?1 AND (?2 IS NULL OR id < ?2)
             ORDER BY id DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![conversation_id, before_id, (limit + 1) as i64],
            |row| {
                Ok(HostMessage {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map(Some)
    })();

    let mut items = match result {
        Ok(Some(items)) => items,
        Ok(None) => {
            return host_error(
                StatusCode::NOT_FOUND,
                "not_found",
                "The conversation was not found on the desktop host.",
            );
        }
        Err(_) => {
            return host_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "The desktop host could not read conversation history.",
            );
        }
    };
    let has_more = items.len() > limit;
    if has_more {
        items.pop();
    }
    items.reverse();
    let next_before_id = if has_more {
        items.first().map(|message| message.id)
    } else {
        None
    };
    Json(json!({ "items": items, "next_before_id": next_before_id })).into_response()
}

async fn host_chat(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    let Some(object) = input.as_object() else {
        return host_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The chat request must be a JSON object.",
        );
    };
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "conversation_id" | "message" | "code" | "language"
        )
    }) || object
        .get("conversation_id")
        .is_some_and(|value| !value.is_string())
        || object
            .get("message")
            .is_some_and(|value| !value.is_string())
        || object.get("code").is_some_and(|value| !value.is_string())
        || object
            .get("language")
            .is_some_and(|value| !value.is_string())
    {
        return host_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The chat request contains unsupported or invalid fields.",
        );
    }
    let conversation_id = input
        .get("conversation_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let message = input
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let code = input
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let language = input
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or("text");

    if Uuid::parse_str(conversation_id).is_err()
        || message.trim().is_empty()
        || message.chars().count() > 50_000
        || code.chars().count() > 50_000
        || language.chars().count() > 40
    {
        return host_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The chat request is invalid or uses an unsupported feature.",
        );
    }

    // Keep the current desktop chat implementation as the canonical path for
    // prompt construction, history persistence, upstream requests and diagnostics.
    // Translate every failed response to a stable public error shape so upstream
    // router URLs, provider text and local DB details are not sent to the phone.
    let response = chat(State(state), Json(input)).await;
    if response.status().is_success() {
        let body = match to_bytes(response.into_body(), 4 * 1024 * 1024).await {
            Ok(body) => body,
            Err(_) => {
                return host_error(
                    StatusCode::BAD_GATEWAY,
                    "upstream_failure",
                    "The desktop host received an unusable response from its AI router.",
                );
            }
        };
        let parsed: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(_) => {
                return host_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "The desktop host could not format the chat response.",
                );
            }
        };
        let Some(answer) = parsed.get("answer").and_then(Value::as_str) else {
            return host_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "The desktop host could not format the chat response.",
            );
        };
        let source_meta = parsed.get("meta").cloned().unwrap_or_else(|| json!({}));
        let latency_ms = source_meta
            .get("latency_ms")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0);
        let model = source_meta
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("auto");
        let mut meta = json!({"latency_ms": latency_ms, "model": model});
        for key in ["prompt_tokens", "completion_tokens", "total_tokens"] {
            if let Some(value) = source_meta.get(key) {
                if value.is_null() || value.as_i64().is_some_and(|count| count >= 0) {
                    meta[key] = value.clone();
                }
            }
        }
        let request_id = Uuid::new_v4().to_string();
        let mut response =
            Json(json!({ "ok": true, "answer": answer, "meta": meta })).into_response();
        response.headers_mut().insert(
            "x-request-id",
            HeaderValue::from_str(&request_id).expect("UUID is a valid header value"),
        );
        return response;
    }

    let upstream_status = response.status();
    let internal_error_body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .ok()
        .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
        .unwrap_or_else(|| json!({}));
    let internal_error_text = internal_error_body
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if internal_error_text == "Add your router's unified API key in Settings first." {
        return host_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "host_not_configured",
            "The desktop host's AI router is not configured.",
        );
    }

    let (status, code, message) = match upstream_status {
        StatusCode::NOT_FOUND => (
            StatusCode::NOT_FOUND,
            "not_found",
            "The conversation was not found on the desktop host.",
        ),
        StatusCode::BAD_REQUEST => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The chat request is invalid.",
        ),
        StatusCode::GATEWAY_TIMEOUT => (
            StatusCode::GATEWAY_TIMEOUT,
            "upstream_timeout",
            "The configured AI router did not respond in time.",
        ),
        StatusCode::BAD_GATEWAY => (
            StatusCode::BAD_GATEWAY,
            "upstream_failure",
            "The configured AI router could not complete the request.",
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "The desktop host could not complete the chat request.",
        ),
    };
    host_error(status, code, message)
}

/// Persist a stable, non-secret UUID for this desktop data profile. The file lives
/// beside the existing private config, is owner-readable only on Unix, and rejects
/// symlinks/corrupt contents. Step 4 should call this during opt-in remote startup;
/// the authenticated health handler also calls it defensively.
pub(super) fn load_or_create_host_id(config_path: &FsPath) -> io::Result<Uuid> {
    let path = config_path.with_file_name("host-id");
    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| FsPath::new("."));
    fs::create_dir_all(parent)?;
    validate_and_secure_parent(parent)?;

    match fs::symlink_metadata(&path) {
        Ok(metadata) => read_existing_host_id(&path, &metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let host_id = Uuid::new_v4();
            let temporary = parent.join(format!(".host-id-{}.tmp", Uuid::new_v4()));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            let create_result = (|| -> io::Result<()> {
                writeln!(file, "{host_id}")?;
                file.sync_all()?;
                drop(file);
                set_host_id_file_permissions(&temporary)?;
                match fs::hard_link(&temporary, &path) {
                    Ok(()) => {}
                    Err(link_error) if link_error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(link_error) if link_error.kind() == io::ErrorKind::Unsupported => {
                        // On filesystems without hard-link support, atomic rename is
                        // the fallback. The listener reads the final file per request.
                        fs::rename(&temporary, &path)?;
                    }
                    Err(link_error) => return Err(link_error),
                }
                let _ = fs::remove_file(&temporary);
                sync_host_id_parent(parent)?;
                let metadata = fs::symlink_metadata(&path)?;
                read_existing_host_id(&path, &metadata).map(|_| ())
            })();
            if create_result.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            create_result?;
            read_existing_host_id(&path, &fs::symlink_metadata(&path)?)
        }
        Err(error) => Err(error),
    }
}

fn validate_and_secure_parent(parent: &FsPath) -> io::Result<()> {
    let metadata = fs::symlink_metadata(parent)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "desktop host identity parent must be a regular directory",
        ));
    }
    super::set_private_permissions(parent, true)?;
    Ok(())
}

fn read_existing_host_id(path: &FsPath, metadata: &fs::Metadata) -> io::Result<Uuid> {
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 128 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "desktop host identity must be a small regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "desktop host identity file permissions are too broad; set the file mode to 600",
            ));
        }
    }
    let value = fs::read_to_string(path)?;
    Uuid::parse_str(value.trim()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "desktop host identity file does not contain a valid UUID",
        )
    })
}

fn set_host_id_file_permissions(path: &FsPath) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn sync_host_id_parent(parent: &FsPath) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;
    use std::{fs, path::PathBuf, time::Duration};
    use tower::ServiceExt;

    struct Fixture {
        root: PathBuf,
        token: String,
        app: Router,
        config_path: PathBuf,
        database_path: PathBuf,
        token_store: HostTokenStore,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("saarthi-host-api-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).expect("create fixture directory");
            super::super::set_private_permissions(&root, true).expect("secure fixture directory");
            let config_path = root.join("config.json");
            let database_path = root.join("history.sqlite3");
            let state = AppState {
                config_path: config_path.clone(),
                database_path: database_path.clone(),
                static_dir: root.join("static"),
                workspace: crate::workspace::WorkspaceStore::load(root.join("workspace.json")),
                client: reqwest::Client::builder()
                    .timeout(Duration::from_secs(2))
                    .build()
                    .expect("build HTTP client"),
            };
            super::super::ensure_setup(&state).expect("initialize fixture config and DB");
            load_or_create_host_id(&state.config_path).expect("initialize host identity");
            let token_store = HostTokenStore::new(root.join("host-auth.json"));
            let token = token_store.create().expect("create test token");
            let app = build_router(state, token_store.clone());
            Self {
                root,
                token,
                app,
                config_path,
                database_path,
                token_store,
            }
        }

        async fn send(
            &self,
            method: Method,
            uri: &str,
            token: Option<&str>,
            body: Option<&str>,
        ) -> Response<Body> {
            let mut builder = Request::builder().method(method).uri(uri);
            if let Some(token) = token {
                builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            if body.is_some() {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
            }
            let request = builder
                .body(
                    body.map(|value| Body::from(value.to_owned()))
                        .unwrap_or_else(Body::empty),
                )
                .expect("build request");
            self.app.clone().oneshot(request).await.expect("run router")
        }

        async fn response_json(response: Response<Body>) -> Value {
            let bytes = to_bytes(response.into_body(), MAX_REQUEST_BODY_BYTES + 1024)
                .await
                .expect("read response body");
            serde_json::from_slice(&bytes).expect("parse JSON response")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn rejects_missing_and_wrong_tokens_with_same_public_error() {
        let fixture = Fixture::new();
        let missing = fixture
            .send(Method::GET, "/api/host/v1/health", None, None)
            .await;
        let wrong = fixture
            .send(
                Method::GET,
                "/api/host/v1/health",
                Some("wrong-token"),
                None,
            )
            .await;
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            missing.headers()[header::WWW_AUTHENTICATE]
                .to_str()
                .unwrap(),
            "Bearer"
        );
        assert!(!missing.headers().contains_key(INTERNAL_ERROR_MARKER));
        let missing_body = Fixture::response_json(missing).await;
        let wrong_body = Fixture::response_json(wrong).await;
        assert_eq!(missing_body["error"]["code"], "unauthorized");
        assert_eq!(wrong_body["error"]["code"], "unauthorized");
        assert_eq!(
            missing_body["error"]["message"],
            wrong_body["error"]["message"]
        );
        assert!(!wrong_body.to_string().contains(&fixture.token));
    }

    #[tokio::test]
    async fn token_rotation_and_revocation_take_effect_without_rebuilding_router() {
        let fixture = Fixture::new();
        let replacement = fixture.token_store.rotate().expect("rotate token");
        let old = fixture
            .send(
                Method::GET,
                "/api/host/v1/health",
                Some(&fixture.token),
                None,
            )
            .await;
        let current = fixture
            .send(Method::GET, "/api/host/v1/health", Some(&replacement), None)
            .await;
        assert_eq!(old.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(current.status(), StatusCode::OK);
        assert_eq!(
            current.headers()[header::CACHE_CONTROL].to_str().unwrap(),
            "no-store"
        );
        assert_eq!(
            current.headers()["x-content-type-options"]
                .to_str()
                .unwrap(),
            "nosniff"
        );

        fixture.token_store.revoke().expect("revoke token");
        let revoked = fixture
            .send(Method::GET, "/api/host/v1/health", Some(&replacement), None)
            .await;
        assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valid_token_reads_sanitized_health_and_stable_host_id() {
        let fixture = Fixture::new();
        let first = fixture
            .send(
                Method::GET,
                "/api/host/v1/health",
                Some(&fixture.token),
                None,
            )
            .await;
        assert_eq!(first.status(), StatusCode::OK);
        let first_body = Fixture::response_json(first).await;
        assert_eq!(first_body["api_version"], API_VERSION);
        assert_eq!(first_body["product"], "Saarthi Studio");
        assert!(first_body.get("router").is_none());
        assert!(first_body.get("base_url").is_none());
        assert!(first_body.get("api_key").is_none());
        let first_id = first_body["host_id"].as_str().expect("host ID").to_owned();
        assert!(Uuid::parse_str(&first_id).is_ok());

        let second = fixture
            .send(
                Method::GET,
                "/api/host/v1/health",
                Some(&fixture.token),
                None,
            )
            .await;
        let second_body = Fixture::response_json(second).await;
        assert_eq!(second_body["host_id"], first_id);
        assert!(fixture.config_path.with_file_name("host-id").is_file());
    }

    #[tokio::test]
    async fn capabilities_match_the_v1_contract_and_do_not_enable_deferred_features() {
        let fixture = Fixture::new();
        let response = fixture
            .send(
                Method::GET,
                "/api/host/v1/capabilities",
                Some(&fixture.token),
                None,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let data = Fixture::response_json(response).await;
        assert_eq!(data["api_version"], API_VERSION);
        assert_eq!(data["model_policy"], "desktop_configured");
        assert_eq!(data["features"]["streaming"], false);
        assert_eq!(data["features"]["model_list"], true);
        assert_eq!(data["features"]["attachments"], false);
        assert_eq!(data["features"]["conversation_delete"], false);
        assert_eq!(data["features"]["usage_diagnostics"], false);
        assert_eq!(data["limits"]["request_body_bytes"], MAX_REQUEST_BODY_BYTES);
    }

    #[tokio::test]
    async fn exposes_only_allow_listed_paths_and_methods() {
        let fixture = Fixture::new();
        for (method, path) in [
            (Method::GET, "/"),
            (Method::GET, "/static/index.html"),
            (Method::GET, "/api/config"),
            (Method::POST, "/api/config"),
            (Method::GET, "/api/test"),
            (Method::GET, "/api/usage"),
            (Method::POST, "/api/conversations"),
            (
                Method::POST,
                "/api/host/v1/conversations/00000000-0000-4000-8000-000000000000/delete",
            ),
            (Method::POST, "/api/host/v1/arbitrary-command"),
        ] {
            let response = fixture.send(method, path, Some(&fixture.token), None).await;
            assert!(
                response.status() == StatusCode::NOT_FOUND
                    || response.status() == StatusCode::METHOD_NOT_ALLOWED,
                "unexpected response for {path}: {}",
                response.status()
            );
        }
    }

    #[tokio::test]
    async fn creates_lists_and_reads_desktop_conversations_without_previews() {
        let fixture = Fixture::new();
        let create = fixture
            .send(
                Method::POST,
                "/api/host/v1/conversations",
                Some(&fixture.token),
                Some(r#"{"title":"Mobile-created conversation"}"#),
            )
            .await;
        assert_eq!(create.status(), StatusCode::CREATED);
        let created = Fixture::response_json(create).await;
        let conversation_id = created["id"]
            .as_str()
            .expect("conversation UUID")
            .to_owned();
        assert!(Uuid::parse_str(&conversation_id).is_ok());

        let list = fixture
            .send(
                Method::GET,
                "/api/host/v1/conversations",
                Some(&fixture.token),
                None,
            )
            .await;
        assert_eq!(list.status(), StatusCode::OK);
        let listing = Fixture::response_json(list).await;
        assert_eq!(listing["items"].as_array().unwrap().len(), 1);
        assert_eq!(listing["items"][0]["id"], conversation_id);
        assert!(listing["items"][0].get("last_message").is_none());
        assert!(listing["next_cursor"].is_null());

        let messages = fixture
            .send(
                Method::GET,
                &format!("/api/host/v1/conversations/{conversation_id}/messages"),
                Some(&fixture.token),
                None,
            )
            .await;
        assert_eq!(messages.status(), StatusCode::OK);
        let history = Fixture::response_json(messages).await;
        assert!(history["items"].as_array().unwrap().is_empty());
        assert!(history["next_before_id"].is_null());
    }

    #[tokio::test]
    async fn conversation_cursor_pages_are_stable_and_do_not_repeat_items() {
        let fixture = Fixture::new();
        for index in 0..3 {
            let response = fixture
                .send(
                    Method::POST,
                    "/api/host/v1/conversations",
                    Some(&fixture.token),
                    Some(&format!(r#"{{"title":"Page {index}"}}"#)),
                )
                .await;
            assert_eq!(response.status(), StatusCode::CREATED);
        }

        let first = fixture
            .send(
                Method::GET,
                "/api/host/v1/conversations?limit=1",
                Some(&fixture.token),
                None,
            )
            .await;
        let first_page = Fixture::response_json(first).await;
        let first_id = first_page["items"][0]["id"].as_str().unwrap().to_owned();
        let cursor = first_page["next_cursor"]
            .as_str()
            .expect("continuation cursor");

        let second = fixture
            .send(
                Method::GET,
                &format!("/api/host/v1/conversations?limit=1&cursor={cursor}"),
                Some(&fixture.token),
                None,
            )
            .await;
        assert_eq!(second.status(), StatusCode::OK);
        let second_page = Fixture::response_json(second).await;
        let second_id = second_page["items"][0]["id"].as_str().unwrap();
        assert_ne!(first_id, second_id);
        assert_eq!(second_page["items"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn message_history_uses_bounded_pages_in_chronological_order() {
        let fixture = Fixture::new();
        let created = fixture
            .send(
                Method::POST,
                "/api/host/v1/conversations",
                Some(&fixture.token),
                Some(r#"{"title":"History fixture"}"#),
            )
            .await;
        let conversation_id = Fixture::response_json(created).await["id"]
            .as_str()
            .expect("conversation ID")
            .to_owned();
        {
            let conn = super::super::open_db(&fixture.database_path).expect("open fixture DB");
            for index in 1..=3 {
                conn.execute(
                    "INSERT INTO messages(conversation_id, role, content, created_at) VALUES(?1, 'user', ?2, ?3)",
                    rusqlite::params![conversation_id, format!("message {index}"), index as f64],
                ).expect("insert fixture message");
            }
        }

        let recent = fixture
            .send(
                Method::GET,
                &format!("/api/host/v1/conversations/{conversation_id}/messages?limit=2"),
                Some(&fixture.token),
                None,
            )
            .await;
        let recent_page = Fixture::response_json(recent).await;
        assert_eq!(recent_page["items"].as_array().unwrap().len(), 2);
        assert_eq!(recent_page["items"][0]["content"], "message 2");
        assert_eq!(recent_page["items"][1]["content"], "message 3");
        let before_id = recent_page["next_before_id"]
            .as_i64()
            .expect("older-page cursor");

        let older = fixture.send(
            Method::GET,
            &format!("/api/host/v1/conversations/{conversation_id}/messages?limit=2&before_id={before_id}"),
            Some(&fixture.token),
            None,
        ).await;
        let older_page = Fixture::response_json(older).await;
        assert_eq!(older_page["items"].as_array().unwrap().len(), 1);
        assert_eq!(older_page["items"][0]["content"], "message 1");
        assert!(older_page["next_before_id"].is_null());
    }

    #[tokio::test]
    async fn unconfigured_router_errors_are_sanitized_for_the_mobile_client() {
        let fixture = Fixture::new();
        let created = fixture
            .send(
                Method::POST,
                "/api/host/v1/conversations",
                Some(&fixture.token),
                Some(r#"{"title":"Chat error fixture"}"#),
            )
            .await;
        let conversation_id = Fixture::response_json(created).await["id"]
            .as_str()
            .expect("conversation ID")
            .to_owned();

        let response = fixture
            .send(
                Method::POST,
                "/api/host/v1/chat",
                Some(&fixture.token),
                Some(&format!(
                    r#"{{"conversation_id":"{conversation_id}","message":"hello"}}"#
                )),
            )
            .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let data = Fixture::response_json(response).await;
        assert_eq!(data["error"]["code"], "host_not_configured");
        let public_body = data.to_string();
        assert!(!public_body.contains("localhost:3001"));
        assert!(!public_body.contains("API key"));
    }

    #[tokio::test]
    async fn malformed_cursor_and_missing_conversation_use_stable_errors() {
        let fixture = Fixture::new();
        let bad_cursor = fixture
            .send(
                Method::GET,
                "/api/host/v1/conversations?cursor=not-a-cursor",
                Some(&fixture.token),
                None,
            )
            .await;
        assert_eq!(bad_cursor.status(), StatusCode::BAD_REQUEST);
        assert!(!bad_cursor.headers().contains_key(INTERNAL_ERROR_MARKER));
        let data = Fixture::response_json(bad_cursor).await;
        assert_eq!(data["error"]["code"], "invalid_request");

        let missing = fixture
            .send(
                Method::GET,
                "/api/host/v1/conversations/00000000-0000-4000-8000-000000000000/messages",
                Some(&fixture.token),
                None,
            )
            .await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            Fixture::response_json(missing).await["error"]["code"],
            "not_found"
        );
    }

    #[tokio::test]
    async fn malformed_json_body_is_rejected_before_conversation_creation() {
        let fixture = Fixture::new();
        let response = fixture
            .send(
                Method::POST,
                "/api/host/v1/conversations",
                Some(&fixture.token),
                Some("{not-json"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let data = Fixture::response_json(response).await;
        assert_eq!(data["error"]["code"], "invalid_request");

        let conn = open_db(&fixture.database_path).expect("open fixture DB");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get(0))
            .expect("count conversations");
        assert_eq!(count, 0, "malformed JSON must not create a conversation");
    }

    #[tokio::test]
    async fn oversized_json_request_is_rejected() {
        let fixture = Fixture::new();
        let oversized = format!(r#"{{"title":"{}"}}"#, "x".repeat(MAX_REQUEST_BODY_BYTES));
        let response = fixture
            .send(
                Method::POST,
                "/api/host/v1/conversations",
                Some(&fixture.token),
                Some(&oversized),
            )
            .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let data = Fixture::response_json(response).await;
        assert_eq!(data["error"]["code"], "payload_too_large");
    }

    #[tokio::test]
    async fn model_list_requires_router_configuration_and_returns_safe_error() {
        let fixture = Fixture::new();
        let response = fixture
            .send(
                Method::GET,
                "/api/host/v1/models",
                Some(&fixture.token),
                None,
            )
            .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let data = Fixture::response_json(response).await;
        assert_eq!(data["error"]["code"], "host_not_configured");
        let body = data.to_string();
        assert!(!body.contains("localhost:3001"));
        assert!(!body.contains("API key"));
    }

    async fn fake_router_models(headers: axum::http::HeaderMap) -> Json<Value> {
        assert_eq!(
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer router-test-secret")
        );
        Json(json!({
            "data": [
                {"id": "model-a", "name": "Model A", "owned_by": "private-provider", "created": 123},
                {"id": "model-a", "name": "Duplicate"},
                {"id": "model-b", "name": "Model B", "extra_private_field": "must-not-leak"},
                {"id": "", "name": "Invalid"}
            ],
            "private_metadata": "must-not-leak"
        }))
    }

    #[tokio::test]
    async fn model_endpoint_returns_bounded_allowlisted_fields_only() {
        let fixture = Fixture::new();
        let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock router");
        let upstream_address = upstream_listener.local_addr().expect("mock router address");
        let upstream = Router::new().route("/v1/models", get(fake_router_models));
        tokio::spawn(async move {
            axum::serve(upstream_listener, upstream)
                .await
                .expect("serve mock router");
        });

        let config = json!({
            "base_url": format!("http://{upstream_address}/v1"),
            "model": "auto",
            "api_key": "router-test-secret"
        });
        fs::write(
            &fixture.config_path,
            serde_json::to_vec_pretty(&config).expect("serialize fixture config"),
        )
        .expect("write fixture config");

        let response = fixture
            .send(
                Method::GET,
                "/api/host/v1/models",
                Some(&fixture.token),
                None,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let data = Fixture::response_json(response).await;
        assert_eq!(data["api_version"], API_VERSION);
        assert_eq!(data["count"], 2);
        assert_eq!(data["data"][0]["id"], "model-a");
        assert_eq!(data["data"][0]["name"], "Model A");
        assert!(data["data"][0].get("owned_by").is_none());
        assert!(data["data"][1].get("extra_private_field").is_none());
        let public_body = data.to_string();
        assert!(!public_body.contains("private-provider"));
        assert!(!public_body.contains("must-not-leak"));
        assert!(!public_body.contains("router-test-secret"));
    }
}
