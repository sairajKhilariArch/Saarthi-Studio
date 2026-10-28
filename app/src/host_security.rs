//! Resource-abuse controls and security headers for the remote host API.
//!
//! Version 1 intentionally uses one process-wide request budget because the
//! desktop host API supports a single trusted companion profile. Do not trust
//! X-Forwarded-For headers: this service has no trusted reverse-proxy contract.
//! If multi-device access is added later, replace this with limits keyed by an
//! authenticated device/token identity and transport peer address.

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderValue, Request, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

const DEFAULT_REQUESTS_PER_WINDOW: usize = 120;
const DEFAULT_WINDOW: Duration = Duration::from_secs(60);
const DEFAULT_MAX_IN_FLIGHT: usize = 8;

#[derive(Clone)]
pub(super) struct HostApiSecurity {
    window: Arc<Mutex<RateWindow>>,
    requests_per_window: usize,
    window_size: Duration,
    in_flight: Arc<Semaphore>,
}

struct RateWindow {
    started_at: Instant,
    accepted: usize,
}

impl Default for HostApiSecurity {
    fn default() -> Self {
        Self::with_limits(
            DEFAULT_REQUESTS_PER_WINDOW,
            DEFAULT_WINDOW,
            DEFAULT_MAX_IN_FLIGHT,
        )
    }
}

impl HostApiSecurity {
    fn with_limits(
        requests_per_window: usize,
        window_size: Duration,
        max_in_flight: usize,
    ) -> Self {
        assert!(requests_per_window > 0, "request limit must be positive");
        assert!(!window_size.is_zero(), "rate-limit window must be positive");
        assert!(max_in_flight > 0, "in-flight limit must be positive");
        Self {
            window: Arc::new(Mutex::new(RateWindow {
                started_at: Instant::now(),
                accepted: 0,
            })),
            requests_per_window,
            window_size,
            in_flight: Arc::new(Semaphore::new(max_in_flight)),
        }
    }

    async fn reserve_request(&self) -> Result<(), u64> {
        let now = Instant::now();
        let mut window = self.window.lock().await;
        let elapsed = now.saturating_duration_since(window.started_at);
        if elapsed >= self.window_size {
            window.started_at = now;
            window.accepted = 0;
        }

        if window.accepted >= self.requests_per_window {
            let elapsed = now.saturating_duration_since(window.started_at);
            let remaining = self.window_size.saturating_sub(elapsed);
            let seconds = remaining
                .as_secs()
                .saturating_add((remaining.subsec_nanos() > 0) as u64)
                .max(1);
            return Err(seconds);
        }
        window.accepted += 1;
        Ok(())
    }

    fn try_acquire_in_flight(&self) -> Option<OwnedSemaphorePermit> {
        self.in_flight.clone().try_acquire_owned().ok()
    }
}

/// Add process-wide request limiting, bounded concurrency, and security headers.
/// This must wrap the authenticated host router before it is served over TLS.
pub(super) fn harden_router(router: Router) -> Router {
    harden_router_with(router, HostApiSecurity::default())
}

fn harden_router_with(router: Router, state: HostApiSecurity) -> Router {
    router.layer(from_fn_with_state(state, enforce_host_api_limits))
}

async fn enforce_host_api_limits(
    State(state): State<HostApiSecurity>,
    request: Request<Body>,
    next: Next,
) -> Response<Body> {
    if let Err(retry_after) = state.reserve_request().await {
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": {
                    "code": "rate_limited",
                    "message": "The desktop host is receiving too many requests. Retry later."
                }
            })),
        )
            .into_response();
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_str(&retry_after.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("1")),
        );
        return add_security_headers(response);
    }

    let permit = match state.try_acquire_in_flight() {
        Some(permit) => permit,
        None => {
            let mut response = (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": {
                        "code": "host_busy",
                        "message": "The desktop host is busy. Retry shortly."
                    }
                })),
            )
                .into_response();
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            return add_security_headers(response);
        }
    };

    let response = next.run(request).await;
    // The remote API currently returns finite JSON responses, not streaming bodies.
    // The permit bounds concurrent handler execution and outbound chat waits.
    drop(permit);
    add_security_headers(response)
}

