//! Named OpenAI-compatible providers (Phase 4).
//!
//! Before this module the app had exactly one endpoint: `base_url` + `api_key`
//! in config.json. That pair is not replaced here — it is exposed as the
//! provider [`LEGACY_ID`] (`default`), so an install that upgrades keeps
//! working with no migration step and the Connection form keeps editing the
//! same two fields. Adding a named provider is opt-in.
//!
//! A named provider carries its own base URL, key, default model and the model
//! ids a successful test cached. The key follows config.json's existing rule:
//! `save_config_file` moves it into the OS credential store under
//! `provider:<id>` and blanks it from the file, `load_config` merges it back,
//! and a store that refuses the write leaves the key in the permission-
//! restricted file exactly like `api_key` does.
//!
//! [`active_config`] is the seam every provider call goes through: it returns
//! a `Config` clone whose router fields are the active provider's. Chat, agent
//! runs, the model list, embeddings, transcription, deep search and the Host
//! API's `/models` all load through it, so one switch moves every one of them.
//! When the active id names nothing — deleted, hand-edited, never set — the
//! legacy endpoint answers, because a broken selection must not make the app
//! unable to reach any model at all.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;

use crate::{
    json_error, keychain, keychain_location, load_config, permissions, save_config_file, AppState,
    Config,
};

/// The provider that *is* `base_url`/`api_key`. It is never stored in
/// `providers`, so there is exactly one copy of those two values.
pub(crate) const LEGACY_ID: &str = "default";

const MAX_NAME: usize = 80;
const MAX_MODEL: usize = 200;
const MAX_MODELS: usize = 500;
const MAX_KEY: usize = 4096;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct ProviderEntry {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// Moved to the credential store on save; merged back on load.
    #[serde(default)]
    pub api_key: String,
    /// The model a request uses when nothing more specific is chosen.
    #[serde(default)]
    pub model: String,
    /// Ids cached by the last successful test, so the model picker can list
    /// them while the provider is offline.
    #[serde(default)]
    pub models: Vec<String>,
}

impl ProviderEntry {
    fn new(id: String, name: String, base_url: String) -> ProviderEntry {
        ProviderEntry {
            id,
            name,
            base_url,
            api_key: String::new(),
            model: String::new(),
            models: Vec::new(),
        }
    }
}

/// The credential-store account name for one provider's key. Not a config
/// field name, so it can never collide with `api_key`/`openai_api_key`.
pub(crate) fn secret_name(id: &str) -> String {
    format!("provider:{id}")
}

fn location(state: &AppState) -> String {
    keychain_location(&state.config_path)
}

/// The selected named provider, or `None` when the legacy endpoint is active.
/// An id that names nothing also reads as `None`: the legacy endpoint is the
/// guaranteed-reachable default.
pub(crate) fn select(cfg: &Config) -> Option<&ProviderEntry> {
    let id = cfg.active_provider.trim();
    if id.is_empty() || id == LEGACY_ID {
        return None;
    }
    cfg.providers.iter().find(|provider| provider.id == id)
}

/// The config every provider call should use. Named providers override the
/// legacy router fields; everything else (context budget, local model, OpenAI
/// platform key, search key) is copied through unchanged.
pub(crate) fn active_config(state: &AppState) -> Config {
    let mut cfg = load_config(state);
    let selected = select(&cfg).map(|active| {
        (
            active.base_url.clone(),
            active.api_key.clone(),
            active.model.trim().to_string(),
        )
    });
    if let Some((base_url, api_key, model)) = selected {
        cfg.base_url = base_url;
        cfg.api_key = api_key;
        if !model.is_empty() {
            cfg.model = model;
        }
    }
    cfg
}

/// One row per provider, legacy first. Never includes a key: `has_key` is the
/// only fact the browser learns about it.
fn rows(cfg: &Config) -> Vec<Value> {
    let active = cfg.active_provider.trim();
    let mut rows = vec![json!({
        "id": LEGACY_ID,
        "name": "Router",
        "base_url": cfg.base_url,
        "model": cfg.model,
        "models": [],
        "model_count": 0,
        "has_key": !cfg.api_key.trim().is_empty(),
        "legacy": true,
        "active": active.is_empty() || active == LEGACY_ID,
    })];
    for provider in &cfg.providers {
        rows.push(json!({
            "id": provider.id,
            "name": provider.name,
            "base_url": provider.base_url,
            "model": provider.model,
            "models": provider.models.iter().take(MAX_MODELS).collect::<Vec<_>>(),
            "model_count": provider.models.len(),
            "has_key": !provider.api_key.trim().is_empty(),
            "legacy": false,
            "active": active == provider.id,
        }));
    }
    rows
}

