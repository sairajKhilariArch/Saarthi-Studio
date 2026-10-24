//! FreeLLMAPI admin console bridge.
//!
//! FreeLLMAPI exposes two APIs on one port: the OpenAI-compatible proxy
//! (`/v1/...`, unlocked with the unified API key) and the dashboard admin API
//! (`/api/...`, unlocked with a dashboard session). The desktop app already
//! talks to the proxy; this module adds the dashboard half so providers, keys,
//! model chains, analytics, logs and the free-tier budget can be managed from
//! the app instead of a second browser tab.
//!
//! Sessions are obtained the way FreeLLMAPI's own desktop client does it:
//! `POST /api/auth/desktop-session`, which the router only allows over
//! loopback. If that is unavailable (a server install with an email/password
//! account), the user signs in once and the token is cached in a 0600 file.
//! Everything here is loopback-only and never leaves this machine.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// Admin API families this bridge will forward. Anything else is refused, so a
/// bug in the UI cannot turn the bridge into an open proxy for the router.
const ALLOWED_PREFIXES: [&str; 20] = [
    "analytics",
    "backups",
    "cache",
    "client-profiles",
    "compression",
    "conversations",
    "decisions",
    "embeddings",
    "fallback",
    "free-tier",
    "health",
    "keys",
    "logs",
    "media",
    "models",
    "premium",
    "profiles",
    "search",
    "settings",
    "update",
];

/// Request bodies are tiny JSON documents; refuse anything larger.
const MAX_BODY_BYTES: u64 = 256 * 1024;
const SESSION_FILE: &str = "router-session.json";

fn http() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

fn origin() -> String {
    format!("http://127.0.0.1:{}", crate::router::router_port())
}

fn session_path() -> PathBuf {
    crate::home_dir()
        .join(".config/free-ai-studio")
        .join(SESSION_FILE)
}

fn write_private(path: &PathBuf, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let temp = path.with_extension("tmp");
    if fs::write(&temp, bytes).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&temp, fs::Permissions::from_mode(0o600));
        }
        let _ = fs::rename(&temp, path);
    }
}

fn read_token() -> Option<String> {
    let bytes = fs::read(session_path()).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

pub(crate) fn store_token(token: &str) {
    let payload = json!({ "token": token, "saved_at": crate::now_seconds() });
    if let Ok(bytes) = serde_json::to_vec(&payload) {
        write_private(&session_path(), &bytes);
    }
}

fn clear_token() {
    let _ = fs::remove_file(session_path());
}

/// Pull a session token out of whatever shape the router answers with. The
/// dashboard has used `token`, `sessionToken` and a nested `data.session` over
/// time, so accept any of them rather than pinning one.
fn extract_token(value: &Value) -> Option<String> {
    const KEYS: [&str; 6] = [
        "token",
        "sessionToken",
        "session_token",
        "session",
        "accessToken",
        "access_token",
    ];
    fn walk(value: &Value, depth: usize) -> Option<String> {
        if depth > 3 {
            return None;
        }
        match value {
            Value::Object(map) => {
                for key in KEYS {
                    if let Some(Value::String(token)) = map.get(key) {
                        if !token.trim().is_empty() {
                            return Some(token.trim().to_string());
                        }
                    }
                }
                map.values().find_map(|inner| walk(inner, depth + 1))
            }
            Value::Array(items) => items.iter().find_map(|inner| walk(inner, depth + 1)),
            _ => None,
        }
    }
    walk(value, 0)
}

/// Normalise a UI-supplied admin path: strip leading slashes and `/api/`, reject
/// traversal and empty segments, and check the family against the allowlist.
pub(crate) fn sanitize_path(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_start_matches('/');
    let trimmed = trimmed.strip_prefix("api/").unwrap_or(trimmed);
    let trimmed = trimmed.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Missing router API path.".into());
    }
    if trimmed.len() > 300 {
        return Err("Router API path is too long.".into());
    }
    let mut segments = Vec::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() {
            return Err("Router API path has an empty segment.".into());
        }
        if segment == "." || segment == ".." {
            return Err("Router API path must stay inside the admin API.".into());
        }
        if !segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '~'))
        {
            return Err("Router API path contains unsupported characters.".into());
        }
        segments.push(segment);
    }
    let family = segments[0];
    if !ALLOWED_PREFIXES.contains(&family) {
        return Err(format!(
            "`{family}` is not an admin API family this app manages."
        ));
    }
    Ok(segments.join("/"))
}

