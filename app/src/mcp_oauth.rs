//! OAuth for remote MCP servers (Phase 7 leftover).
//!
//! MCP's HTTP transports can sit behind an OAuth 2.1 gate. Before this module,
//! the only way to reach such a server was hand-written header values, which
//! expire and never rotate. The flow here is the one the MCP authorization
//! spec describes, in the pieces a local desktop app can run itself:
//!
//! 1. **Discovery** — the protected-resource metadata (RFC 9728) lists the
//!    authorization servers; each one is described by its authorization-server
//!    metadata (RFC 8414). When the resource metadata is missing, the server's
//!    own URL is tried as the issuer directly.
//! 2. **Registration** — RFC 7591 dynamic client registration as a public
//!    client. The client is registered once and the resulting id (plus any
//!    client secret ØAuth returns) lives in the OS credential store with the
//!    tokens, never in `mcp.json`.
//! 3. **Sign-in** — an authorization request with PKCE (RFC 7636) opens in the
//!    user's browser and redirects back to `http://127.0.0.1:<port>/api/mcp/
//!    oauth/callback` (RFC 8252 loopback redirect). The browser carries the
//!    code; only the code. The `state` parameter is what ties it to this flow.
//! 4. **Exchange** — the code is swapped for tokens at the token endpoint and
//!    stored in the OS credential store.
//! 5. **Use and refresh** — every remote connection carries `Authorization:
//!    Bearer …` unless the user has written their own `authorization` header
//!    (a hand-written header wins). A stored token that is past its expiry is
//!    refreshed silently before use; a server that answers 401 anyway surfaces
//!    "sign in again" rather than retrying a tool call that may have side
//!    effects.
//!
//! Nothing here trusts user-supplied state: the in-flight map is the only
//! thing that can turn a browser callback into an exchanged code.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

use crate::AppState;

/// One authorization request may wait this long for the browser to come back.
const FLOW_TIMEOUT: Duration = Duration::from_secs(300);

/// The keychain account name for one server's OAuth material
/// (`io.github.sairajkhilariarch.SarthiStudio` is the service).
fn credential_account(server_name: &str) -> String {
    format!("mcp_oauth/{server_name}")
}

/// Everything one server's OAuth needs, stored together in the OS credential
/// store: the registered client, the endpoints discovery produced last time,
/// and the tokens. One read gives the caller all of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Credentials {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub token_endpoint: String,
    /// The `resource` parameter (RFC 8707), when discovery found one.
    #[serde(default)]
    pub resource: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    /// Unix seconds; `0` means the expiry is unknown.
    #[serde(default)]
    pub expires_at: f64,
}

impl Credentials {
    pub(crate) fn registered(&self) -> bool {
        !self.client_id.is_empty()
    }

    pub(crate) fn authorized(&self) -> bool {
        !self.access_token.is_empty()
    }

    fn expired(&self) -> bool {
        self.expires_at > 0.0 && expired_at(self.expires_at)
    }
}

/// Past expiry, with 30 seconds of clock margin. Pure so it is testable.
/// `0` means the expiry is unknown, and an unknown expiry is never expired.
fn expired_at(expires_at: f64) -> bool {
    if expires_at <= 0.0 {
        return false;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    now >= expires_at - 30.0
}

fn location(state: &AppState) -> String {
    crate::keychain_location(&state.config_path)
}

/// The stored credentials for one server, or `None` when there are none.
pub(crate) fn credentials(state: &AppState, server_name: &str) -> Option<Credentials> {
    crate::keychain::read_secret(&location(state), &credential_account(server_name))
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
}

fn save(state: &AppState, server_name: &str, credentials: &Credentials) -> Result<(), String> {
    let bytes = serde_json::to_string(credentials).map_err(|e| e.to_string())?;
    crate::keychain::write_secret(&location(state), &credential_account(server_name), &bytes)
}

pub(crate) fn forget(state: &AppState, server_name: &str) {
    let _ = crate::keychain::delete_secret(&location(state), &credential_account(server_name));
}

// ---------------------------------------------------------------------------
// Discovery

/// RFC 8414 authorization-server metadata, trimmed to what this flow uses.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AuthorizationMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    registration_endpoint: String,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

/// RFC 9728 protected-resource metadata.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ResourceMetadata {
    #[serde(default)]
    resource: String,
    #[serde(default)]
    authorization_servers: Vec<String>,
}

