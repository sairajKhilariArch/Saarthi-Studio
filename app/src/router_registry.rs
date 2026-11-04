//! Phase 2 router registry: what Saarthi Studio knows about each router it can
//! manage, independent of which one is installed.
//!
//! This is the data layer the Router Manager is built on. It deliberately does
//! not duplicate the lifecycle code in `router.rs`: that module already owns the
//! compose project, the ownership file and the delete-safe stop path, and this
//! module points at those facts (config location, health endpoint, install
//! method) instead of re-deriving them.
//!
//! Slice 1 (this file) is pure data plus platform gating, so it is testable
//! without Docker, without a network and without a GUI. The `/api/routers`
//! route and the live probes build on it next.

use crate::AppState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path as FsPath, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How a router can be obtained on a platform. A router may offer several.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum InstallMethod {
    /// `docker compose up -d` in the documented project directory.
    DockerCompose,
    /// A native executable from the project's release page or package manager.
    NativeBinary,
    /// Installed by the distribution's package manager.
    PackageManager,
    /// Runs elsewhere; Studio only talks to its endpoint.
    Remote,
}

impl InstallMethod {
    fn label(self) -> &'static str {
        match self {
            InstallMethod::DockerCompose => "docker-compose",
            InstallMethod::NativeBinary => "native-binary",
            InstallMethod::PackageManager => "package-manager",
            InstallMethod::Remote => "remote",
        }
    }
}

/// Static description of one manageable router.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RouterSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// `std::env::consts::OS` values this router can run on locally.
    pub os: &'static [&'static str],
    /// CPU architectures with a supported local install ("x86_64", "arm64").
    pub arches: &'static [&'static str],
    pub methods: &'static [InstallMethod],
    /// Whether Studio starts this router at launch when the user has not chosen.
    /// FreeLLMAPI keeps the behaviour it has always had; the gateways were never
    /// started on launch, so enabling one is an explicit choice.
    pub auto_start_default: bool,
    /// Path of the configuration the app reads or writes, relative to `$HOME`.
    pub config: &'static str,
    /// Health probe the manager uses; a running process is not proof of health.
    pub health: &'static str,
    pub docs: &'static str,
}

// NOTE: only routers with an existing implementation in this crate are listed.
// Adding a router means adding its probe and its start/stop path first; an entry
// without a working lifecycle would be a claim the app cannot honour.
const ROUTERS: &[RouterSpec] = &[
    RouterSpec {
        id: "freellmapi",
        name: "FreeLLMAPI",
        description: "Self-hosted OpenAI-compatible router and key manager.",
        os: &["linux", "macos", "windows"],
        arches: &["x86_64", "arm64"],
        methods: &[InstallMethod::DockerCompose, InstallMethod::Remote],
        auto_start_default: true,
        config: ".config/free-ai-suite/freellmapi-dir",
        health: "GET /v1/models",
        docs: "https://github.com/sairajKhilariArch/Saarthi-Studio#router",
    },
    RouterSpec {
        id: "omniroute",
        name: "OmniRoute",
        description: "Multi-provider routing front end with its own console.",
        os: &["linux", "macos", "windows"],
        arches: &["x86_64", "arm64"],
        methods: &[InstallMethod::DockerCompose, InstallMethod::Remote],
        auto_start_default: false,
        config: ".config/free-ai-suite/omniroute-dir",
        // The probe in `router.rs` calls /v1/models and counts any answer as
        // reachable (401 included), so the registry documents that same path:
        // the connection tester sends exactly what the health probe sends.
        health: "GET /v1/models",
        docs: "https://github.com/sairajKhilariArch/Saarthi-Studio#router",
    },
    RouterSpec {
        id: "ollama",
        name: "Ollama",
        description: "Local model runtime used when no router is configured.",
        os: &["linux", "macos", "windows"],
        arches: &["x86_64", "arm64"],
        methods: &[
            InstallMethod::NativeBinary,
            InstallMethod::PackageManager,
            InstallMethod::Remote,
        ],
        auto_start_default: false,
        config: ".config/free-ai-studio/local.json",
        health: "GET /api/tags",
        docs: "https://github.com/sairajKhilariArch/Saarthi-Studio#local-runtime",
    },
];

#[allow(dead_code)] // used by the CLI listing in slice 3
pub(crate) fn registry() -> &'static [RouterSpec] {
    ROUTERS
}

pub(crate) fn find(id: &str) -> Option<&'static RouterSpec> {
    ROUTERS.iter().find(|spec| spec.id == id)
}

/// The host OS in `std::env::consts::OS` spelling, which is what the registry
/// lists. Kept as a function so tests can ask about another platform.
pub(crate) fn current_os() -> &'static str {
    std::env::consts::OS
}

/// The host architecture in the registry's spelling. Linux/Windows report
/// `x86_64`/`aarch64`; the registry says `arm64` because that is what the
/// router projects publish.
pub(crate) fn current_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    }
}

/// Normalise an architecture string from `uname`, a release asset name or the
/// registry into the registry's spelling.
#[allow(dead_code)] // consumed by the version/compatibility checks in slice 3
pub(crate) fn normalise_arch(arch: &str) -> &'static str {
    let lower = arch.trim().to_ascii_lowercase();
    match lower.as_str() {
        "aarch64" | "arm64" | "arm64e" => "arm64",
        "amd64" | "x86_64" | "x64" | "x86-64" => "x86_64",
        "i386" | "i686" | "x86" => "x86",
        _ => "unknown",
    }
}

fn lists(list: &[&str], value: &str) -> bool {
    list.contains(&value)
}

/// Whether this router can run locally on the given OS/architecture.
/// A router that is only reachable remotely is still usable on a platform it
/// cannot be installed on, which is why `methods` is separate from `os`.
pub(crate) fn runs_locally(spec: &RouterSpec, os: &str, arch: &str) -> bool {
    lists(spec.os, os) && lists(spec.arches, arch)
}

/// Android cannot host any of these routers locally; it talks to a desktop Host
/// API or a remote endpoint instead (spec §14). Callers should present the
/// remote method rather than an install wizard.
pub(crate) fn local_install_supported(spec: &RouterSpec, os: &str, arch: &str) -> bool {
    os != "android"
        && runs_locally(spec, os, arch)
        && spec.methods.iter().any(|m| *m != InstallMethod::Remote)
}

/// Presentation-ready view of one registry entry for a given platform. Contains
/// only what the registry can prove: no installed/running/healthy claims, which
/// come from the probes in the next slice.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Capability {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub os: &'static [&'static str],
    pub arches: &'static [&'static str],
    pub methods: Vec<&'static str>,
    pub config: &'static str,
    pub health: &'static str,
    pub docs: &'static str,
    pub installable_here: bool,
    pub remote_here: bool,
}

pub(crate) fn capability(spec: &'static RouterSpec, os: &str, arch: &str) -> Capability {
    Capability {
        id: spec.id,
        name: spec.name,
        description: spec.description,
        os: spec.os,
        arches: spec.arches,
        methods: spec.methods.iter().map(|m| m.label()).collect(),
        config: spec.config,
        health: spec.health,
        docs: spec.docs,
        installable_here: local_install_supported(spec, os, arch),
        remote_here: spec.methods.contains(&InstallMethod::Remote),
    }
}

/// Every registry entry described for the given platform.
pub(crate) fn capabilities(os: &str, arch: &str) -> Vec<Capability> {
    ROUTERS
        .iter()
        .map(|spec| capability(spec, os, arch))
        .collect()
}

// ---------------------------------------------------------------------------
// Live state

/// What the existing probes can prove about one router right now. Every field
/// is measured; nothing here is guessed from the presence of a config file.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Probe {
    /// A usable installation or a configured remote endpoint was found.
    pub installed: bool,
    /// The health probe answered just now.
    pub reachable: bool,
    /// The container or process is up, whether or not it answers yet.
    pub running: bool,
    /// This app started it, so this app may stop it again.
    pub owned_by_app: bool,
    pub endpoint: String,
    pub version: String,
    pub message: String,
}

impl Probe {
    fn not_installed(endpoint: &str, message: &str) -> Self {
        Probe {
            installed: false,
            reachable: false,
            running: false,
            owned_by_app: false,
            endpoint: endpoint.to_string(),
            version: String::new(),
            message: message.to_string(),
        }
    }
}

/// Registry capability plus live state, as the router list renders it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct RouterView {
    #[serde(flatten)]
    pub capability: Capability,
    pub installed: bool,
    pub reachable: bool,
    /// The only acceptable definition of "up": the health probe answered.
    pub healthy: bool,
    pub running: bool,
    pub owned_by_app: bool,
    /// The user's launch preference for this router, defaulted by the registry.
    pub auto_start: bool,
    pub endpoint: String,
    pub version: String,
    pub message: String,
}

/// Combine capability with a measured probe. Kept pure so it is testable: the
/// rule that matters is that a process which has not answered is not healthy.
pub(crate) fn view(capability: Capability, probe: Probe, auto_start: bool) -> RouterView {
    RouterView {
        installed: probe.installed,
        reachable: probe.reachable,
        healthy: probe.reachable,
        running: probe.running,
        owned_by_app: probe.owned_by_app,
        auto_start,
        endpoint: probe.endpoint,
        version: probe.version,
        message: probe.message,
        capability,
    }
}

/// Live state of every registered router, using the probes that already exist in
/// `router.rs` and `local.rs` rather than new ones that could disagree with them.
pub(crate) async fn views(state: &AppState) -> Vec<RouterView> {
    let os = current_os();
    let arch = current_arch();
    let caps = capabilities(os, arch);

    // The three probes are independent network calls; run them together so the
    // endpoint costs roughly the slowest one instead of their sum. Each still
    // returns exactly what its own endpoint returns, so nothing here can
    // disagree with /api/router, /api/omniroute or /api/local/status.
    let base_url = local_base_url(state);
    let prefs = preferences(state);
    let (router_status, omni_status, local_version) = tokio::join!(
        crate::router::status(),
        crate::router::omniroute_status(),
        crate::local::fetch_version(&base_url),
    );

    let mut out = Vec::with_capacity(ROUTERS.len());
    for (spec, cap) in ROUTERS.iter().zip(caps) {
        let probe = match spec.id {
            "freellmapi" => {
                let endpoint = format!("http://127.0.0.1:{}", router_status.port);
                Probe {
                    // Either deployment counts: the user's compose project, or the
                    // container Studio installed for them.
                    installed: router_status.compose_dir.is_some()
                        || crate::router::freellmapi_container_exists(),
                    reachable: router_status.reachable,
                    running: router_status.running,
                    owned_by_app: router_status.owned_by_app,
                    endpoint,
                    version: String::new(),
                    message: router_status.message.clone(),
                }
            }
            "omniroute" => {
                let endpoint = format!("http://127.0.0.1:{}", omni_status.port);
                Probe {
                    // No compose project to look for: a running container, a
                    // stopped container or an answered endpoint is the
                    // installation evidence.
                    installed: omni_status.running
                        || omni_status.reachable
                        || crate::router::omniroute_container_exists(),
                    reachable: omni_status.reachable,
                    running: omni_status.running,
                    owned_by_app: omni_status.owned_by_app,
                    endpoint,
                    version: String::new(),
                    message: omni_status.message.clone(),
                }
            }
            "ollama" => match &local_version {
                Ok(version) => Probe {
                    installed: true,
                    reachable: true,
                    running: true,
                    // The runtime is installed and run by the user; Studio
                    // never owns its lifecycle.
                    owned_by_app: false,
                    endpoint: base_url.clone(),
                    version: version.clone(),
                    message: "Local runtime is responding.".to_string(),
                },
                Err(error) => {
                    // An install this app performed counts as installed even when
                    // the runtime is not running: pressing Install again would
                    // only re-download it.
                    let mut probe = Probe::not_installed(&base_url, error);
                    if let Some(binary) = installed_ollama_binary(state) {
                        probe.installed = true;
                        probe.message = format!(
                            "Installed at {}, and not answering yet. Studio does not own the local runtime's lifecycle, so start it yourself.",
                            binary.display()
                        );
                    }
                    probe
                }
            },
            other => Probe::not_installed("", &format!("No probe is implemented for {other} yet.")),
        };
        out.push(view(cap, probe, prefs.auto_start_for(spec)));
    }
    out
}

