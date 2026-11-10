//! Schema versioning and the copy taken before an upgrade.
//!
//! A studio database outlives every binary that opens it, so the rules that
//! protect it live here rather than in the migration code:
//!
//! * the version is stored in SQLite's own `user_version`, next to the data it
//!   describes, so a copy carries its version with it;
//! * a copy is made with `VACUUM INTO`, which asks SQLite for a consistent
//!   snapshot instead of copying a file that may be half-written, and the copy
//!   is opened and checked before a migration is allowed to touch the original;
//! * putting it back goes through a temporary name first, so an interrupted
//!   restore cannot leave a truncated database at the real path.

use rusqlite::{Connection, OptionalExtension};
use std::{
    fs,
    path::{Path as FsPath, PathBuf},
};

/// The schema this build writes. `1` is the first recorded version: every
/// database that existed before it reports `0`, so the first boot of this build
/// upgrades exactly once and takes one copy.
pub(crate) const SCHEMA_VERSION: i32 = 1;

/// `PRAGMA user_version`, which SQLite keeps in the database header beside the
/// data, so a copy is self-describing.
pub(crate) fn read_version(conn: &Connection) -> rusqlite::Result<i32> {
    conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))
}

pub(crate) fn write_version(conn: &Connection, version: i32) -> rusqlite::Result<()> {
    conn.pragma_update(None, "user_version", version)
}

/// Where the copy taken for an upgrade from `from` lives: beside the database
/// and named for the database and version it came from, so a person can find it
/// without being told a random id.
pub(crate) fn backup_path(db: &FsPath, from: i32) -> PathBuf {
    let name = db
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "history.sqlite3".to_string());
    db.with_file_name(format!("{name}.pre-{from}.bak"))
}

/// A path as a SQL string literal. `VACUUM INTO` takes an expression and a
/// Windows path is full of backslashes, which is exactly why this is quoted
/// here instead of being spliced in raw.
fn quoted(path: &FsPath) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

/// Take a consistent, verified copy of `db` and answer its path.
///
/// The copy is only reported when it opens cleanly, passes `quick_check` and
/// still reports the version the original reported: a copy that cannot be read
/// is worse than no copy, because the restore path would trust it.
pub(crate) fn backup(conn: &Connection, db: &FsPath, from: i32) -> Result<PathBuf, String> {
    let target = backup_path(db, from);
    // `VACUUM INTO` refuses an existing file, and a copy left by an earlier
    // attempt is by definition not the database as it stands now.
    let _ = fs::remove_file(&target);
    conn.execute_batch(&format!("VACUUM INTO {}", quoted(&target)))
        .map_err(|error| {
            format!(
                "Could not copy {} to {} before the upgrade: {error}. Nothing was changed.",
                db.display(),
                target.display()
            )
        })?;

    let verified = (|| -> Result<(), String> {
        let copy = Connection::open(&target)
            .map_err(|error| format!("the copy does not open ({error})"))?;
        let check: Option<String> = copy
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .optional()
            .map_err(|error| format!("the copy cannot be checked ({error})"))?;
        match check.as_deref() {
            Some("ok") => {}
            other => {
                return Err(format!(
                    "the copy is not clean ({})",
                    other.unwrap_or("no answer")
                ))
            }
        }
        let version =
            read_version(&copy).map_err(|error| format!("the copy has no version ({error})"))?;
        if version != from {
            return Err(format!("the copy reports schema {version}, not {from}"));
        }
        Ok(())
    })();

    if let Err(reason) = verified {
        let _ = fs::remove_file(&target);
        return Err(format!(
            "Could not verify the copy of {} taken before the upgrade: {reason}. Nothing was changed.",
            db.display()
        ));
    }
    Ok(target)
}

/// The sidecar files that belong to a database file. They describe the database
/// being replaced, so leaving one beside a restored copy is how a rollback reads
/// a journal that is not its own.
fn sidecars(db: &FsPath) -> Vec<PathBuf> {
    ["-wal", "-shm", "-journal"]
        .iter()
        .map(|suffix| {
            let mut name = db.as_os_str().to_os_string();
            name.push(suffix);
            PathBuf::from(name)
        })
        .collect()
}

