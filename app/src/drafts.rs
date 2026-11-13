//! Unsaved editor buffers, kept across an unexpected shutdown.
//!
//! While a buffer is dirty the editor mirrors it here, in the application's own
//! database rather than the folder, and removes the row once the file is saved,
//! reverted or the tab is closed with the changes discarded. Nothing in this
//! module writes the workspace: a draft is the app's copy of text that was never
//! saved. Drafts are scoped to the folder they were typed in, so reopening that
//! folder offers the text back and another folder never sees it.
//!
//! The store is best effort: it keeps at most `MAX_DRAFTS` rows and
//! `MAX_DRAFT_TOTAL_BYTES` of text per folder and prunes the oldest first. A
//! recovery that silently drops the least recently typed buffer is better than
//! an editor that stops working because recovery filled the disk.

use super::{json_error, open_db, AppState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Component, Path as FsPath},
    time::{SystemTime, UNIX_EPOCH},
};

/// Largest buffer one draft may hold. The editor refuses larger files, so a
/// draft above this could not have been typed in it.
const MAX_DRAFT_BYTES: usize = 1024 * 1024;
/// Drafts one folder may keep; a crash with many tabs open is normal.
const MAX_DRAFTS: usize = 200;
/// Text one folder's drafts may hold in total.
const MAX_DRAFT_TOTAL_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn initialize(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS editor_drafts (
            root TEXT NOT NULL,
            path TEXT NOT NULL,
            content TEXT NOT NULL,
            bytes INTEGER NOT NULL,
            updated_at REAL NOT NULL,
            PRIMARY KEY (root, path)
        );
        CREATE INDEX IF NOT EXISTS editor_drafts_age ON editor_drafts(root, updated_at);",
    )
}

/// The limits are a value so a test can hold a small one.
#[derive(Clone, Copy)]
struct Limits {
    bytes: usize,
    drafts: usize,
    total: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: MAX_DRAFT_BYTES,
            drafts: MAX_DRAFTS,
            total: MAX_DRAFT_TOTAL_BYTES,
        }
    }
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

fn root_key(root: &FsPath) -> String {
    root.to_string_lossy().to_string()
}

/// The key a draft is stored under. The check is lexical on purpose: a draft is
/// text in the app's own database, never a filesystem write, and it has to stay
/// readable after the file or its folder was deleted — that is exactly the crash
/// this store exists for. Absolute paths, `..` and Windows prefixes are refused,
/// so no row can name anything outside the folder it is scoped to.
fn draft_key(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Name the file the draft belongs to.".to_string());
    }
    let candidate = FsPath::new(trimmed);
    if candidate.is_absolute() {
        return Err("Drafts are stored by a path inside the open folder.".to_string());
    }
    for component in candidate.components() {
        match component {
            Component::Normal(name) if !name.is_empty() => {}
            Component::CurDir => {}
            _ => return Err("Draft paths must stay inside the open folder.".to_string()),
        }
    }
    Ok(trimmed.to_string())
}