/// Origin of a server URL (`scheme://host[:port]`), so the well-known paths can
/// be attached right.
fn origin_of(url: &str) -> String {
    url.split_once("://")
        .map(|(scheme, rest)| {
            let authority = rest.split('/').next().unwrap_or(rest);
            format!("{scheme}://{authority}")
        })
        .unwrap_or_else(|| url.to_string())
}

/// Path component without a trailing slash (`""` for the origin root).
fn path_of(url: &str) -> String {
    url.split_once("://")
        .map(|(_, rest)| rest.split_once('/').map(|(_, path)| format!("/{path}")).unwrap_or_default())
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string()
}

/// Where one URL's well-known documents live, most specific first. The bare
/// URL spelling strips trailing slashes first, so a root URL never yields a
/// doubled slash or a duplicate of the origin-level spelling.
fn well_known_candidates(url: &str, well_known: &str) -> Vec<String> {
    let path = path_of(url);
    let origin = origin_of(url);
    let bare = url.trim_end_matches('/');
    let mut candidates = Vec::new();
    if !path.is_empty() {
        // RFC 9728 appends the resource path after the suffix.
        candidates.push(format!("{origin}/{well_known}{path}"));
    }
    candidates.push(format!("{origin}/{well_known}"));
    candidates.push(format!("{bare}/{well_known}"));
    candidates.dedup();
    candidates
}

async fn get_json(url: &str, client: &reqwest::Client) -> Result<Value, String> {
    let response = client
        .get(url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|error| format!("Could not reach {url}: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Reading {url} failed: {error}"))?;
    if !status.is_success() {
        return Err(format!("{url} answered HTTP {}.", status.as_u16()));
    }
    serde_json::from_str(&body).map_err(|error| format!("{url} is not JSON metadata: {error}"))
}

/// Discover the authorization metadata for one MCP server URL, returning the
/// resource identifier to use in requests (RFC 8707) and the metadata.
/// The resource metadata is tried first, exactly as MCP prescribes; its own
/// issuer list is followed; and a direct `/.well-known/oauth-authorization-
/// server` beside the server URL is the last resort.
pub(crate) async fn discover(
    url: &str,
    client: &reqwest::Client,
) -> Result<(String, AuthorizationMetadata), String> {
    for candidate in well_known_candidates(url, ".well-known/oauth-protected-resource") {
        match get_json(&candidate, client).await {
            Ok(body) => {
                let resource: ResourceMetadata = serde_json::from_value(body.clone())
                    .map_err(|error| format!("{candidate} is not resource metadata: {error}"))?;
                let issuer = resource
                    .authorization_servers
                    .first()
                    .cloned()
                    .unwrap_or_else(|| origin_of(url));
                let metadata =
                    fetch_authorization_metadata(issuer.trim_end_matches('/'), client).await?;
                return Ok((
                    if resource.resource.is_empty() { url.to_string() } else { resource.resource },
                    metadata,
                ));
            }
            // Trying the next well-known spelling is the point of this loop.
            Err(_) => continue,
        }
    }
    for candidate in well_known_candidates(url, ".well-known/oauth-authorization-server") {
        if let Ok(body) = get_json(&candidate, client).await {
            if let Ok(metadata) = serde_json::from_value::<AuthorizationMetadata>(body) {
                let expected = candidate
                    .strip_suffix("/.well-known/oauth-authorization-server")
                    .unwrap_or(&candidate);
                if metadata.issuer.trim_end_matches('/') != expected.trim_end_matches('/') {
                    return Err(format!(
                        "The metadata found at {candidate} claims issuer \"{}\", which does not match where it was found; refusing it.",
                        metadata.issuer
                    ));
                }
                return Ok((url.to_string(), metadata));
            }
        }
    }
    Err(format!(
        "No OAuth metadata answered at {url}: neither a protected-resource document (RFC 9728) nor an authorization-server document (RFC 8414)."
    ))
}

