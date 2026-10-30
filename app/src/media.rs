//! Attachments and the embeddings probe.
//!
//! Attachments are uploaded once, stored as files beside the SQLite data file,
//! and referenced inside chat messages by `att-<uuid>.<ext>`. The DB keeps only
//! the reference: replaying history never re-uploads media, and the packed
//! provider context carries media only on the message it was attached to.
//!
//! Start small and honest: image, audio and video are accepted, but delivery to
//! the provider uses the OpenAI-style multimodal content shape, so video works
//! only with a router/model that actually speaks it — a provider rejection
//! surfaces as a clear chat error, not a fake success.

use crate::AppState;
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::path::PathBuf;

pub(crate) const MAX_ATTACHMENTS_PER_MESSAGE: usize = 4;

const IMAGE_MIME: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const AUDIO_MIME: [&str; 4] = ["audio/wav", "audio/mpeg", "audio/ogg", "audio/mp4"];
const VIDEO_MIME: [&str; 2] = ["video/mp4", "video/webm"];
/// Bytes on disk, not base64 characters.
const IMAGE_LIMIT: usize = 6 * 1024 * 1024;
const AUDIO_LIMIT: usize = 12 * 1024 * 1024;
const VIDEO_LIMIT: usize = 40 * 1024 * 1024;

/// Mutating routes are local-origin guarded like every other POST route; the
/// attachment file server is read-only and therefore outside the guard.
pub(crate) fn router(port: u16) -> Router<crate::AppState> {
    let guarded = Router::new()
        .route("/api/attachments", post(save_attachment))
        .route("/api/embeddings", post(embeddings))
        .route_layer(axum::middleware::from_fn_with_state(
            crate::workspace::LocalOriginGuard::new(port),
            crate::workspace::guard_local_origin,
        ));
    let read_only = Router::new().route("/attachments/:file", get(serve_attachment));
    guarded.merge(read_only)
}

/// Base64 decode without pulling in another crate. Standard alphabet with or
/// without padding; anything else (including whitespace) is rejected.
pub(crate) fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some((byte - b'A') as u32),
            b'a'..=b'z' => Some((byte - b'a') as u32 + 26),
            b'0'..=b'9' => Some((byte - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let body = input.trim().trim_end_matches('=');
    if body.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(body.len() / 4 * 3 + 3);
    for group in body.as_bytes().chunks(4) {
        if group.len() == 1 {
            return None;
        }
        let mut bits = 0u32;
        let mut held = 0u32;
        for byte in group.iter() {
            held = (held << 6) | value(*byte)?;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push(((held >> bits) & 0xFF) as u8);
            }
        }
    }
    Some(out)
}

pub(crate) fn attachment_dir(state: &AppState) -> PathBuf {
    let dir = state.database_path.with_file_name("attachments");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn kind_limits(kind: &str) -> Option<(&[&'static str], usize)> {
    match kind {
        "image" => Some((&IMAGE_MIME, IMAGE_LIMIT)),
        "audio" => Some((&AUDIO_MIME, AUDIO_LIMIT)),
        "video" => Some((&VIDEO_MIME, VIDEO_LIMIT)),
        _ => None,
    }
}

fn mime_for_name(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

/// POST /api/attachments — {"kind":"image","data_url":"data:image/png;base64,…"}
/// Returns a server-named reference; the DB stores just that string.
async fn save_attachment(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let kind = input.get("kind").and_then(Value::as_str).unwrap_or("");
    let data_url = input.get("data_url").and_then(Value::as_str).unwrap_or("");
    let (allowed, limit) = match kind_limits(kind) {
        Some(found) => found,
        None => return bad("Attachment kind must be image, audio or video."),
    };
    let (mime, payload) = match data_url.split_once(";base64,") {
        Some((rest, payload)) => match rest.strip_prefix("data:") {
            Some(mime) => (mime, payload),
            None => return bad("The attachment must be a data: URL."),
        },
        None => return bad("The attachment must be a base64 data: URL."),
    };
    if !allowed.contains(&mime) {
        return bad(&format!(
            "{kind} attachments must use one of: {}.",
            allowed.join(", ")
        ));
    }
    let bytes = match decode_base64(payload) {
        Some(bytes) => bytes,
        None => return bad("The attachment payload is not valid base64."),
    };
    if bytes.is_empty() {
        return bad("The attachment payload is empty.");
    }
    if bytes.len() > limit {
        return bad(&format!(
            "{kind} attachments are limited to {} MiB.",
            limit / (1024 * 1024)
        ));
    }
    let ext = match mime.split('/').nth(1).unwrap_or("bin") {
        "jpeg" => "jpg",
        "mpeg" => "mp3",
        other => other,
    };
    let name = format!("att-{}.{ext}", uuid::Uuid::new_v4());
    match tokio::fs::write(attachment_dir(&state).join(&name), &bytes).await {
        Ok(_) => Json(json!({
            "attachment": {
                "kind": kind,
                "ref": name,
                "size": bytes.len(),
                "mime": mime,
            }
        }))
        .into_response(),
        Err(_) => crate::json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not store the attachment.",
        ),
    }
}

/// GET /attachments/:file — serves a stored attachment by exact filename.
/// Names are server-generated, so no traversal or arbitrary read is possible.
async fn serve_attachment(
    State(state): State<AppState>,
    AxumPath(file): AxumPath<String>,
) -> Response<Body> {
    if !is_valid_attachment_name(&file) {
        return not_found();
    }
    match std::fs::read(attachment_dir(&state).join(&file)) {
        Ok(bytes) => {
            let mut response = (StatusCode::OK, bytes).into_response();
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(mime_for_name(&file)),
            );
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=3600"),
            );
            response
        }
        Err(_) => not_found(),
    }
}

