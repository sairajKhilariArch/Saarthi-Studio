//! Local inference: detect, inspect and use an Ollama runtime on this machine.
//!
//! The adapter keeps local inference separate from the remote router: a chat
//! or agent run that selected a local model talks ONLY to the configured
//! local endpoint and never silently falls back to a remote provider — an
//! unreachable runtime is a visible error with setup guidance. Model data
//! always comes from the runtime itself (`/api/tags`, `/api/version`);
//! nothing here invents models. `pull` is an explicit user action with live
//! progress published on the SSE event bus.
//!
//! Hardware diagnostics are best-effort and platform-aware: anything that
//! cannot be determined is reported as missing/Unknown rather than guessed.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;

use crate::AppState;

/// Longer than most local chats take to think; each request carries its own
/// timeout because the adapter uses its own client (the shared AppState
/// client is capped at 120s, too short for local generation).
const CHAT_TIMEOUT: Duration = Duration::from_secs(600);
const DETECT_TIMEOUT: Duration = Duration::from_secs(4);
const PULL_TIMEOUT_ALL_TURNS: Duration = Duration::from_secs(1800);

fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(1800))
            .build()
            .unwrap_or_default()
    })
}

fn base_url_of(state: &AppState) -> String {
    crate::load_config(state)
        .local_base_url
        .trim()
        .trim_end_matches('/')
        .to_string()
}

// ---------------------------------------------------------------------------
// Runtime detection and model discovery

/// GET /api/version → {"version":"0.3.14"} — the reachability probe.
pub(crate) async fn fetch_version(base_url: &str) -> Result<String, String> {
    let url = format!("{base_url}/api/version");
    let response = client()
        .get(&url)
        .timeout(DETECT_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("Cannot reach the local runtime at {base_url}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "The local runtime answered HTTP {}.",
            response.status().as_u16()
        ));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("The local runtime version response was unreadable: {e}"))?;
    Ok(body
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string())
}