/// Fetch and validate one authorization server's metadata from its issuer
/// URL. RFC 8414: the published `issuer` must equal the metadata's own
/// location, so a document found elsewhere that claims another issuer is
/// refused rather than trusted.
async fn fetch_authorization_metadata(
    issuer: &str,
    client: &reqwest::Client,
) -> Result<AuthorizationMetadata, String> {
    let metadata_url = format!(
        "{}/.well-known/oauth-authorization-server",
        issuer.trim_end_matches('/')
    );
    let body = get_json(&metadata_url, client).await?;
    let metadata: AuthorizationMetadata = serde_json::from_value(body)
        .map_err(|error| format!("{metadata_url} is not authorization-server metadata: {error}"))?;
    let expected = issuer.trim_end_matches('/');
    if metadata.issuer.trim_end_matches('/') != expected {
        return Err(format!(
            "The metadata found at {metadata_url} claims issuer \"{}\", which does not match where it was found; refusing it.",
            metadata.issuer
        ));
    }
    Ok(metadata)
}

// ---------------------------------------------------------------------------
// Dynamic client registration (RFC 7591)

/// Register a public client for this app. Only meant to run when no client id
/// is on record; a re-registration would leave the previous one orphaned at
/// the provider.
pub(crate) async fn register_client(
    metadata: &AuthorizationMetadata,
    redirect_uri: &str,
    name: &str,
    client: &reqwest::Client,
) -> Result<Credentials, String> {
    if metadata.registration_endpoint.trim().is_empty() {
        return Err(
            "This authorization server does not advertise dynamic client registration, and no client is registered yet."
                .to_string(),
        );
    }
    let body = json!({
        "client_name": "Sarthi Studio MCP",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    });
    let response = client
        .post(&metadata.registration_endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Client registration failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Reading the registration answer failed: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "Registration answered HTTP {} with: {}",
            status.as_u16(),
            body.chars().take(300).collect::<String>()
        ));
    }
    let answer: Value = serde_json::from_str(&body)
        .map_err(|error| format!("The registration answer is not JSON: {error}"))?;
    let client_id = answer
        .get("client_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if client_id.is_empty() {
        return Err("The registration answer contained no client_id.".to_string());
    }
    Ok(Credentials {
        client_id,
        client_secret: answer
            .get("client_secret")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        token_endpoint: metadata.token_endpoint.clone(),
        resource: name.to_string(),
        scopes: metadata.scopes_supported.clone(),
        ..Credentials::default()
    })
}

// ---------------------------------------------------------------------------
// PKCE (RFC 7636)

/// A challenge-able verifier: two UUID simple spellings is 64 url-safe characters.
pub(crate) fn pkce_verifier() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// `BASE64URL(SHA256(verifier))`, unpadded — RFC 7636's S256 method.
pub(crate) fn pkce_challenge(verifier: &str) -> String {
    base64url_sha256(verifier.as_bytes())
}

pub(crate) fn base64url_sha256(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let mut out = String::with_capacity(43);
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    for byte in digest.iter().copied() {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            let index = ((buffer >> bits) & 0b11_1111) as usize;
            out.push(ALPHABET[index] as char);
        }
    }
    if bits > 0 {
        let index = ((buffer << (6 - bits)) & 0b11_1111) as usize;
        out.push(ALPHABET[index] as char);
    }
    out
}