pub(crate) fn method_name(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        _ => "GET",
    }
}

/// Ask for a loopback desktop session. Returns a token or an explanation.
async fn request_desktop_session() -> Result<String, String> {
    let response = http()
        .post(format!("{}/api/auth/desktop-session", origin()))
        .json(&json!({}))
        .send()
        .await
        .map_err(|error| format!("Could not reach the router: {error}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if let Some(token) = extract_token(&body) {
        store_token(&token);
        return Ok(token);
    }
    if status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED {
        return Err(
            "This router build has no desktop session; sign in with its dashboard account.".into(),
        );
    }
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(
            "The router refused a desktop session; sign in with its dashboard account.".into(),
        );
    }
    Err(format!(
        "The router did not return a session ({}).",
        body.get("error")
            .and_then(Value::as_str)
            .unwrap_or_else(|| status.as_str())
    ))
}

async fn current_token() -> Result<String, String> {
    if let Some(token) = read_token() {
        return Ok(token);
    }
    request_desktop_session().await
}

/// `POST /api/router/console/login` — email/password sign-in for server installs.
#[derive(Deserialize)]
pub(crate) struct LoginBody {
    email: String,
    password: String,
}

pub(crate) async fn login(Json(body): Json<LoginBody>) -> Response<Body> {
    if body.email.trim().is_empty() || body.password.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Enter the router dashboard email and password.",
        );
    }
    let response = match http()
        .post(format!("{}/api/auth/login", origin()))
        .json(&json!({ "email": body.email.trim(), "password": body.password }))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            return json_error(
                StatusCode::BAD_GATEWAY,
                format!("Could not reach the router: {error}"),
            )
        }
    };
    let status = response.status();
    let payload: Value = response.json().await.unwrap_or(Value::Null);
    if let Some(token) = extract_token(&payload) {
        store_token(&token);
        return Json(json!({ "ok": true })).into_response();
    }
    let message = payload
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            if status == StatusCode::UNAUTHORIZED {
                "Wrong email or password.".to_string()
            } else {
                format!("The router rejected the sign-in ({status}).")
            }
        });
    json_error(StatusCode::UNAUTHORIZED, message)
}

/// `GET /api/router/console/status`
pub(crate) async fn status(State(_state): State<crate::AppState>) -> Response<Body> {
    let reachable = crate::router::reachable(crate::router::router_port()).await;
    if !reachable {
        return Json(json!({
            "ok": true,
            "reachable": false,
            "authenticated": false,
            "mode": "offline",
            "message": "The router is not running.",
        }))
        .into_response();
    }
    match current_token().await {
        Ok(_) => Json(json!({
            "ok": true,
            "reachable": true,
            "authenticated": true,
            "mode": "ready",
            "message": "Router console is connected.",
        }))
        .into_response(),
        Err(message) => Json(json!({
            "ok": true,
            "reachable": true,
            "authenticated": false,
            "mode": "login",
            "message": message,
        }))
        .into_response(),
    }
}