/// GET /api/tags → {"models":[{name,size,digest,details:{...}}]}
pub(crate) async fn fetch_models(base_url: &str) -> Result<Vec<Value>, String> {
    let url = format!("{base_url}/api/tags");
    let response = client()
        .get(&url)
        .timeout(DETECT_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("Cannot list models from {base_url}: {e}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("Model listing (HTTP {status}) was unreadable: {e}"))?;
    if !status.is_success() {
        return Err(format!("Model listing failed with HTTP {}.", status.as_u16()));
    }
    Ok(body
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

/// One /api/tags entry, normalized for the UI. Pure so it is testable.
pub(crate) fn model_json(entry: &Value) -> Option<Value> {
    let name = entry.get("name").and_then(Value::as_str)?.to_string();
    Some(json!({
        "name": name,
        "size_bytes": entry.get("size").and_then(Value::as_u64),
        "digest": entry.get("digest").and_then(Value::as_str).unwrap_or(""),
        "family": entry.pointer("/details/family").and_then(Value::as_str).unwrap_or("unknown"),
        "parameter_size": entry.pointer("/details/parameter_size").and_then(Value::as_str).unwrap_or("unknown"),
        "quantization": entry.pointer("/details/quantization_level").and_then(Value::as_str).unwrap_or("unknown"),
    }))
}

pub(crate) struct LocalSelection {
    pub(crate) base_url: String,
    pub(crate) selected_model: String,
    pub(crate) chat: bool,
}

pub(crate) fn selection_of(state: &AppState) -> LocalSelection {
    let cfg = crate::load_config(state);
    LocalSelection {
        base_url: cfg.local_base_url,
        selected_model: cfg.local_model,
        chat: cfg.local_chat,
    }
}

fn is_model_installed(models: &[Value], name: &str) -> bool {
    models
        .iter()
        .any(|m| m.get("name").and_then(Value::as_str) == Some(name))
}

// ---------------------------------------------------------------------------
// Local chat: POST /api/chat with stream:false (one-shot turns)

/// Build the /api/chat request body. Kept pure for tests.
pub(crate) fn chat_request_body(model: &str, messages: &[Value], stream: bool) -> Value {
    json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    })
}

/// One non-streaming local chat turn (agent runs; streaming chat folds the
/// same endpoint's NDJSON frames separately in main.rs).
pub(crate) async fn chat_once(
    base_url: &str,
    model: &str,
    messages: &[Value],
) -> Result<(String, i64), String> {
    if model.trim().is_empty() {
        return Err("No local model is selected. Pick one on the Local AI page.".to_string());
    }
    let response = client()
        .post(format!("{base_url}/api/chat"))
        .timeout(CHAT_TIMEOUT)
        .json(&chat_request_body(model, messages, false))
        .send()
        .await
        .map_err(|e| {
            format!(
                "The local runtime is unreachable: {e}. Install or start Ollama (it answers on the configured local endpoint), then retry."
            )
        })?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("Local chat HTTP {status} with an unreadable body: {e}"))?;
    if let Some(error) = body.get("error").and_then(Value::as_str) {
        return Err(format!(
            "Local chat failed (HTTP {}): {error} — if the model is missing, try `ollama pull {model}`.",
            status.as_u16()
        ));
    }
    if !status.is_success() {
        return Err(format!(
            "Local chat failed with HTTP {} — make sure model \"{model}\" exists.",
            status.as_u16()
        ));
    }
    let answer = body
        .pointer("/message/content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if answer.trim().is_empty() {
        return Err(format!("The local model \"{model}\" returned an empty reply."));
    }
    Ok((
        answer,
        body.get("eval_count").and_then(Value::as_i64).unwrap_or(-1),
    ))
}

/// Fold one `/api/chat` NDJSON line into the upstream SSE payload shape the
/// shared stream parser (`parse_stream_payload`) already understands, so the
/// local runtime can feed the browser stream with zero special cases. Pure.
pub(crate) fn ndjson_to_upstream_payload(line: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(line.trim()).ok()?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Some(format!(
            "data: {}\n\n",
            json!({ "error": { "message": format!("Local runtime error: {error}") } })
        ));
    }
    let content = value
        .pointer("/message/content")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty());
    if let Some(text) = content {
        return Some(format!(
            "data: {}\n\n",
            json!({ "choices": [{ "delta": { "content": text } }] })
        ));
    }
    if value.get("done").and_then(Value::as_bool) == Some(true) {
        // Final stats frame: surface token usage when the runtime reports it.
        let tokens = value.get("eval_count").and_then(Value::as_i64);
        return tokens.map(|tokens| {
            format!(
                "data: {}\n\n",
                json!({ "usage": { "completion_tokens": tokens, "total_tokens": tokens } })
            )
        });
    }
    None
}