// ---------------------------------------------------------------------------
// Per-router actions

/// The local runtime's base URL, resolved the way `/api/routers` and the local
/// probes resolve it, so a hand-configured Ollama address is tested where it
/// actually lives instead of a guessed default.
pub(crate) fn local_base_url(state: &AppState) -> String {
    let base_url = crate::local::selection_of(state).base_url;
    if base_url.trim().is_empty() {
        "http://127.0.0.1:11434".to_string()
    } else {
        base_url.trim().trim_end_matches('/').to_string()
    }
}

/// The endpoint this router is addressed at right now: the port the installer
/// recorded for the gateways, the configured base URL for the local runtime.
fn endpoint_for(state: &AppState, id: &str) -> String {
    match id {
        "freellmapi" => format!("http://127.0.0.1:{}", crate::router::router_port()),
        "omniroute" => format!("http://127.0.0.1:{}", crate::router::omniroute_port()),
        _ => local_base_url(state),
    }
}

/// The method and path out of a registry `health` string, e.g. `GET /v1/models`.
/// Only GET probes exist today; a different method is not silently sent as GET.
fn health_path(spec: &RouterSpec) -> Result<&'static str, String> {
    spec.health
        .strip_prefix("GET ")
        .map(str::trim)
        .filter(|path| path.starts_with('/'))
        .ok_or_else(|| format!("{} has no usable health probe ({})", spec.name, spec.health))
}

/// Outcome of the connection tester. `ok` means the endpoint answered, which is
/// what an HTTP status code proves; the status itself is reported so a 401 (a
/// listener that wants a key) is not mistaken for a dead router.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct ConnectionTest {
    pub id: &'static str,
    pub endpoint: String,
    pub path: String,
    pub ok: bool,
    pub status: Option<u16>,
    pub latency_ms: u64,
    pub message: String,
}

impl ConnectionTest {
    fn from_response(
        spec: &'static RouterSpec,
        endpoint: String,
        path: &str,
        status: u16,
        latency_ms: u64,
    ) -> Self {
        let message = if (200..300).contains(&status) {
            format!("{endpoint}{path} answered {status} in {latency_ms} ms.")
        } else {
            format!(
                "{endpoint}{path} answered {status} in {latency_ms} ms: the endpoint is listening but refused the request, so a key or dashboard sign-in is probably missing."
            )
        };
        ConnectionTest {
            id: spec.id,
            endpoint,
            path: path.to_string(),
            ok: true,
            status: Some(status),
            latency_ms,
            message,
        }
    }

    fn from_error(
        spec: &'static RouterSpec,
        endpoint: String,
        path: &str,
        latency_ms: u64,
        error: &str,
    ) -> Self {
        ConnectionTest {
            id: spec.id,
            endpoint: endpoint.clone(),
            path: path.to_string(),
            ok: false,
            status: None,
            latency_ms,
            message: format!("No answer from {endpoint}{path} in {latency_ms} ms: {error}"),
        }
    }
}

/// Probe one router on demand, with the latency and status code the periodic
/// probe throws away. This is a real request to the documented health path; it
/// never reports success from a config file or a running process.
pub(crate) async fn test_connection(state: &AppState, id: &str) -> Result<ConnectionTest, String> {
    let spec = find(id).ok_or_else(|| format!("Unknown router id: {id}"))?;
    let path = health_path(spec)?;
    let endpoint = endpoint_for(state, id);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|error| format!("Could not build an HTTP client: {error}"))?;
    let started = Instant::now();
    let response = client.get(format!("{endpoint}{path}")).send().await;
    // Measure before formatting: the message must not inflate the latency it
    // reports.
    let elapsed = started.elapsed().as_millis() as u64;
    Ok(match response {
        Ok(response) => {
            ConnectionTest::from_response(spec, endpoint, path, response.status().as_u16(), elapsed)
        }
        Err(error) => ConnectionTest::from_error(spec, endpoint, path, elapsed, &error.to_string()),
    })
}

const DEFAULT_LOG_LINES: usize = 200;
const MAX_LOG_LINES: usize = 1000;
/// One read may not pull an unbounded log into the response.
const MAX_LOG_BYTES: usize = 64 * 1024;

/// `?tail=` is user input, so it is clamped rather than trusted.
pub(crate) fn clamp_tail(requested: Option<usize>) -> usize {
    requested
        .unwrap_or(DEFAULT_LOG_LINES)
        .clamp(1, MAX_LOG_LINES)
}

/// Keep the newest `limit` lines within a byte cap. Docker is already asked for
/// a tail, so this is the guard that still holds if it sends more than asked.
fn tail_lines(text: &str, limit: usize) -> (Vec<String>, bool) {
    let mut lines: Vec<String> = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = false;
    for line in text.lines().rev() {
        if lines.len() >= limit || bytes + line.len() > MAX_LOG_BYTES {
            truncated = true;
            break;
        }
        bytes += line.len() + 1;
        lines.push(line.to_string());
    }
    lines.reverse();
    (lines, truncated)
}

/// A router's own recent output, as the panel prints it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct RouterLogs {
    pub id: &'static str,
    /// Where the lines were read from, named for the command that produced them.
    pub source: String,
    pub tail: usize,
    pub lines: Vec<String>,
    /// Earlier lines were cut, either by the tail or by the byte cap.
    pub truncated: bool,
}

/// Read one router's recent output. Only routers whose process Studio drives
/// have logs here; the local runtime's output belongs to the user's own service
/// manager, so asking for it is refused instead of returning an empty box.
pub(crate) async fn logs(id: &str, requested: Option<usize>) -> Result<RouterLogs, String> {
    let spec = find(id).ok_or_else(|| format!("Unknown router id: {id}"))?;
    let tail = clamp_tail(requested);
    let (source, text) = match id {
        "freellmapi" => {
            let dir = crate::router::compose_dir().ok_or_else(|| {
                "No FreeLLMAPI compose project was found on this machine, so there is no container output to read."
                    .to_string()
            })?;
            let read_dir = dir.clone();
            let text =
                tokio::task::spawn_blocking(move || crate::router::compose_logs(&read_dir, tail))
                    .await
                    .map_err(|error| format!("Log task failed: {error}"))??;
            (format!("docker compose logs · {}", dir.display()), text)
        }
        "omniroute" => {
            let text = tokio::task::spawn_blocking(move || crate::router::omniroute_logs(tail))
                .await
                .map_err(|error| format!("Log task failed: {error}"))??;
            ("docker logs omniroute".to_string(), text)
        }
        _ => {
            return Err(format!(
            "Studio does not read the {} runtime's output: it belongs to your own service manager.",
            spec.name
        ))
        }
    };
    let (lines, truncated) = tail_lines(&text, tail);
    Ok(RouterLogs {
        id: spec.id,
        source,
        tail,
        lines,
        truncated,
    })
}

