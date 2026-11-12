//! Watch mode: rerun one build or test preset when the open folder changes.
//!
//! The tree is fingerprinted with `workspace::fingerprint_tree` — the same
//! cheap signal the explorer polls — every 700 ms. A change starts the preset
//! through the runner's normal capture path, so a watched run has the same exit
//! code, output, problems and test summary as a manual one. Nothing is invented:
//! a status answer is the last real run's result.
//!
//! Deliberate behaviour:
//!
//! * **One watch, one command slot.** A watch holds the runner's only slot for
//!   its lifetime, so a manual Run cannot start a second compiler beside it. The
//!   UI disables Run while watching.
//! * **The permission is decided once, at start.** A background thread cannot
//!   prompt, so starting the watch asks the `command` permission with the exact
//!   command line as its target; approving it covers the reruns until Stop.
//! * **A run cannot trigger itself.** The baseline is taken again after each
//!   run, so a command that writes generated files does not start an endless
//!   loop. A save made while the command runs is folded into that run's
//!   baseline rather than queued immediately; the next save reruns as usual.
//! * **Stop kills the command.** The runner polls the stop flag every 120 ms, so
//!   Stop does not wait for a 30 s test to finish.
//! * **The watch stops when the folder changes.** A watcher rooted in the old
//!   folder would keep running a command against files the person can no longer
//!   see.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::permissions::{guard_user, Action};
use crate::runner::{self, Preset};
use crate::workspace;
use crate::AppState;

/// How often the tree is fingerprinted. Fast enough to feel immediate, slow
/// enough that a large tree is not walked continuously.
const INTERVAL: Duration = Duration::from_millis(700);
/// Characters of a run's output kept in the status answer. The Problems pane
/// shows this tail; the full 200 k output is not carried on every poll.
const STATUS_OUTPUT_CHARS: usize = 12_000;

struct Handle {
    id: u64,
    preset: Preset,
    started_ms: u64,
    stop: Arc<AtomicBool>,
    progress: Arc<Mutex<Progress>>,
}

#[derive(Default)]
struct Progress {
    runs: u64,
    last_change_ms: Option<u64>,
    last_run: Option<Value>,
    error: Option<String>,
    truncated: bool,
}

static WATCH: LazyLock<Mutex<Option<Handle>>> = LazyLock::new(|| Mutex::new(None));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Signal the active watch to stop, without waiting. Called when the open
/// folder changes and by the Stop route; the thread clears the slot itself.
pub(crate) fn stop_active() {
    if let Ok(guard) = WATCH.lock() {
        if let Some(handle) = guard.as_ref() {
            handle.stop.store(true, Ordering::Relaxed);
        }
    }
}

async fn start_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => return error.into_response(),
    };
    let id = input.get("id").and_then(Value::as_str).unwrap_or("");
    let Some(preset) = runner::all(&root, &crate::command_presets(&state))
        .into_iter()
        .find(|preset| preset.id == id)
    else {
        return runner::bad("Pick one of the detected or saved commands.");
    };

    // The permission is for this watch session, not for every rerun: a
    // background thread cannot ask again, and a deny rule still wins here.
    if let Some(response) = guard_user(
        &state,
        Action::Command,
        &preset.command,
        "",
        runner::approval(&input),
        "watch a command and rerun it when files change",
    ) {
        return response;
    }

    if let Ok(guard) = WATCH.lock() {
        if let Some(handle) = guard.as_ref() {
            return runner::fail(
                StatusCode::CONFLICT,
                format!(
                    "Already watching `{}`. Stop it before starting another.",
                    handle.preset.command
                ),
            );
        }
    } else {
        return runner::fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The watcher is unavailable.",
        );
    }

    let Some(slot) = runner::acquire_slot() else {
        return runner::fail(
            StatusCode::CONFLICT,
            "A command is already running in this folder. Wait for it to finish.",
        );
    };

    let id_number = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Mutex::new(Progress::default()));
    let handle = Handle {
        id: id_number,
        preset: preset.clone(),
        started_ms: now_ms(),
        stop: stop.clone(),
        progress: progress.clone(),
    };
    let description = describe(&handle);

    // Insert before spawning so a very short first run cannot finish and clear
    // the slot before the handle exists.
    if let Ok(mut guard) = WATCH.lock() {
        *guard = Some(handle);
    } else {
        return runner::fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The watcher is unavailable.",
        );
    }

    let spawned = std::thread::Builder::new()
        .name("sarthi-watch".into())
        .spawn(move || {
            let _slot = slot;
            watch_loop(&root, &preset, &stop, &progress);
            if let Ok(mut guard) = WATCH.lock() {
                if guard.as_ref().map(|handle| handle.id) == Some(id_number) {
                    *guard = None;
                }
            }
        });
    if spawned.is_err() {
        if let Ok(mut guard) = WATCH.lock() {
            if guard.as_ref().map(|handle| handle.id) == Some(id_number) {
                *guard = None;
            }
        }
        return runner::fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not start the watcher.",
        );
    }

    Json(description).into_response()
}

async fn stop_route() -> Json<Value> {
    let guard = match WATCH.lock() {
        Ok(guard) => guard,
        Err(_) => return Json(json!({ "ok": false, "error": "The watcher is unavailable." })),
    };
    match guard.as_ref() {
        Some(handle) => {
            handle.stop.store(true, Ordering::Relaxed);
            Json(json!({
                "ok": true,
                "watching": true,
                "stopping": true,
                "message": format!("Stopping `{}`…", handle.preset.command),
            }))
        }
        None => Json(json!({ "ok": true, "watching": false, "stopping": false })),
    }
}