/// POST /api/chat with stream:true, returning the body as a byte stream of
/// upstream-shaped SSE events. On failure the stream instead carries one
/// synthetic error event and ends; the shared recorder still saves partial
/// text and the shared `finish()` frame closes the client stream.
pub(crate) async fn chat_stream_bytes(
    base_url: &str,
    model: &str,
    messages: &[Value],
) -> Result<futures_util::stream::BoxStream<'static, Result<bytes::Bytes, reqwest::Error>>, String>
{
    if model.trim().is_empty() {
        return Err("No local model is selected. Pick one on the Local AI page.".to_string());
    }
    let response = client()
        .post(format!("{base_url}/api/chat"))
        .timeout(CHAT_TIMEOUT)
        .json(&chat_request_body(model, messages, true))
        .send()
        .await
        .map_err(|e| {
            format!(
                "The local runtime is unreachable: {e}. Install or start Ollama (it answers on the configured local endpoint), then retry."
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<Value>(&body)
            .ok()
            .and_then(|value| value.get("error").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| body.chars().take(300).collect());
        return Err(format!(
            "Local chat failed (HTTP {}): {detail} — if the model is missing, try `ollama pull {model}`.",
            status.as_u16()
        ));
    }
    type ByteStream = futures_util::stream::BoxStream<'static, Result<bytes::Bytes, reqwest::Error>>;
    let upstream: ByteStream = response.bytes_stream().boxed();
    Ok(futures_util::stream::unfold(
        (upstream, String::new()),
        |(mut upstream, mut carry)| async move {
            loop {
                if let Some(index) = carry.find('\n') {
                    let line: String = carry.drain(..=index).collect();
                    if let Some(payload) = ndjson_to_upstream_payload(&line) {
                        return Some((Ok(bytes::Bytes::from(payload)), (upstream, carry)));
                    }
                    continue;
                }
                match upstream.next().await {
                    Some(Ok(chunk)) => carry.push_str(&String::from_utf8_lossy(&chunk)),
                    Some(Err(error)) => {
                        let payload = format!(
                            "data: {}\n\n",
                            json!({ "error": { "message": format!("The local runtime stream failed: {error}") } })
                        );
                        return Some((
                            Ok(bytes::Bytes::from(payload)),
                            (upstream, carry),
                        ));
                    }
                    None => return None, // ended: sse_response's finish() sends the done frame
                }
            }
        },
    )
    .boxed())
}

// ---------------------------------------------------------------------------
// Model pull (explicit user action, progress on the event bus)

/// Status/total/completed of one /api/pull NDJSON line. Pure.
pub(crate) fn pull_progress(line: &str) -> Option<(String, u64, u64)> {
    let value = serde_json::from_str::<Value>(line.trim()).ok()?;
    let status = value.get("status").and_then(Value::as_str)?.to_string();
    Some((
        status,
        value.get("total").and_then(Value::as_u64).unwrap_or(0),
        value.get("completed").and_then(Value::as_u64).unwrap_or(0),
    ))
}

// Only one pull at a time, and only for one runtime — simples in-flight guard.
struct PullState {
    model: Option<String>,
}

fn pull_state() -> &'static std::sync::Mutex<PullState> {
    static PULL_STATE: std::sync::OnceLock<std::sync::Mutex<PullState>> = std::sync::OnceLock::new();
    PULL_STATE.get_or_init(|| std::sync::Mutex::new(PullState { model: None }))
}

fn try_begin_pull(model: &str) -> bool {
    let Ok(mut state) = pull_state().lock() else { return false };
    match state.model.as_deref() {
        Some(running) if !running.is_empty() => false,
        _ => {
            state.model = Some(model.to_string());
            true
        }
    }
}

fn end_pull() {
    if let Ok(mut state) = pull_state().lock() {
        state.model = None;
    }
}

fn pull_in_flight() -> Option<String> {
    pull_state().lock().ok().and_then(|s| s.model.clone())
}

fn publish_pull(state: &AppState, model: &str, phase: &str, detail: &str) {
    state.events.publish(json!({
        "type": "local-pull",
        "model": model,
        "phase": phase,
        "detail": detail,
        "at": crate::now_seconds(),
    }));
}

fn cap_line(text: &str, cap: usize) -> String {
    text.chars().take(cap).collect()
}

async fn run_pull(state: AppState, model: String) {
    let base_url = base_url_of(&state);
    let response = client()
        .post(format!("{base_url}/api/pull"))
        .timeout(PULL_TIMEOUT_ALL_TURNS)
        .json(&json!({ "name": model, "stream": true }))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            publish_pull(&state, &model, "failed", &format!("connection error: {error}"));
            end_pull();
            return;
        }
    };
    if !response.status().is_success() {
        let code = response.status().as_u16();
        let detail = response.text().await.unwrap_or_default();
        publish_pull(
            &state,
            &model,
            "failed",
            &cap_line(&format!("HTTP {code}: {detail}"), 300),
        );
        end_pull();
        return;
    }
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut carry = String::new();
    let mut last_pct: i64 = -1;
    loop {
        let chunk = match stream.next().await {
            Some(Ok(chunk)) => chunk,
            Some(Err(error)) => {
                publish_pull(&state, &model, "failed", &format!("stream error: {error}"));
                break;
            }
            None => break,
        };
        carry.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = carry.find('\n') {
            let line: String = carry.drain(..=index).collect();
            if let Some((status, total, completed)) = pull_progress(line.trim()) {
                if total > 0 {
                    let pct = (completed * 100 / total.max(1)) as i64;
                    if pct != last_pct {
                        last_pct = pct;
                        publish_pull(&state, &model, "progress", &format!("{pct}%"));
                    }
                } else if status == "success" {
                    publish_pull(&state, &model, "success", "");
                }
            }
        }
    }
    publish_pull(&state, &model, "finish", "");
    end_pull();
}