/// Put `copy` back at `db`, replacing whatever is there now.
pub(crate) fn restore(copy: &FsPath, db: &FsPath) -> Result<(), String> {
    if !copy.is_file() {
        return Err(format!("{} is gone", copy.display()));
    }
    let name = db
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "history.sqlite3".to_string());
    let staging = db.with_file_name(format!("{name}.restoring"));
    let _ = fs::remove_file(&staging);
    // Copy beside the target first: until the rename below, the current file is
    // still the one at the real path, so a failure here never loses both.
    fs::copy(copy, &staging).map_err(|error| {
        format!(
            "Could not stage {} at {} ({error})",
            copy.display(),
            staging.display()
        )
    })?;
    for sidecar in sidecars(db) {
        let _ = fs::remove_file(sidecar);
    }
    if db.exists() {
        fs::remove_file(db)
            .map_err(|error| format!("Could not replace {} ({error})", db.display()))?;
    }
    fs::rename(&staging, db).map_err(|error| {
        format!(
            "The copy is safe at {} but could not be moved back to {} ({error})",
            copy.display(),
            db.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "free-ai-studio-schema-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(backup_path(&path, 0));
        path
    }

    fn notes(path: &FsPath) -> Vec<String> {
        let conn = Connection::open(path).expect("open");
        let mut stmt = conn
            .prepare("SELECT body FROM notes ORDER BY id")
            .expect("prepare");
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .expect("rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("collect");
        rows
    }

    #[test]
    fn a_copy_holds_the_rows_as_they_were_at_the_old_version() {
        let path = scratch("copy.sqlite3");
        let conn = Connection::open(&path).expect("open");
        conn.execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);")
            .expect("table");
        conn.execute("INSERT INTO notes(body) VALUES('before')", [])
            .expect("seed");
        write_version(&conn, 0).expect("version");

        let copy = backup(&conn, &path, 0).expect("copy");
        // Change the original after the copy was taken.
        conn.execute("INSERT INTO notes(body) VALUES('after')", [])
            .expect("later row");
        write_version(&conn, 7).expect("later version");
        drop(conn);

        let old = Connection::open(&copy).expect("open copy");
        assert_eq!(read_version(&old).expect("version"), 0);
        drop(old);
        assert_eq!(notes(&copy), vec!["before".to_string()]);
        assert_eq!(
            notes(&path),
            vec!["before".to_string(), "after".to_string()]
        );

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&copy);
    }

    #[test]
    fn a_second_copy_replaces_the_first_instead_of_failing() {
        let path = scratch("copy-twice.sqlite3");
        let conn = Connection::open(&path).expect("open");
        conn.execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);")
            .expect("table");
        write_version(&conn, 0).expect("version");

        let first = backup(&conn, &path, 0).expect("first copy");
        conn.execute("INSERT INTO notes(body) VALUES('later')", [])
            .expect("later row");
        let second = backup(&conn, &path, 0).expect("second copy");
        drop(conn);

        assert_eq!(first, second, "one copy per source version");
        assert_eq!(notes(&second), vec!["later".to_string()]);

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&second);
    }

    #[test]
    fn restoring_puts_the_copy_back_and_takes_the_sidecars_with_it() {
        let path = scratch("restore.sqlite3");
        let conn = Connection::open(&path).expect("open");
        conn.execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);")
            .expect("table");
        conn.execute("INSERT INTO notes(body) VALUES('kept')", [])
            .expect("seed");
        write_version(&conn, 0).expect("version");
        let copy = backup(&conn, &path, 0).expect("copy");
        conn.execute("INSERT INTO notes(body) VALUES('added later')", [])
            .expect("later row");
        write_version(&conn, 5).expect("later version");
        drop(conn);

        for sidecar in sidecars(&path) {
            fs::write(&sidecar, b"stale").expect("sidecar");
        }

        restore(&copy, &path).expect("restore");

        assert_eq!(notes(&path), vec!["kept".to_string()]);
        let restored = Connection::open(&path).expect("open restored");
        assert_eq!(read_version(&restored).expect("version"), 0);
        drop(restored);
        for sidecar in sidecars(&path) {
            assert!(
                !sidecar.exists(),
                "{} survived the restore",
                sidecar.display()
            );
        }

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&copy);
    }

    #[test]
    fn a_copy_that_lost_its_version_is_not_reported_as_usable() {
        let path = scratch("wrong-version.sqlite3");
        let conn = Connection::open(&path).expect("open");
        conn.execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);")
            .expect("table");
        write_version(&conn, 3).expect("version");

        // Asked for version 0, the copy is taken and then refused because it
        // reports 3 — and it must not be left behind to be trusted later.
        let error = backup(&conn, &path, 0).expect_err("a mismatched copy is refused");
        assert!(error.contains("reports schema 3"), "{error}");
        assert!(!backup_path(&path, 0).exists());
        drop(conn);
        let _ = fs::remove_file(&path);
    }
}
