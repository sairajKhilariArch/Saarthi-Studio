//! Leases between live agent runs over the files in the shared workspace.
//!
//! Two runs can work at once, and nothing in the filesystem stops both of them
//! writing the same file: whichever writes last wins, silently. A lease is not a
//! lock on the file — nothing outside this app can see it — but it answers three
//! questions before a step runs:
//!
//! * would this write clobber work another live run claimed,
//! * would it overwrite a change this run has not seen,
//! * and would this command run while another run is already running commands in
//!   the same checkout, where it can write anywhere?
//!
//! A refusal is not fatal: the model sees the message as a step and can re-read
//! the file or work elsewhere.
//!
//! The ledger lives in the studio database, not in a process variable, so a
//! headless server and a desktop build pointed at the same database see each
//! other's claims. Each row is keyed by the database file, the checkout a run is
//! working in (its own worktree, or the shared folder), and the path. A claim
//! whose run has reached a terminal state is dropped on the next claim, which is
//! what keeps a crash from blocking a file forever; a paused run keeps its reads
//! but loses its claims, because it is not going to step again until resumed.

use rusqlite::{params, Connection};

use crate::AppState;

/// How many paths one run's read fingerprints are remembered for. Past this a
/// run has touched more files than a shared folder can usefully guard, so new
/// paths stop being recorded instead of the table growing without limit.
const MAX_TRACKED_PATHS: usize = 256;

/// The path a command claim is stored under: one per checkout, not per file,
/// because a command can write anywhere inside it.
const COMMAND_SCOPE: &str = "*command*";

const TERMINAL: &str = "'succeeded','failed','cancelled','interrupted'";