// ---------------------------------------------------------------------------
// Hardware diagnostics

#[derive(Debug, Default)]
pub(crate) struct DiskReport {
    pub(crate) total_bytes: Option<u64>,
    pub(crate) free_bytes: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct HardwareReport {
    pub(crate) logical_cores: usize,
    pub(crate) cpu_model: String,
    pub(crate) ram_total_bytes: Option<u64>,
    pub(crate) gpus: Vec<String>,
}

fn hardware_report() -> HardwareReport {
    HardwareReport {
        logical_cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        cpu_model: cpu_platform_model().unwrap_or_else(|| "Unknown".to_string()),
        ram_total_bytes: ram_platform_total(),
        gpus: gpu_platform_list(),
    }
}

#[cfg(target_os = "linux")]
fn cpu_platform_model() -> Option<String> {
    let info = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    info.lines()
        .find_map(|line| line.strip_prefix("model name"))
        .map(|rest| {
            rest.trim_start()
                .trim_start_matches(':')
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "linux")]
fn ram_platform_total() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    info.lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| {
            rest.trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map(|kb| kb * 1024)
}

#[cfg(target_os = "linux")]
fn gpu_platform_list() -> Vec<String> {
    if let Ok(output) = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name", "--format=csv,noheader"])
        .output()
    {
        if output.status.success() {
            let gpus = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if !gpus.is_empty() {
                return gpus;
            }
        }
    }
    if let Ok(output) = std::process::Command::new("lspci").output() {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| {
                    line.contains("VGA compatible controller")
                        || line.contains("Display controller")
                })
                .map(|line| line.trim().to_string())
                .collect();
        }
    }
    Vec::new()
}

#[cfg(not(target_os = "linux"))]
fn cpu_platform_model() -> Option<String> {
    None // Unknown: no std-only probe on this platform yet
}

#[cfg(not(target_os = "linux"))]
fn ram_platform_total() -> Option<u64> {
    None
}

#[cfg(not(target_os = "linux"))]
fn gpu_platform_list() -> Vec<String> {
    Vec::new()
}

fn disk_free_for_path(path: &std::path::Path) -> DiskReport {
    #[cfg(target_os = "linux")]
    {
        // `df` keeps this dependency-free and honest on Linux.
        if let Ok(output) = std::process::Command::new("df")
            .args(["-P", "-B", "1"])
            .arg(path)
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let fields = text
                    .lines()
                    .last()
                    .unwrap_or("")
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if fields.len() >= 4 {
                    let report = DiskReport {
                        total_bytes: fields[1].parse().ok(),
                        free_bytes: fields[3].parse().ok(),
                    };
                    if report.total_bytes.is_some() || report.free_bytes.is_some() {
                        return report;
                    }
                }
            }
        }
        DiskReport::default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        DiskReport::default()
    }
}