fn list_response(cfg: &Config) -> Response {
    (
        StatusCode::OK,
        [(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        )],
        Json(json!({ "ok": true, "active": cfg.active_provider, "providers": rows(cfg) })),
    )
        .into_response()
}

fn text(input: &Value, field: &str) -> String {
    input
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn model_ids(input: &Value) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let rows = input
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for row in rows {
        // A test's cached rows are objects; a hand-typed list is strings.
        let id = row
            .as_str()
            .map(str::to_string)
            .or_else(|| row.get("id").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
        let id = id.trim().to_string();
        if id.is_empty() || id.chars().count() > MAX_MODEL || ids.contains(&id) {
            continue;
        }
        ids.push(id);
        if ids.len() == MAX_MODELS {
            break;
        }
    }
    ids
}

/// A provider endpoint is a base URL *with* a path, because gateways put the
/// API under `/v1` or `/openai/v1` — unlike the remote-router setting, which
/// names a host and nothing else. The rest of the guard is the same: https, or
/// http only on loopback, no query or fragment, and never credentials inside
/// the URL.
fn validate_base_url(raw: &str) -> Result<String, String> {
    const MAX_LEN: usize = 2048;
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Enter the provider's OpenAI-compatible base URL (ending at /v1).".to_string());
    }
    if trimmed.len() > MAX_LEN {
        return Err(format!("That URL is longer than {MAX_LEN} characters."));
    }
    if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("A URL cannot contain spaces or control characters.".to_string());
    }
    let parsed =
        reqwest::Url::parse(trimmed).map_err(|error| format!("{trimmed} is not a URL: {error}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "Only http and https endpoints are supported, not {other}."
            ))
        }
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(
            "A provider URL cannot carry credentials; the key is configured separately."
                .to_string(),
        );
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(
            "A provider endpoint is a base URL: remove the query and fragment.".to_string(),
        );
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("{trimmed} has no host."))?;
    let loopback = {
        let bare = host.trim_start_matches('[').trim_end_matches(']');
        bare.eq_ignore_ascii_case("localhost")
            || bare
                .parse::<std::net::IpAddr>()
                .map(|address| address.is_loopback())
                .unwrap_or(false)
    };
    if parsed.scheme() == "http" && !loopback {
        return Err(format!(
            "{host} is not loopback, so it needs https: a plain-text endpoint would expose the key."
        ));
    }
    Ok(trimmed.to_string())
}

/// Validate a create/update body.
fn draft(input: &Value) -> Result<(String, String, String, String, Vec<String>), String> {
    let name = text(input, "name");
    if name.is_empty() {
        return Err("Give the provider a name, so it can be told apart in the list.".to_string());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("A provider name is at most {MAX_NAME} characters."));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("A provider name cannot contain control characters.".to_string());
    }
    let raw_url = text(input, "base_url");
    if raw_url.is_empty() {
        return Err("Enter the provider's OpenAI-compatible base URL (ending at /v1).".to_string());
    }
    let base_url = validate_base_url(&raw_url)?;
    let model = text(input, "model");
    if model.chars().count() > MAX_MODEL {
        return Err(format!("A model id is at most {MAX_MODEL} characters."));
    }
    let api_key = match input.get("api_key").and_then(Value::as_str) {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.chars().count() > MAX_KEY {
                return Err(format!("An API key is at most {MAX_KEY} characters."));
            }
            if trimmed.chars().any(|c| c.is_control()) {
                return Err("An API key cannot contain control characters.".to_string());
            }
            trimmed.to_string()
        }
        None => String::new(),
    };
    Ok((name, base_url, api_key, model, model_ids(input)))
}

