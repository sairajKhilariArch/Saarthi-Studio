//! Format one editor buffer with the project's own formatter.
//!
//! Formatting is an editor action, not a write: the route takes the buffer text
//! and returns what the formatter printed. The editor marks the result dirty and
//! the normal save path — with its permission and file-claim checks — writes it.
//! Nothing here can bypass a rule a save follows.
//!
//! Only a fixed set of formatters is run, chosen by the file's extension:
//!
//! * `.rs` → `rustfmt --emit stdout --edition 2021`, reading stdin
//! * `.go` → `gofmt`, reading stdin
//! * JS/TS/JSON/CSS/SCSS/HTML/Markdown/YAML → Prettier from the open folder's
//!   `node_modules/.bin` first, then PATH, with `--stdin-filepath`
//!
//! No formatter is guessed. An unknown extension, a missing tool or a parse
//! error is a 400 that names what was looked for; a failed formatter never
//! replaces the buffer.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::runner::{bad, fail, read_capped};
use crate::AppState;

const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug)]
struct Formatter {
    name: &'static str,
    program: String,
    args: Vec<String>,
}

/// A workspace-relative path, normalised, with escapes refused. The path is not
/// opened here: formatting a brand-new unsaved buffer is allowed.
fn clean_path(value: &str) -> Result<String, String> {
    let path = value.trim().replace('\\', "/");
    if path.is_empty() {
        return Err("Open a file first.".to_string());
    }
    if path.starts_with('/') || path.as_bytes().get(1) == Some(&b':') {
        return Err("Use a path relative to the open folder.".to_string());
    }
    if path.chars().any(|ch| ch.is_control()) {
        return Err("That path contains a control character.".to_string());
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err("That path leaves the open folder.".to_string());
    }
    Ok(path)
}

fn formatter_for(root: &Path, path: &str) -> Result<Formatter, String> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".rs") {
        return Ok(Formatter {
            name: "rustfmt",
            program: "rustfmt".to_string(),
            args: ["--emit", "stdout", "--edition", "2021"]
                .iter()
                .map(|arg| arg.to_string())
                .collect(),
        });
    }
    if lower.ends_with(".go") {
        return Ok(Formatter {
            name: "gofmt",
            program: "gofmt".to_string(),
            args: Vec::new(),
        });
    }
    const PRETTIER_EXTENSIONS: &[&str] = &[
        ".js",
        ".jsx",
        ".mjs",
        ".cjs",
        ".ts",
        ".tsx",
        ".json",
        ".css",
        ".scss",
        ".html",
        ".md",
        ".markdown",
        ".yaml",
        ".yml",
    ];
    if PRETTIER_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return Ok(prettier(root, path));
    }
    Err(format!(
        "No formatter is configured for `{path}`. Supported: Rust (.rs), Go (.go) and Prettier's JS/TS/JSON/CSS/HTML/Markdown/YAML."
    ))
}

fn prettier(root: &Path, path: &str) -> Formatter {
    let bin = root.join("node_modules").join(".bin");
    #[cfg(target_os = "windows")]
    {
        // npm's shim is a .cmd file, which CreateProcess cannot start directly.
        let local = bin.join("prettier.cmd");
        if local.is_file() {
            return Formatter {
                name: "prettier",
                program: "cmd".to_string(),
                args: vec![
                    "/C".to_string(),
                    local.display().to_string(),
                    "--stdin-filepath".to_string(),
                    path.to_string(),
                ],
            };
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let local = bin.join("prettier");
        if local.is_file() {
            return Formatter {
                name: "prettier",
                program: local.display().to_string(),
                args: vec!["--stdin-filepath".to_string(), path.to_string()],
            };
        }
    }
    let _ = bin;
    Formatter {
        name: "prettier",
        program: "prettier".to_string(),
        args: vec!["--stdin-filepath".to_string(), path.to_string()],
    }
}

fn start_error(formatter: &Formatter, error: std::io::Error) -> String {
    // A missing formatter is the common case, and the fix belongs in the message.
    if formatter.name == "prettier" {
        format!(
            "Could not start prettier: {error}. Install it in this folder (node_modules/.bin/prettier) or on PATH."
        )
    } else {
        format!(
            "Could not start {}: {error}. Install it on PATH.",
            formatter.name
        )
    }
}

fn run_formatter(root: &Path, formatter: &Formatter, content: &str) -> Result<String, String> {
    let started = Instant::now();
    let mut command = Command::new(&formatter.program);
    command
        .args(&formatter.args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| start_error(formatter, error))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("{} did not accept the buffer.", formatter.name))?;
    let input = content.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{}'s output could not be captured.", formatter.name))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("{}'s error stream could not be captured.", formatter.name))?;
    let stdout_thread = std::thread::spawn(move || read_capped(stdout, MAX_CONTENT_BYTES));
    let stderr_thread = std::thread::spawn(move || read_capped(stderr, 64 * 1024));

    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if started.elapsed() >= TIMEOUT {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = writer.join();
    let out = stdout_thread.join().unwrap_or(crate::runner::Capture {
        text: String::new(),
        truncated: false,
    });
    let err = stderr_thread.join().unwrap_or(crate::runner::Capture {
        text: String::new(),
        truncated: false,
    });

    if timed_out {
        return Err(format!(
            "{} did not finish within {} seconds.",
            formatter.name,
            TIMEOUT.as_secs()
        ));
    }
    if out.truncated {
        return Err(format!(
            "{} produced more than 2 MiB of output, so nothing was replaced.",
            formatter.name
        ));
    }
    if let Some(status) = status {
        if !status.success() {
            let detail: String = err.text.trim().chars().take(600).collect();
            return Err(if detail.is_empty() {
                format!(
                    "{} could not format this file (exit {}).",
                    formatter.name,
                    status.code().unwrap_or(-1)
                )
            } else {
                format!("{} could not format this file: {detail}", formatter.name)
            });
        }
    }
    if out.text.is_empty() && !content.is_empty() {
        return Err(format!(
            "{} returned an empty file, so nothing was replaced.",
            formatter.name
        ));
    }
    Ok(out.text)
}

async fn format_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => return error.into_response(),
    };
    let raw_path = input.get("path").and_then(Value::as_str).unwrap_or("");
    let path = match clean_path(raw_path) {
        Ok(path) => path,
        Err(message) => return bad(message),
    };
    let Some(original) = input.get("content").and_then(Value::as_str) else {
        return bad("Provide the buffer to format.");
    };
    if original.is_empty() {
        return bad("There is nothing to format.");
    }
    if original.len() > MAX_CONTENT_BYTES {
        return bad("That buffer is larger than 2 MiB.");
    }
    let formatter = match formatter_for(&root, &path) {
        Ok(formatter) => formatter,
        Err(message) => return bad(message),
    };
    let name = formatter.name;
    let buffer = original.to_string();
    let task_root = root.clone();
    match tokio::task::spawn_blocking(move || run_formatter(&task_root, &formatter, &buffer)).await
    {
        Ok(Ok(formatted)) => Json(json!({
            "ok": true,
            "path": path,
            "formatter": name,
            "content": formatted,
            "changed": formatted != original,
            "bytes": formatted.len(),
        }))
        .into_response(),
        Ok(Err(message)) => bad(message),
        Err(_) => fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The formatter stopped unexpectedly.",
        ),
    }
}