/// Restart a router through the lifecycle this build already owns. Composed
/// from the same explicit stop/start paths the buttons use, so the ownership
/// rules are unchanged: OmniRoute still refuses to stop a gateway Studio did
/// not start, and the local runtime has no lifecycle here to restart.
pub(crate) async fn restart(id: &str) -> Result<String, String> {
    match id {
        "freellmapi" => {
            crate::router::manual_stop().await?;
            let status = crate::router::manual_start().await?;
            Ok(if status.reachable {
                format!("FreeLLMAPI restarted and is answering on port {}.", status.port)
            } else {
                format!(
                    "FreeLLMAPI was restarted but is not answering yet on port {}.",
                    status.port
                )
            })
        }
        "omniroute" => {
            crate::router::omniroute_stop().await?;
            let status = crate::router::omniroute_start().await?;
            Ok(if status.reachable {
                format!("OmniRoute restarted and is answering on port {}.", status.port)
            } else {
                format!(
                    "OmniRoute was restarted but is not answering yet on port {}.",
                    status.port
                )
            })
        }
        "ollama" => Err(
            "Studio does not manage the Ollama runtime's lifecycle. Restart it with your own service manager (`ollama serve`, systemd, or the desktop app)."
                .to_string(),
        ),
        other => Err(format!("Unknown router id: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Launch preferences

/// Stored beside `config.json` rather than under the real `$HOME`, so
/// `FREE_AI_STUDIO_HOME` keeps sandboxed runs off the user's own settings.
const PREFS_FILE: &str = "router-prefs.json";

/// Per-router settings the user chose. Only what this build can honour is
/// stored: an id the registry does not know is never written, and a missing id
/// falls back to that router's own default instead of a guess.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Preferences {
    #[serde(default)]
    auto_start: BTreeMap<String, bool>,
}

impl Preferences {
    pub(crate) fn auto_start_for(&self, spec: &RouterSpec) -> bool {
        self.auto_start
            .get(spec.id)
            .copied()
            .unwrap_or(spec.auto_start_default)
    }

    fn set_auto_start(&mut self, id: &str, enabled: bool) {
        self.auto_start.insert(id.to_string(), enabled);
    }
}

fn prefs_path(state: &AppState) -> PathBuf {
    state.config_path.with_file_name(PREFS_FILE)
}

/// Read from an explicit path so a test never touches the real `$HOME`. A missing
/// or corrupt file means "no choices yet", never a failed launch.
fn load_prefs_from(path: &FsPath) -> Preferences {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_prefs_to(path: &FsPath, prefs: &Preferences) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(prefs)
        .map_err(|error| format!("Could not encode preferences: {error}"))?;
    crate::router::write_private_json(path, &bytes)
        .map_err(|error| format!("Could not save preferences to {}: {error}", path.display()))
}

pub(crate) fn preferences(state: &AppState) -> Preferences {
    load_prefs_from(&prefs_path(state))
}

/// Store one router's launch preference in an explicit file. Unknown ids are
/// rejected before anything is written, so the file only ever names routers this
/// build can act on.
fn set_auto_start_at(path: &FsPath, id: &str, enabled: bool) -> Result<bool, String> {
    let spec = find(id).ok_or_else(|| format!("Unknown router id: {id}"))?;
    let mut prefs = load_prefs_from(path);
    prefs.set_auto_start(spec.id, enabled);
    save_prefs_to(path, &prefs)?;
    Ok(enabled)
}

/// Store one router's launch preference and return the value as stored.
pub(crate) fn set_auto_start(state: &AppState, id: &str, enabled: bool) -> Result<bool, String> {
    set_auto_start_at(&prefs_path(state), id, enabled)
}

// ---------------------------------------------------------------------------
// Launch sweep

/// Routers this build can start by itself. The local runtime is absent on
/// purpose: Studio never owns that process.
fn can_start_locally(id: &str) -> bool {
    matches!(id, "freellmapi" | "omniroute")
}

/// The auto-start rule, pure so it needs neither Docker nor a network. Only a
/// router the user enabled, that this build can start, that is installed here and
/// that is not already answering is started — a router that answers is never
/// adopted, so closing Studio cannot stop something the user started.
pub(crate) fn should_auto_start(
    pref: bool,
    startable: bool,
    installable_here: bool,
    installed: bool,
    healthy: bool,
) -> bool {
    pref && startable && installable_here && installed && !healthy
}

/// Why an enabled router was left alone, in the words the boot log uses.
fn skip_reason(entry: &RouterView) -> String {
    if entry.healthy {
        "it is already answering, and Studio does not adopt a router it did not start".to_string()
    } else if !entry.installed {
        "it is not installed here".to_string()
    } else if !entry.capability.installable_here {
        "this build cannot run it locally".to_string()
    } else {
        "this build has no start path for it".to_string()
    }
}

/// What the sweep did with one router.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct StartupOutcome {
    pub id: &'static str,
    pub started: bool,
    pub message: String,
}

/// Start the routers whose preference says so. One router failing is recorded and
/// the sweep continues: a broken gateway must not stop the app from opening.
pub(crate) async fn startup_sweep(state: &AppState) -> Vec<StartupOutcome> {
    let prefs = preferences(state);
    let mut out = Vec::new();
    for entry in views(state).await {
        let Some(spec) = find(entry.capability.id) else {
            continue;
        };
        let enabled = prefs.auto_start_for(spec);
        if !should_auto_start(
            enabled,
            can_start_locally(spec.id),
            entry.capability.installable_here,
            entry.installed,
            entry.healthy,
        ) {
            if enabled {
                out.push(StartupOutcome {
                    id: spec.id,
                    started: false,
                    message: format!(
                        "{} is set to start with Studio, but {}.",
                        spec.name,
                        skip_reason(&entry)
                    ),
                });
            }
            continue;
        }
        out.push(match spec.id {
            "freellmapi" => match crate::router::ensure_started().await {
                status if status.reachable => StartupOutcome {
                    id: spec.id,
                    started: true,
                    message: format!(
                        "Started FreeLLMAPI at launch; it answers on port {}.",
                        status.port
                    ),
                },
                status => StartupOutcome {
                    id: spec.id,
                    started: false,
                    message: format!(
                        "Tried to start FreeLLMAPI at launch; nothing answers on port {} yet.",
                        status.port
                    ),
                },
            },
            "omniroute" => match crate::router::omniroute_start().await {
                Ok(status) if status.reachable => StartupOutcome {
                    id: spec.id,
                    started: true,
                    message: format!(
                        "Started OmniRoute at launch; it answers on port {}.",
                        status.port
                    ),
                },
                Ok(status) => StartupOutcome {
                    id: spec.id,
                    started: false,
                    message: format!(
                        "Tried to start OmniRoute at launch; nothing answers on port {} yet.",
                        status.port
                    ),
                },
                Err(error) => StartupOutcome {
                    id: spec.id,
                    started: false,
                    message: format!("Could not start OmniRoute at launch: {error}"),
                },
            },
            other => StartupOutcome {
                id: spec.id,
                started: false,
                message: format!("{other} has no launch path in this build."),
            },
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Installation

/// One measured prerequisite. Nothing here is assumed: Docker is asked and the
/// container is looked up when the plan is built, not when it is written.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Prerequisite {
    pub key: &'static str,
    pub what: String,
    pub satisfied: bool,
    pub detail: String,
}

/// One command the install would run, shown before anything executes.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct InstallStep {
    pub program: String,
    pub args: Vec<String>,
    pub purpose: String,
}

/// One fetch the install would make, shown before anything executes. The URL
/// travels with the vendor's published sha256, so approving a plan approves
/// exact bytes rather than whatever the server happens to serve later.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct DownloadStep {
    pub url: String,
    pub sha256: String,
    /// The asset name, which is also the file it is saved as.
    pub file: String,
    pub archive: crate::installer::Archive,
    pub purpose: String,
}

/// What installing one router would do. Building a plan has no side effects.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct InstallPlan {
    pub id: &'static str,
    /// Whether this build can install it at all.
    pub supported: bool,
    pub method: String,
    pub source: String,
    /// Files or Docker objects the install creates.
    pub writes: Vec<String>,
    /// How the fetched content is checked, in plain words.
    pub verification: String,
    pub prerequisites: Vec<Prerequisite>,
    pub downloads: Vec<DownloadStep>,
    pub steps: Vec<InstallStep>,
    /// Set when the install cannot proceed, with the reason a person needs.
    pub blocked: Option<String>,
    /// Binds an approval to exactly these steps: the installer rebuilds the plan
    /// and refuses if the digest it was given no longer matches.
    pub digest: String,
}

impl InstallPlan {
    /// Whether pressing Install would actually run the steps.
    pub(crate) fn ready(&self) -> bool {
        self.supported
            && self.blocked.is_none()
            && self.prerequisites.iter().all(|item| item.satisfied)
    }
}

/// SHA-256 over the material fields only: the operation, not the measurements.
/// Two plans with the same id, method, source, writes and steps digest the same
/// even if a prerequisite flipped, so an approval can never cover other steps.
pub(crate) fn plan_digest(
    id: &str,
    method: &str,
    source: &str,
    writes: &[String],
    downloads: &[DownloadStep],
    steps: &[InstallStep],
) -> String {
    let mut canonical = String::new();
    canonical.push_str(id);
    canonical.push('\u{1f}');
    canonical.push_str(method);
    canonical.push('\u{1f}');
    canonical.push_str(source);
    for write in writes {
        canonical.push('\u{1f}');
        canonical.push_str(write);
    }
    // A download is part of the operation: its URL, the hash it must match, how
    // it is unpacked and where it lands all have to survive an approval.
    for download in downloads {
        canonical.push('\u{1f}');
        canonical.push_str(&download.url);
        canonical.push('\u{1e}');
        canonical.push_str(&download.sha256);
        canonical.push('\u{1e}');
        canonical.push_str(download.archive.label());
        canonical.push('\u{1e}');
        canonical.push_str(&download.file);
    }
    for step in steps {
        canonical.push('\u{1f}');
        canonical.push_str(&step.program);
        for arg in &step.args {
            canonical.push('\u{1e}');
            canonical.push_str(arg);
        }
    }
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

const GHCR_ACCEPT: &str = "application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v2+json, application/vnd.oci.image.manifest.v1+json";

/// A `sha256:…` digest as a registry reports it. Pure, so the shape is testable
/// without a registry.
pub(crate) fn parse_registry_digest(value: &str) -> Result<String, String> {
    let value = value.trim();
    match value.strip_prefix("sha256:") {
        Some(hex) if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) => {
            Ok(format!("sha256:{}", hex.to_ascii_lowercase()))
        }
        _ => Err(format!("{value} is not a sha256 manifest digest")),
    }
}

/// Resolve a tag's manifest digest from a registry that hands out an anonymous
/// pull token first, which is what `ghcr.io` requires. Only the digest is read:
/// no layer is downloaded here.
pub(crate) async fn resolve_ghcr_digest(image: &str) -> Result<String, String> {
    let (repo, tag) = split_image(image);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| format!("Could not build an HTTP client: {error}"))?;
    let token_url = format!("https://ghcr.io/token?scope=repository:{repo}:pull");
    let token: serde_json::Value = client
        .get(&token_url)
        .send()
        .await
        .map_err(|error| format!("Could not reach {token_url}: {error}"))?
        .json()
        .await
        .map_err(|error| format!("Could not read the registry token: {error}"))?;
    let token = token
        .get("token")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "The registry issued no pull token.".to_string())?;

    let manifest_url = format!("https://ghcr.io/v2/{repo}/manifests/{tag}");
    let response = client
        .head(&manifest_url)
        .bearer_auth(token)
        .header("Accept", GHCR_ACCEPT)
        .send()
        .await
        .map_err(|error| format!("Could not reach {manifest_url}: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "{manifest_url} answered {}.",
            response.status().as_u16()
        ));
    }
    let digest = response
        .headers()
        .get("docker-content-digest")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| format!("{manifest_url} reported no manifest digest."))?;
    parse_registry_digest(digest)
}

/// The registry's tag endpoint reports the manifest-list digest for a tag: the
/// identity a pinned install names. Parsing is pure so the network shape is
/// testable without a network.
fn parse_tag_digest(body: &serde_json::Value) -> Result<String, String> {
    match body.get("digest").and_then(serde_json::Value::as_str) {
        Some(digest) if digest.starts_with("sha256:") && digest.len() > "sha256:".len() => {
            Ok(digest.to_string())
        }
        Some(other) => Err(format!("The registry reported an unusable digest: {other}")),
        None => Err("The registry did not report a manifest digest for this tag.".to_string()),
    }
}

/// Split `repo[:tag]` for the registry API. No tag means `latest`, which is what
/// Docker means by it.
fn split_image(image: &str) -> (String, String) {
    match image.rsplit_once(':') {
        Some((repo, tag)) if !tag.contains('/') => (repo.to_string(), tag.to_string()),
        _ => (image.to_string(), "latest".to_string()),
    }
}

/// Ask the registry what a tag points at right now, so an install can name that
/// exact image instead of a tag that may move under it.
pub(crate) async fn resolve_image_digest(image: &str) -> Result<String, String> {
    let (repo, tag) = split_image(image);
    let url = format!("https://hub.docker.com/v2/repositories/{repo}/tags/{tag}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| format!("Could not build an HTTP client: {error}"))?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("Could not reach the registry at {url}: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "The registry at {url} answered {}.",
            response.status().as_u16()
        ));
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("Could not read the registry answer: {error}"))?;
    parse_tag_digest(&body)
}

