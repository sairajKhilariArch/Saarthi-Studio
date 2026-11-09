//! Phase 17 — a record of what happened that outlives the process.
//!
//! `diagnostics.rs` projects the rows the app already stores, which means it can
//! only show an event that reached SQLite. A crash between the failing call and
//! the row, a provider error on a request that never got far enough to be
//! logged, or a router install that died halfway leaves nothing behind.
//!
//! This module is the missing floor: one JSON object per line appended to
//! `studio.log` beside the database, plus a bounded in-memory ring so the
//! Diagnostics view can read the newest entries without touching the disk.
//!
//! Three rules keep it honest:
//!
//! * **It never breaks its caller.** Every failure (a full disk, a locked file,
//!   a directory that cannot be created) is ignored; a log line is worth less
//!   than the operation it describes.
//! * **It redacts before it writes.** Detail strings go through the same
//!   `diagnostics::redact` the support bundle uses, so the file can be read by a
//!   person who is helping and a secret cannot land on disk in the first place.
//! * **It is bounded.** The ring holds [`RING`] entries and the file is rotated
//!   at [`MAX_FILE_BYTES`] to one previous file, so an app left open for a month
//!   does not fill a disk.

use crate::diagnostics;
use crate::now_seconds;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Newest entries kept in memory for the Diagnostics view.
pub(crate) const RING: usize = 200;
/// The file is rotated once it reaches this size.
pub(crate) const MAX_FILE_BYTES: u64 = 1_048_576;
/// Detail is clipped the way the timeline clips it: one entry, one screen line.
const DETAIL_CHARS: usize = 400;

static LOG: OnceLock<Log> = OnceLock::new();

struct Held {
    ring: VecDeque<Value>,
    file: Option<File>,
}

struct Inner {
    path: PathBuf,
    max_bytes: u64,
    held: Mutex<Held>,
}

/// A log handle. Cheap to clone; every clone writes to the same ring and file.
#[derive(Clone)]
pub(crate) struct Log {
    inner: Arc<Inner>,
}

impl Log {
    /// Open `dir/studio.log`, creating the directory and the file if needed.
    pub(crate) fn open(dir: &Path) -> Log {
        Self::open_with_limit(dir, MAX_FILE_BYTES)
    }

    /// The rotation limit is a parameter so a test can exercise it with a few
    /// hundred bytes instead of a megabyte.
    fn open_with_limit(dir: &Path, max_bytes: u64) -> Log {
        let _ = fs::create_dir_all(dir);
        let path = dir.join("studio.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        Log {
            inner: Arc::new(Inner {
                path,
                max_bytes,
                held: Mutex::new(Held {
                    ring: VecDeque::with_capacity(RING),
                    file,
                }),
            }),
        }
    }

    /// Where the file lives, whether or not it could be opened.
    pub(crate) fn path(&self) -> String {
        self.inner.path.to_string_lossy().to_string()
    }

    /// Append one entry. `level` is `info`, `warn` or `error`.
    pub(crate) fn push(&self, level: &str, area: &str, event: &str, detail: &str) {
        // The calling request's id, when there is one, is what lets a log line
        // be matched to the audit row and the HTTP call that produced it.
        let request = crate::trace::current();
        let mut entry = json!({
            "at": now_seconds(),
            "level": level,
            "area": area,
            "event": event.chars().take(120).collect::<String>(),
            "detail": diagnostics::clip(&diagnostics::redact(detail), DETAIL_CHARS),
        });
        if !request.is_empty() {
            entry["request_id"] = json!(request);
        }
        let Ok(mut held) = self.inner.held.lock() else {
            return;
        };
        if held.ring.len() >= RING {
            held.ring.pop_front();
        }
        held.ring.push_back(entry.clone());
        let Some(file) = held.file.as_mut() else {
            return;
        };
        // Rotate before the write so the file never holds more than the limit
        // plus the entry that crossed it. The old file is replaced, not kept
        // forever: two generations is the whole budget.
        let written = fs::metadata(&self.inner.path)
            .map(|meta| meta.len())
            .unwrap_or(0);
        if written >= self.inner.max_bytes {
            let previous = self.inner.path.with_extension("log.1");
            held.file = None;
            let _ = fs::remove_file(&previous);
            let _ = fs::rename(&self.inner.path, &previous);
            let mut reopened = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.inner.path)
                .ok();
            if let Some(file) = reopened.as_mut() {
                let _ = writeln!(file, "{entry}");
                let _ = file.flush();
            }
            held.file = reopened;
            return;
        }
        // Flushed per line: the entry that matters most is the last one before
        // a crash, and this log's whole reason to exist is surviving that.
        let _ = writeln!(file, "{entry}");
        let _ = file.flush();
    }

    /// The newest `limit` entries, oldest first.
    pub(crate) fn tail(&self, limit: usize) -> Vec<Value> {
        let Ok(held) = self.inner.held.lock() else {
            return Vec::new();
        };
        let skip = held.ring.len().saturating_sub(limit);
        held.ring.iter().skip(skip).cloned().collect()
    }
}