fn query_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// The URL the browser opens. Every query value is percent-encoded here rather
/// than trusted, because the metadata arrives from the network.
pub(crate) fn build_authorize_url(
    endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    verifier: &str,
    resource: &str,
    scopes: &[String],
) -> String {
    let mut params: Vec<(String, String)> = vec![
        ("response_type".to_string(), "code".to_string()),
        ("client_id".to_string(), client_id.to_string()),
        ("redirect_uri".to_string(), redirect_uri.to_string()),
        ("state".to_string(), state.to_string()),
        (
            "code_challenge".to_string(),
            pkce_challenge(verifier),
        ),
        ("code_challenge_method".to_string(), "S256".to_string()),
    ];
    if !resource.trim().is_empty() {
        params.push(("resource".to_string(), resource.to_string()));
    }
    if !scopes.is_empty() {
        params.push(("scope".to_string(), scopes.join(" ")));
    }
    let query = params
        .iter()
        .map(|(key, value)| format!("{key}={}", query_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    let separator = if endpoint.contains('?') { "&" } else { "?" };
    format!("{endpoint}{separator}{query}")
}

/// Exchange an authorization code for tokens.
pub(crate) async fn exchange(
    credentials: &Credentials,
    redirect_uri: &str,
    code: &str,
    verifier: &str,
    http: &reqwest::Client,
) -> Result<Credentials, String> {
    let answered = token_request(
        &credentials.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ],
        &credentials.client_id,
        &credentials.client_secret,
        extras(credentials),
        http,
    )
    .await?;
    from_token_response(&credentials.token_endpoint, answered, credentials)
}

async fn token_request(
    token_endpoint: &str,
    params: &[(&str, &str)],
    client_id: &str,
    client_secret: &str,
    resource: Option<&str>,
    http: &reqwest::Client,
) -> Result<Value, String> {
    let mut form: BTreeMap<String, String> = params
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    form.insert("client_id".to_string(), client_id.to_string());
    if !client_secret.trim().is_empty() {
        form.insert("client_secret".to_string(), client_secret.to_string());
    }
    if let Some(resource) = resource {
        form.insert("resource".to_string(), resource.to_string());
    }
    let response = http
        .post(token_endpoint)
        .form(&form)
        .send()
        .await
        .map_err(|error| format!("Talking to the token endpoint failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Reading the token answer failed: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "The token endpoint answered HTTP {} with: {}",
            status.as_u16(),
            body.chars().take(300).collect::<String>()
        ));
    }
    serde_json::from_str(&body)
        .map_err(|error| format!("The token answer is not JSON: {error}"))
}

/// The token endpoint's answer, folded into what gets stored. `expires_in` is
/// the field the spec names; an absent expiry is stored as 0 and treated as
/// "unknown", never as a guess of a year.
fn from_token_response(
    endpoint: &str,
    answer: Value,
    previous: &Credentials,
) -> Result<Credentials, String> {
    if let Some(error) = answer.get("error").and_then(Value::as_str) {
        let detail = answer
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or("");
        return Err(format!("The token endpoint refused: {error}{detail}"));
    }
    let access_token = answer
        .get("access_token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if access_token.is_empty() {
        return Err("The token answer contained no access_token.".to_string());
    }
    let expires_in = answer.get("expires_in").and_then(Value::as_u64);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    Ok(Credentials {
        client_id: previous.client_id.clone(),
        client_secret: previous.client_secret.clone(),
        token_endpoint: endpoint.to_string(),
        resource: previous.resource.clone(),
        scopes: previous.scopes.clone(),
        access_token,
        refresh_token: answer
            .get("refresh_token")
            .and_then(Value::as_str)
            .unwrap_or(previous.refresh_token.as_str())
            .to_string(),
        expires_at: expires_in.map(|seconds| now + seconds as f64).unwrap_or(0.0),
    })
}

fn extras(credentials: &Credentials) -> Option<&str> {
    let resource = credentials.resource.trim();
    if resource.is_empty() {
        None
    } else {
        Some(resource)
    }
}

/// Refresh the stored access token from the refresh token.
pub(crate) async fn refresh(
    state: &AppState,
    server_name: &str,
    http: &reqwest::Client,
) -> Result<Credentials, String> {
    let Some(existing) = credentials(state, server_name) else {
        return Err(format!("\"{server_name}\" has no saved OAuth session to refresh."));
    };
    if existing.refresh_token.trim().is_empty() {
        return Err(format!(
            "\"{server_name}\" has no refresh token; sign in again."
        ));
    }
    let resource = extras(&existing);
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", existing.refresh_token.as_str()),
    ];
    let answered = token_request(
        &existing.token_endpoint,
        &params,
        &existing.client_id,
        &existing.client_secret,
        resource,
        http,
    )
    .await?;
    let renewed = from_token_response(&existing.token_endpoint, answered, &existing)?;
    save(state, server_name, &renewed)?;
    Ok(renewed)
}