/// Store or replace one draft. When the folder would hold more than the count or
/// size limit, the oldest drafts are deleted first and their paths returned.
fn save_draft(
    db: &FsPath,
    root: &FsPath,
    path: &str,
    content: &str,
    limits: Limits,
) -> Result<(Value, Vec<String>), String> {
    let key = draft_key(path)?;
    if content.len() > limits.bytes {
        return Err(format!(
            "That buffer is larger than the {}-byte draft limit.",
            limits.bytes
        ));
    }
    if content.len() > limits.total {
        return Err("That buffer is larger than the folder's draft budget.".to_string());
    }
    let mut conn = open_db(db).map_err(|error| error.to_string())?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let folder = root_key(root);
    let existing: Option<i64> = tx
        .query_row(
            "SELECT bytes FROM editor_drafts WHERE root = ?1 AND path = ?2",
            params![folder, key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let mut total: i64 = tx
        .query_row(
            "SELECT COALESCE(SUM(bytes), 0) FROM editor_drafts WHERE root = ?1",
            params![folder],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let mut count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM editor_drafts WHERE root = ?1",
            params![folder],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    // Replacing a row does not grow the folder by that row's old size.
    if let Some(bytes) = existing {
        total -= bytes;
        count -= 1;
    }
    let mut pruned = Vec::new();
    while count >= limits.drafts as i64 || total + content.len() as i64 > limits.total as i64 {
        let victim: Option<(String, i64)> = tx
            .query_row(
                "SELECT path, bytes FROM editor_drafts WHERE root = ?1 \
                 ORDER BY updated_at ASC, path ASC LIMIT 1",
                params![folder],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((oldest, bytes)) = victim else {
            break;
        };
        tx.execute(
            "DELETE FROM editor_drafts WHERE root = ?1 AND path = ?2",
            params![folder, oldest],
        )
        .map_err(|error| error.to_string())?;
        pruned.push(oldest);
        total -= bytes;
        count -= 1;
    }
    let updated = now();
    tx.execute(
        "INSERT INTO editor_drafts(root, path, content, bytes, updated_at) \
         VALUES(?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(root, path) DO UPDATE SET content = excluded.content, \
         bytes = excluded.bytes, updated_at = excluded.updated_at",
        params![folder, key, content, content.len() as i64, updated],
    )
    .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    Ok((
        json!({ "path": key, "bytes": content.len(), "updatedAt": updated }),
        pruned,
    ))
}

fn list_drafts(db: &FsPath, root: &FsPath) -> Result<Vec<Value>, String> {
    let conn = open_db(db).map_err(|error| error.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT path, bytes, updated_at FROM editor_drafts WHERE root = ?1 \
             ORDER BY updated_at DESC, path ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![root_key(root)], |row| {
            Ok(json!({
                "path": row.get::<_, String>(0)?,
                "bytes": row.get::<_, i64>(1)?,
                "updatedAt": row.get::<_, f64>(2)?,
            }))
        })
        .map_err(|error| error.to_string())?;
    Ok(rows.flatten().collect())
}

fn read_draft(db: &FsPath, root: &FsPath, path: &str) -> Result<Option<Value>, String> {
    let key = draft_key(path)?;
    let conn = open_db(db).map_err(|error| error.to_string())?;
    conn.query_row(
        "SELECT path, content, updated_at FROM editor_drafts WHERE root = ?1 AND path = ?2",
        params![root_key(root), key],
        |row| {
            Ok(json!({
                "path": row.get::<_, String>(0)?,
                "content": row.get::<_, String>(1)?,
                "updatedAt": row.get::<_, f64>(2)?,
            }))
        },
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn remove_draft(db: &FsPath, root: &FsPath, path: &str) -> Result<bool, String> {
    let key = draft_key(path)?;
    let conn = open_db(db).map_err(|error| error.to_string())?;
    let removed = conn
        .execute(
            "DELETE FROM editor_drafts WHERE root = ?1 AND path = ?2",
            params![root_key(root), key],
        )
        .map_err(|error| error.to_string())?;
    Ok(removed > 0)
}

#[derive(Deserialize)]
pub(crate) struct DraftQuery {
    #[serde(default)]
    path: String,
}

async fn list(State(state): State<AppState>) -> Response {
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => return error.into_response(),
    };
    match list_drafts(&state.database_path, &root) {
        Ok(drafts) => Json(json!({ "ok": true, "drafts": drafts })).into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn read(State(state): State<AppState>, Query(query): Query<DraftQuery>) -> Response {
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => return error.into_response(),
    };
    match read_draft(&state.database_path, &root, &query.path) {
        Ok(draft) => Json(json!({ "ok": true, "draft": draft })).into_response(),
        Err(error) => json_error(StatusCode::BAD_REQUEST, error),
    }
}

async fn save(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let Some(path) = input.get("path").and_then(Value::as_str) else {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Provide the file the draft belongs to.",
        );
    };
    let Some(content) = input.get("content").and_then(Value::as_str) else {
        return json_error(StatusCode::BAD_REQUEST, "Provide the draft's text.");
    };
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => return error.into_response(),
    };
    match save_draft(
        &state.database_path,
        &root,
        path,
        content,
        Limits::default(),
    ) {
        Ok((saved, pruned)) => {
            Json(json!({ "ok": true, "saved": saved, "pruned": pruned })).into_response()
        }
        Err(error) => json_error(StatusCode::BAD_REQUEST, error),
    }
}

async fn remove(State(state): State<AppState>, Query(query): Query<DraftQuery>) -> Response {
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => return error.into_response(),
    };
    match remove_draft(&state.database_path, &root, &query.path) {
        Ok(removed) => Json(json!({ "ok": true, "removed": removed })).into_response(),
        Err(error) => json_error(StatusCode::BAD_REQUEST, error),
    }
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/workspace/drafts", get(list))
        .route("/api/workspace/draft", get(read).put(save).delete(remove))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "free-ai-studio-drafts-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let db = base.join("studio.db");
        let root = base.join("project");
        fs::create_dir_all(&root).expect("project folder");
        fs::write(root.join("saved.rs"), "fn saved() {}\n").expect("file");
        (db, root)
    }

    fn database(path: &FsPath) -> std::path::PathBuf {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("database folder");
        }
        let conn = open_db(path).expect("database");
        initialize(&conn).expect("schema");
        path.to_path_buf()
    }

    #[test]
    fn a_draft_is_stored_read_listed_and_removed_per_folder() {
        let (db, root) = scratch("round-trip");
        let db = database(&db);
        let other = root.parent().expect("parent").join("other");
        fs::create_dir_all(&other).expect("other folder");
        fs::create_dir_all(root.join("src")).expect("source folder");

        let (saved, pruned) = save_draft(
            &db,
            &root,
            "src/lib.rs",
            "fn edited() {}\n",
            Limits::default(),
        )
        .unwrap();
        assert_eq!(saved["path"], "src/lib.rs");
        assert!(pruned.is_empty());
        assert_eq!(list_drafts(&db, &root).unwrap().len(), 1);
        let draft = read_draft(&db, &root, "src/lib.rs")
            .unwrap()
            .expect("draft");
        assert_eq!(draft["content"], "fn edited() {}\n");
        // Another folder with the same relative path sees nothing.
        assert!(read_draft(&db, &other, "src/lib.rs").unwrap().is_none());
        assert!(list_drafts(&db, &other).unwrap().is_empty());
        // Saving replaces rather than duplicating.
        save_draft(
            &db,
            &root,
            "src/lib.rs",
            "fn edited_twice() {}\n",
            Limits::default(),
        )
        .unwrap();
        assert_eq!(list_drafts(&db, &root).unwrap().len(), 1);
        // Removal reports whether anything was there.
        assert!(remove_draft(&db, &root, "src/lib.rs").unwrap());
        assert!(!remove_draft(&db, &root, "src/lib.rs").unwrap());
        assert!(list_drafts(&db, &root).unwrap().is_empty());
        let _ = fs::remove_dir_all(root.parent().expect("parent"));
    }

    #[test]
    fn drafts_refuse_paths_outside_the_folder_and_oversized_buffers() {
        let (db, root) = scratch("refusals");
        let db = database(&db);
        let small = Limits {
            bytes: 16,
            drafts: 10,
            total: 64,
        };

        assert!(save_draft(&db, &root, "", "x", small).is_err());
        assert!(save_draft(&db, &root, "  ", "x", small).is_err());
        assert!(save_draft(&db, &root, "../escape.rs", "x", small).is_err());
        assert!(save_draft(&db, &root, "C:/Windows/win.ini", "x", small).is_err());
        assert!(save_draft(&db, &root, "/etc/passwd", "x", small).is_err());
        // A folder that no longer exists is still a valid key: the draft is the
        // only copy left after a deletion, so it must remain readable.
        save_draft(&db, &root, "gone/deep/file.rs", "x", small).unwrap();
        assert!(read_draft(&db, &root, "gone/deep/file.rs")
            .unwrap()
            .is_some());
        // Over the per-buffer limit.
        let error = save_draft(&db, &root, "saved.rs", &"a".repeat(17), small).unwrap_err();
        assert!(error.contains("draft limit"), "{error}");
        // The saved file itself is a valid draft target, as is a new file whose
        // folder exists.
        save_draft(&db, &root, "saved.rs", "edited", small).unwrap();
        save_draft(&db, &root, "new.rs", "brand new", small).unwrap();
        // The saved file, the new file and the draft whose folder is gone.
        assert_eq!(list_drafts(&db, &root).unwrap().len(), 3);
        let _ = fs::remove_dir_all(root.parent().expect("parent"));
    }

    #[test]
    fn the_oldest_drafts_are_pruned_before_the_limits_are_crossed() {
        let (db, root) = scratch("prune");
        let db = database(&db);
        let tiny = Limits {
            bytes: 64,
            drafts: 2,
            total: 64,
        };

        save_draft(&db, &root, "a.rs", "aaa", tiny).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        save_draft(&db, &root, "b.rs", "bbb", tiny).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        // A third draft goes over the count limit: `a.rs` is the oldest.
        let (_, pruned) = save_draft(&db, &root, "c.rs", "ccc", tiny).unwrap();
        assert_eq!(pruned, vec!["a.rs".to_string()]);
        let names: Vec<String> = list_drafts(&db, &root)
            .unwrap()
            .iter()
            .map(|draft| draft["path"].as_str().unwrap_or("").to_string())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"b.rs".to_string()));
        assert!(names.contains(&"c.rs".to_string()));
        let _ = fs::remove_dir_all(root.parent().expect("parent"));
    }
}