/// Conservative sizing guidance without promising a tokens-per-second rate.
pub(crate) fn recommendation(report: &HardwareReport) -> &'static str {
    if report.logical_cores == 0 {
        return "Hardware detection was not conclusive; try refreshing this panel.";
    }
    match report.ram_total_bytes {
        Some(ram) if ram >= 16 * 1_024 * 1_024 * 1_024 => {
            "Quantized 7–8B models are within reach for coding tasks; generation stays CPU-paced."
        }
        Some(ram) if ram >= 8 * 1_024 * 1_024 * 1_024 => {
            "Aim for quantized 3–4B models (e.g. qwen2.5-coder:3b) for the most responsive runs."
        }
        Some(_) => "Memory is tight; prefer 1–2B quantized models and expect slow generation.",
        None => "RAM was not detectable; prefer smaller quantized models.",
    }
}

// ---------------------------------------------------------------------------
// Settings guard: local runs must stay on localhost endpoints.

pub(crate) fn validate_base_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("The endpoint URL is empty.".to_string());
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|e| format!("Invalid URL: {e}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("Use an http(s) endpoint URL.".to_string());
    }
    let host = parsed.host_str().unwrap_or("");
    let local = host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host.ends_with(".localhost");
    if !local {
        return Err("The local runtime endpoint must be a localhost address.".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP surface. Reads stay unguarded (they only reflect runtime state);
// mutators keep the loopback-origin guard like agent/media/terminal routes.

pub(crate) fn read_routes() -> Router<AppState> {
    Router::new()
        .route("/api/local/status", get(local_status))
        .route("/api/local/models", get(local_models))
        .route("/api/local/hardware", get(local_hardware))
}

pub(crate) fn mutate_routes(port: u16) -> Router<AppState> {
    use axum::middleware;
    use crate::workspace::{guard_local_origin, LocalOriginGuard};
    Router::new()
        .route("/api/local/select", post(local_select))
        .route("/api/local/pull", post(local_pull))
        .route_layer(middleware::from_fn_with_state(
            LocalOriginGuard::new(port),
            guard_local_origin,
        ))
}

fn setup_hint(base_url: &str) -> &'static str {
    if base_url.trim().trim_end_matches('/') == "http://127.0.0.1:11434" {
        return "Install Ollama from ollama.com/download and run `ollama serve` (it answers on 127.0.0.1:11434).";
    }
    "The runtime did not answer on this endpoint — install Ollama from ollama.com/download, run `ollama serve`, or fix the endpoint URL below."
}

/// Local mode turns every chat/stream turn into a local-only request.
pub(crate) fn effective_base_url(configured: &str) -> String {
    let trimmed = configured.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        "http://127.0.0.1:11434".to_string()
    } else {
        trimmed.to_string()
    }
}

async fn local_status(State(state): State<AppState>) -> Response {
    let selection = selection_of(&state);
    let base_url = if selection.base_url.trim().is_empty() {
        "http://127.0.0.1:11434".to_string()
    } else {
        selection.base_url.trim().trim_end_matches('/').to_string()
    };
    let (installed, version, runtime_error) = match fetch_version(&base_url).await {
        Ok(version) => (true, version, String::new()),
        Err(error) => (false, String::new(), error),
    };
    let models = if installed {
        fetch_models(&base_url).await.unwrap_or_default()
    } else {
        Vec::new()
    };
    let selected = selection.selected_model.as_str();
    Json(json!({
        "runtime": {
            "name": "Ollama",
            "installed": installed,
            "version": version,
            "base_url": base_url,
            "error": runtime_error,
            "setup_url": "https://ollama.com/download",
            "setup_hint": if installed { "" } else { setup_hint(&base_url) },
        },
        "model_count": models.len(),
        "selected_model": selection.selected_model,
        // A configured selection is only "usable" if the runtime lists it —
        // never trust a stored name alone.
        "selected_model_is_installed": if selected.is_empty() { None } else { Some(is_model_installed(&models, selected)) },
        "chat_through_local": selection.chat,
        "pulling": pull_in_flight(),
    }))
    .into_response()
}

