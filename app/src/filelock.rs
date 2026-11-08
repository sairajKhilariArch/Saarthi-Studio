//! Leases between live agent runs over the files in the shared workspace.
//!
//! Two runs work in one folder and nothing in the filesystem stops both of them
//! writing the same file: whichever writes last wins, silently. A lease is not a
//! lock on the file — nothing outside this process can see it — but it answers
//! one question before every write: *would this write clobber work this run has
//! not seen?* There are two ways the answer is yes, and both refuse the write
//! with a reason instead of letting it land:
//!
//! * another live run has already claimed the path, and
//! * the content on disk is not what this run last read of that path.
//!
//! A refusal is not fatal to the run: the model sees the message as a step and
//! can re-read the file and rewrite from what is actually there. That is why
//! `claim_write` returns a reason rather than panicking or failing the run.
//!
//! The ledger is keyed by the database it belongs to. Runs are records in a
//! database, so liveness is only meaningful for the one that owns them: keying by
//! both is what stops a second database in the same process — a test fixture, or
//! a headless server next to a desktop build — from deciding that this database's
//! runs are dead and dropping their claims.
//!
//! A lease is only as good as the record behind it, so claims held by runs that
//! are no longer live are dropped on the next claim rather than blocking a file
//! forever after a crash.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use crate::AppState;

/// How many paths one run's read fingerprints are remembered for. Past this a
/// run has touched more files than a shared folder can usefully guard, so new
/// paths stop being recorded instead of the map growing without limit.
const MAX_TRACKED_PATHS: usize = 256;

type Scope = String;

#[derive(Default)]
struct Ledger {
    /// (database, path) → the live run that claimed it for writing, and what it
    /// saw then.
    owners: HashMap<(Scope, String), (String, u64)>,
    /// (database, run) → path → fingerprint of the content that run last read.
    reads: HashMap<(Scope, String), HashMap<String, u64>>,
}

static LEDGER: LazyLock<Mutex<Ledger>> = LazyLock::new(|| Mutex::new(Ledger::default()));

/// A stable fingerprint of file content. `None` (a file that does not exist, or
/// one that cannot be read as text) hashes differently from any content, so
/// "created since I looked" and "deleted since I looked" are both changes.
pub(crate) fn fingerprint(content: Option<&str>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// Which database's runs this ledger entry belongs to. Two processes sharing one
/// database still see separate ledgers; that limitation is the one a cross-process
/// lock would remove.
fn scope(state: &AppState) -> Scope {
    state.database_path.to_string_lossy().to_string()
}

/// Runs that can still step: one working, or one stopped on a human question.
/// A paused run has released its loop, so its claim is not worth waiting for —
/// and its recorded reads are kept, so a resumed run still notices a file that
/// changed while it was parked.
fn live_runs(state: &AppState) -> Vec<String> {
    let Ok(conn) = crate::open_db(&state.database_path) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT id FROM agent_runs WHERE status IN ('running','awaiting_approval') ORDER BY created_at",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

/// Remember what a run just read, so a later write can tell whether the file
/// moved underneath it.
pub(crate) fn record_read(
    state: &AppState,
    root: &str,
    run_id: &str,
    path: &str,
    content: Option<&str>,
) {
    // Two runs in their own worktrees can legitimately touch the same relative
    // path; the claim is over a file, so the checkout is part of the key.
    let path = &format!("{root}|{path}");
    let mut ledger = LEDGER.lock().unwrap();
    let paths = ledger
        .reads
        .entry((scope(state), run_id.to_string()))
        .or_default();
    if !paths.contains_key(path) && paths.len() >= MAX_TRACKED_PATHS {
        return;
    }
    paths.insert(path.to_string(), fingerprint(content));
}

/// Ask to write `path`. `current` is the content on disk right now, as the
/// caller has just read it for its checkpoint — the caller's view must not be
/// trusted, the file is what matters.
pub(crate) fn claim_write(
    state: &AppState,
    root: &str,
    run_id: &str,
    path: &str,
    current: Option<&str>,
) -> Result<(), String> {
    let shown = path;
    let path = &format!("{root}|{path}");
    let db = scope(state);
    let live = live_runs(state);
    let fingerprint_now = fingerprint(current);
    let mut ledger = LEDGER.lock().unwrap();
    // Forget claims in this database held by runs that will never step again: a
    // cancelled or finished run must not keep a file locked, and its record is
    // why we cannot simply trust the map.
    ledger.owners.retain(|(owner_db, _), (owner, _)| {
        *owner_db != db || owner == run_id || live.iter().any(|id| id == owner)
    });
    let key = (db.clone(), path.to_string());
    if let Some((owner, _)) = ledger.owners.get(&key) {
        if owner != run_id {
            return Err(format!(
                "{shown} is claimed by run {owner}, which is still live. Two agents writing \
                 one file would overwrite each other, so this write was refused and nothing \
                 was changed. Work on a different file, or wait for that run to finish."
            ));
        }
    }
    let read_key = (db.clone(), run_id.to_string());
    if let Some(seen) = ledger
        .reads
        .get(&read_key)
        .and_then(|paths| paths.get(path))
        .copied()
    {
        if seen != fingerprint_now {
            return Err(format!(
                "{shown} changed on disk since this run last read it — another run or another \
                 program wrote it. This write was refused so that change is not silently \
                 overwritten. Read {shown} again and rewrite from what is there now."
            ));
        }
    }
    let paths = ledger.reads.entry(read_key).or_default();
    if paths.len() < MAX_TRACKED_PATHS || paths.contains_key(path) {
        paths.insert(path.to_string(), fingerprint_now);
    }
    ledger
        .owners
        .insert(key, (run_id.to_string(), fingerprint_now));
    Ok(())
}

/// Drop everything a run holds. Called when it reaches a terminal state: its
/// claim on a file is over, and its read fingerprints would only mislead a
/// later run that happens to reuse the id.
pub(crate) fn release(state: &AppState, run_id: &str) {
    let db = scope(state);
    let mut ledger = LEDGER.lock().unwrap();
    ledger.reads.remove(&(db.clone(), run_id.to_string()));
    ledger
        .owners
        .retain(|(owner_db, _), (owner, _)| !(*owner_db == db && owner == run_id));
}