/// Merged into `workspace::router`, so the route inherits the loopback
/// `Host`/`Origin` guard and the body cap.
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/api/workspace/format", post(format_route))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "sarthi-format-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn relative_paths_refuse_escapes_and_absolute_names() {
        assert_eq!(clean_path("src/main.rs").unwrap(), "src/main.rs");
        assert_eq!(
            clean_path("web\\src\\app.ts").unwrap(),
            "web/src/app.ts",
            "Windows separators are normalised, not rejected"
        );
        assert!(clean_path("").is_err());
        assert!(clean_path("../secret.rs").is_err());
        assert!(clean_path("src/../../secret.rs").is_err());
        assert!(clean_path("/etc/passwd").is_err());
        assert!(clean_path("C:/Windows/x.rs").is_err());
        assert!(clean_path("src/a\nb.rs").is_err());
    }

    #[test]
    fn formatting_is_chosen_by_extension_and_never_guessed() {
        let root = temp_dir("detect");
        assert_eq!(formatter_for(&root, "src/main.rs").unwrap().name, "rustfmt");
        assert_eq!(formatter_for(&root, "cmd/tool.go").unwrap().name, "gofmt");
        assert_eq!(
            formatter_for(&root, "web/app.tsx").unwrap().name,
            "prettier"
        );
        assert_eq!(formatter_for(&root, "README.md").unwrap().name, "prettier");
        let error = formatter_for(&root, "notes.xyz").unwrap_err();
        assert!(error.contains("No formatter is configured"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_local_prettier_is_preferred_over_path() {
        let root = temp_dir("prettier");
        let bin = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&bin).unwrap();
        #[cfg(target_os = "windows")]
        let local = bin.join("prettier.cmd");
        #[cfg(not(target_os = "windows"))]
        let local = bin.join("prettier");
        std::fs::write(&local, "#!/bin/sh\ncat\n").unwrap();
        let formatter = formatter_for(&root, "web/app.js").unwrap();
        assert_eq!(formatter.name, "prettier");
        assert!(
            formatter.program != "prettier",
            "the local tool must win over PATH: {}",
            formatter.program
        );
        assert!(
            formatter.args.iter().any(|arg| arg.contains("app.js")),
            "the path is carried so Prettier can infer a parser: {:?}",
            formatter.args
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rustfmt_formats_the_buffer_and_writes_nothing() {
        let root = temp_dir("rustfmt");
        let formatter = formatter_for(&root, "src/lib.rs").unwrap();
        let formatted = run_formatter(&root, &formatter, "fn main(){let x=1;}\n")
            .expect("rustfmt is part of the Rust toolchain");
        assert!(formatted.contains("fn main() {"), "{formatted}");
        assert!(formatted.contains("    let x = 1;"), "{formatted}");
        assert!(
            !root.join("src").exists(),
            "formatting a buffer must not create or write a file"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_formatter_that_cannot_start_is_named() {
        let root = temp_dir("missing");
        let formatter = Formatter {
            name: "rustfmt",
            program: "definitely-not-a-formatter-xyz".to_string(),
            args: Vec::new(),
        };
        let error = run_formatter(&root, &formatter, "fn main() {}\n").unwrap_err();
        assert!(error.contains("Could not start rustfmt"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
