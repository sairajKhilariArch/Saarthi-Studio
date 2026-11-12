//! What the router manager needs from the host: the measured toolchain the
//! install plans talk about, and the one router whose configuration is a real
//! file on disk.
//!
//! Spec §5.3 asks for OS, distribution, architecture, package managers, Docker
//! and the required runtime dependencies to be *detected*. §5.6 asks for a
//! configuration editor. Both live here so the probing rules and the editing
//! rules are testable without Docker, a network or a GUI.
//!
//! Nothing here guesses: a tool is present when its executable is on PATH, a
//! version is reported only when the tool printed one, and a configuration file
//! exists when it is a file in the directory the install recorded.

use std::path::{Path as FsPath, PathBuf};

use serde::Serialize;

use crate::router;

/// The files a person may edit for a compose-based router. `docker config`
/// accepts the compose names; `.env` is what the compose file interpolates.
pub(crate) const EDITABLE_FILES: [&str; 5] = [
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
    ".env",
];

/// An editor that refuses to load more than this into a textarea.
pub(crate) const MAX_CONFIG_BYTES: usize = 64 * 1024;

/// One executable the manager can name. `version` is empty when the tool does
/// not answer `--version` (present is still a measured fact).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Tool {
    pub name: String,
    pub path: String,
    pub version: String,
}

impl Tool {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({ "name": self.name, "path": self.path, "version": self.version })
    }
}

/// The host, as measured now.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Environment {
    pub os: String,
    pub os_version: String,
    pub arch: String,
    /// Package managers found on PATH, with their versions where they answer.
    pub package_managers: Vec<Tool>,
    pub docker: Option<Tool>,
    /// `docker compose` (the plugin) or the legacy `docker-compose`.
    pub compose: Option<Tool>,
}

impl Environment {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "os": self.os,
            "os_version": self.os_version,
            "arch": self.arch,
            "package_managers": self.package_managers.iter().map(Tool::to_json).collect::<Vec<_>>(),
            "docker": self.docker.as_ref().map(Tool::to_json),
            "compose": self.compose.as_ref().map(Tool::to_json),
        })
    }
}

/// Package managers worth naming per platform. Linux distributions differ, so
/// the list is the union the router projects' own install docs mention; the
/// measured answer decides which ones are actually usable here.
const PACKAGE_MANAGERS: [&str; 10] = [
    "apt-get", "dnf", "pacman", "zypper", "apk",  // Linux
    "brew", // macOS and Linux
    "winget", "choco", "scoop", "port", // Windows and macOS extras
];