/// Start the process log under the app's data directory. Called once at boot;
/// a second call is ignored.
pub(crate) fn init(dir: &Path) {
    let _ = LOG.set(Log::open(dir));
}

/// Write one entry to the process log. A no-op before [`init`], so a unit test
/// or a library caller never writes to the user's real file by accident.
pub(crate) fn record(level: &str, area: &str, event: &str, detail: impl AsRef<str>) {
    if let Some(log) = LOG.get() {
        log.push(level, area, event, detail.as_ref());
    }
}

/// The file's path, once the log is initialized.
pub(crate) fn path() -> Option<String> {
    LOG.get().map(Log::path)
}

/// Newest entries from the in-memory ring.
pub(crate) fn tail(limit: usize) -> Vec<Value> {
    LOG.get().map(|log| log.tail(limit)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("saarthi-oplog-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn lines(dir: &Path) -> Vec<Value> {
        fs::read_to_string(dir.join("studio.log"))
            .expect("log file")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
            .collect()
    }

    #[test]
    fn appends_one_json_object_per_line() {
        let dir = dir("append");
        let log = Log::open(&dir);
        log.push("info", "app", "started", "v2.2.0 on windows/x86_64");
        log.push("warn", "router", "start-failed", "docker is not running");

        let rows = lines(&dir);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["level"], "info");
        assert_eq!(rows[0]["area"], "app");
        assert_eq!(rows[0]["event"], "started");
        assert_eq!(rows[1]["level"], "warn");
        assert!(rows[1]["at"].as_f64().unwrap_or(0.0) > 0.0);
        assert_eq!(log.tail(10).len(), 2);
    }

    #[test]
    fn ring_is_bounded_and_keeps_the_newest() {
        let dir = dir("ring");
        let log = Log::open(&dir);
        for index in 0..(RING + 25) {
            log.push("info", "agent", "step", &format!("step {index}"));
        }
        let tail = log.tail(RING + 100);
        assert_eq!(tail.len(), RING);
        assert_eq!(tail[0]["detail"], format!("step {}", 25));
        assert_eq!(tail[RING - 1]["detail"], format!("step {}", RING + 24));
        assert_eq!(log.tail(5).len(), 5);
    }

    #[test]
    fn rotates_once_the_file_reaches_the_limit() {
        let dir = dir("rotate");
        let log = Log::open_with_limit(&dir, 300);
        // Prose, not one long run of characters: a long run is redacted as a
        // credential and would shrink the entry instead of growing the file.
        let filler = "note ".repeat(30);
        log.push("info", "app", "one", &filler);
        log.push("info", "app", "two", &filler);
        // Two entries crossed 200 bytes; the third should rotate first.
        log.push("info", "app", "three", &filler);

        assert!(dir.join("studio.log.1").is_file(), "previous file kept");

        assert_eq!(log.tail(10)[2]["event"], "three");
        let current = lines(&dir);
        assert_eq!(current.len(), 1);
        assert_eq!(current[0]["event"], "three");
        // The ring is unaffected by rotation.
        assert_eq!(log.tail(10).len(), 3);
    }

    #[test]
    fn detail_is_redacted_and_clipped_before_it_reaches_disk() {
        let dir = dir("redact");
        let log = Log::open(&dir);
        log.push(
            "error",
            "provider",
            "request-failed",
            "Authorization: Bearer sk-live-abcdef123456 rejected",
        );
        log.push("info", "agent", "step", &"note ".repeat(120));

        let rows = lines(&dir);
        let detail = rows[0]["detail"].as_str().expect("detail");
        assert!(
            !detail.contains("sk-live-abcdef123456"),
            "a credential reached the log file: {detail}"
        );
        assert!(detail.contains("[redacted]"));
        let clipped = rows[1]["detail"].as_str().expect("detail");
        assert_eq!(clipped.chars().count(), DETAIL_CHARS + 1, "clipped plus …");
    }

    #[test]
    fn an_unwritable_location_still_logs_in_memory() {
        // A path under a file, not a directory: create_dir_all and the file open
        // both fail, and neither is allowed to panic or lose the ring.
        let base = dir("unwritable");
        let blocked = base.join("studio.log");
        fs::write(&blocked, "not a directory").expect("seed file");
        let log = Log::open_with_limit(&blocked, MAX_FILE_BYTES);
        log.push("info", "app", "started", "still readable");
        assert_eq!(log.tail(1).len(), 1);
        assert_eq!(log.tail(1)[0]["event"], "started");
    }

    #[test]
    fn record_before_init_is_a_no_op() {
        record("info", "app", "started", "nothing to write to yet");
        // No assertion on the file: the point is that this returns.
    }
}
