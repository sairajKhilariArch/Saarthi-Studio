//! FreeLLMAPI router lifecycle: start it when needed, and stop it on exit
//! only when this app started it.
//!
//! The installer records where the compose project lives and which port the
//! router uses. This module reads those files, tracks ownership in a small
//! state file, and exposes `free-ai-studio router start|stop|status` plus the
//! `/api/router*` endpoints the UI uses.

use crate::home_dir;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path as FsPath, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const OWNERSHIP_FILE: &str = ".config/free-ai-studio/router-owned.json";
const INSTALLER_DIR_FILE: &str = ".config/free-ai-suite/freellmapi-dir";
const INSTALLER_PORT_FILE: &str = ".config/free-ai-suite/freellmapi-port";
const COMPOSE_FILES: [&str; 4] = [
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
];

#[derive(Clone, Serialize, Deserialize, Default)]
struct Ownership {
    owned_by_app: bool,
}

fn ownership_path() -> PathBuf {
    home_dir().join(OWNERSHIP_FILE)
}

fn load_ownership() -> Ownership {
    fs::read(ownership_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Ownership>(&bytes).ok())
        .unwrap_or_default()
}

fn save_ownership(ownership: &Ownership) {
    save_ownership_file(&ownership_path(), ownership);
}

fn save_ownership_file(path: &FsPath, ownership: &Ownership) {
    let Ok(bytes) = serde_json::to_vec(ownership) else {
        return;
    };
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

fn has_compose_file(dir: &FsPath) -> bool {
    COMPOSE_FILES.iter().any(|name| dir.join(name).is_file())
}

/// Where the FreeLLMAPI compose project lives, if it is installed.
pub(crate) fn compose_dir() -> Option<PathBuf> {
    let configured = fs::read_to_string(home_dir().join(INSTALLER_DIR_FILE))
        .ok()
        .map(|value| PathBuf::from(value.trim()));
    configured
        .into_iter()
        .chain(std::iter::once(home_dir().join("freellmapi")))
        .find(|candidate| has_compose_file(candidate))
}

/// Router port as recorded by the installer (default 3001).
pub(crate) fn router_port() -> u16 {
    std::env::var("FREE_AI_STUDIO_ROUTER_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            fs::read_to_string(home_dir().join(INSTALLER_PORT_FILE))
                .ok()
                .and_then(|value| value.trim().parse().ok())
        })
        .unwrap_or(3001)
}

fn run_compose(dir: &FsPath, args: &[&str]) -> Result<String, String> {
    let output = Command::new("docker")
        .arg("compose")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("Could not run docker compose: {error}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else if stderr.to_lowercase().contains("permission denied") {
        Err("Docker refused the request (permission denied). Add your user to the docker group: sudo usermod -aG docker $USER, then log out and back in.".into())
    } else {
        Err(if stderr.is_empty() {
            format!("docker compose {} failed", args.join(" "))
        } else {
            stderr
        })
    }
}

pub(crate) fn compose_running(dir: &FsPath) -> bool {
    run_compose(dir, &["ps", "-q"])
        .map(|output| !output.trim().is_empty())
        .unwrap_or(false)
}

pub(crate) fn start_compose(dir: &FsPath) -> Result<String, String> {
    run_compose(dir, &["up", "-d"])
}

pub(crate) fn stop_compose(dir: &FsPath) -> Result<String, String> {
    run_compose(dir, &["down"])
}

/// Is the router answering on its port right now?
pub(crate) async fn reachable(port: u16) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };
    client
        .get(format!("http://127.0.0.1:{port}/api/ping"))
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

pub(crate) struct Status {
    pub port: u16,
    pub reachable: bool,
    pub compose_dir: Option<PathBuf>,
    pub running: bool,
    pub owned_by_app: bool,
    pub message: String,
}