async fn local_models(State(state): State<AppState>) -> Response {
    let selection = selection_of(&state);
    let base_url = if selection.base_url.trim().is_empty() {
        "http://127.0.0.1:11434".to_string()
    } else {
        selection.base_url.trim().trim_end_matches('/').to_string()
    };
    match fetch_models(&base_url).await {
        Ok(entries) => {
            let models = entries.iter().filter_map(model_json).collect::<Vec<_>>();
            Json(json!({
                "ok": true,
                "models": models,
                "selected_model": selection.selected_model,
                "chat_through_local": selection.chat,
                "pulling": pull_in_flight(),
            }))
            .into_response()
        }
        Err(error) => Json(json!({
            "ok": false,
            "models": [],
            "error": error,
            "pulling": pull_in_flight(),
        }))
        .into_response(),
    }
}

async fn local_select(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let mut cfg = crate::load_config(&state);
    if let Some(base_url) = input.get("base_url").and_then(Value::as_str) {
        let base_url = base_url.trim();
        if !base_url.is_empty() {
            if let Err(error) = validate_base_url(base_url) {
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response();
            }
            cfg.local_base_url = base_url.trim_end_matches('/').to_string();
        }
    }
    if let Some(model) = input.get("model").and_then(Value::as_str) {
        let model = model.trim();
        if model.chars().count() > 120 {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "The model name is too long." })),
            )
                .into_response();
        }
        cfg.local_model = model.to_string();
    }
    if let Some(chat) = input.get("chat").and_then(Value::as_bool) {
        if chat && cfg.local_model.trim().is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Select a local model first." })),
            )
                .into_response();
        }
        cfg.local_chat = chat;
    }
    if let Err(error) = crate::save_config_file(&state.config_path, &cfg) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Could not save settings: {error}") })),
        )
            .into_response();
    }
    let cfg = crate::load_config(&state);
    Json(json!({
        "ok": true,
        "base_url": cfg.local_base_url,
        "selected_model": cfg.local_model,
        "chat_through_local": cfg.local_chat,
    }))
    .into_response()
}

async fn local_pull(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let model = input
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if model.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Which model should be pulled? Use its full tag, e.g. \"qwen2.5-coder:3b\"." })),
        )
            .into_response();
    }
    if model.chars().count() > 120 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "The model name is too long." })),
        )
            .into_response();
    }
    if let Some(running) = pull_in_flight() {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": format!("A pull of \"{running}\" is already in progress.") })),
        )
            .into_response();
    }
    let base_url = base_url_of(&state);
    let base_url = if base_url.is_empty() {
        "http://127.0.0.1:11434".to_string()
    } else {
        base_url
    };
    if fetch_version(&base_url).await.is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "The local runtime is not reachable, so nothing can be pulled. Start Ollama first." })),
        )
            .into_response();
    }
    if let Ok(models) = fetch_models(&base_url).await {
        if is_model_installed(&models, &model) {
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": format!("\"{model}\" is already installed.") })),
            )
                .into_response();
        }
    }
    if !try_begin_pull(&model) {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "Another pull just started; try again in a moment." })),
        )
            .into_response();
    }
    tokio::spawn(run_pull(state.clone(), model.clone()));
    state.events.publish(json!({ "type": "local-models-changed", "at": crate::now_seconds() }));
    Json(json!({ "ok": true, "model": model })).into_response()
}