fn add_security_headers(mut response: Response<Body>) -> Response<Body> {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{host_api, host_auth::HostTokenStore, AppState};
    use axum::{
        body::to_bytes,
        http::{header, Method, Request},
        routing::get,
    };
    use std::{fs, path::PathBuf};
    use tower::ServiceExt;
    use uuid::Uuid;

    struct Fixture {
        root: PathBuf,
        token: String,
        app: Router,
    }

    impl Fixture {
        fn new(request_limit: usize) -> Self {
            let root =
                std::env::temp_dir().join(format!("saarthi-host-security-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).expect("create fixture directory");
            super::super::set_private_permissions(&root, true).expect("secure fixture directory");
            let config_path = root.join("config.json");
            let database_path = root.join("history.sqlite3");
            let state = AppState {
                config_path,
                database_path,
                static_dir: root.join("static"),
                workspace: crate::workspace::WorkspaceStore::load(root.join("workspace.json")),
                client: reqwest::Client::builder()
                    .timeout(Duration::from_secs(2))
                    .build()
                    .expect("build test HTTP client"),
                terminal: crate::terminal::hub(),
                models_cache: std::sync::Arc::new(std::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
            };
            super::super::ensure_setup(&state).expect("initialize fixture config and database");
            let token_store = HostTokenStore::new(root.join("host-auth.json"));
            let token = token_store.create().expect("create test token");
            let base = host_api::build_router(state, token_store);
            let app = harden_router_with(
                base,
                HostApiSecurity::with_limits(request_limit, Duration::from_secs(60), 8),
            );
            Self { root, token, app }
        }

        async fn send(&self, method: Method, uri: &str, token: Option<&str>) -> Response<Body> {
            let mut builder = Request::builder().method(method).uri(uri);
            if let Some(token) = token {
                builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            self.app
                .clone()
                .oneshot(builder.body(Body::empty()).expect("build request"))
                .await
                .expect("execute request")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn security_headers_are_added_to_success_and_authentication_errors() {
        let fixture = Fixture::new(10);
        for token in [None, Some(fixture.token.as_str())] {
            let response = fixture
                .send(Method::GET, "/api/host/v1/health", token)
                .await;
            assert_eq!(
                response.headers()[header::CACHE_CONTROL].to_str().unwrap(),
                "no-store"
            );
            assert_eq!(
                response.headers()["x-content-type-options"]
                    .to_str()
                    .unwrap(),
                "nosniff"
            );
            assert_eq!(
                response.headers()["referrer-policy"].to_str().unwrap(),
                "no-referrer"
            );
            assert_eq!(
                response.headers()["x-frame-options"].to_str().unwrap(),
                "DENY"
            );
            assert!(response.headers().contains_key("content-security-policy"));
        }
    }

    #[tokio::test]
    async fn rate_limit_wraps_real_host_api_and_applies_to_failed_authentication_too() {
        let fixture = Fixture::new(2);
        let first = fixture
            .send(Method::GET, "/api/host/v1/health", Some(&fixture.token))
            .await;
        assert_eq!(first.status(), StatusCode::OK);

        let second = fixture.send(Method::GET, "/api/host/v1/health", None).await;
        assert_eq!(second.status(), StatusCode::UNAUTHORIZED);

        let third = fixture
            .send(Method::GET, "/api/host/v1/health", Some(&fixture.token))
            .await;
        assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(third.headers().contains_key(header::RETRY_AFTER));
        assert_eq!(third.headers()[header::CACHE_CONTROL], "no-store");
        let body = to_bytes(third.into_body(), 16 * 1024)
            .await
            .expect("read rate-limit response");
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("parse rate-limit response");
        assert_eq!(json["error"]["code"], "rate_limited");
        assert!(!json.to_string().contains(&fixture.token));
    }

    #[tokio::test]
    async fn request_budget_resets_after_window_expiry() {
        let state = HostApiSecurity::with_limits(1, Duration::from_secs(1), 1);
        assert!(state.reserve_request().await.is_ok());
        assert!(state.reserve_request().await.is_err());
        {
            let mut window = state.window.lock().await;
            window.started_at = Instant::now() - Duration::from_secs(2);
        }
        assert!(state.reserve_request().await.is_ok());
    }

    #[tokio::test]
    async fn in_flight_limit_rejects_excess_concurrency_and_returns_retry_after() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let hold = Arc::new(tokio::sync::Notify::new());
        let handler = {
            let entered = entered.clone();
            let hold = hold.clone();
            get(move || {
                let entered = entered.clone();
                let hold = hold.clone();
                async move {
                    entered.notify_one();
                    hold.notified().await;
                    StatusCode::OK
                }
            })
        };
        let base = Router::new().route("/slow", handler);
        let app = harden_router_with(
            base,
            HostApiSecurity::with_limits(10, Duration::from_secs(60), 1),
        );
        let first_app = app.clone();
        let first = tokio::spawn(async move {
            first_app
                .oneshot(Request::builder().uri("/slow").body(Body::empty()).unwrap())
                .await
                .unwrap()
        });
        entered.notified().await;

        let second = app
            .oneshot(Request::builder().uri("/slow").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(second.headers().contains_key(header::RETRY_AFTER));

        hold.notify_one();
        let first_response = first.await.expect("join first request");
        assert_eq!(first_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unregistered_paths_remain_unavailable_behind_the_hardened_router() {
        let fixture = Fixture::new(10);
        let response = fixture
            .send(Method::GET, "/api/config", Some(&fixture.token))
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
}