impl Status {
    pub fn to_json(&self) -> Value {
        json!({
            "port": self.port,
            "reachable": self.reachable,
            "compose_dir": self.compose_dir.as_ref().map(|dir| dir.display().to_string()),
            "running": self.running,
            "owned_by_app": self.owned_by_app,
            "message": self.message,
        })
    }
}

pub(crate) async fn status() -> Status {
    let port = router_port();
    let reachable_now = reachable(port).await;
    let dir = compose_dir();
    let running = dir
        .as_ref()
        .map(|dir| compose_running(dir))
        .unwrap_or(false);
    let owned = load_ownership().owned_by_app;
    let message = if reachable_now {
        "Router is responding.".to_string()
    } else if let Some(dir) = &dir {
        if running {
            "Router containers are up but the API is not answering yet.".to_string()
        } else {
            format!(
                "Router is stopped. Start it with `free-ai-studio router start` or from {}.",
                dir.display()
            )
        }
    } else {
        "No FreeLLMAPI compose project was found on this machine.".to_string()
    };
    Status {
        port,
        reachable: reachable_now,
        compose_dir: dir,
        running,
        owned_by_app: owned,
        message,
    }
}

/// Start the router if it is not already answering. Records ownership so the
/// app only stops what it started itself.
pub(crate) async fn ensure_started() -> Status {
    let port = router_port();
    if reachable(port).await {
        // Already running: never adopt a router the user started.
        if load_ownership().owned_by_app {
            save_ownership(&Ownership {
                owned_by_app: false,
            });
        }
        return status().await;
    }
    let Some(dir) = compose_dir() else {
        return status().await;
    };
    let started = tokio::task::spawn_blocking(move || start_compose(&dir))
        .await
        .unwrap_or_else(|error| Err(format!("Router task failed: {error}")));
    match started {
        Ok(_) => save_ownership(&Ownership { owned_by_app: true }),
        Err(error) => println!("Could not start the router automatically: {error}"),
    }
    status().await
}

/// Bring the router down if this app started it. Called on every clean exit.
pub(crate) async fn shutdown_cleanup() {
    if !load_ownership().owned_by_app {
        return;
    }
    if let Some(dir) = compose_dir() {
        let stopped = tokio::task::spawn_blocking(move || stop_compose(&dir))
            .await
            .unwrap_or_else(|error| Err(format!("Router task failed: {error}")));
        match stopped {
            Ok(_) => println!("Stopped the FreeLLMAPI router this app started."),
            Err(error) => println!("Could not stop the router this app started: {error}"),
        }
    }
    save_ownership(&Ownership {
        owned_by_app: false,
    });
}

/// Explicit start/stop for the CLI and the UI buttons.
pub(crate) async fn manual_start() -> Result<Status, String> {
    let Some(dir) = compose_dir() else {
        return Err("No FreeLLMAPI compose project was found on this machine.".into());
    };
    if reachable(router_port()).await {
        // The router is already answering, so this app is not what brings it
        // up: never adopt it, otherwise closing the app would stop a router the
        // user started themselves.
        return Ok(status().await);
    }
    tokio::task::spawn_blocking(move || start_compose(&dir))
        .await
        .map_err(|error| format!("Router task failed: {error}"))??;
    save_ownership(&Ownership { owned_by_app: true });
    Ok(status().await)
}

pub(crate) async fn manual_stop() -> Result<Status, String> {
    let Some(dir) = compose_dir() else {
        return Err("No FreeLLMAPI compose project was found on this machine.".into());
    };
    tokio::task::spawn_blocking(move || stop_compose(&dir))
        .await
        .map_err(|error| format!("Router task failed: {error}"))??;
    save_ownership(&Ownership {
        owned_by_app: false,
    });
    Ok(status().await)
}