fn not_found() -> Response<Body> {
    crate::json_error(StatusCode::NOT_FOUND, "Attachment not found.")
}

fn bad(message: &str) -> Response<Body> {
    crate::json_error(StatusCode::BAD_REQUEST, message)
}

/// Server-generated name, checked before any filesystem touch:
/// `att-` + a 36-character UUID + `.` + a known extension.
pub(crate) fn is_valid_attachment_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() != 44 && bytes.len() != 45 {
        return false;
    }
    bytes.starts_with(b"att-")
        && bytes[4..40]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        && matches!(
            &name[41..],
            "png" | "jpg" | "webp" | "gif" | "wav" | "mp3" | "ogg" | "m4a" | "mp4" | "webm"
        )
}

/// Validate an attachments array sent with a chat message and load the bytes.
pub(crate) fn resolve_attachments(
    state: &AppState,
    input: &Value,
) -> Result<Vec<(String, String, Vec<u8>)>, String> {
    let Some(items) = input.get("attachments").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    if items.len() > MAX_ATTACHMENTS_PER_MESSAGE {
        return Err(format!(
            "A message can carry at most {MAX_ATTACHMENTS_PER_MESSAGE} attachments."
        ));
    }
    let mut out = Vec::new();
    for item in items {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        let name = item.get("ref").and_then(Value::as_str).unwrap_or("");
        if kind_limits(kind).is_none() {
            return Err("Attachment kind must be image, audio or video.".into());
        }
        if !is_valid_attachment_name(name) {
            return Err("Attachment reference is invalid.".into());
        }
        let path: PathBuf = attachment_dir(state).join(name);
        let bytes = std::fs::read(&path).map_err(|_| "Attachment file is missing.".to_string())?;
        out.push((kind.to_string(), mime_for_name(name).to_string(), bytes));
    }
    Ok(out)
}

/// Fold resolved attachments into the newest user message of a packed context.
/// Older history stays text-only: replaying every image into every request
/// would burn context, and most routers re-reject stale media anyway.
pub(crate) fn apply_media(messages: &mut [Value], attachments: &[(String, String, Vec<u8>)]) {
    if attachments.is_empty() {
        return;
    }
    let index = messages
        .iter()
        .rposition(|m| m.get("role").and_then(Value::as_str) == Some("user"));
    let Some(index) = index else { return };
    let text = messages[index]
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut parts = Vec::new();
    for (kind, mime, bytes) in attachments {
        let data_url = format!("data:{mime};base64,{}", encode_base64(bytes));
        match kind.as_str() {
            "image" => parts.push(json!({ "type": "image_url", "image_url": { "url": data_url } })),
            "audio" => parts.push(json!({
                "type": "input_audio",
                "input_audio": { "data": encode_base64(bytes), "format": mime.trim_start_matches("audio/") }
            })),
            // OpenAI-compatible video ("video_url") is not a settled form; a
            // provider without it answers with a clear upstream error.
            "video" => parts.push(json!({ "type": "video_url", "video_url": { "url": data_url } })),
            _ => {}
        }
    }
    parts.insert(0, json!({ "type": "text", "text": text }));
    messages[index]["content"] = Value::Array(parts);
}