/// Apply a validated body to the config. `id` selects an existing provider;
/// `None` creates one. The legacy id edits `base_url`/`api_key`/`model`
/// in place, which is what the Connection form already does.
fn upsert(cfg: &mut Config, id: Option<&str>, input: &Value) -> Result<String, String> {
    let (name, base_url, api_key, model, models) = draft(input)?;
    // An empty key on update means "keep the saved one"; the UI never sends a
    // blank field over a key it did not read. `forget_key` is the explicit
    // removal, so a mistake cannot silently delete a working credential.
    let forget_key = input
        .get("forget_key")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match id {
        Some(LEGACY_ID) => {
            cfg.base_url = base_url;
            if forget_key {
                cfg.api_key.clear();
            } else if !api_key.is_empty() {
                cfg.api_key = api_key;
            }
            if !model.is_empty() {
                cfg.model = model;
            }
            Ok(LEGACY_ID.to_string())
        }
        Some(existing) => {
            let provider = cfg
                .providers
                .iter_mut()
                .find(|provider| provider.id == existing)
                .ok_or_else(|| "No such provider.".to_string())?;
            provider.name = name;
            provider.base_url = base_url;
            provider.model = model;
            if forget_key {
                provider.api_key.clear();
            } else if !api_key.is_empty() {
                provider.api_key = api_key;
            }
            if !models.is_empty() {
                provider.models = models;
            }
            Ok(existing.to_string())
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            let mut provider = ProviderEntry::new(id.clone(), name, base_url);
            provider.api_key = api_key;
            provider.model = model;
            provider.models = models;
            cfg.providers.push(provider);
            Ok(id)
        }
    }
}

/// Remove a provider and stop pointing at it. Deleting the legacy row is
/// refused: it is the fallback every request lands on.
fn remove(cfg: &mut Config, id: &str) -> Result<(), String> {
    if id == LEGACY_ID {
        return Err(
            "The built-in Router provider cannot be deleted; point it at another endpoint instead."
                .to_string(),
        );
    }
    let before = cfg.providers.len();
    cfg.providers.retain(|provider| provider.id != id);
    if cfg.providers.len() == before {
        return Err("No such provider.".to_string());
    }
    if cfg.active_provider.trim() == id {
        cfg.active_provider.clear();
    }
    Ok(())
}

fn activate(cfg: &mut Config, id: &str) -> Result<(), String> {
    if id == LEGACY_ID {
        cfg.active_provider.clear();
        return Ok(());
    }
    if !cfg.providers.iter().any(|provider| provider.id == id) {
        return Err("No such provider.".to_string());
    }
    cfg.active_provider = id.to_string();
    Ok(())
}

/// Drop every cached `/models` answer after a provider change, so the picker
/// does not keep showing the previous endpoint's catalogue.
fn invalidate_models(state: &AppState) {
    state.models_cache.lock().unwrap().clear();
}

fn guard(state: &AppState, input: &Value, summary: &str) -> Option<Response> {
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    permissions::guard_user(
        state,
        permissions::Action::Settings,
        "config.provider",
        "",
        approve,
        summary,
    )
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/providers", get(list).post(create))
        .route("/api/providers/:id", post(update))
        .route("/api/providers/:id/delete", post(delete))
        .route("/api/providers/:id/activate", post(set_active))
        .route("/api/providers/:id/test", post(test))
        .route("/api/providers/:id/models", get(models))
}

async fn list(State(state): State<AppState>) -> Response {
    list_response(&load_config(&state))
}

async fn create(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    if let Some(response) = guard(&state, &input, "add a provider") {
        return response;
    }
    let mut cfg = load_config(&state);
    let id = match upsert(&mut cfg, None, &input) {
        Ok(id) => id,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error),
    };
    if let Err(error) = save_config_file(&state.config_path, &cfg) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    invalidate_models(&state);
    let saved = load_config(&state);
    let body = json!({ "ok": true, "id": id, "providers": rows(&saved) });
    (StatusCode::CREATED, Json(body)).into_response()
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    if let Some(response) = guard(&state, &input, "change a provider") {
        return response;
    }
    let mut cfg = load_config(&state);
    if let Err(error) = upsert(&mut cfg, Some(&id), &input) {
        let status = if error == "No such provider." {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        return json_error(status, error);
    }
    if let Err(error) = save_config_file(&state.config_path, &cfg) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    invalidate_models(&state);
    let saved = load_config(&state);
    Json(json!({ "ok": true, "id": id, "providers": rows(&saved) })).into_response()
}

async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    if let Some(response) = guard(&state, &input, "remove a provider") {
        return response;
    }
    let mut cfg = load_config(&state);
    if let Err(error) = remove(&mut cfg, &id) {
        let status = if error == "No such provider." {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        return json_error(status, error);
    }
    if let Err(error) = save_config_file(&state.config_path, &cfg) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    // The key is only ever ours to remove while the row exists.
    if id != LEGACY_ID {
        let _ = keychain::delete_secret(&location(&state), &secret_name(&id));
    }
    invalidate_models(&state);
    let saved = load_config(&state);
    Json(json!({ "ok": true, "providers": rows(&saved) })).into_response()
}