async fn local_hardware(State(state): State<AppState>) -> Response {
    let report = hardware_report();
    let root = state.workspace.current_root();
    let (disk, disk_path) = match &root {
        Some(root) => (disk_free_for_path(root), root.display().to_string()),
        None => (
            disk_free_for_path(&std::env::temp_dir()),
            String::new(),
        ),
    };
    Json(json!({
        "logical_cores": if report.logical_cores > 0 { Some(report.logical_cores) } else { None },
        "cpu_model": report.cpu_model,
        "ram_total_bytes": report.ram_total_bytes,
        "gpu_list": if report.gpus.is_empty() { None } else { Some(report.gpus.clone()) },
        "disk_path": disk_path,
        "disk_total_bytes": disk.total_bytes,
        "disk_free_bytes": disk.free_bytes,
        "recommendation": recommendation(&report),
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_json_keeps_name_and_details() {
        let entry = json!({
            "name": "qwen2.5-coder:3b",
            "size": 1_900_000_000u64,
            "digest": "abc",
            "details": {
                "family": "Qwen2",
                "parameter_size": "3.1B",
                "quantization_level": "Q4_K_M"
            }
        });
        let output = model_json(&entry).expect("parsed");
        assert_eq!(output["name"], "qwen2.5-coder:3b");
        assert_eq!(output["size_bytes"], 1_900_000_000u64);
        assert_eq!(output["parameter_size"], "3.1B");
        assert_eq!(output["quantization"], "Q4_K_M");
        assert_eq!(output["family"], "Qwen2");
        assert!(model_json(&json!({ "no_name": true })).is_none());
    }

    #[test]
    fn chat_request_body_is_stream_switchable() {
        let body = chat_request_body("llama3.2", &[json!({"role": "user", "content": "hi"})], false);
        assert_eq!(body["model"], "llama3.2");
        assert_eq!(body["stream"], false);
        assert_eq!(chat_request_body("m", &[], true)["stream"], true);
    }

    #[test]
    fn ndjson_folds_into_upstream_frames() {
        assert_eq!(
            ndjson_to_upstream_payload(
                r#"{"model":"m","message":{"role":"assistant","content":"Hel"},"done":false}"#
            ),
            Some(format!(
                "data: {}\n\n",
                json!({ "choices": [{ "delta": { "content": "Hel" } }] })
            ))
        );
        // Final stats frame: usage only, nothing else.
        assert_eq!(
            ndjson_to_upstream_payload(r#"{"message":{"content":""},"done":true,"eval_count":42}"#),
            Some(format!(
                "data: {}\n\n",
                json!({ "usage": { "completion_tokens": 42, "total_tokens": 42 } })
            ))
        );
        assert_eq!(ndjson_to_upstream_payload("not json"), None);
        assert_eq!(
            ndjson_to_upstream_payload(r#"{"error":"model not found"}"#),
            Some(format!(
                "data: {}\n\n",
                json!({ "error": { "message": "Local runtime error: model not found" } })
            ))
        );
    }

    #[test]
    fn effective_base_url_defaults_when_blank() {
        assert_eq!(effective_base_url(""), "http://127.0.0.1:11434");
        assert_eq!(effective_base_url(" http://127.0.0.1:11500/ "), "http://127.0.0.1:11500");
    }

    #[test]
    fn pull_progress_reads_deltas() {
        assert_eq!(
            pull_progress(r#"{"status":"pulling x","total":100,"completed":10}"#),
            Some(("pulling x".to_string(), 100, 10))
        );
        assert_eq!(
            pull_progress(r#"{"status":"verifying sha256 digest"}"#),
            Some(("verifying sha256 digest".to_string(), 0, 0))
        );
        assert_eq!(
            pull_progress(r#"{"status":"success"}"#),
            Some(("success".to_string(), 0, 0))
        );
        assert_eq!(pull_progress("{}"), None);
        assert_eq!(pull_progress("garbage"), None);
    }

    #[test]
    fn localhost_endpoints_only() {
        assert!(validate_base_url("http://127.0.0.1:11434").is_ok());
        assert!(validate_base_url("http://localhost:11434").is_ok());
        assert!(validate_base_url("http://api.localhost:11434").is_ok());
        assert_eq!(
            validate_base_url("https://api.example.com").err().unwrap(),
            "The local runtime endpoint must be a localhost address."
        );
        assert!(validate_base_url("not a url").is_err());
        assert!(validate_base_url("").is_err());
        assert!(validate_base_url("ftp://localhost").is_err());
    }

    #[test]
    fn recommendation_matches_ram_classes() {
        let report = |ram: Option<u64>| HardwareReport {
            logical_cores: 4,
            cpu_model: "test".to_string(),
            ram_total_bytes: ram,
            gpus: Vec::new(),
        };
        assert!(recommendation(&report(Some(16 * 1_024 * 1_024 * 1_024))).contains("7\u{2013}8B"));
        assert!(recommendation(&report(Some(8 * 1_024 * 1_024 * 1_024))).contains("3\u{2013}4B"));
        assert!(recommendation(&report(Some(4 * 1_024 * 1_024 * 1_024))).contains("tight"));
        assert!(recommendation(&report(None)).contains("not detectable"));
    }

    #[tokio::test]
    async fn unreachable_runtime_reports_a_clear_error() {
        let error = fetch_version("http://127.0.0.1:19999").await.unwrap_err();
        assert!(error.contains("Cannot reach the local runtime"), "got: {error}");
    }

    #[tokio::test]
    async fn model_listing_from_a_mock_ollama_is_normalized() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = axum::Router::new()
            .route(
                "/api/tags",
                axum::routing::get(|| async {
                    axum::Json(json!({
                        "models": [
                            { "name": "llama3.2:latest", "size": 2u64.pow(30), "digest": "d1",
                              "details": { "family": "Llama", "parameter_size": "3.2B",
                                           "quantization_level": "Q4_K_M" } }
                        ]
                    }))
                }),
            )
            .route(
                "/api/version",
                axum::routing::get(|| async { axum::Json(json!({ "version": "0.3.14-test" })) }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let version = fetch_version(&format!("http://{address}")).await.unwrap();
        assert_eq!(version, "0.3.14-test");
        let models = fetch_models(&format!("http://{address}")).await.unwrap();
        let one = model_json(&models[0]).unwrap();
        assert_eq!(one["name"], "llama3.2:latest");
        assert_eq!(one["parameter_size"], "3.2B");
        assert_eq!(one["quantization"], "Q4_K_M");
        assert_eq!(one["size_bytes"], 1_073_741_824u64);
    }

    #[tokio::test]
    async fn local_chat_roundtrip_against_a_mock_runtime() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/api/chat",
            axum::routing::post(|body: axum::Json<Value>| async move {
                assert_eq!(body["model"], "llama3.2");
                assert_eq!(body["stream"], false);
                axum::Json(json!({
                    "model": body["model"],
                    "message": { "role": "assistant", "content": "Hello from local Ollama" },
                    "done": true,
                    "eval_count": 42,
                }))
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let (answer, tokens) = chat_once(
            &format!("http://{address}"),
            "llama3.2",
            &[json!({"role": "user", "content": "hi"})],
        )
        .await
        .unwrap();
        assert_eq!(answer, "Hello from local Ollama");
        assert_eq!(tokens, 42);
    }

    #[tokio::test]
    async fn missing_model_reports_pull_guidance() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/api/chat",
            axum::routing::post(|| async {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({ "error": "model \"nope\" not found" })),
                )
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let error = chat_once(&format!("http://{address}"), "nope", &[]).await.unwrap_err();
        assert!(error.contains("ollama pull nope"), "got: {error}");
    }

    #[tokio::test]
    async fn unreachable_chat_reports_setup_guidance() {
        let error = chat_once("http://127.0.0.1:19999", "llama3.2", &[]).await.unwrap_err();
        assert!(error.contains("unreachable"), "got: {error}");
        assert!(!error.to_lowercase().contains("remote"));
    }

    #[test]
    fn setup_hint_mentions_the_install_path() {
        assert!(setup_hint("http://127.0.0.1:11434").contains("ollama.com/download"));
    }
}