/// The `Authorization` header value a connection should carry for this server,
/// refreshing first when the stored token is past its 30-second margin.
/// `None` means "no header from here": the caller's own header wins, a stdio
/// server needs nothing, or there is no usable session to authorize with.
pub(crate) async fn attachable_header(
    state: &AppState,
    server: &crate::mcp::Server,
) -> Option<String> {
    if server.headers.contains_key("authorization") {
        return None;
    }
    if server.transport != "http" && server.transport != "sse" {
        return None;
    }
    let existing = credentials(state, &server.name)?;
    if !existing.authorized() {
        return None;
    }
    let current = if existing.expired() && !existing.refresh_token.trim().is_empty() {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .ok()?;
        match refresh(state, &server.name, &http).await {
            Ok(renewed) => renewed,
            // An expired token the provider did not refresh: the caller sees
            // the server's own 401 instead of this module guessing.
            Err(_) => existing,
        }
    } else {
        existing
    };
    Some(format!("Bearer {}", current.access_token))
}

// ---------------------------------------------------------------------------
// The in-flight flow: browser <-> callback

/// A sign-in waiting for its browser redirect. The verifier stays here — the
/// browser never sees it, so an exchanged code is only tokenizable by the
/// process that started the flow.
struct PendingFlow {
    server_name: String,
    verifier: String,
    done: oneshot::Sender<CallbackAnswer>,
}

pub(crate) struct CallbackAnswer {
    code: String,
    error: String,
    /// The flow's own PKCE verifier, handed back with the code so the
    /// exchange can only happen in the process that armed the flow.
    verifier: String,
    server_name: String,
}

fn flows() -> &'static Mutex<HashMap<String, PendingFlow>> {
    static FLOWS: OnceLock<Mutex<HashMap<String, PendingFlow>>> = OnceLock::new();
    FLOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// What a started sign-in hands back to the route that opened the browser.
pub(crate) struct StartedFlow {
    pub url: String,
    pub(crate) receiver: oneshot::Receiver<CallbackAnswer>,
}

/// Begin a sign-in: discover, register if needed, arm the in-flight entry and
/// build the URL the user's browser must open. The caller shows the URL to the
/// browser, then awaits `wait_for_browser`.
pub(crate) async fn prepare(
    state: &AppState,
    server_name: &str,
    server_url: &str,
    port: u16,
) -> Result<StartedFlow, String> {
    // A state token nobody else can guess: two UUID simple spellings, the same
    // trick the PKCE verifier uses.
    let flow_state = pkce_verifier();
    let verifier = pkce_verifier();
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("Could not build an HTTP client: {error}"))?;
    let (resource, metadata) = discover(server_url, &http).await?;
    let existing = credentials(state, server_name);
    let credentials: Result<Credentials, String> = match existing {
        Some(stored) if stored.registered() => Ok(stored),
        _ => register_client(&metadata, &redirect_uri_for(port), server_url, &http)
            .await
            .and_then(|registered| {
                save(state, server_name, &registered)?;
                Ok(registered)
            }),
    };
    let credentials = credentials?;
    let url = build_authorize_url(
        &metadata.authorization_endpoint,
        &credentials.client_id,
        &redirect_uri_for(port),
        &flow_state,
        &verifier,
        &resource,
        &credentials.scopes,
    );
    let (done, receiver) = oneshot::channel();
    flows().lock().unwrap().insert(
        flow_state,
        PendingFlow {
            server_name: server_name.to_string(),
            verifier,
            done,
        },
    );
    Ok(StartedFlow { url, receiver })
}

/// The loopback redirect the authorization server must send the browser to.
/// The port is this app's own HTTP port, so no second listener is bound.
pub(crate) fn redirect_uri_for(port: u16) -> String {
    format!("http://127.0.0.1:{port}/api/mcp/oauth/callback")
}

/// The callback route: redeem the browser's code against the armed flow and
/// hand it, together with the flow's own verifier, to whoever is waiting.
/// Returns a short HTML answer for the tab.
pub(crate) fn browser_callback(
    code: &str,
    state_value: &str,
    error: &str,
) -> Result<&'static str, String> {
    let pending = flows()
        .lock()
        .unwrap()
        .remove(state_value)
        .ok_or_else(|| "No sign-in is waiting for that state value.".to_string())?;
    let _ = pending.done.send(CallbackAnswer {
        code: code.to_string(),
        error: error.to_string(),
        verifier: pending.verifier,
        server_name: pending.server_name,
    });
    Ok("<html><body><p>Authorization received. You can close this window and return to Sarthi Studio.</p></body></html>")
}