/// Base64 encode used to re-serialise stored media for the provider request.
pub(crate) fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | (*chunk.get(2).unwrap_or(&0) as u32);
        out.push(ALPHABET[(word >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(word >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(word >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(word & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// POST /api/embeddings — {"input":"text","model":""} → a real vector probe
/// through the configured router, so the UI can show actual dimensions and
/// token usage instead of inventing an embedding.
async fn embeddings(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    let Some(text) = input.get("input").and_then(Value::as_str) else {
        return crate::json_error(StatusCode::BAD_REQUEST, "Provide text to embed.");
    };
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 8000 {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "Embedding text must be 1–8,000 characters.",
        );
    }
    let cfg = crate::load_config(&state);
    if cfg.api_key.trim().is_empty() {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "Add your router's unified API key in Settings first.",
        );
    }
    let mut body = json!({ "input": text });
    if let Some(model) = input
        .get("model")
        .and_then(Value::as_str)
        .filter(|m| !m.trim().is_empty())
    {
        body["model"] = json!(model.trim());
    }
    let response = state
        .client
        .post(crate::router_url(&cfg, "embeddings"))
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return crate::json_error(
                StatusCode::BAD_GATEWAY,
                crate::reach_error(&cfg.base_url, error.to_string()),
            );
        }
    };
    let status = response.status();
    let parsed: Value =
        serde_json::from_str(&response.text().await.unwrap_or_default()).unwrap_or(Value::Null);
    if !status.is_success() {
        let detail = parsed
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("The provider rejected the embeddings request.");
        return crate::json_error(status, detail);
    }
    let Some(items) = parsed.get("data").and_then(Value::as_array) else {
        return crate::json_error(
            StatusCode::BAD_GATEWAY,
            "The provider returned no embedding.",
        );
    };
    let dims = items
        .first()
        .and_then(|first| first.get("embedding"))
        .and_then(Value::as_array)
        .map(|vector| vector.len())
        .unwrap_or(0);
    let usage = parsed.get("usage").cloned().unwrap_or_else(|| json!({}));
    Json(json!({
        "ok": true,
        "dims": dims,
        "count": items.len(),
        "usage": usage,
        "model": parsed.get("model").and_then(Value::as_str).unwrap_or(""),
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_round_trips() {
        for sample in ["", "a", "ab", "abc", "hello world", "\u{1F680}"] {
            let encoded = encode_base64(sample.as_bytes());
            assert_eq!(decode_base64(&encoded).as_deref(), Some(sample.as_bytes()));
        }
    }

    #[test]
    fn base64_rejects_broken_input() {
        assert!(decode_base64("!!!====").is_none());
        assert!(decode_base64("A").is_none());
        assert!(decode_base64("AAA A").is_none());
    }

    #[test]
    fn media_folds_into_the_newest_user_message() {
        let mut messages = vec![
            json!({"role": "user", "content": "older"}),
            json!({"role": "assistant", "content": "older answer"}),
            json!({"role": "user", "content": "look at this"}),
        ];
        apply_media(
            &mut messages,
            &[("image".into(), "image/png".into(), vec![1, 2, 3])],
        );
        let newest = &messages[2];
        let parts = newest.get("content").and_then(Value::as_array).unwrap();
        assert_eq!(
            parts[0],
            json!({"type": "text", "text": "look at this"}),
            "text rides with the media"
        );
        assert_eq!(parts[1]["type"], "image_url");
        assert!(
            messages[0]["content"].is_string(),
            "history stays text-only"
        );
    }

    #[test]
    fn media_with_no_attachments_changes_nothing() {
        let mut messages = vec![json!({"role": "user", "content": "plain"})];
        apply_media(&mut messages, &[]);
        assert_eq!(messages[0]["content"], json!("plain"));
    }
    #[test]
    fn attachment_names_are_strict() {
        assert!(is_valid_attachment_name(
            "att-3fa85f64-5717-4562-b3fc-2c963f66afa6.png"
        ));
        assert!(is_valid_attachment_name(
            "att-3fa85f64-5717-4562-b3fc-2c963f66afa6.webm"
        ));
        assert!(!is_valid_attachment_name("../etc/passwd"));
        assert!(!is_valid_attachment_name(
            "att-3fa85f64-5717-4562-b3fc-2c963f66afa6.png.zip"
        ));
        assert!(!is_valid_attachment_name("att-short.png"));
        assert!(!is_valid_attachment_name(
            "att-3fa85f64-5717-4562-b3fc-2c963f66afa6.txt"
        ));
    }
}
