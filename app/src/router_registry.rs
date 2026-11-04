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
use serde::Serialize;

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
        config: ".config/free-ai-suite/omniroute-dir",
        health: "GET /health",
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
        config: ".config/free-ai-studio/local.json",
        health: "GET /api/tags",
        docs: "https://github.com/sairajKhilariArch/Saarthi-Studio#local-runtime",
    },
];

#[allow(dead_code)] // used by the CLI listing in slice 3
pub(crate) fn registry() -> &'static [RouterSpec] {
    ROUTERS
}

#[allow(dead_code)] // used by the CLI/UI wiring in the next slice
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
    os != "android" && runs_locally(spec, os, arch) && spec.methods.iter().any(|m| *m != InstallMethod::Remote)
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
    ROUTERS.iter().map(|spec| capability(spec, os, arch)).collect()
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
    pub endpoint: String,
    pub version: String,
    pub message: String,
}

/// Combine capability with a measured probe. Kept pure so it is testable: the
/// rule that matters is that a process which has not answered is not healthy.
pub(crate) fn view(capability: Capability, probe: Probe) -> RouterView {
    RouterView {
        installed: probe.installed,
        reachable: probe.reachable,
        healthy: probe.reachable,
        running: probe.running,
        owned_by_app: probe.owned_by_app,
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
    let mut out = Vec::with_capacity(ROUTERS.len());
    let caps = capabilities(os, arch);
    for (spec, cap) in ROUTERS.iter().zip(caps) {
        let probe = match spec.id {
            "freellmapi" => {
                let status = crate::router::status().await;
                let endpoint = format!("http://127.0.0.1:{}", status.port);
                Probe {
                    installed: status.compose_dir.is_some(),
                    reachable: status.reachable,
                    running: status.running,
                    owned_by_app: status.owned_by_app,
                    endpoint,
                    version: String::new(),
                    message: status.message,
                }
            }
            "omniroute" => {
                let status = crate::router::omniroute_status().await;
                let endpoint = format!("http://127.0.0.1:{}", status.port);
                Probe {
                    // No compose project to look for: the container or an
                    // answered endpoint is the installation evidence.
                    installed: status.running || status.reachable,
                    reachable: status.reachable,
                    running: status.running,
                    owned_by_app: status.owned_by_app,
                    endpoint,
                    version: String::new(),
                    message: status.message,
                }
            }
            "ollama" => {
                let selection = crate::local::selection_of(state);
                let base_url = if selection.base_url.trim().is_empty() {
                    "http://127.0.0.1:11434".to_string()
                } else {
                    selection.base_url.trim().trim_end_matches('/').to_string()
                };
                match crate::local::fetch_version(&base_url).await {
                    Ok(version) => Probe {
                        installed: true,
                        reachable: true,
                        running: true,
                        // The runtime is installed and run by the user; Studio
                        // never owns its lifecycle.
                        owned_by_app: false,
                        endpoint: base_url,
                        version,
                        message: "Local runtime is responding.".to_string(),
                    },
                    Err(error) => Probe::not_installed(&base_url, &error),
                }
            }
            other => Probe::not_installed("", &format!("No probe is implemented for {other} yet.")),
        };
        out.push(view(cap, probe));
    }
    out
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
            assert!(!spec.methods.is_empty(), "{} lists no install method", spec.id);
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
            assert!(spec.docs.starts_with("https://"), "{} docs must be a URL", spec.id);
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
            assert!(value.get(key).is_some(), "serialised capability lacks {key}");
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

        let up = view(cap.clone(), reachable_probe());
        assert!(up.healthy && up.reachable && up.installed);

        // Containers up but nothing answering: running, not healthy.
        let mut half = reachable_probe();
        half.reachable = false;
        half.message = "Router containers are up but the API is not answering yet.".into();
        let not_yet = view(cap.clone(), half);
        assert!(not_yet.running);
        assert!(!not_yet.healthy, "a process that has not answered is not healthy");
        assert!(!not_yet.reachable);

        let missing = view(cap, Probe::not_installed("http://127.0.0.1:3001", "no project"));
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
        let view = view(capability(spec, "macos", "arm64"), probe);
        assert!(!view.owned_by_app, "an external runtime is never owned");
        assert_eq!(view.version, "0.3.14");
    }

    #[test]
    fn view_serialises_capability_and_state_side_by_side() {
        let spec = find("freellmapi").unwrap();
        let value = serde_json::to_value(view(capability(spec, "linux", "x86_64"), reachable_probe()))
            .unwrap();
        for key in ["id", "name", "methods", "health", "installable_here", "remote_here"] {
            assert!(value.get(key).is_some(), "view lacks capability field {key}");
        }
        for key in ["installed", "reachable", "healthy", "running", "owned_by_app", "endpoint", "version", "message"] {
            assert!(value.get(key).is_some(), "view lacks state field {key}");
        }
        assert_eq!(value["healthy"], true);
    }

    #[test]
    fn host_detection_returns_registry_spellings() {
        assert!(lists(&["linux", "macos", "windows"], current_os()), "unexpected host OS");
        assert!(
            matches!(current_arch(), "x86_64" | "arm64" | "x86" | "unknown"),
            "unexpected host arch"
        );
    }
}