/// Build the plan for one router from measured facts, so the whole thing is
/// testable without Docker, a network or a GUI.
fn build_plan(spec: &'static RouterSpec, facts: &Facts) -> InstallPlan {
    let docker = facts.docker;
    let container_exists = facts.container_exists && !facts.allow_existing;
    let port = facts.port;
    let pinned = &facts.pinned;
    // The local runtime is neither a container nor a compose project: its plan
    // is a verified archive download, built on its own.
    if spec.id == "ollama" {
        return ollama_plan(spec, facts);
    }
    let docker_prerequisite = Prerequisite {
        key: "docker",
        what: "Docker Engine or Docker Desktop on PATH".to_string(),
        satisfied: docker,
        detail: if docker {
            "docker answered on PATH".to_string()
        } else {
            "docker was not found on PATH".to_string()
        },
    };

    let (method, source, writes, verification, prerequisites, steps, blocked) = match spec.id {
        "omniroute" => {
            let image = crate::router::omniroute_image();
            // The pinned reference is what both the plan and the run name. An
            // unresolved digest keeps the tag in `source` for display but blocks
            // the install, because a tag is not a promise about content.
            let image_ref = match pinned {
                Ok(digest) => format!("{image}@{digest}"),
                Err(_) => image.to_string(),
            };
            let steps = vec![InstallStep {
                program: "docker".to_string(),
                args: crate::router::omniroute_run_args(port, &image_ref),
                purpose: format!(
                    "Pull {image_ref} and create the omniroute container on port {port}"
                ),
            }];
            let digest_prerequisite = Prerequisite {
                key: "image_digest",
                what: "A manifest digest for the image, resolved from the registry".to_string(),
                satisfied: pinned.is_ok(),
                detail: match pinned {
                    Ok(digest) => format!("resolved {digest}"),
                    Err(error) => error.clone(),
                },
            };
            let blocked = if !docker {
                Some(
                    "Docker was not found, so nothing can be pulled or started. Install Docker first."
                        .to_string(),
                )
            } else if container_exists {
                Some(
                    "An OmniRoute container already exists. Start it from this panel instead of installing it again."
                        .to_string(),
                )
            } else if let Err(error) = pinned {
                Some(format!(
                    "The image could not be pinned to a digest, so the install would fetch whatever the tag points at. {error}"
                ))
            } else {
                None
            };
            let verification = match pinned {
                Ok(digest) => format!(
                    "Pinned to the manifest digest {digest}: Docker verifies every layer against it, so this install cannot silently become a different image."
                ),
                Err(_) => "Not pinned: the registry digest could not be resolved, so there is nothing to verify against."
                    .to_string(),
            };
            (
                "docker-image".to_string(),
                image_ref,
                vec![
                    "docker container omniroute".to_string(),
                    "docker volume omniroute-data".to_string(),
                ],
                verification,
                vec![docker_prerequisite, digest_prerequisite],
                steps,
                blocked,
            )
        }
        "freellmapi" => {
            let image = crate::router::freellmapi_image();
            let image_ref = match pinned {
                Ok(digest) => format!("{image}@{digest}"),
                Err(_) => image.to_string(),
            };
            let steps = vec![InstallStep {
                program: "docker".to_string(),
                args: crate::router::freellmapi_run_args(port, &image_ref),
                purpose: format!(
                    "Pull {image_ref} and create the freellmapi container on port {port}, bound to loopback like the project's own compose file"
                ),
            }];
            let digest_prerequisite = Prerequisite {
                key: "image_digest",
                what: "A manifest digest for the image, resolved from the registry".to_string(),
                satisfied: pinned.is_ok(),
                detail: match pinned {
                    Ok(digest) => format!("resolved {digest}"),
                    Err(error) => error.clone(),
                },
            };
            let verification = match pinned {
                Ok(digest) => format!(
                    "Pinned to the manifest digest {digest}: Docker verifies every layer against it, so this install cannot silently become a different image."
                ),
                Err(_) => "Not pinned: the registry digest could not be resolved, so there is nothing to verify against."
                    .to_string(),
            };
            let blocked = if !docker {
                Some(
                    "Docker was not found, so nothing can be pulled or started. Install Docker first."
                        .to_string(),
                )
            } else if container_exists {
                Some(
                    "A FreeLLMAPI container already exists. Start it from this panel instead of installing it again."
                        .to_string(),
                )
            } else if let Err(error) = pinned {
                Some(format!(
                    "The image could not be pinned to a digest, so the install would fetch whatever the tag points at. {error}"
                ))
            } else {
                None
            };
            (
                "docker-image".to_string(),
                image_ref,
                vec![
                    "docker container freellmapi".to_string(),
                    "docker volume freellmapi-data".to_string(),
                ],
                verification,
                vec![docker_prerequisite.clone(), digest_prerequisite],
                steps,
                blocked,
            )
        }
        _ => (
            "native-binary".to_string(),
            spec.docs.to_string(),
            Vec::new(),
            "The vendor's installer is not re-signed or checksummed by Studio, so treating it as verified would be a claim Studio cannot check."
                .to_string(),
            Vec::new(),
            Vec::new(),
            Some(format!(
                "Studio does not run an installer for {}. Download it from the official page, then Studio will use the runtime it finds.",
                spec.name
            )),
        ),
    };

    InstallPlan {
        id: spec.id,
        supported: !steps.is_empty(),
        method,
        source,
        writes,
        verification,
        prerequisites,
        downloads: Vec::new(),
        steps,
        blocked,
        digest: String::new(),
    }
    .with_digest()
}

impl InstallPlan {
    /// Fill in the digest that binds an approval to exactly this operation.
    fn with_digest(mut self) -> Self {
        self.digest = plan_digest(
            self.id,
            &self.method,
            &self.source,
            &self.writes,
            &self.downloads,
            &self.steps,
        );
        self
    }
}

/// Everything a plan is built from, collected once. Keeping the measurements in
/// one value is what lets the whole plan be a pure function of facts, testable
/// without Docker, a network or a GUI.
#[derive(Clone, Debug)]
struct Facts {
    docker: bool,
    container_exists: bool,
    port: u16,
    pinned: Result<String, String>,
    release: Result<Release, String>,
    /// An update runs against a container that exists by definition.
    allow_existing: bool,
    install_root: PathBuf,
}

/// One OS/architecture's official Ollama asset: the file name, how it is packed,
/// and the binary the archive is expected to contain.
pub(crate) fn ollama_asset(
    os: &str,
    arch: &str,
) -> Option<(&'static str, crate::installer::Archive, &'static str)> {
    use crate::installer::Archive;
    match (os, normalise_arch(arch)) {
        ("windows", "x86_64") => Some(("ollama-windows-amd64.zip", Archive::Zip, "ollama.exe")),
        ("windows", "arm64") => Some(("ollama-windows-arm64.zip", Archive::Zip, "ollama.exe")),
        ("macos", _) => Some((
            "Ollama-darwin.zip",
            Archive::Zip,
            "Ollama.app/Contents/MacOS/ollama",
        )),
        ("linux", "x86_64") => Some(("ollama-linux-amd64.tar.zst", Archive::TarZst, "bin/ollama")),
        ("linux", "arm64") => Some(("ollama-linux-arm64.tar.zst", Archive::TarZst, "bin/ollama")),
        _ => None,
    }
}

/// The release asset a plan would fetch, with the hash published beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Release {
    pub tag: String,
    pub asset: &'static str,
    pub url: String,
    pub sha256: String,
    pub archive: crate::installer::Archive,
    pub binary: &'static str,
}

const RELEASES_API: &str = "https://api.github.com/repos/ollama/ollama/releases/latest";

async fn github_text(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("Could not build an HTTP client: {error}"))?;
    let response = client
        .get(url)
        // GitHub rejects API requests that carry no User-Agent.
        .header("User-Agent", "saarthi-studio")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| format!("Could not reach {url}: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("{url} answered {}.", response.status().as_u16()));
    }
    response
        .text()
        .await
        .map_err(|error| format!("Could not read {url}: {error}"))
}

/// Resolve this platform's official asset and the sha256 published beside it.
/// Nothing is downloaded here: the plan only names what it would fetch, so a
/// preview stays free of side effects.
pub(crate) async fn resolve_ollama_release(os: &str, arch: &str) -> Result<Release, String> {
    let (asset, archive, binary) = ollama_asset(os, arch)
        .ok_or_else(|| format!("Ollama publishes no release asset for {os}/{arch}."))?;
    let latest: serde_json::Value = serde_json::from_str(&github_text(RELEASES_API).await?)
        .map_err(|error| format!("Could not read the release list: {error}"))?;
    let tag = latest
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .filter(|tag| {
            !tag.is_empty()
                && tag
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        })
        .ok_or_else(|| "The release list did not name a usable tag.".to_string())?
        .to_string();
    // The sums live in the release itself, so the hash a plan names is the one
    // that release publishes, not a value this app carries around.
    let sums_url =
        format!("https://github.com/ollama/ollama/releases/download/{tag}/sha256sum.txt");
    let sums = crate::installer::parse_sha256sums(&github_text(&sums_url).await?);
    let sha256 = sums
        .get(&asset.to_ascii_lowercase())
        .cloned()
        .ok_or_else(|| format!("{tag} publishes no sha256 for {asset}."))?;
    Ok(Release {
        url: format!("https://github.com/ollama/ollama/releases/download/{tag}/{asset}"),
        tag,
        asset,
        sha256,
        archive,
        binary,
    })
}

/// The local runtime's plan: one archive whose hash is published, then the binary
/// it contains. Nothing here claims the runtime will be started: Studio does not
/// own that process.
fn ollama_plan(spec: &'static RouterSpec, facts: &Facts) -> InstallPlan {
    let dest = facts.install_root.join("ollama");
    let writes = vec![dest.display().to_string()];
    match &facts.release {
        Ok(release) => InstallPlan {
            id: spec.id,
            supported: true,
            method: "native-binary".to_string(),
            source: release.url.clone(),
            writes,
            verification: format!(
                "The download must hash to {} — the value {} publishes in its own sha256sum.txt. A mismatch is discarded before anything is unpacked.",
                release.sha256, release.tag
            ),
            prerequisites: vec![Prerequisite {
                key: "release_sha256",
                what: "A sha256 published beside the release asset".to_string(),
                satisfied: true,
                detail: format!("{} for {}", release.sha256, release.asset),
            }],
            downloads: vec![DownloadStep {
                url: release.url.clone(),
                sha256: release.sha256.clone(),
                file: release.asset.to_string(),
                archive: release.archive,
                purpose: format!(
                    "Download {} from the {} release and check it against the published sha256",
                    release.asset, release.tag
                ),
            }],
            steps: vec![InstallStep {
                program: dest.join(release.binary).display().to_string(),
                args: vec!["--version".to_string()],
                purpose: "Run the unpacked binary once, so the install proves it starts".to_string(),
            }],
            blocked: None,
            digest: String::new(),
        },
        Err(error) => InstallPlan {
            id: spec.id,
            supported: true,
            method: "native-binary".to_string(),
            source: "https://ollama.com/download".to_string(),
            writes,
            verification: "Nothing was fetched, so there was nothing to verify.".to_string(),
            prerequisites: vec![Prerequisite {
                key: "release_sha256",
                what: "A sha256 published beside the release asset".to_string(),
                satisfied: false,
                detail: error.clone(),
            }],
            downloads: Vec::new(),
            steps: Vec::new(),
            blocked: Some(format!(
                "Studio installs Ollama from its official release asset and checks it against that release's published sha256, but neither could be resolved, so it would have nothing to verify. Download it from https://ollama.com/download, or try again when GitHub is reachable. {error}"
            )),
            digest: String::new(),
        },
    }
    .with_digest()
}