/// `free-ai-studio router <start|stop|status>`
pub(crate) fn run_cli(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let action = args.first().map(String::as_str).unwrap_or("status");
    let runtime = tokio::runtime::Runtime::new()?;
    match action {
        "start" => runtime.block_on(async {
            match manual_start().await {
                Ok(status) => {
                    println!("{}", status.to_json());
                    Ok(())
                }
                Err(error) => Err(error.into()),
            }
        }),
        "stop" => runtime.block_on(async {
            match manual_stop().await {
                Ok(status) => {
                    println!("{}", status.to_json());
                    Ok(())
                }
                Err(error) => Err(error.into()),
            }
        }),
        _ => runtime.block_on(async {
            println!("{}", status().await.to_json());
            Ok(())
        }),
    }
}

/* ---------------------------------------------------------------- */
/* OmniRoute: the second self-hosted gateway Studio can drive.       */
/* Runs as a Docker container (diegosouzapw/omniroute) on 20128; the */
/* same ownership rule as FreeLLMAPI applies: never stop what we did  */
/* not start.                                                        */
/* ---------------------------------------------------------------- */

const OMNIROUTE_PORT: u16 = 20128;
const OMNIROUTE_OWNERSHIP_FILE: &str = ".config/free-ai-studio/omniroute-owned.json";
const OMNIROUTE_CONTAINER: &str = "omniroute";
const OMNIROUTE_IMAGE: &str = "diegosouzapw/omniroute:latest";

fn omniroute_ownership_path() -> PathBuf {
    home_dir().join(OMNIROUTE_OWNERSHIP_FILE)
}

fn load_omniroute_ownership() -> Ownership {
    fs::read(omniroute_ownership_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Ownership>(&bytes).ok())
        .unwrap_or_default()
}

fn save_omniroute_ownership(owned_by_app: bool) {
    save_ownership_file(&omniroute_ownership_path(), &Ownership { owned_by_app });
}

/// Studio's own port override, else the OmniRoute default.
pub(crate) fn omniroute_port() -> u16 {
    std::env::var("FREE_AI_STUDIO_OMNIROUTE_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(OMNIROUTE_PORT)
}

/// Any HTTP response counts as reachable: /v1/models may answer 401 without
/// a key, and that still proves the gateway is listening.
pub(crate) async fn omniroute_reachable(port: u16) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };
    client
        .get(format!("http://127.0.0.1:{port}/v1/models"))
        .send()
        .await
        .is_ok()
}

fn run_cmd(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("Could not run {program}: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else if stderr.is_empty() {
        Err(format!("{program} {} failed", args.join(" ")))
    } else {
        Err(stderr)
    }
}

fn omniroute_container_running() -> bool {
    run_cmd(
        "docker",
        &["ps", "-q", "--filter", "name=^omniroute$"],
    )
    .map(|ids| !ids.is_empty())
    .unwrap_or(false)
}

pub(crate) struct OmniStatus {
    pub port: u16,
    pub reachable: bool,
    pub running: bool,
    pub owned_by_app: bool,
    pub message: String,
}

impl OmniStatus {
    pub fn to_json(&self) -> Value {
        json!({
            "port": self.port,
            "reachable": self.reachable,
            "running": self.running,
            "owned_by_app": self.owned_by_app,
            "message": self.message,
        })
    }
}

pub(crate) async fn omniroute_status() -> OmniStatus {
    let port = omniroute_port();
    let reachable_now = omniroute_reachable(port).await;
    let running = omniroute_container_running();
    let owned = load_omniroute_ownership().owned_by_app;
    let message = if reachable_now {
        "OmniRoute is responding.".to_string()
    } else if running {
        "OmniRoute container is up but the API is not answering yet.".to_string()
    } else {
        "OmniRoute is stopped. Start it below, or run `omniroute` after `npm i -g omniroute`."
            .to_string()
    };
    OmniStatus {
        port,
        reachable: reachable_now,
        running,
        owned_by_app: owned,
        message,
    }
}