async fn set_active(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    if let Some(response) = guard(&state, &input, "switch the active provider") {
        return response;
    }
    let mut cfg = load_config(&state);
    if let Err(error) = activate(&mut cfg, &id) {
        return json_error(StatusCode::NOT_FOUND, error);
    }
    if let Err(error) = save_config_file(&state.config_path, &cfg) {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    invalidate_models(&state);
    let saved = load_config(&state);
    Json(json!({ "ok": true, "active": saved.active_provider, "providers": rows(&saved) }))
        .into_response()
}

/// The endpoint and key a test should use for `id`.
fn endpoint(cfg: &Config, id: &str) -> Result<(String, String), String> {
    if id == LEGACY_ID {
        return Ok((cfg.base_url.clone(), cfg.api_key.clone()));
    }
    cfg.providers
        .iter()
        .find(|provider| provider.id == id)
        .map(|provider| (provider.base_url.clone(), provider.api_key.clone()))
        .ok_or_else(|| "No such provider.".to_string())
}

async fn test(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    if let Some(response) = guard(&state, &input, "test a provider") {
        return response;
    }
    let cfg = load_config(&state);
    let (base_url, api_key) = match endpoint(&cfg, &id) {
        Ok(pair) => pair,
        Err(error) => return json_error(StatusCode::NOT_FOUND, error),
    };
    if base_url.trim().is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Add the endpoint before testing it.",
        );
    }
    let mut probe = cfg.clone();
    probe.base_url = base_url;
    probe.api_key = api_key;
    let started = Instant::now();
    match crate::fetch_models(&state, &probe).await {
        Ok(models) => {
            let ids: Vec<String> = models
                .iter()
                .filter_map(|row| row.get("id").and_then(Value::as_str).map(str::to_string))
                .collect();
            let count = ids.len();
            // A successful test is also the cheapest way to refresh the cached
            // catalogue, so the model picker can list it offline.
            if id != LEGACY_ID {
                let mut saved = load_config(&state);
                if let Some(provider) = saved
                    .providers
                    .iter_mut()
                    .find(|provider| provider.id == id)
                {
                    provider.models = ids;
                    let _ = save_config_file(&state.config_path, &saved);
                }
                invalidate_models(&state);
            }
            Json(json!({
                "ok": true,
                "latency_ms": started.elapsed().as_millis() as u64,
                "model_count": count,
                "message": format!("Connected in {} ms. The endpoint returned {count} model(s).", started.elapsed().as_millis()),
            }))
            .into_response()
        }
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "ok": false, "error": error })),
        )
            .into_response(),
    }
}

