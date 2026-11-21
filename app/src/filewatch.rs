//! Keeps the code index fresh when the open folder changes outside the app.
//!
//! The app reindexes every write it performs itself (`workspace.rs`, agent
//! edits), but a `git checkout`, a formatter run from a terminal, or another
//! editor used to leave retrieval quoting bytes that no longer exist until the
//! next manual refresh. This module watches the open folder and refreshes the
//! changed paths.
//!
//! Shape:
//! - `notify` reports raw events on a channel; a dedicated thread coalesces
//!   them for [`DEBOUNCE`], so a checkout or a save burst becomes one batch
//!   instead of hundreds of database writes.
//! - `index::watch_relative` filters the batch: build and dependency folders
//!   (`target`, `node_modules`, `.git`, …) and secret-shaped files never reach
//!   the indexer, so a build cannot cause an indexing storm.
//! - A large batch is answered with one full `index::refresh` walk, which is
//!   cheaper and more consistent than hundreds of per-file upserts.
//! - One watcher per process, owned by whichever folder is open. Opening,
//!   picking or switching a folder stops the previous watcher first; closing
//!   the folder stops it too. A flush re-checks that the folder is still the
//!   open one, so a late batch can never write rows for a folder the person
//!   has just left.

use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crate::{index, AppState};

/// How long the event stream must be quiet before a batch is flushed.
const DEBOUNCE: Duration = Duration::from_millis(400);

struct Active {
    watcher: notify::RecommendedWatcher,
    alive: Arc<AtomicBool>,
}

fn slot() -> &'static Mutex<Option<Active>> {
    static SLOT: OnceLock<Mutex<Option<Active>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Stop the watcher, if one is running. Dropping the watcher also drops the
/// event sender, so the worker thread sees a disconnect and exits.
pub(crate) fn stop() {
    if let Some(active) = slot().lock().unwrap().take() {
        active.alive.store(false, Ordering::Relaxed);
        drop(active.watcher);
    }
}