/// `ANY /api/router/console/*path` — authenticated passthrough to the admin API.
pub(crate) async fn proxy(
    State(_state): State<crate::AppState>,
    Path(path): Path<String>,
    method: Method,
    body: Body,
) -> Response<Body> {
    let path = match sanitize_path(&path) {
        Ok(path) => path,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    if !crate::router::reachable(crate::router::router_port()).await {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "The router is not running.",
        );
    }

    let payload = match axum::body::to_bytes(body, MAX_BODY_BYTES as usize).await {
        Ok(bytes) => bytes,
        Err(_) => return json_error(StatusCode::PAYLOAD_TOO_LARGE, "Request body is too large."),
    };

    let token = match current_token().await {
        Ok(token) => token,
        Err(message) => return json_error(StatusCode::UNAUTHORIZED, message),
    };

    let mut attempt = 0;
    loop {
        attempt += 1;
        let mut request = http()
            .request(
                match method_name(&method) {
                    "POST" => Method::POST,
                    "PUT" => Method::PUT,
                    "PATCH" => Method::PATCH,
                    "DELETE" => Method::DELETE,
                    _ => Method::GET,
                },
                format!("{}/api/{path}", origin()),
            )
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header("x-dashboard-token", &token);
        if !matches!(method, Method::GET) {
            let json_body = if payload.is_empty() {
                json!({})
            } else {
                serde_json::from_slice(&payload).unwrap_or(Value::Null)
            };
            request = request.json(&json_body);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                return json_error(
                    StatusCode::BAD_GATEWAY,
                    format!("Router request failed: {error}"),
                )
            }
        };

        // A stale cached session is common after a router restart: drop it,
        // mint a fresh desktop session, and retry exactly once.
        if response.status() == StatusCode::UNAUTHORIZED && attempt == 1 {
            clear_token();
            match request_desktop_session().await {
                Ok(_) => continue,
                Err(message) => return json_error(StatusCode::UNAUTHORIZED, message),
            }
        }

        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/json")
            .to_string();
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                return json_error(
                    StatusCode::BAD_GATEWAY,
                    format!("Router response failed: {error}"),
                )
            }
        };
        return Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(bytes))
            .unwrap_or_else(|_| {
                json_error(
                    StatusCode::BAD_GATEWAY,
                    "Could not read the router response.",
                )
                .into_response()
            });
    }
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response<Body> {
    (
        status,
        Json(json!({ "ok": false, "error": message.into() })),
    )
        .into_response()
}

/// Local router for the console endpoints. Mounted behind the same loopback
/// origin guard as the rest of the local UI.
pub(crate) fn router(port: u16) -> axum::Router<crate::AppState> {
    use axum::routing::{any, get, post};
    axum::Router::new()
        .route("/api/router/console/status", get(status))
        .route("/api/router/console/login", post(login))
        .route("/api/router/console/*path", any(proxy))
        .route_layer(axum::middleware::from_fn_with_state(
            crate::workspace::LocalOriginGuard::new(port),
            crate::workspace::guard_local_origin,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_tokens_from_known_shapes() {
        assert_eq!(
            extract_token(&json!({ "token": "abc" })).as_deref(),
            Some("abc")
        );
        assert_eq!(
            extract_token(&json!({ "sessionToken": " s1 " })).as_deref(),
            Some("s1")
        );
        assert_eq!(
            extract_token(&json!({ "data": { "session": { "access_token": "deep" } } })).as_deref(),
            Some("deep")
        );
        assert_eq!(
            extract_token(&json!({ "user": { "email": "a@b.c" } })),
            None
        );
        assert_eq!(extract_token(&json!({ "token": "   " })), None);
        assert_eq!(extract_token(&Value::Null), None);
    }

    #[test]
    fn sanitizes_admin_paths() {
        assert_eq!(sanitize_path("/api/keys").unwrap(), "keys");
        assert_eq!(sanitize_path("api/keys/12").unwrap(), "keys/12");
        assert_eq!(sanitize_path("models/").unwrap(), "models");
        assert_eq!(
            sanitize_path("fallback/routing").unwrap(),
            "fallback/routing"
        );
        assert!(sanitize_path("logs?x=1").is_err());
    }

    #[test]
    fn refuses_paths_outside_the_admin_api() {
        for bad in [
            "",
            "/",
            "api/",
            "v1/chat/completions",
            "../keys",
            "keys/../../auth",
            "keys//12",
            "keys/12/../../settings",
            "auth/login",
        ] {
            assert!(sanitize_path(bad).is_err(), "{bad} should be refused");
        }
        let long = "a".repeat(400);
        assert!(sanitize_path(&format!("keys/{long}")).is_err());
    }

    #[test]
    fn maps_http_methods() {
        assert_eq!(method_name(&Method::GET), "GET");
        assert_eq!(method_name(&Method::PATCH), "PATCH");
        assert_eq!(method_name(&Method::DELETE), "DELETE");
    }
}