/// The route that opened the browser: wait (bounded) for the callback, then
/// exchange the code, store the credentials and record the sign-in. Every
/// outcome is explicit — a window closed early, a provider refusal or a
/// timeout is a message, not a silent hang. A timed-out flow may still be
/// armed behind a stale state value; the entry is not re-usable either way,
/// because its one-shot consumer is gone.
pub(crate) async fn wait_for_browser(
    state: &AppState,
    receiver: oneshot::Receiver<CallbackAnswer>,
    port: u16,
) -> Result<Credentials, String> {
    let answer = match tokio::time::timeout(FLOW_TIMEOUT, receiver).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(_)) => return Err("The sign-in flow was dropped; start it again.".to_string()),
        Err(_) => {
            return Err(
                "The browser did not come back within 5 minutes; the sign-in was cancelled."
                    .to_string(),
            )
        }
    };
    if !answer.error.is_empty() {
        return Err(format!("The authorization server said: {}", answer.error));
    }
    if answer.code.is_empty() {
        return Err("The browser came back without an authorization code.".to_string());
    }
    let Some(existing) = credentials(state, &answer.server_name) else {
        return Err("The client registration vanished mid-sign-in; start again.".to_string());
    };
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("Could not build an HTTP client: {error}"))?;
    let renewed = exchange(
        &existing,
        &redirect_uri_for(port),
        &answer.code,
        &answer.verifier,
        &http,
    )
    .await?;
    save(state, &answer.server_name, &renewed)?;
    crate::permissions::record(
        state,
        crate::permissions::Action::Network,
        &format!("mcp:{}", answer.server_name),
        crate::permissions::Decision::Allow,
        crate::permissions::Scope::Session,
        "",
        "OAuth sign-in for the MCP server completed",
    );
    Ok(renewed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pkce_challenge_is_standards_shaped() {
        let verifier = pkce_verifier();
        assert_eq!(verifier.len(), 64);
        assert!(verifier.chars().all(|c| c.is_ascii_alphanumeric()));
        let challenge = pkce_challenge(&verifier);
        // SHA-256 is 32 bytes; unpadded base64url is ceil(32/3*4)-2 = 43 chars.
        assert_eq!(challenge.len(), 43);
        // RFC 7636's worked example is a good grammar check.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn well_known_candidates_walk_the_documented_spellings() {
        let candidates = well_known_candidates("https://mcp.example.com/mcp/v1", ".well-known/x");
        assert_eq!(
            candidates,
            vec![
                "https://mcp.example.com/.well-known/x/mcp/v1",
                "https://mcp.example.com/.well-known/x",
                "https://mcp.example.com/mcp/v1/.well-known/x",
            ]
        );
        // A root URL has no path-specific spelling to try.
        let root = well_known_candidates("https://auth.example.com/", ".well-known/y");
        assert_eq!(root, vec!["https://auth.example.com/.well-known/y"]);
    }

    #[test]
    fn the_authorize_url_carries_every_required_parameter() {
        let url = build_authorize_url(
            "https://auth.example.com/authorize",
            "client-42",
            "http://127.0.0.1:8765/api/mcp/oauth/callback",
            "state-772",
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            "https://mcp.example.com/mcp",
            &["read".to_string(), "write".to_string()],
        );
        assert!(url.starts_with("https://auth.example.com/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client-42"));
        assert!(url.contains(&format!(
            "redirect_uri={}",
            query_encode("http://127.0.0.1:8765/api/mcp/oauth/callback")
        )));
        assert!(url.contains("state=state-772"));
        assert!(url.contains(
            "code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        ));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("scope=read%20write"));
        assert!(url.contains(&format!(
            "resource={}",
            query_encode("https://mcp.example.com/mcp")
        )));
        // An existing query mark is preserved, not broken into a second ?.
        assert!(build_authorize_url(
            "https://auth.example.com/authorize?t=1",
            "c",
            "",
            "",
            "",
            "",
            &[]
        )
        .contains("?t=1&"));
    }

    #[test]
    fn token_answers_become_stored_credentials_honestly() {
        let previous = Credentials {
            client_id: "client-42".to_string(),
            token_endpoint: "https://auth.example.com/token".to_string(),
            resource: "https://mcp.example.com/mcp".to_string(),
            ..Credentials::default()
        };
        let renewed = from_token_response(
            &previous.token_endpoint,
            json!({
                "access_token": "access-1",
                "refresh_token": "refresh-1",
                "expires_in": 3600,
            }),
            &previous,
        )
        .expect("tokens");
        assert_eq!(renewed.access_token, "access-1");
        assert_eq!(renewed.refresh_token, "refresh-1");
        assert!(renewed.expires_at > 0.0);
        assert!(renewed.authorized());

        // No expiry at all is stored as unknown, never mistaken for a value.
        let open_ended = from_token_response(
            &previous.token_endpoint,
            json!({ "access_token": "a" }),
            &previous,
        )
        .expect("tokens");
        assert_eq!(open_ended.expires_at, 0.0);
        assert!(!open_ended.expired(), "unknown expiry is not an expired token");

        // A provider refusal is text, not a stored credential.
        assert!(from_token_response(
            &previous.token_endpoint,
            json!({ "error": "invalid_grant", "error_description": " no code" }),
            &previous
        )
        .unwrap_err()
        .contains("invalid_grant"));
    }

    #[test]
    fn expiries_get_a_clock_margin() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        assert!(expired_at(now - 1.0), "the past is expired");
        assert!(expired_at(now + 5.0), "inside the 30 s margin is expired");
        assert!(!expired_at(now + 60.0), "a future minute is not expired");
        assert!(!expired_at(0.0), "unknown is never treated as expired");
    }

    #[tokio::test]
    async fn the_full_flow_round_trips_against_a_mock_server() {
        // A mock authorization server carrying every document the flow asks
        // for: the server metadata, the registration endpoint and the token
        // endpoint (which mints tokens for whatever code it is handed).
        use axum::routing::{get as axum_get, post as axum_post};
        use axum::Router;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route(
                "/.well-known/oauth-authorization-server",
                axum_get({
                    let base = base.clone();
                    move || {
                        let base = base.clone();
                        async move {
                            axum::Json(json!({
                                "issuer": base,
                                "authorization_endpoint": format!("{base}/authorize"),
                                "token_endpoint": format!("{base}/token"),
                                "registration_endpoint": format!("{base}/register"),
                                "scopes_supported": ["read"],
                            }))
                        }
                    }
                }),
            )
            .route(
                "/register",
                axum_post(|| async {
                    axum::Json(json!({ "client_id": "client-from-registration" }))
                }),
            )
            .route(
                "/token",
                axum_post(|| async {
                    axum::Json(json!({
                        "access_token": "access-from-exchange",
                        "refresh_token": "refresh-from-exchange",
                        "expires_in": 1800,
                    }))
                }),
            );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let http = reqwest::Client::builder().build().unwrap();
        let (resource, metadata) = discover(&base, &http).await.expect("discovery");
        assert_eq!(resource, base, "no resource metadata; the server URL is the resource");
        assert!(metadata.registration_endpoint.ends_with("/register"));
        let credentials =
            register_client(&metadata, "http://127.0.0.1:8765/cb", &base, &http)
                .await
                .expect("registration");
        assert_eq!(credentials.client_id, "client-from-registration");
        let exchanged = exchange(
            &credentials,
            "http://127.0.0.1:8765/cb",
            "the-authorization-code",
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            &http,
        )
        .await
        .expect("exchange");
        assert_eq!(exchanged.access_token, "access-from-exchange");
        assert_eq!(exchanged.refresh_token, "refresh-from-exchange");
        assert_eq!(exchanged.scopes, vec!["read".to_string()]);
    }

    #[tokio::test]
    async fn the_callback_binds_code_and_verifier_to_the_armed_state() {
        let state = format!("test-state-{}", uuid::Uuid::new_v4());
        let (done, receiver) = oneshot::channel();
        flows().lock().unwrap().insert(
            state.clone(),
            PendingFlow {
                server_name: "mock".to_string(),
                verifier: "the-verifier".to_string(),
                done,
            },
        );
        let answer_page = browser_callback("code-1", &state, "").expect("callback");
        assert!(answer_page.contains("Authorization received"));
        let answer = receiver.await.expect("the waiter gets exactly one answer");
        assert_eq!(answer.code, "code-1");
        assert_eq!(answer.verifier, "the-verifier");
        assert_eq!(answer.server_name, "mock");
        // A second use of the same state finds nothing: one browser = one call.
        assert!(browser_callback("code-2", &state, "").is_err());
    }
}