/// Watch `root` for changes and refresh the index. Replaces any previous
/// watcher. A platform that cannot start one (or a folder the OS refuses to
/// watch, such as a network mount without inotify support) simply leaves the
/// manual refresh path in place — no index state is worse than a stale one.
pub(crate) fn start(state: &AppState, root: &Path) {
    stop();
    let alive = Arc::new(AtomicBool::new(true));
    let (tx, rx) = mpsc::channel();
    let handler_alive = alive.clone();
    let mut watcher =
        match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if handler_alive.load(Ordering::Relaxed) {
                let _ = tx.send(event);
            }
        }) {
            Ok(watcher) => watcher,
            Err(_) => return,
        };
    if watcher.watch(root, RecursiveMode::Recursive).is_err() {
        return;
    }
    let watched = root.to_path_buf();
    let thread_alive = alive.clone();
    let worker_state = state.clone();
    let spawned = thread::Builder::new()
        .name("studio-index-watch".to_string())
        .spawn(move || {
            let mut pending: BTreeSet<PathBuf> = BTreeSet::new();
            loop {
                if !thread_alive.load(Ordering::Relaxed) {
                    break;
                }
                match rx.recv_timeout(DEBOUNCE) {
                    Ok(Ok(event)) => {
                        // Reads open and close files constantly; reacting to
                        // them would reindex whatever is merely being viewed.
                        if matches!(event.kind, EventKind::Access(_)) {
                            continue;
                        }
                        pending.extend(event.paths);
                    }
                    Ok(Err(_)) => {}
                    Err(RecvTimeoutError::Timeout) => {
                        if pending.is_empty() {
                            continue;
                        }
                        flush(&worker_state, &watched, &mut pending);
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
    if spawned.is_ok() {
        *slot().lock().unwrap() = Some(Active { watcher, alive });
    }
}

/// Refresh one quiet period's worth of paths. The batch is cleared before the
/// work starts, so a file that changes during the flush belongs to the next
/// window instead of being silently dropped.
fn flush(state: &AppState, root: &Path, pending: &mut BTreeSet<PathBuf>) {
    let paths: Vec<PathBuf> = std::mem::take(pending).into_iter().collect();
    // A late batch for a folder that is no longer open must not write rows.
    let still_open = state
        .workspace
        .current_root()
        .map(|current| current == root)
        .unwrap_or(false);
    if !still_open {
        return;
    }
    let cfg = crate::load_config(state);
    let mut relative: Vec<String> = paths
        .iter()
        .filter_map(|path| index::watch_relative(root, path))
        .collect();
    relative.sort();
    relative.dedup();
    if relative.is_empty() {
        return;
    }
    let _ = index::refresh_many(
        &state.database_path,
        root,
        &relative,
        &cfg.privacy_exclusions,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;

    fn fixture(name: &str) -> AppState {
        let dir =
            std::env::temp_dir().join(format!("sarthi-index-watch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("fixture dir");
        let state = AppState {
            config_path: dir.join("config.json"),
            database_path: dir.join("studio.db"),
            static_dir: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static"),
            workspace: crate::workspace::WorkspaceStore::load(dir.join("workspace.json")),
            client: reqwest::Client::new(),
            terminal: crate::terminal::hub(),
            events: crate::events::Hub::new(),
            models_cache: Arc::new(Mutex::new(HashMap::new())),
        };
        crate::ensure_setup(&state).expect("ensure_setup");
        state
    }

    #[test]
    fn a_flush_indexes_a_source_file_and_never_a_build_folder() {
        let state = fixture("flush");
        let root = std::path::PathBuf::from(state.database_path.parent().unwrap());
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("src/main.rs"), "pub fn hello() -> u32 { 42 }\n").unwrap();
        std::fs::write(root.join("target/debug/noise.rs"), "pub fn noise() {}\n").unwrap();
        state.workspace.open(root.to_str().unwrap()).expect("open");

        let mut pending: BTreeSet<PathBuf> =
            [root.join("src/main.rs"), root.join("target/debug/noise.rs")]
                .into_iter()
                .collect();
        flush(&state, &root, &mut pending);

        let conn = crate::open_db(&state.database_path).expect("db");
        let indexed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM code_index_files WHERE path='src/main.rs'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(indexed, 1, "the changed source file is indexed");
        let build: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM code_index_files WHERE path LIKE 'target/%'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(build, 0, "a build folder never reaches the index");
    }

    #[test]
    fn a_batch_for_a_folder_that_is_no_longer_open_is_dropped() {
        let state = fixture("stale");
        let open_root = std::path::PathBuf::from(state.database_path.parent().unwrap());
        let other = open_root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("hidden.rs"), "pub fn hidden() {}\n").unwrap();
        state
            .workspace
            .open(open_root.to_str().unwrap())
            .expect("open");

        let mut pending: BTreeSet<PathBuf> = [other.join("hidden.rs")].into_iter().collect();
        flush(&state, &other, &mut pending);

        let conn = crate::open_db(&state.database_path).expect("db");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM code_index_files", [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(rows, 0, "a late batch for a closed folder writes nothing");
    }

    #[test]
    fn the_os_watcher_reports_a_write() {
        let dir =
            std::env::temp_dir().join(format!("sarthi-index-watch-event-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let _ = tx.send(event);
            })
            .expect("watcher");
        watcher
            .watch(&dir, RecursiveMode::Recursive)
            .expect("watch dir");
        std::fs::write(dir.join("observed.rs"), "pub fn observed() {}\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if rx.recv_timeout(Duration::from_millis(200)).is_ok() {
                return;
            }
        }
        panic!("the platform watcher produced no event for a write within 5s");
    }

    #[test]
    fn a_quiet_period_is_the_flush_window() {
        // The debounce is a promise about behaviour, not a tunable: long enough
        // to swallow a save burst, short enough that search catches up while a
        // person is still looking at the result.
        assert!(DEBOUNCE >= Duration::from_millis(200));
        assert!(DEBOUNCE <= Duration::from_secs(1));
    }
}
