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
//!   instead of hundreds of database writes. A stream that never goes quiet
//!   (`npm install` writing for minutes) still flushes at [`MAX_WAIT`] from the
//!   first event, so indexing cannot be postponed forever.
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
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::{index, AppState};

/// How long the event stream must be quiet before a batch is flushed.
const DEBOUNCE: Duration = Duration::from_millis(400);
/// The hard ceiling from the first event in a batch: continuous events can
/// delay a flush, never skip it.
const MAX_WAIT: Duration = Duration::from_secs(2);
/// A flush also tells the run conflict ledger about outside edits. Only text at
/// or below this size is marked: a huge binary cannot match the text reads and
/// writes the ledger compares, so hashing it would add cost without an answer.
const MAX_MARK_BYTES: u64 = 1024 * 1024;
/// A checkout can touch thousands of files at once; the ledger retains a bounded
/// number of marks anyway, so marking a bounded prefix of a huge batch is enough.
const MAX_MARKS_PER_FLUSH: usize = 256;

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
            let mut batch = Batch::default();
            loop {
                if !thread_alive.load(Ordering::Relaxed) {
                    break;
                }
                match rx.recv_timeout(DEBOUNCE) {
                    Ok(Ok(event)) => {
                        let now = Instant::now();
                        // Reads open and close files constantly; they never
                        // join a batch, but they must not postpone one either —
                        // a polling reader would otherwise keep the loop out of
                        // its timeout branch forever.
                        if !matches!(event.kind, EventKind::Access(_)) {
                            batch.add(event.paths, now);
                        }
                        if batch.due(now) || batch.quiet(now) {
                            flush(&worker_state, &watched, &batch.take());
                        }
                    }
                    Ok(Err(_)) => {}
                    Err(RecvTimeoutError::Timeout) => {
                        if batch.is_empty() {
                            continue;
                        }
                        flush(&worker_state, &watched, &batch.take());
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
    if spawned.is_ok() {
        *slot().lock().unwrap() = Some(Active { watcher, alive });
    }
}

/// One debounce window: quiet for [`DEBOUNCE`] flushes it, but the first event
/// starts a clock that forces a flush at [`MAX_WAIT`]. Without the ceiling a
/// build that writes continuously would keep resetting the quiet timer and the
/// index would never catch up.
#[derive(Default)]
struct Batch {
    paths: BTreeSet<PathBuf>,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Batch {
    /// Add a burst and answer whether it must be flushed now.
    fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>, now: Instant) -> bool {
        if self.first.is_none() {
            self.first = Some(now);
        }
        self.last = Some(now);
        self.paths.extend(paths);
        self.due(now)
    }

    fn due(&self, now: Instant) -> bool {
        self.first
            .is_some_and(|first| now.saturating_duration_since(first) >= MAX_WAIT)
    }

    /// True when the last change is already [`DEBOUNCE`] old. Checked on every
    /// event because access-only traffic never reaches the timeout branch.
    fn quiet(&self, now: Instant) -> bool {
        self.last
            .is_some_and(|last| now.saturating_duration_since(last) >= DEBOUNCE)
    }

    /// Take the batch and reset its clock. Taken before the work starts, so a
    /// path that changes during the flush belongs to the next window.
    fn take(&mut self) -> Vec<PathBuf> {
        self.first = None;
        self.last = None;
        std::mem::take(&mut self.paths).into_iter().collect()
    }

    fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// Refresh one batch of paths for the folder that was watched when they were
/// collected.
fn flush(state: &AppState, root: &Path, paths: &[PathBuf]) {
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
    // A change made by a formatter, a checkout or another editor used to be
    // invisible to the run/editor conflict ledger until Studio's own editor had
    // saved the path. Record the bytes now on disk, so the next run that has not
    // read the file refuses to overwrite them.
    for relative in relative.iter().take(MAX_MARKS_PER_FLUSH) {
        let path = root.join(relative);
        match fs::metadata(&path) {
            Ok(meta) if meta.is_file() && meta.len() <= MAX_MARK_BYTES => {
                if let Ok(text) = fs::read_to_string(&path) {
                    crate::filelock::record_external_write(
                        state,
                        root_str(root),
                        relative,
                        Some(&text),
                    );
                }
            }
            Ok(_) => {}
            Err(_) => crate::filelock::record_external_write(state, root_str(root), relative, None),
        }
    }
}

fn root_str(root: &Path) -> &str {
    root.to_str().unwrap_or("")
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
        // The store canonicalizes what it opens (macOS /var is a symlink to
        // /private/var, Windows canonical paths carry a verbatim prefix), and
        // the watcher is handed that canonical root. Compare like the app does.
        let root = std::fs::canonicalize(state.database_path.parent().unwrap())
            .expect("canonical fixture root");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("src/main.rs"), "pub fn hello() -> u32 { 42 }\n").unwrap();
        std::fs::write(root.join("target/debug/noise.rs"), "pub fn noise() {}\n").unwrap();
        state.workspace.open(root.to_str().unwrap()).expect("open");

        let pending = vec![root.join("src/main.rs"), root.join("target/debug/noise.rs")];
        flush(&state, &root, &pending);

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
    fn a_flush_records_an_outside_change_for_the_run_conflict_ledger() {
        let state = fixture("outside-mark");
        let root = std::fs::canonicalize(state.database_path.parent().unwrap())
            .expect("canonical fixture root");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/lib.rs");
        std::fs::write(&file, "pub fn v1() {}\n").unwrap();
        state.workspace.open(root.to_str().unwrap()).expect("open");

        flush(&state, &root, &[file]);

        let conn = crate::open_db(&state.database_path).expect("db");
        let mark: i64 = conn
            .query_row(
                "SELECT fingerprint FROM editor_writes WHERE path='src/lib.rs'",
                [],
                |row| row.get(0),
            )
            .expect("the outside change has a mark");
        assert_eq!(
            mark,
            crate::filelock::fingerprint(Some("pub fn v1() {}\n")) as i64
        );
    }

    #[test]
    fn a_batch_for_a_folder_that_is_no_longer_open_is_dropped() {
        let state = fixture("stale");
        let open_root = std::fs::canonicalize(state.database_path.parent().unwrap())
            .expect("canonical fixture root");
        let other = open_root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("hidden.rs"), "pub fn hidden() {}\n").unwrap();
        state
            .workspace
            .open(open_root.to_str().unwrap())
            .expect("open");

        let pending = [other.join("hidden.rs")];
        flush(&state, &other, &pending);

        let conn = crate::open_db(&state.database_path).expect("db");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM code_index_files", [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(rows, 0, "a late batch for a closed folder writes nothing");
    }

    #[test]
    fn a_started_watcher_indexes_a_file_written_after_open() {
        let state = fixture("live");
        let root = std::fs::canonicalize(state.database_path.parent().unwrap())
            .expect("canonical fixture root");
        std::fs::create_dir_all(root.join("src")).unwrap();
        state.workspace.open(root.to_str().unwrap()).expect("open");
        start(&state, &root);
        // Give the platform watcher time to register before the write, so the
        // test is about indexing rather than about setup latency.
        std::thread::sleep(Duration::from_millis(150));
        std::fs::write(root.join("src/fresh.rs"), "pub fn fresh() -> u8 { 1 }\n").unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let conn = crate::open_db(&state.database_path).expect("db");
            let indexed: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM code_index_files WHERE path='src/fresh.rs'",
                    [],
                    |row| row.get(0),
                )
                .expect("count");
            if indexed == 1 {
                stop();
                return;
            }
            let give_up = Instant::now() >= deadline;
            drop(conn);
            if give_up {
                stop();
                panic!("the watcher did not index a new file within 10s");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
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
        // The ceiling is above one quiet period, so ordinary saves still take
        // the quiet path rather than the forced one.
        assert!(MAX_WAIT > DEBOUNCE);
        assert!(MAX_WAIT <= Duration::from_secs(10));
    }

    #[test]
    fn a_stream_that_never_quiets_still_flushes_at_the_ceiling() {
        let start = Instant::now();
        let mut batch = Batch::default();
        assert!(!batch.add([PathBuf::from("a")], start));
        assert!(
            !batch.add(
                [PathBuf::from("b")],
                start + MAX_WAIT - Duration::from_millis(1)
            ),
            "events below the ceiling keep collecting"
        );
        assert!(
            batch.add([PathBuf::from("c")], start + MAX_WAIT),
            "the first event's clock forces the flush"
        );
        assert_eq!(batch.take().len(), 3);
        assert!(batch.is_empty(), "taking the batch resets it");
        assert!(!batch.due(start + MAX_WAIT), "and its clock");
    }

    #[test]
    fn access_events_do_not_postpone_a_pending_batch() {
        let start = Instant::now();
        let mut batch = Batch::default();
        batch.add([PathBuf::from("src/lib.rs")], start);
        // A reader polling the folder produces only access events; the batch's
        // own quiet clock must still expire.
        assert!(!batch.quiet(start + DEBOUNCE - Duration::from_millis(1)));
        assert!(batch.quiet(start + DEBOUNCE));
        assert!(batch.due(start + MAX_WAIT), "and so must the ceiling");
    }
}