async fn models(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let cfg = load_config(&state);
    let (base_url, api_key) = match endpoint(&cfg, &id) {
        Ok(pair) => pair,
        Err(error) => return json_error(StatusCode::NOT_FOUND, error),
    };
    let mut probe = cfg.clone();
    probe.base_url = base_url;
    probe.api_key = api_key;
    let cached: Vec<String> = cfg
        .providers
        .iter()
        .find(|provider| provider.id == id)
        .map(|provider| provider.models.clone())
        .unwrap_or_default();
    match crate::fetch_models(&state, &probe).await {
        Ok(models) => {
            Json(json!({ "ok": true, "cached": false, "models": models })).into_response()
        }
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "ok": false, "error": error, "cached": true, "models": cached })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        let mut cfg = crate::Config::defaults();
        cfg.base_url = "https://legacy.example.com/v1".to_string();
        cfg.api_key = "legacy-key".to_string();
        cfg.model = "legacy-model".to_string();
        cfg
    }

    fn body(name: &str, url: &str) -> Value {
        json!({ "name": name, "base_url": url })
    }

    #[test]
    fn an_install_with_no_providers_still_has_the_legacy_row() {
        let rows = rows(&config());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], LEGACY_ID);
        assert_eq!(rows[0]["active"], true);
        assert_eq!(rows[0]["has_key"], true);
    }

    #[test]
    fn a_named_provider_round_trips_and_becomes_active() {
        let mut cfg = config();
        let id = upsert(
            &mut cfg,
            None,
            &json!({
                "name": "Groq",
                "base_url": "https://api.groq.com/openai/v1",
                "model": "llama-3.3-70b",
                "api_key": "groq-key",
                "models": ["llama-3.3-70b", {"id": "mixtral"}],
            }),
        )
        .expect("create");
        activate(&mut cfg, &id).expect("activate");
        assert!(select(&cfg).is_some_and(|provider| provider.id == id));
        assert_eq!(cfg.providers[0].models, vec!["llama-3.3-70b", "mixtral"]);
        // The legacy pair is untouched, so switching back costs nothing.
        assert_eq!(cfg.base_url, "https://legacy.example.com/v1");
        assert_eq!(cfg.api_key, "legacy-key");
    }

    #[test]
    fn deleting_the_active_provider_falls_back_to_the_legacy_endpoint() {
        let mut cfg = config();
        let id = upsert(
            &mut cfg,
            None,
            &body("Groq", "https://api.groq.com/openai/v1"),
        )
        .expect("create");
        activate(&mut cfg, &id).expect("activate");
        remove(&mut cfg, &id).expect("delete");
        assert!(cfg.active_provider.is_empty());
        assert!(
            select(&cfg).is_none(),
            "the legacy endpoint is the fallback"
        );
    }

    #[test]
    fn the_legacy_row_cannot_be_deleted() {
        let mut cfg = config();
        assert!(remove(&mut cfg, LEGACY_ID).is_err());
    }

    #[test]
    fn an_unknown_active_id_reads_as_the_legacy_endpoint() {
        let mut cfg = config();
        cfg.active_provider = "gone".to_string();
        assert!(select(&cfg).is_none());
    }

    #[test]
    fn a_provider_endpoint_must_be_a_base_url_without_credentials() {
        let mut cfg = config();
        assert!(upsert(&mut cfg, None, &body("Bad", "http://example.com/v1")).is_err());
        assert!(upsert(
            &mut cfg,
            None,
            &body("Bad", "https://user:pass@example.com/v1")
        )
        .is_err());
        assert!(upsert(&mut cfg, None, &body("Bad", "https://example.com/v1?key=1")).is_err());
        assert!(upsert(&mut cfg, None, &body("Bad", "https://example.com/v1#frag")).is_err());
        // A gateway path is normal (`/v1`, `/openai/v1`) and must be kept.
        assert!(upsert(
            &mut cfg,
            None,
            &body("Groq", "https://api.groq.com/openai/v1")
        )
        .is_ok());
        assert!(upsert(&mut cfg, None, &body("Bad", "ftp://example.com/v1")).is_err());
        // A local runtime is the one plain-http case, because nothing leaves
        // the machine.
        assert!(upsert(&mut cfg, None, &body("Ollama", "http://127.0.0.1:11434/v1")).is_ok());
    }

    #[test]
    fn a_provider_needs_a_name() {
        let mut cfg = config();
        assert!(upsert(
            &mut cfg,
            None,
            &body("  ", "https://api.groq.com/openai/v1")
        )
        .is_err());
    }

    #[test]
    fn an_empty_key_on_update_keeps_the_saved_one() {
        let mut cfg = config();
        let id = upsert(
            &mut cfg,
            None,
            &json!({"name": "Groq", "base_url": "https://api.groq.com/openai/v1", "api_key": "kept"}),
        )
        .expect("create");
        upsert(
            &mut cfg,
            Some(&id),
            &json!({"name": "Groq 2", "base_url": "https://api.groq.com/openai/v1"}),
        )
        .expect("update");
        assert_eq!(cfg.providers[0].api_key, "kept");
        assert_eq!(cfg.providers[0].name, "Groq 2");
        upsert(
            &mut cfg,
            Some(&id),
            &json!({"name": "Groq 2", "base_url": "https://api.groq.com/openai/v1", "forget_key": true}),
        )
        .expect("forget");
        assert!(cfg.providers[0].api_key.is_empty());
    }

    #[test]
    fn the_legacy_row_edits_the_router_fields_in_place() {
        let mut cfg = config();
        upsert(
            &mut cfg,
            Some(LEGACY_ID),
            &json!({
                "name": "Router",
                "base_url": "https://router.example.com/v1",
                "api_key": "new-key",
                "model": "auto",
            }),
        )
        .expect("edit");
        assert_eq!(cfg.base_url, "https://router.example.com/v1");
        assert_eq!(cfg.api_key, "new-key");
        assert_eq!(cfg.model, "auto");
        assert!(
            cfg.providers.is_empty(),
            "the legacy row is not a stored entry"
        );
    }

    #[test]
    fn no_row_ever_carries_a_key() {
        let mut cfg = config();
        upsert(
            &mut cfg,
            None,
            &json!({"name": "Groq", "base_url": "https://api.groq.com/openai/v1", "api_key": "secret-key"}),
        )
        .expect("create");
        let rendered = serde_json::to_string(&rows(&cfg)).expect("json");
        assert!(!rendered.contains("secret-key"));
        assert!(rendered.contains("\"has_key\":true"));
    }
}