/// Find an executable on PATH without running it. Windows needs the PATHEXT
/// suffixes because `where`-style lookup is what the shell would do.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|part| !part.is_empty())
            .map(|part| part.to_ascii_lowercase())
            .collect()
    } else {
        Vec::new()
    };
    for dir in std::env::split_paths(&path) {
        let direct = dir.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        for extension in &extensions {
            let candidate = dir.join(format!("{name}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The first version-looking token in a tool's `--version` output, with a
/// leading `v` dropped. Returns `None` instead of inventing one.
pub(crate) fn parse_version(text: &str) -> Option<String> {
    for token in text.split(|c: char| c.is_whitespace() || c == ',' || c == '(') {
        let candidate = token.trim_start_matches('v');
        let digits = candidate
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect::<String>();
        // A version has at least one dot and does not end with one: "27.3.1"
        // yes, "3001" and "3." no.
        if digits.contains('.') && !digits.ends_with('.') && !digits.is_empty() {
            return Some(digits);
        }
    }
    None
}

fn tool(name: &str, version_args: &[&str]) -> Option<Tool> {
    let path = find_on_path(name)?;
    let version = router::run_cmd(name, version_args)
        .ok()
        .and_then(|text| parse_version(&text))
        .unwrap_or_default();
    Some(Tool {
        name: name.to_string(),
        path: path.to_string_lossy().to_string(),
        version,
    })
}

/// Measure the host. Blocking (runs `--version` commands); callers on an async
/// thread wrap this in `spawn_blocking`.
pub(crate) fn environment() -> Environment {
    let compose = tool("docker", &["compose", "version"])
        .or_else(|| {
            // The legacy standalone binary is still what some distros ship.
            tool("docker-compose", &["--version"])
        })
        .map(|mut found| {
            // Report the compose plugin under its own name, not "docker".
            if found.name == "docker" {
                found.name = "docker compose".to_string();
            }
            found
        });
    Environment {
        os: std::env::consts::OS.to_string(),
        os_version: os_version(),
        arch: crate::router_registry::current_arch().to_string(),
        package_managers: PACKAGE_MANAGERS
            .iter()
            .filter_map(|name| tool(name, &["--version"]))
            .collect(),
        docker: tool("docker", &["--version"]),
        compose,
    }
}

/// The distribution on Linux, the product version on macOS, and the build line
/// Windows reports. Empty when the platform does not say.
pub(crate) fn os_version() -> String {
    if cfg!(target_os = "linux") {
        // The same file the package managers read: PRETTY_NAME is the human
        // string, ID is the fallback when a minimal image omits it.
        let text = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        for key in ["PRETTY_NAME", "ID"] {
            if let Some(value) = text
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{key}=")))
                .map(|value| value.trim().trim_matches('"').to_string())
                .filter(|value| !value.is_empty())
            {
                return value;
            }
        }
        String::new()
    } else if cfg!(target_os = "macos") {
        router::run_cmd("sw_vers", &["-productVersion"]).unwrap_or_default()
    } else if cfg!(windows) {
        // `ver` prints "Microsoft Windows [Version 10.0.22631.4460]".
        router::run_cmd("cmd", &["/c", "ver"])
            .ok()
            .and_then(|text| {
                text.split_once('[')
                    .and_then(|(_, rest)| rest.split_once(']'))
                    .map(|(version, _)| version.trim().to_string())
            })
            .unwrap_or_default()
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// Router configuration files

/// One file the editor may open.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct ConfigFile {
    pub name: String,
    pub path: String,
    pub bytes: u64,
}

fn is_editable(name: &str) -> bool {
    EDITABLE_FILES.contains(&name)
}

/// Reject anything that is not a plain, whitelisted file name in `dir`. A
/// separator or a `..` can never be part of a name this module accepts, so a
/// request cannot escape the directory the install recorded.
pub(crate) fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Name one of the router's configuration files.".to_string());
    }
    if trimmed != name {
        return Err("Configuration file names have no surrounding spaces.".to_string());
    }
    if trimmed.contains(['/', '\\']) || trimmed.contains("..") {
        return Err(format!(
            "\"{name}\" is not a file name; use one of: {}.",
            EDITABLE_FILES.join(", ")
        ));
    }
    if !is_editable(trimmed) {
        return Err(format!(
            "The router's editor only opens {}.",
            EDITABLE_FILES.join(", ")
        ));
    }
    Ok(())
}

/// The whitelisted files that actually exist in `dir`.
pub(crate) fn list_in(dir: &FsPath) -> Vec<ConfigFile> {
    EDITABLE_FILES
        .iter()
        .filter_map(|name| {
            let path = dir.join(name);
            let metadata = std::fs::metadata(&path).ok()?;
            if !metadata.is_file() {
                return None;
            }
            Some(ConfigFile {
                name: name.to_string(),
                path: path.to_string_lossy().to_string(),
                bytes: metadata.len(),
            })
        })
        .collect()
}

pub(crate) fn read_in(dir: &FsPath, name: &str) -> Result<String, String> {
    validate_name(name)?;
    let path = dir.join(name);
    if !path.is_file() {
        return Err(format!("\"{name}\" does not exist in {}.", dir.display()));
    }
    let metadata = std::fs::metadata(&path).map_err(|error| format!("{name}: {error}"))?;
    if metadata.len() as usize > MAX_CONFIG_BYTES {
        return Err(format!(
            "\"{name}\" is {} bytes; the editor stops at {MAX_CONFIG_BYTES}.",
            metadata.len()
        ));
    }
    let bytes = std::fs::read(&path).map_err(|error| format!("{name}: {error}"))?;
    String::from_utf8(bytes).map_err(|_| format!("\"{name}\" is not a text file."))
}

/// Write through a temp file and a rename, private on unix where the file can
/// carry credentials. Returns the new size.
pub(crate) fn write_in(dir: &FsPath, name: &str, content: &str) -> Result<usize, String> {
    validate_name(name)?;
    if content.len() > MAX_CONFIG_BYTES {
        return Err(format!(
            "The file is {} bytes; the editor stops at {MAX_CONFIG_BYTES}.",
            content.len()
        ));
    }
    let path = dir.join(name);
    router::write_private_json(&path, content.as_bytes())
        .map_err(|error| format!("Could not write {name}: {error}"))?;
    Ok(content.len())
}

/// Where a router's editable configuration lives, if it has one.
pub(crate) fn config_dir(id: &str) -> Result<PathBuf, String> {
    match id {
        "freellmapi" => router::compose_dir().ok_or_else(|| {
            "No FreeLLMAPI compose project is installed, so there is no configuration file to edit."
                .to_string()
        }),
        "omniroute" => Err(
            "OmniRoute keeps its configuration inside its container; there is no file on disk to edit."
                .to_string(),
        ),
        "ollama" => Err(
            "Studio manages the local runtime's settings through the Local runtime panel, which validates them before saving."
                .to_string(),
        ),
        other => Err(format!("Unknown router id: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_come_from_the_tool_output_only() {
        assert_eq!(
            parse_version("Docker version 27.3.1, build ce12230").as_deref(),
            Some("27.3.1")
        );
        assert_eq!(
            parse_version("Docker Compose version v2.29.7").as_deref(),
            Some("2.29.7")
        );
        // No dot: a port number is not a version.
        assert_eq!(parse_version("listening on 3001"), None);
        assert_eq!(
            parse_version("apt 2.4.12 (amd64)").as_deref(),
            Some("2.4.12")
        );
    }

    #[test]
    fn only_plain_whitelisted_names_are_editable() {
        assert!(validate_name("docker-compose.yml").is_ok());
        assert!(validate_name(".env").is_ok());
        assert!(validate_name("../docker-compose.yml").is_err());
        assert!(validate_name("..\\docker-compose.yml").is_err());
        assert!(validate_name("sub/docker-compose.yml").is_err());
        assert!(validate_name("docker-compose.yml ").is_err());
        assert!(validate_name("config.json").is_err());
        assert!(validate_name("").is_err());
    }

    #[test]
    fn files_round_trip_inside_the_directory() {
        let dir =
            std::env::temp_dir().join(format!("sarthi-router-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(list_in(&dir).is_empty());

        write_in(
            &dir,
            "docker-compose.yml",
            "services:\n  freellmapi:\n    image: x\n",
        )
        .unwrap();
        write_in(&dir, ".env", "PORT=3001\n").unwrap();
        let files = list_in(&dir);
        let names: Vec<&str> = files.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(names, ["docker-compose.yml", ".env"]);

        assert_eq!(
            read_in(&dir, "docker-compose.yml").unwrap(),
            "services:\n  freellmapi:\n    image: x\n"
        );
        assert!(read_in(&dir, "config.json").is_err());
        assert!(
            read_in(&dir, "missing.yml").is_err(),
            "not a whitelisted name"
        );

        // An oversized file is refused rather than truncated.
        let big = "x".repeat(MAX_CONFIG_BYTES + 1);
        assert!(write_in(&dir, ".env", &big).is_err());
        // The previous content is untouched by a refused write.
        assert_eq!(read_in(&dir, ".env").unwrap(), "PORT=3001\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn routers_without_a_file_say_so() {
        assert!(config_dir("omniroute").is_err());
        assert!(config_dir("ollama").is_err());
        assert!(config_dir("nope")
            .unwrap_err()
            .contains("Unknown router id"));
    }
}