async fn status_route() -> Json<Value> {
    match WATCH.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(handle) => Json(describe(handle)),
            None => Json(json!({ "watching": false, "stopping": false })),
        },
        Err(_) => Json(json!({
            "watching": false,
            "stopping": false,
            "error": "The watcher is unavailable.",
        })),
    }
}

fn describe(handle: &Handle) -> Value {
    let (runs, last_change_ms, mut last_run, error, truncated) = match handle.progress.lock() {
        Ok(progress) => (
            progress.runs,
            progress.last_change_ms,
            progress.last_run.clone(),
            progress.error.clone(),
            progress.truncated,
        ),
        Err(_) => (
            0,
            None,
            None,
            Some("The watcher state is unavailable.".to_string()),
            false,
        ),
    };
    if let Some(object) = last_run.as_mut().and_then(Value::as_object_mut) {
        if let Some(output) = object.get("output").and_then(Value::as_str) {
            if output.chars().count() > STATUS_OUTPUT_CHARS {
                object.insert(
                    "output".into(),
                    json!(runner::tail(output, STATUS_OUTPUT_CHARS)),
                );
                object.insert("outputTruncated".into(), json!(true));
            }
        }
    }
    json!({
        "watching": true,
        "stopping": handle.stop.load(Ordering::Relaxed),
        "presetId": handle.preset.id,
        "label": handle.preset.label,
        "command": handle.preset.command,
        "startedAt": handle.started_ms,
        "intervalMs": INTERVAL.as_millis() as u64,
        "runs": runs,
        "lastChangeAt": last_change_ms,
        "lastRun": last_run,
        "error": error,
        "truncated": truncated,
    })
}

fn watch_loop(root: &Path, preset: &Preset, stop: &AtomicBool, progress: &Mutex<Progress>) {
    let (mut baseline, _, truncated) = workspace::fingerprint_tree(root);
    if let Ok(mut state) = progress.lock() {
        state.truncated = truncated;
    }
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(INTERVAL);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let (current, _, truncated) = workspace::fingerprint_tree(root);
        if let Ok(mut state) = progress.lock() {
            state.truncated = truncated;
        }
        if current == baseline {
            continue;
        }
        let result = runner::run_watched(root, preset, stop);
        let finished_at = now_ms();
        // Take the baseline after the run: the command's own generated files
        // must not look like the next edit.
        baseline = workspace::fingerprint_tree(root).0;
        if let Ok(mut state) = progress.lock() {
            state.runs += 1;
            state.last_change_ms = Some(finished_at);
            match result {
                Ok(mut value) => {
                    if let Some(object) = value.as_object_mut() {
                        object.insert("finishedAt".into(), json!(finished_at));
                    }
                    state.last_run = Some(value);
                    state.error = None;
                }
                Err(message) => {
                    state.error = Some(message);
                }
            }
        }
    }
}

/// Merged into `workspace::router`, so every route inherits the loopback
/// `Host`/`Origin` guard and the body cap.
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/workspace/watch", get(status_route))
        .route("/api/workspace/watch/start", post(start_route))
        .route("/api/workspace/watch/stop", post(stop_route))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::custom;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "sarthi-watch-{name}-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn preset(command: &str) -> Preset {
        custom(&[("Watch test".to_string(), command.to_string())])
            .into_iter()
            .next()
            .expect("a custom preset")
    }

    fn snapshot(progress: &Arc<Mutex<Progress>>) -> (u64, Option<Value>) {
        let state = progress.lock().unwrap();
        (state.runs, state.last_run.clone())
    }

    #[test]
    fn a_change_runs_once_and_generated_files_do_not_retrigger_it() {
        let root = temp_dir("rerun");
        std::fs::write(root.join("trigger.txt"), "one").unwrap();
        let preset = preset("echo changed >> watched.txt");
        let stop = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(Progress::default()));
        let thread = {
            let root = root.clone();
            let preset = preset.clone();
            let stop = stop.clone();
            let progress = progress.clone();
            std::thread::spawn(move || watch_loop(&root, &preset, &stop, &progress))
        };

        // Let the loop take its first fingerprint before the edit.
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(root.join("trigger.txt"), "one changed, so the size moves").unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let run = loop {
            let (runs, last) = snapshot(&progress);
            if runs >= 1 {
                break last.expect("the run result");
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the change never ran the command"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(run["success"], json!(true), "{run}");
        assert!(root.join("watched.txt").is_file(), "the command really ran");

        // The command wrote a file; the post-run baseline must absorb it. Give
        // the loop two intervals to prove no second run starts.
        std::thread::sleep(Duration::from_millis(1_600));
        assert_eq!(snapshot(&progress).0, 1, "the run retriggered itself");

        stop.store(true, Ordering::Relaxed);
        thread.join().unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stopping_kills_a_running_command_instead_of_waiting_for_it() {
        let root = temp_dir("stop");
        let command = if cfg!(target_os = "windows") {
            "ping -n 30 127.0.0.1 >nul"
        } else {
            "sleep 30"
        };
        let preset = preset(command);
        let stop = Arc::new(AtomicBool::new(false));
        let stopper = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            stopper.store(true, Ordering::Relaxed);
        });

        let started = std::time::Instant::now();
        let result = runner::run_watched(&root, &preset, &stop).expect("the command starts");
        assert_eq!(result["stopped"], json!(true));
        assert_eq!(result["success"], json!(false));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "Stop waited for the command instead of killing it"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