/// A stable fingerprint of file content. `None` (a file that does not exist, or
/// one that cannot be read as text) hashes differently from any content, so
/// "created since I looked" and "deleted since I looked" are both changes.
pub(crate) fn fingerprint(content: Option<&str>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn scope(state: &AppState) -> String {
    state.database_path.to_string_lossy().to_string()
}

fn connect(state: &AppState) -> Result<Connection, String> {
    let conn = crate::open_db(&state.database_path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS run_leases (
             scope TEXT NOT NULL,
             checkout TEXT NOT NULL,
             path TEXT NOT NULL,
             run_id TEXT NOT NULL,
             fingerprint INTEGER NOT NULL,
             PRIMARY KEY (scope, checkout, path)
         );
         CREATE TABLE IF NOT EXISTS run_reads (
             scope TEXT NOT NULL,
             checkout TEXT NOT NULL,
             path TEXT NOT NULL,
             run_id TEXT NOT NULL,
             fingerprint INTEGER NOT NULL,
             PRIMARY KEY (scope, checkout, path, run_id)
         );",
    )
    .map_err(|e| e.to_string())?;
    Ok(conn)
}

/// Forget claims held by runs that will never step again, in this database.
fn purge(conn: &Connection, scope: &str) -> Result<(), String> {
    conn.execute(
        &format!("DELETE FROM run_leases WHERE scope=?1 AND run_id IN (SELECT id FROM agent_runs WHERE status IN ({TERMINAL}))"),
        [scope],
    )
    .map_err(|e| e.to_string())?;
    // A paused run holds no claim (it will not step), but its recorded reads are
    // kept so a resumed run still notices a file that changed while it was parked.
    conn.execute(
        "DELETE FROM run_leases WHERE scope=?1 AND run_id IN (SELECT id FROM agent_runs WHERE status='paused')",
        [scope],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        &format!("DELETE FROM run_reads WHERE scope=?1 AND run_id IN (SELECT id FROM agent_runs WHERE status IN ({TERMINAL}))"),
        [scope],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
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
    let Ok(conn) = connect(state) else {
        return;
    };
    let db = scope(state);
    let Ok(count) = conn.query_row(
        "SELECT COUNT(*) FROM run_reads WHERE scope=?1 AND checkout=?2 AND run_id=?3",
        params![db, root, run_id],
        |row| row.get::<_, i64>(0),
    ) else {
        return;
    };
    let known: bool = conn
        .query_row(
            "SELECT 1 FROM run_reads WHERE scope=?1 AND checkout=?2 AND path=?3 AND run_id=?4",
            params![db, root, path, run_id],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if !known && count as usize >= MAX_TRACKED_PATHS {
        return;
    }
    let _ = conn.execute(
        "INSERT INTO run_reads(scope, checkout, path, run_id, fingerprint) VALUES(?1,?2,?3,?4,?5) \
         ON CONFLICT(scope, checkout, path, run_id) DO UPDATE SET fingerprint=excluded.fingerprint",
        params![db, root, path, run_id, fingerprint(content) as i64],
    );
}

/// Ask to write `path`. `current` is the content on disk right now; the file is
/// what matters, never the caller's view of it.
pub(crate) fn claim_write(
    state: &AppState,
    root: &str,
    run_id: &str,
    path: &str,
    current: Option<&str>,
) -> Result<(), String> {
    let conn = connect(state)?;
    let db = scope(state);
    purge(&conn, &db)?;
    let owner: Option<String> = conn
        .query_row(
            "SELECT run_id FROM run_leases WHERE scope=?1 AND checkout=?2 AND path=?3",
            params![db, root, path],
            |row| row.get(0),
        )
        .ok();
    if let Some(owner) = owner {
        if owner != run_id {
            return Err(format!(
                "{path} is claimed by run {owner}, which is still live. Two agents writing \
                 one file would overwrite each other, so this write was refused and nothing \
                 was changed. Work on a different file, or wait for that run to finish."
            ));
        }
    }
    let now = fingerprint(current) as i64;
    let seen: Option<i64> = conn
        .query_row(
            "SELECT fingerprint FROM run_reads WHERE scope=?1 AND checkout=?2 AND path=?3 AND run_id=?4",
            params![db, root, path, run_id],
            |row| row.get(0),
        )
        .ok();
    if let Some(seen) = seen {
        if seen != now {
            return Err(format!(
                "{path} changed on disk since this run last read it — another run or another \
                 program wrote it. This write was refused so that change is not silently \
                 overwritten. Read {path} again and rewrite from what is there now."
            ));
        }
    }
    conn.execute(
        "INSERT INTO run_reads(scope, checkout, path, run_id, fingerprint) VALUES(?1,?2,?3,?4,?5) \
         ON CONFLICT(scope, checkout, path, run_id) DO UPDATE SET fingerprint=excluded.fingerprint",
        params![db, root, path, run_id, now],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO run_leases(scope, checkout, path, run_id, fingerprint) VALUES(?1,?2,?3,?4,?5) \
         ON CONFLICT(scope, checkout, path) DO UPDATE SET run_id=excluded.run_id, fingerprint=excluded.fingerprint",
        params![db, root, path, run_id, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Who, if anyone, a live run has claimed `path` against in this checkout.
///
/// The editor asks this before saving: the agent's own `write` refuses a file
/// another run holds, and this is the same question asked in the other direction,
/// so a human save and a run's write cannot race in either order.
pub(crate) fn claimed_by(state: &AppState, root: &str, path: &str) -> Option<String> {
    let conn = connect(state).ok()?;
    let db = scope(state);
    let _ = purge(&conn, &db);
    conn.query_row(
        "SELECT run_id FROM run_leases WHERE scope=?1 AND checkout=?2 AND path=?3",
        params![db, root, path],
        |row| row.get(0),
    )
    .ok()
}

/// The live run, if any, that is running commands in this checkout. It can write
/// anywhere inside it, so every path in the folder is effectively claimed while
/// that command runs.
pub(crate) fn command_claim(state: &AppState, root: &str) -> Option<String> {
    claimed_by(state, root, COMMAND_SCOPE)
}

/// Claim the whole checkout for one command. A command can write anywhere inside
/// its working directory, so per-file leases cannot express it; two runs may not
/// run commands in the same checkout at the same time.
pub(crate) fn claim_command(state: &AppState, root: &str, run_id: &str) -> Result<(), String> {
    let conn = connect(state)?;
    let db = scope(state);
    purge(&conn, &db)?;
    let owner: Option<String> = conn
        .query_row(
            "SELECT run_id FROM run_leases WHERE scope=?1 AND checkout=?2 AND path=?3",
            params![db, root, COMMAND_SCOPE],
            |row| row.get(0),
        )
        .ok();
    if let Some(owner) = owner {
        if owner != run_id {
            return Err(format!(
                "Another live run ({owner}) is running a command in this folder right now. A \
                 command can write anywhere inside it, so this one was refused rather than \
                 interleaved with that work. Try again when it finishes."
            ));
        }
    }
    conn.execute(
        "INSERT INTO run_leases(scope, checkout, path, run_id, fingerprint) VALUES(?1,?2,?3,?4,0) \
         ON CONFLICT(scope, checkout, path) DO UPDATE SET run_id=excluded.run_id",
        params![db, root, COMMAND_SCOPE, run_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// The command finished (or failed): the checkout is free again.
pub(crate) fn release_command(state: &AppState, root: &str, run_id: &str) {
    let Ok(conn) = connect(state) else {
        return;
    };
    let _ = conn.execute(
        "DELETE FROM run_leases WHERE scope=?1 AND checkout=?2 AND path=?3 AND run_id=?4",
        params![scope(state), root, COMMAND_SCOPE, run_id],
    );
}

/// Drop everything a run holds. Called when it reaches a terminal state: its
/// claims are over, and its read fingerprints would only mislead a later run
/// that happens to reuse the id.
pub(crate) fn release(state: &AppState, run_id: &str) {
    let Ok(conn) = connect(state) else {
        return;
    };
    let db = scope(state);
    let _ = conn.execute(
        "DELETE FROM run_leases WHERE scope=?1 AND run_id=?2",
        params![db, run_id],
    );
    let _ = conn.execute(
        "DELETE FROM run_reads WHERE scope=?1 AND run_id=?2",
        params![db, run_id],
    );
}