/// Start OmniRoute: reuse a stopped container, create one if Docker is there,
/// and otherwise explain the npm path instead of failing silently.
pub(crate) async fn omniroute_start() -> Result<OmniStatus, String> {
    if omniroute_reachable(omniroute_port()).await {
        // Already answering: never adopt a gateway the user started.
        if load_omniroute_ownership().owned_by_app {
            save_omniroute_ownership(false);
        }
        return Ok(omniroute_status().await);
    }
    if run_cmd("docker", &["--version"]).is_err() {
        return Err(
            "Docker was not found. Either install Docker, or install OmniRoute yourself with `npm i -g omniroute` and run `omniroute`."
                .into(),
        );
    }
    let created = if omniroute_container_running() {
        run_cmd("docker", &["start", OMNIROUTE_CONTAINER])
    } else {
        // A stopped container with the same name must be started, not recreated.
        let exists = run_cmd("docker", &["ps", "-a", "-q", "--filter", "name=^omniroute$"])
            .map(|ids| !ids.is_empty())
            .unwrap_or(false);
        if exists {
            run_cmd("docker", &["start", OMNIROUTE_CONTAINER])
        } else {
            run_cmd(
                "docker",
                &[
                    "run",
                    "-d",
                    "--name",
                    OMNIROUTE_CONTAINER,
                    "--restart",
                    "unless-stopped",
                    "-p",
                    &format!("{OMNIROUTE_PORT}:{OMNIROUTE_PORT}"),
                    "-v",
                    "omniroute-data:/app/data",
                    OMNIROUTE_IMAGE,
                ],
            )
        }
    };
    match created {
        Ok(_) => {
            save_omniroute_ownership(true);
            Ok(omniroute_status().await)
        }
        Err(error) => Err(format!("Could not start OmniRoute: {error}")),
    }
}

/// Stop OmniRoute only when Studio started it; a hand-started gateway stays.
pub(crate) async fn omniroute_stop() -> Result<OmniStatus, String> {
    if !load_omniroute_ownership().owned_by_app {
        return Err(
            "OmniRoute was not started by Studio, so it was left running. Stop it yourself with `docker stop omniroute`."
                .into(),
        );
    }
    let stopped = run_cmd("docker", &["stop", OMNIROUTE_CONTAINER]);
    save_omniroute_ownership(false);
    match stopped {
        Ok(_) => Ok(omniroute_status().await),
        Err(error) => Err(format!("Could not stop OmniRoute: {error}")),
    }
}

/// Stop OmniRoute on clean exit when this run started it.
pub(crate) async fn omniroute_shutdown_cleanup() {
    if !load_omniroute_ownership().owned_by_app {
        return;
    }
    match tokio::task::spawn_blocking(|| run_cmd("docker", &["stop", OMNIROUTE_CONTAINER]))
        .await
        .unwrap_or_else(|error| Err(format!("OmniRoute task failed: {error}")))
    {
        Ok(_) => println!("Stopped the OmniRoute gateway this app started."),
        Err(error) => println!("Could not stop the OmniRoute gateway this app started: {error}"),
    }
    save_omniroute_ownership(false);
}

/// `free-ai-studio omniroute <start|stop|status>`
pub(crate) fn run_omniroute_cli(
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let action = args.first().map(String::as_str).unwrap_or("status");
    let runtime = tokio::runtime::Runtime::new()?;
    match action {
        "start" => runtime.block_on(async {
            match omniroute_start().await {
                Ok(status) => {
                    println!("{}", status.to_json());
                    Ok(())
                }
                Err(error) => Err(error.into()),
            }
        }),
        "stop" => runtime.block_on(async {
            match omniroute_stop().await {
                Ok(status) => {
                    println!("{}", status.to_json());
                    Ok(())
                }
                Err(error) => Err(error.into()),
            }
        }),
        _ => runtime.block_on(async {
            println!("{}", omniroute_status().await.to_json());
            Ok(())
        }),
    }
}