/// Where a router Studio installed from a release archive keeps its files.
fn install_root(state: &AppState) -> PathBuf {
    state.database_path.with_file_name("routers")
}

/// The Ollama binary a Studio install left behind, if any. Its presence means
/// "installed but not started", which the probe reports separately from
/// `reachable`.
fn installed_ollama_binary(state: &AppState) -> Option<PathBuf> {
    let dir = install_root(state).join("ollama");
    [
        dir.join("ollama.exe"),
        dir.join("bin/ollama"),
        dir.join("Ollama.app/Contents/MacOS/ollama"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

/// A refusal, kept as a type so the route can answer with the right status
/// instead of guessing from a message.
pub(crate) enum InstallError {
    Unknown(String),
    Blocked(String),
    NotApproved(String),
    Failed(String),
}

impl InstallError {
    pub(crate) fn message(&self) -> &str {
        match self {
            InstallError::Unknown(message)
            | InstallError::Blocked(message)
            | InstallError::NotApproved(message)
            | InstallError::Failed(message) => message,
        }
    }
}

/// The port a router is installed onto. Each gateway has its own: planning
/// FreeLLMAPI against OmniRoute's port would publish it where nothing looks.
fn install_port(id: &str) -> u16 {
    match id {
        "freellmapi" => crate::router::router_port(),
        _ => crate::router::omniroute_port(),
    }
}

/// The plan as the app would run it now: Docker and the container are measured
/// live, so a stale page cannot install against a machine that changed.
pub(crate) async fn install_plan(state: &AppState, id: &str) -> Result<InstallPlan, InstallError> {
    plan_with(state, id, false).await
}

/// The plan to move an existing install onto what is published now. It is the
/// install plan with one exception: the container that already exists is the
/// reason to update, not a reason to refuse.
pub(crate) async fn update_plan(state: &AppState, id: &str) -> Result<InstallPlan, InstallError> {
    plan_with(state, id, true).await
}

async fn plan_with(
    state: &AppState,
    id: &str,
    allow_existing: bool,
) -> Result<InstallPlan, InstallError> {
    let spec = find(id).ok_or_else(|| InstallError::Unknown(format!("Unknown router id: {id}")))?;
    let target = spec.id.to_string();
    let (docker, container_exists) = tokio::task::spawn_blocking(move || {
        let docker = crate::router::run_cmd("docker", &["--version"]).is_ok();
        // Only the router being planned is looked up: another router's container
        // must never decide whether this install is blocked.
        let exists = docker
            && match target.as_str() {
                "omniroute" => crate::router::omniroute_container_exists(),
                "freellmapi" => crate::router::freellmapi_container_exists(),
                _ => false,
            };
        (docker, exists)
    })
    .await
    .map_err(|error| InstallError::Failed(format!("Install plan task failed: {error}")))?;
    // Only the Docker router has an image to pin; the others must not wait on a
    // registry they never use.
    let pinned = match spec.id {
        "omniroute" => resolve_image_digest(crate::router::omniroute_image()).await,
        "freellmapi" => resolve_ghcr_digest(crate::router::freellmapi_image()).await,
        _ => Ok(String::new()),
    };
    // The local runtime is installed from a release archive, so it waits on the
    // release feed instead of a registry. Neither is asked when unused.
    let release = if spec.id == "ollama" {
        resolve_ollama_release(current_os(), current_arch()).await
    } else {
        Err("this router is not installed from a release archive".to_string())
    };
    Ok(build_plan(
        spec,
        &Facts {
            docker,
            container_exists,
            port: install_port(spec.id),
            pinned,
            release,
            allow_existing,
            install_root: install_root(state),
        },
    ))
}

/// The refusals that must happen before anything runs, as pure checks.
fn approval_error(plan: &InstallPlan, approve: bool, digest: &str) -> Option<InstallError> {
    if !plan.supported {
        return Some(InstallError::Blocked(plan.blocked.clone().unwrap_or_else(
            || "This build has no install path for that router.".to_string(),
        )));
    }
    if !plan.ready() {
        return Some(InstallError::Blocked(plan.blocked.clone().unwrap_or_else(
            || "The install prerequisites are not met.".to_string(),
        )));
    }
    if !approve {
        return Some(InstallError::NotApproved(
            "The install was not approved. Show the plan, then send {\"approve\": true} with the digest it returned."
                .to_string(),
        ));
    }
    if digest != plan.digest {
        return Some(InstallError::Blocked(
            "The plan changed since it was shown, so nothing was installed. Review the plan again and approve it."
                .to_string(),
        ));
    }
    None
}

const INSTALLS_FILE: &str = "router-installs.json";

/// What an install did, recorded so the panel can show when and how a router was
/// installed instead of only that it exists.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InstallRecord {
    pub id: String,
    pub method: String,
    pub source: String,
    /// The identity of what was installed — an image digest, or the sha256 of a
    /// release asset — so a later update can be compared against it.
    #[serde(default)]
    pub artifact: String,
    pub digest: String,
    pub installed_at_unix: u64,
    /// "ok" or "failed"; a failed install is recorded too, with its error.
    pub outcome: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Installs {
    #[serde(default)]
    records: BTreeMap<String, InstallRecord>,
}

fn installs_path(state: &AppState) -> PathBuf {
    state.config_path.with_file_name(INSTALLS_FILE)
}

fn load_installs_from(path: &FsPath) -> Installs {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_installs_to(path: &FsPath, installs: &Installs) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(installs)
        .map_err(|error| format!("Could not encode install records: {error}"))?;
    crate::router::write_private_json(path, &bytes).map_err(|error| {
        format!(
            "Could not save install records to {}: {error}",
            path.display()
        )
    })
}

fn record_install_at(path: &FsPath, record: &InstallRecord) -> Result<(), String> {
    let mut installs = load_installs_from(path);
    installs.records.insert(record.id.clone(), record.clone());
    save_installs_to(path, &installs)
}

/// The last install attempt for one router, if this app made one.
pub(crate) fn install_record(
    state: &AppState,
    id: &str,
) -> Result<Option<InstallRecord>, InstallError> {
    if find(id).is_none() {
        return Err(InstallError::Unknown(format!("Unknown router id: {id}")));
    }
    Ok(load_installs_from(&installs_path(state))
        .records
        .get(id)
        .cloned())
}

/// Record a failed attempt and turn it into the error the route answers with. A
/// failure is recorded too: the panel should not pretend the router was never
/// attempted.
fn record_failure(
    state: &AppState,
    plan: &InstallPlan,
    message: String,
    verb: &str,
) -> InstallError {
    let record = InstallRecord {
        id: plan.id.to_string(),
        method: plan.method.clone(),
        source: plan.source.clone(),
        artifact: plan.artifact(),
        digest: plan.digest.clone(),
        installed_at_unix: now_unix(),
        outcome: "failed".to_string(),
        message: message.clone(),
    };
    let _ = record_install_at(&installs_path(state), &record);
    InstallError::Failed(format!("{verb} failed: {message}"))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Install a router, but only when the caller approved the plan that now runs.
/// Every refusal happens before the first command is spawned.
pub(crate) async fn install(
    state: &AppState,
    id: &str,
    approve: bool,
    digest: &str,
) -> Result<InstallRecord, InstallError> {
    let plan = install_plan(state, id).await?;
    if let Some(error) = approval_error(&plan, approve, digest) {
        return Err(error);
    }
    run_plan(state, &plan, "Install").await
}

/// Update an install that already exists. The new artifact is pulled first, so a
/// failed pull leaves the running deployment exactly as it was; only then is the
/// container replaced.
pub(crate) async fn update(
    state: &AppState,
    id: &str,
    approve: bool,
    digest: &str,
) -> Result<InstallRecord, InstallError> {
    let plan = update_plan(state, id).await?;
    if let Some(error) = approval_error(&plan, approve, digest) {
        return Err(error);
    }
    if let Some((container, _)) = crate::router::gateway_container(plan.id) {
        let image = plan.source.clone();
        let pulled = tokio::task::spawn_blocking(move || {
            crate::router::run_cmd("docker", &["pull", &image])
        })
        .await
        .map_err(|error| InstallError::Failed(format!("Update task failed: {error}")))?;
        if let Err(error) = pulled {
            return Err(record_failure(state, &plan, error, "Update"));
        }
        let removed = tokio::task::spawn_blocking(move || {
            crate::router::run_cmd("docker", &["rm", "-f", container])
        })
        .await
        .map_err(|error| InstallError::Failed(format!("Update task failed: {error}")))?;
        if let Err(error) = removed {
            return Err(record_failure(state, &plan, error, "Update"));
        }
    }
    run_plan(state, &plan, "Update").await
}

/// Run a plan the caller already approved: verified downloads, then commands.
/// Install and update share exactly this path, so neither can drift.
async fn run_plan(
    state: &AppState,
    plan: &InstallPlan,
    verb: &str,
) -> Result<InstallRecord, InstallError> {
    // Downloads come first: a verified archive is what the commands afterwards
    // are allowed to trust. A hash mismatch stops the install before unpacking.
    let dest = install_root(state).join(plan.id);
    for download in &plan.downloads {
        let archive = dest.join(&download.file);
        // An archive already on disk that still hashes correctly is not fetched
        // again; a damaged one fails this check and is downloaded afresh.
        let reused = archive.is_file()
            && crate::installer::sha256_file(&archive)
                .map(|actual| actual.eq_ignore_ascii_case(download.sha256.trim()))
                .unwrap_or(false);
        let fetched = if reused {
            Ok(())
        } else {
            match crate::installer::download_verified(&download.url, &download.sha256, &archive)
                .await
            {
                Ok(_) => Ok(()),
                Err(error) => Err(error),
            }
        }
        .and_then(|_| crate::installer::extract(download.archive, &archive, &dest).map(|_| ()));
        if let Err(error) = fetched {
            return Err(record_failure(state, plan, error, verb));
        }
    }

    for step in &plan.steps {
        let program = step.program.clone();
        let args = step.args.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
            crate::router::run_cmd(&program, &argv)
        })
        .await
        .map_err(|error| InstallError::Failed(format!("Install task failed: {error}")))?;

        if let Err(error) = outcome {
            return Err(record_failure(state, plan, error, verb));
        }
    }

    let record = InstallRecord {
        id: plan.id.to_string(),
        method: plan.method.clone(),
        source: plan.source.clone(),
        artifact: plan.artifact(),
        digest: plan.digest.clone(),
        installed_at_unix: now_unix(),
        outcome: "ok".to_string(),
        message: if verb == "Update" {
            format!("Updated {} to {}.", plan.id, plan.artifact())
        } else {
            format!("Installed {} from {}.", plan.id, plan.source)
        },
    };
    record_install_at(&installs_path(state), &record).map_err(InstallError::Failed)?;
    Ok(record)
}

impl InstallPlan {
    /// What installing this plan puts on the machine, as one comparable string:
    /// an image digest, or the sha256 of the release asset.
    pub(crate) fn artifact(&self) -> String {
        if let Some(download) = self.downloads.first() {
            return download.sha256.clone();
        }
        pinned_digest(&self.source).unwrap_or_default()
    }
}

/// The digest a pinned reference names, if it names one.
fn pinned_digest(reference: &str) -> Option<String> {
    reference
        .split_once('@')
        .map(|(_, digest)| digest.trim().to_string())
        .filter(|digest| digest.starts_with("sha256:") && digest.len() > "sha256:".len())
}

/// How an installed router compares with what is published now. Read-only: the
/// plan it carries is the one an update must be approved against.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct UpdateStatus {
    pub id: &'static str,
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub update_available: bool,
    pub message: String,
    pub plan: InstallPlan,
}

pub(crate) async fn update_status(
    state: &AppState,
    id: &str,
) -> Result<UpdateStatus, InstallError> {
    let spec = find(id).ok_or_else(|| InstallError::Unknown(format!("Unknown router id: {id}")))?;
    let record = install_record(state, id)?;
    let plan = update_plan(state, id).await?;
    let installed = record
        .as_ref()
        .map(|record| record.artifact.clone())
        .filter(|artifact| !artifact.is_empty());
    let latest = Some(plan.artifact()).filter(|artifact| !artifact.is_empty());
    let update_available = match (&installed, &latest) {
        (Some(installed), Some(latest)) => installed != latest,
        // Nothing to compare means nothing is called out of date.
        _ => false,
    };
    let message = if latest.is_none() {
        format!(
            "Nothing could be resolved for {} right now, so there is nothing to compare.",
            spec.name
        )
    } else if installed.is_none() {
        format!(
            "Studio has no install record for {}, so it cannot say whether it is current.",
            spec.name
        )
    } else if update_available {
        format!(
            "{} is installed at an older artifact than the one published now.",
            spec.name
        )
    } else {
        format!("{} is current.", spec.name)
    };
    Ok(UpdateStatus {
        id: spec.id,
        installed,
        latest,
        update_available,
        message,
        plan,
    })
}

/// What an uninstall removed, in the words the panel shows.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct UninstallOutcome {
    pub id: String,
    pub removed: Vec<String>,
    pub message: String,
}

/// Stop and remove an install. The named volume holds the user's data, so it is
/// kept unless `purge` says otherwise.
pub(crate) async fn uninstall(
    state: &AppState,
    id: &str,
    purge: bool,
) -> Result<UninstallOutcome, InstallError> {
    let spec = find(id).ok_or_else(|| InstallError::Unknown(format!("Unknown router id: {id}")))?;
    let mut removed: Vec<String> = Vec::new();
    if let Some((container, volume)) = crate::router::gateway_container(spec.id) {
        match tokio::task::spawn_blocking(move || {
            crate::router::run_cmd("docker", &["rm", "-f", container])
        })
        .await
        .map_err(|error| InstallError::Failed(format!("Uninstall task failed: {error}")))?
        {
            Ok(_) => removed.push(format!("container {container}")),
            // Already absent is the goal, not a failure.
            Err(error) if error.to_lowercase().contains("no such container") => {}
            Err(error) => return Err(InstallError::Failed(format!("Uninstall failed: {error}"))),
        }
        if purge {
            match tokio::task::spawn_blocking(move || {
                crate::router::run_cmd("docker", &["volume", "rm", volume])
            })
            .await
            .map_err(|error| InstallError::Failed(format!("Uninstall task failed: {error}")))?
            {
                Ok(_) => removed.push(format!("volume {volume} and its data")),
                Err(error) if error.to_lowercase().contains("no such volume") => {}
                Err(error) => {
                    return Err(InstallError::Failed(format!("Uninstall failed: {error}")))
                }
            }
        }
    } else {
        let dir = install_root(state).join(spec.id);
        if dir.exists() {
            let target = dir.clone();
            tokio::task::spawn_blocking(move || fs::remove_dir_all(&target))
                .await
                .map_err(|error| InstallError::Failed(format!("Uninstall task failed: {error}")))?
                .map_err(|error| InstallError::Failed(format!("Uninstall failed: {error}")))?;
            removed.push(dir.display().to_string());
        }
    }
    crate::router::forget_ownership(spec.id);
    // The record described something that no longer exists here.
    let mut installs = load_installs_from(&installs_path(state));
    installs.records.remove(spec.id);
    save_installs_to(&installs_path(state), &installs).map_err(InstallError::Failed)?;
    let message = if removed.is_empty() {
        format!("Nothing was installed for {}.", spec.name)
    } else {
        format!("Removed {}: {}.", spec.name, removed.join(", "))
    };
    Ok(UninstallOutcome {
        id: spec.id.to_string(),
        removed,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs() -> &'static [RouterSpec] {
        registry()
    }

    #[test]
    fn ids_are_unique_and_slug_shaped() {
        let mut seen = Vec::new();
        for spec in specs() {
            assert!(!seen.contains(&spec.id), "duplicate router id {}", spec.id);
            seen.push(spec.id);
            assert!(
                spec.id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "router id {} is not slug-shaped",
                spec.id
            );
        }
    }

    #[test]
    fn every_entry_is_described_enough_to_render() {
        for spec in specs() {
            assert!(!spec.name.is_empty(), "{} has no display name", spec.id);
            assert!(
                !spec.description.is_empty() && spec.description.len() > 10,
                "{} has no usable description",
                spec.id
            );
            assert!(!spec.os.is_empty(), "{} lists no OS", spec.id);
            assert!(!spec.arches.is_empty(), "{} lists no architecture", spec.id);
            assert!(
                !spec.methods.is_empty(),
                "{} lists no install method",
                spec.id
            );
            // A router the manager cannot health-check cannot be called running.
            assert!(
                spec.health.starts_with("GET "),
                "{} has no HTTP health probe",
                spec.id
            );
            assert!(
                !spec.config.is_empty() && !spec.config.starts_with('/'),
                "{} config must be HOME-relative",
                spec.id
            );
            assert!(
                spec.docs.starts_with("https://"),
                "{} docs must be a URL",
                spec.id
            );
        }
    }

    #[test]
    fn entries_are_known_routers_only() {
        // Guards against a copy-paste id that no probe would ever match.
        let ids: Vec<&str> = specs().iter().map(|spec| spec.id).collect();
        assert_eq!(ids, vec!["freellmapi", "omniroute", "ollama"]);
        assert!(find("freellmapi").is_some());
        assert!(find("nope").is_none());
    }

    #[test]
    fn arch_normalisation_matches_release_asset_names() {
        for alias in ["aarch64", "arm64", "ARM64", "arm64e"] {
            assert_eq!(normalise_arch(alias), "arm64", "alias {alias}");
        }
        for alias in ["x86_64", "amd64", "X64", "x86-64"] {
            assert_eq!(normalise_arch(alias), "x86_64", "alias {alias}");
        }
        assert_eq!(normalise_arch("mips"), "unknown");
        assert_eq!(normalise_arch(""), "unknown");
    }

    #[test]
    fn desktop_platforms_can_install_and_android_cannot() {
        for spec in specs() {
            assert!(
                local_install_supported(spec, "linux", "x86_64"),
                "{} should install on linux/x86_64",
                spec.id
            );
            assert!(
                local_install_supported(spec, "windows", "arm64"),
                "{} should install on windows/arm64",
                spec.id
            );
            assert!(
                !local_install_supported(spec, "android", "arm64"),
                "{} must not claim a local install on android",
                spec.id
            );
        }
    }

    #[test]
    fn unknown_architecture_is_never_installable() {
        let spec = find("freellmapi").unwrap();
        assert!(!runs_locally(spec, "linux", "unknown"));
        assert!(!local_install_supported(spec, "linux", "unknown"));
    }

    #[test]
    fn capability_keeps_remote_available_where_install_is_not() {
        let spec = find("freellmapi").unwrap();
        let on_android = capability(spec, "android", "arm64");
        assert!(!on_android.installable_here);
        assert!(on_android.remote_here);
        assert_eq!(on_android.methods, vec!["docker-compose", "remote"]);

        let on_linux = capability(spec, "linux", "x86_64");
        assert!(on_linux.installable_here);
        assert!(on_linux.remote_here);
    }

    #[test]
    fn ollama_native_method_is_reported_for_desktop() {
        let spec = find("ollama").unwrap();
        let cap = capability(spec, "macos", "arm64");
        assert!(cap.installable_here);
        assert!(cap.methods.contains(&"native-binary"));
        assert!(cap.methods.contains(&"package-manager"));
    }

    #[test]
    fn capabilities_serialise_with_stable_field_names() {
        let list = capabilities("linux", "x86_64");
        assert_eq!(list.len(), specs().len());
        let value = serde_json::to_value(&list[0]).unwrap();
        for key in [
            "id",
            "name",
            "description",
            "os",
            "arches",
            "methods",
            "config",
            "health",
            "installable_here",
            "remote_here",
        ] {
            assert!(
                value.get(key).is_some(),
                "serialised capability lacks {key}"
            );
        }
        assert_eq!(value["id"], "freellmapi");
    }

    fn reachable_probe() -> Probe {
        Probe {
            installed: true,
            reachable: true,
            running: true,
            owned_by_app: true,
            endpoint: "http://127.0.0.1:3001".into(),
            version: "1.2.3".into(),
            message: "Router is responding.".into(),
        }
    }

    #[test]
    fn healthy_means_the_probe_answered() {
        let spec = find("freellmapi").unwrap();
        let cap = capability(spec, "linux", "x86_64");

        let up = view(cap.clone(), reachable_probe(), true);
        assert!(up.healthy && up.reachable && up.installed);

        // Containers up but nothing answering: running, not healthy.
        let mut half = reachable_probe();
        half.reachable = false;
        half.message = "Router containers are up but the API is not answering yet.".into();
        let not_yet = view(cap.clone(), half, true);
        assert!(not_yet.running);
        assert!(
            !not_yet.healthy,
            "a process that has not answered is not healthy"
        );
        assert!(!not_yet.reachable);

        let missing = view(
            cap,
            Probe::not_installed("http://127.0.0.1:3001", "no project"),
            false,
        );
        assert!(!missing.installed && !missing.healthy && !missing.running);
        assert!(!missing.owned_by_app);
        assert_eq!(missing.message, "no project");
    }

    #[test]
    fn ownership_and_version_are_passed_through_untouched() {
        let spec = find("ollama").unwrap();
        let mut probe = reachable_probe();
        probe.owned_by_app = false;
        probe.version = "0.3.14".into();
        let view = view(capability(spec, "macos", "arm64"), probe, false);
        assert!(!view.owned_by_app, "an external runtime is never owned");
        assert_eq!(view.version, "0.3.14");
    }

    #[test]
    fn view_serialises_capability_and_state_side_by_side() {
        let spec = find("freellmapi").unwrap();
        let value = serde_json::to_value(view(
            capability(spec, "linux", "x86_64"),
            reachable_probe(),
            true,
        ))
        .unwrap();
        for key in [
            "id",
            "name",
            "methods",
            "health",
            "installable_here",
            "remote_here",
        ] {
            assert!(
                value.get(key).is_some(),
                "view lacks capability field {key}"
            );
        }
        for key in [
            "installed",
            "reachable",
            "healthy",
            "running",
            "owned_by_app",
            "auto_start",
            "endpoint",
            "version",
            "message",
        ] {
            assert!(value.get(key).is_some(), "view lacks state field {key}");
        }
        assert_eq!(value["auto_start"], true);
        assert_eq!(value["healthy"], true);
    }

    #[test]
    fn host_detection_returns_registry_spellings() {
        assert!(
            lists(&["linux", "macos", "windows"], current_os()),
            "unexpected host OS"
        );
        assert!(
            matches!(current_arch(), "x86_64" | "arm64" | "x86" | "unknown"),
            "unexpected host arch"
        );
    }

    #[test]
    fn every_health_probe_is_a_get_path() {
        for spec in specs() {
            let path = health_path(spec).unwrap_or_else(|error| panic!("{error}"));
            assert!(
                path.starts_with('/'),
                "{} health path {path} is not absolute",
                spec.id
            );
        }
        // A probe with no method or a PUT cannot be sent by a GET tester.
        let base = *find("freellmapi").unwrap();
        let no_method = RouterSpec {
            health: "/v1/models",
            ..base
        };
        assert!(health_path(&no_method).is_err());
        let wrong_method = RouterSpec {
            health: "POST /v1/models",
            ..base
        };
        assert!(health_path(&wrong_method).is_err());
    }

    #[test]
    fn connection_test_separates_an_answer_from_no_answer() {
        let spec = find("freellmapi").unwrap();

        // A 401 still proves a listener, so it is ok with the status recorded.
        let refused = ConnectionTest::from_response(
            spec,
            "http://127.0.0.1:3001".into(),
            "/v1/models",
            401,
            12,
        );
        assert!(refused.ok);
        assert_eq!(refused.status, Some(401));
        assert_eq!(refused.latency_ms, 12);
        assert!(refused.message.contains("401"));
        assert!(refused.message.contains("listening"));

        let answered = ConnectionTest::from_response(
            spec,
            "http://127.0.0.1:3001".into(),
            "/v1/models",
            200,
            7,
        );
        assert!(answered.ok);
        assert_eq!(answered.status, Some(200));

        let silent = ConnectionTest::from_error(
            spec,
            "http://127.0.0.1:3001".into(),
            "/v1/models",
            3000,
            "operation timed out",
        );
        assert!(!silent.ok, "no answer is never a passing test");
        assert_eq!(silent.status, None);
        assert!(silent.message.contains("operation timed out"));
        assert!(silent.message.contains("3000 ms"));
    }

    #[test]
    fn connection_test_serialises_what_the_ui_renders() {
        let value = serde_json::to_value(ConnectionTest::from_response(
            find("omniroute").unwrap(),
            "http://127.0.0.1:20128".into(),
            "/v1/models",
            200,
            3,
        ))
        .unwrap();
        for key in [
            "id",
            "endpoint",
            "path",
            "ok",
            "status",
            "latency_ms",
            "message",
        ] {
            assert!(value.get(key).is_some(), "test result lacks {key}");
        }
        assert_eq!(value["id"], "omniroute");
        assert_eq!(value["ok"], true);
    }

    fn temp_prefs(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "free-ai-studio-prefs-{}-{name}.json",
            std::process::id()
        ))
    }

    #[test]
    fn registry_defaults_keep_freellmapi_launching() {
        assert!(find("freellmapi").unwrap().auto_start_default);
        assert!(!find("omniroute").unwrap().auto_start_default);
        assert!(
            !find("ollama").unwrap().auto_start_default,
            "the local runtime is the user's to start"
        );
    }

    #[test]
    fn preferences_round_trip_and_keep_registry_defaults() {
        let path = temp_prefs("round-trip");
        let _ = fs::remove_file(&path);

        // Nothing stored: each router keeps its own default.
        let empty = load_prefs_from(&path);
        assert!(empty.auto_start_for(find("freellmapi").unwrap()));
        assert!(!empty.auto_start_for(find("omniroute").unwrap()));

        let mut prefs = empty.clone();
        prefs.set_auto_start("freellmapi", false);
        save_prefs_to(&path, &prefs).unwrap();

        let reloaded = load_prefs_from(&path);
        assert!(!reloaded.auto_start_for(find("freellmapi").unwrap()));
        assert!(!reloaded.auto_start_for(find("ollama").unwrap()));

        // A corrupt file means "no choices yet", not a failed launch.
        fs::write(&path, b"{not json").unwrap();
        assert!(load_prefs_from(&path).auto_start_for(find("freellmapi").unwrap()));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn auto_start_rule_never_adopts_what_already_answers() {
        // pref, startable, installable_here, installed, healthy
        assert!(should_auto_start(true, true, true, true, false));
        assert!(
            !should_auto_start(true, true, true, true, true),
            "a router that already answers is never adopted"
        );
        assert!(
            !should_auto_start(false, true, true, true, false),
            "not enabled"
        );
        assert!(
            !should_auto_start(true, false, true, true, false),
            "no start path"
        );
        assert!(
            !should_auto_start(true, true, false, true, false),
            "cannot install here"
        );
        assert!(
            !should_auto_start(true, true, true, false, false),
            "not installed"
        );
    }

    #[test]
    fn only_routers_with_a_lifecycle_can_auto_start() {
        assert!(can_start_locally("freellmapi"));
        assert!(can_start_locally("omniroute"));
        assert!(
            !can_start_locally("ollama"),
            "the local runtime belongs to the user"
        );
        assert!(!can_start_locally("nope"));
    }

    #[test]
    fn set_auto_start_writes_only_known_routers() {
        let path = temp_prefs("set");
        let _ = fs::remove_file(&path);

        let error = set_auto_start_at(&path, "nope", true).unwrap_err();
        assert!(error.contains("Unknown router id"), "got: {error}");
        assert!(!path.exists(), "a rejected id must not create the file");

        assert!(set_auto_start_at(&path, "omniroute", true).unwrap());
        let stored = load_prefs_from(&path);
        assert!(stored.auto_start_for(find("omniroute").unwrap()));
        // FreeLLMAPI was never mentioned, so it keeps its registry default.
        assert!(stored.auto_start_for(find("freellmapi").unwrap()));
        let _ = fs::remove_file(&path);
    }

    fn facts(docker: bool, container: bool, port: u16) -> Facts {
        Facts {
            docker,
            container_exists: container,
            port,
            pinned: Ok("sha256:abc123".to_string()),
            release: Err("no release was asked for".to_string()),
            allow_existing: false,
            install_root: PathBuf::from("/tmp/studio-routers"),
        }
    }

    fn plan_for(id: &str, docker: bool, container: bool) -> InstallPlan {
        build_plan(find(id).unwrap(), &facts(docker, container, 20128))
    }

    fn sample_release() -> Release {
        Release {
            tag: "v1.2.3".to_string(),
            asset: "ollama-windows-amd64.zip",
            url:
                "https://github.com/ollama/ollama/releases/download/v1.2.3/ollama-windows-amd64.zip"
                    .to_string(),
            sha256: "ab".repeat(32),
            archive: crate::installer::Archive::Zip,
            binary: "ollama.exe",
        }
    }

    #[test]
    fn omniroute_plan_names_the_exact_command() {
        let plan = plan_for("omniroute", true, false);
        assert!(plan.supported && plan.ready());
        assert_eq!(plan.method, "docker-image");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].program, "docker");
        assert_eq!(plan.steps[0].args[0], "run");
        assert!(plan.steps[0].args.iter().any(|arg| arg == &plan.source));
        // Loopback only: a bare `-p` would bind every interface.
        assert!(plan.steps[0]
            .args
            .iter()
            .any(|arg| arg == "127.0.0.1:20128:20128"));
        assert_eq!(plan.writes.len(), 2);
        assert_eq!(plan.digest.len(), 64);
        assert!(plan.digest.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn omniroute_plan_is_blocked_without_docker_or_when_installed() {
        let no_docker = plan_for("omniroute", false, false);
        assert!(!no_docker.ready());
        assert!(no_docker
            .blocked
            .expect("a blocked plan says why")
            .contains("Docker was not found"));
        assert!(!no_docker.prerequisites[0].satisfied);

        let installed = plan_for("omniroute", true, true);
        assert!(!installed.ready());
        assert!(installed
            .blocked
            .expect("a blocked plan says why")
            .contains("already exists"));
    }

    #[test]
    fn freellmapi_plan_mirrors_the_project_deployment() {
        let plan = plan_for("freellmapi", true, false);
        assert!(plan.supported);
        assert!(plan.ready(), "blocked: {:?}", plan.blocked);
        assert_eq!(plan.method, "docker-image");
        // The approved reference is pinned, not the moving tag.
        assert!(plan.source.ends_with("@sha256:abc123"), "{}", plan.source);
        let args = &plan.steps[0].args;
        // Loopback only, the project's own volume and container port.
        assert!(args.iter().any(|arg| arg == "127.0.0.1:20128:3001"));
        assert!(args
            .iter()
            .any(|arg| arg == "freellmapi-data:/app/server/data"));
        assert!(args.iter().any(|arg| arg == "PORT=3001"));
        assert_eq!(args.last().unwrap(), &plan.source);

        // Missing Docker, or a container that already exists, blocks instead.
        let no_docker = plan_for("freellmapi", false, false);
        assert!(!no_docker.ready());
        assert!(no_docker.blocked.unwrap().contains("Docker"));
        let exists = plan_for("freellmapi", true, true);
        assert!(!exists.ready());
        assert!(exists.blocked.unwrap().contains("already exists"));
    }

    #[test]
    fn registry_digests_are_validated_before_they_are_trusted() {
        assert_eq!(
            parse_registry_digest(
                "sha256:22FB2364A629C72B735C802942366AB2D737FA72E84B74F16FBD72715186E5E1"
            )
            .unwrap(),
            "sha256:22fb2364a629c72b735c802942366ab2d737fa72e84b74f16fbd72715186e5e1"
        );
        assert!(parse_registry_digest("sha256:abc").is_err());
        assert!(parse_registry_digest("md5:22fb").is_err());
        assert!(parse_registry_digest("").is_err());
    }

    #[test]
    fn update_plan_treats_an_existing_container_as_the_reason_to_update() {
        let mut measured = facts(true, true, 20128);
        measured.pinned = Ok("sha256:new".to_string());
        let spec = find("omniroute").unwrap();
        let install = build_plan(spec, &measured);
        assert!(
            !install.ready(),
            "an install must not overwrite a container"
        );
        measured.allow_existing = true;
        let update = build_plan(spec, &measured);
        assert!(update.ready(), "blocked: {:?}", update.blocked);
        assert!(update.source.ends_with("@sha256:new"));
    }

    #[test]
    fn an_install_names_what_it_put_there() {
        let mut measured = facts(true, false, 20128);
        measured.pinned = Ok("sha256:feed".to_string());
        let image = build_plan(find("omniroute").unwrap(), &measured);
        assert_eq!(image.artifact(), "sha256:feed");

        let mut local = facts(true, false, 20128);
        local.release = Ok(sample_release());
        let native = build_plan(find("ollama").unwrap(), &local);
        assert_eq!(native.artifact(), "ab".repeat(32));

        assert!(pinned_digest("ghcr.io/a/b:1@sha256:abc").is_some());
        assert!(pinned_digest("ghcr.io/a/b:1").is_none());
    }

    #[test]
    fn each_gateway_is_planned_on_its_own_port() {
        // Regression: both plans were once built with OmniRoute's port, which
        // would have published FreeLLMAPI where the app never looks for it.
        assert_eq!(install_port("freellmapi"), crate::router::router_port());
        assert_eq!(install_port("omniroute"), crate::router::omniroute_port());
    }

    #[test]
    fn ollama_plan_downloads_a_hash_checked_asset() {
        let mut measured = facts(true, false, 20128);
        measured.release = Ok(sample_release());
        let plan = build_plan(find("ollama").unwrap(), &measured);
        assert!(plan.supported);
        assert!(plan.ready(), "blocked: {:?}", plan.blocked);
        assert_eq!(plan.downloads.len(), 1);
        assert_eq!(plan.downloads[0].sha256, "ab".repeat(32));
        assert!(plan.downloads[0]
            .url
            .ends_with("v1.2.3/ollama-windows-amd64.zip"));
        // The command it runs is the binary the archive contains, and the plan
        // fetches nothing else.
        assert_eq!(plan.steps.len(), 1);
        assert!(plan.steps[0].program.ends_with("ollama.exe"));
        assert_eq!(plan.steps[0].args, vec!["--version".to_string()]);

        // The digest covers the URL and the hash, so an approval names exact
        // bytes: swapping the hash is a different plan.
        let mut swapped = facts(true, false, 20128);
        let mut other_release = sample_release();
        other_release.sha256 = "cd".repeat(32);
        swapped.release = Ok(other_release);
        assert_ne!(
            plan.digest,
            build_plan(find("ollama").unwrap(), &swapped).digest
        );
    }

    #[test]
    fn an_unresolved_release_blocks_instead_of_guessing() {
        let plan = build_plan(find("ollama").unwrap(), &facts(true, false, 20128));
        assert!(!plan.ready());
        assert!(plan.downloads.is_empty(), "nothing may be fetched unpinned");
        assert!(plan.steps.is_empty());
        let blocked = plan.blocked.expect("a blocked plan must say why");
        assert!(blocked.contains("nothing to verify"), "{blocked}");
        assert!(plan
            .prerequisites
            .iter()
            .any(|item| item.key == "release_sha256" && !item.satisfied));
    }

    #[test]
    fn asset_table_covers_only_published_platforms() {
        assert!(ollama_asset("windows", "x86_64").is_some());
        assert!(ollama_asset("windows", "arm64").is_some());
        // `aarch64` is the same architecture under its other spelling.
        assert!(ollama_asset("linux", "aarch64").is_some());
        assert!(ollama_asset("macos", "arm64").is_some());
        // Nothing is published for these, so no asset may be invented.
        assert!(ollama_asset("android", "arm64").is_none());
        assert!(ollama_asset("linux", "i686").is_none());
    }

    #[test]
    fn digest_is_stable_and_binds_the_steps() {
        assert_eq!(
            plan_for("omniroute", true, false).digest,
            plan_for("omniroute", true, false).digest
        );
        // A different port is a different command, so a different digest.
        let other = build_plan(find("omniroute").unwrap(), &facts(true, false, 20129));
        assert_ne!(plan_for("omniroute", true, false).digest, other.digest);
    }

    #[test]
    fn approval_must_match_the_plan_that_runs() {
        let plan = plan_for("omniroute", true, false);
        assert!(approval_error(&plan, true, &plan.digest).is_none());
        assert!(matches!(
            approval_error(&plan, false, &plan.digest),
            Some(InstallError::NotApproved(_))
        ));
        assert!(matches!(
            approval_error(&plan, true, "00"),
            Some(InstallError::Blocked(_))
        ));

        let blocked = plan_for("omniroute", false, false);
        assert!(matches!(
            approval_error(&blocked, true, &blocked.digest),
            Some(InstallError::Blocked(_))
        ));

        let unsupported = plan_for("ollama", true, false);
        assert!(matches!(
            approval_error(&unsupported, true, &unsupported.digest),
            Some(InstallError::Blocked(_))
        ));
    }

    #[test]
    fn plan_pins_the_image_to_the_registry_digest() {
        let mut measured = facts(true, false, 20128);
        measured.pinned = Ok("sha256:feed".to_string());
        let plan = build_plan(find("omniroute").unwrap(), &measured);
        assert!(plan.ready());
        assert_eq!(
            plan.source,
            format!("{}@sha256:feed", crate::router::omniroute_image())
        );
        // The argv names the pinned reference, not the moving tag.
        assert_eq!(plan.steps[0].args.last().unwrap(), &plan.source);
        assert!(plan.verification.contains("Pinned"));
        assert!(plan
            .prerequisites
            .iter()
            .any(|item| item.key == "image_digest" && item.satisfied));

        // An unresolved digest refuses the install rather than fetching a tag.
        let mut unpinned_facts = facts(true, false, 20128);
        unpinned_facts.pinned = Err("the registry is unreachable".to_string());
        let unpinned = build_plan(find("omniroute").unwrap(), &unpinned_facts);
        assert!(!unpinned.ready());
        assert!(unpinned
            .blocked
            .expect("a blocked plan says why")
            .contains("could not be pinned"));
        assert!(unpinned
            .prerequisites
            .iter()
            .any(|item| item.key == "image_digest" && !item.satisfied));
    }

    #[test]
    fn tag_digest_parsing_reads_the_manifest_list() {
        let ok = serde_json::json!({"digest": "sha256:abc123", "name": "latest"});
        assert_eq!(parse_tag_digest(&ok).unwrap(), "sha256:abc123");

        assert!(parse_tag_digest(&serde_json::json!({"name": "latest"})).is_err());
        assert!(parse_tag_digest(&serde_json::json!({"digest": "md5:abc"})).is_err());
        assert!(parse_tag_digest(&serde_json::json!({"digest": "sha256:"})).is_err());
    }

    #[test]
    fn image_split_defaults_to_the_latest_tag() {
        assert_eq!(
            split_image("diegosouzapw/omniroute:latest"),
            ("diegosouzapw/omniroute".to_string(), "latest".to_string())
        );
        assert_eq!(
            split_image("diegosouzapw/omniroute"),
            ("diegosouzapw/omniroute".to_string(), "latest".to_string())
        );
        assert_eq!(
            split_image("ghcr.io/a/b:1.2"),
            ("ghcr.io/a/b".to_string(), "1.2".to_string())
        );
        // A registry host with a port is not a tag.
        assert_eq!(
            split_image("localhost:5000/img"),
            ("localhost:5000/img".to_string(), "latest".to_string())
        );
    }

    #[test]
    fn install_record_round_trips_per_router() {
        let path = temp_prefs("installs");
        let _ = fs::remove_file(&path);
        let record = InstallRecord {
            id: "omniroute".into(),
            method: "docker-image".into(),
            source: "diegosouzapw/omniroute:latest".into(),
            artifact: "sha256:abc".into(),
            digest: "abc".into(),
            installed_at_unix: 1_760_000_000,
            outcome: "ok".into(),
            message: "done".into(),
        };
        record_install_at(&path, &record).unwrap();
        assert_eq!(
            load_installs_from(&path).records.get("omniroute"),
            Some(&record)
        );
        assert!(!load_installs_from(&path).records.contains_key("freellmapi"));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn install_plan_serialises_every_field_the_ui_shows() {
        let value = serde_json::to_value(plan_for("omniroute", true, false)).unwrap();
        for key in [
            "id",
            "supported",
            "method",
            "source",
            "writes",
            "verification",
            "prerequisites",
            "steps",
            "blocked",
            "digest",
        ] {
            assert!(value.get(key).is_some(), "plan lacks {key}");
        }
        assert!(value["steps"][0]["program"].is_string());
        assert!(value["prerequisites"][0]["satisfied"].is_boolean());
    }

    #[test]
    fn tail_is_clamped_to_what_one_read_may_ask_for() {
        assert_eq!(clamp_tail(None), 200);
        assert_eq!(clamp_tail(Some(50)), 50);
        assert_eq!(clamp_tail(Some(0)), 1, "a zero tail still shows a line");
        assert_eq!(clamp_tail(Some(usize::MAX)), 1000);
    }

    #[test]
    fn tail_keeps_the_newest_lines_and_says_what_it_cut() {
        let text = "one\ntwo\nthree\n";
        let (all, truncated) = tail_lines(text, 3);
        assert_eq!(all, vec!["one", "two", "three"]);
        assert!(!truncated, "nothing was dropped");

        let (newest, truncated) = tail_lines(text, 2);
        assert_eq!(newest, vec!["two", "three"]);
        assert!(truncated);

        let (empty, truncated) = tail_lines("", 10);
        assert!(empty.is_empty());
        assert!(!truncated);
    }

    #[tokio::test]
    async fn logs_refuse_routers_without_a_managed_process() {
        // The local runtime is the user's process, so Studio has no logs for it.
        let error = logs("ollama", None).await.unwrap_err();
        assert!(error.contains("service manager"), "got: {error}");

        let unknown = logs("nope", None).await.unwrap_err();
        assert!(unknown.contains("Unknown router id"), "got: {unknown}");
    }

    #[test]
    fn router_logs_serialise_what_the_panel_renders() {
        let value = serde_json::to_value(RouterLogs {
            id: "omniroute",
            source: "docker logs omniroute".into(),
            tail: 200,
            lines: vec!["listening on 20128".to_string()],
            truncated: false,
        })
        .unwrap();
        for key in ["id", "source", "tail", "lines", "truncated"] {
            assert!(value.get(key).is_some(), "logs respond without {key}");
        }
        assert_eq!(value["lines"][0], "listening on 20128");
    }

    #[tokio::test]
    async fn restart_refuses_what_this_build_cannot_own() {
        // The local runtime is the user's process, so no restart path exists.
        let error = restart("ollama").await.unwrap_err();
        assert!(error.contains("does not manage"), "got: {error}");

        let unknown = restart("nope").await.unwrap_err();
        assert!(unknown.contains("Unknown router id"), "got: {unknown}");
    }
}
