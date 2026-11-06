//! Project code index: which files exist, what language they are, and the
//! symbols they declare — kept incrementally so a large folder stays cheap to
//! re-index.
//!
//! The walk is deliberately boring: directories that never hold project source
//! are skipped by name, files over the size cap or carrying a NUL byte are
//! skipped, and anything matching the user's privacy exclusions is skipped
//! before its bytes are read. What survives is stored in SQLite
//! (`code_index_files`, `code_index_symbols`) with a content digest, so a
//! refresh only re-extracts the files whose bytes changed.
//!
//! Everything here is derived from disk on this machine: the index is local
//! state, and search returns a path and a line for every hit, so a result can be
//! opened and checked rather than trusted.

use crate::{active_project_id, json_error, load_config, open_db, AppState};
use axum::{
    body::Body,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path as FsPath,
};

/// Folders that never hold source worth indexing. The list mirrors the outline's
/// skips and adds the language-ecosystem build/dependency folders.
const SKIP_DIRS: [&str; 20] = [
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "build",
    "dist",
    "out",
    ".gradle",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".nuxt",
    ".cache",
    "vendor",
    "coverage",
    ".idea",
    ".terraform",
    "Pods",
];
/// A generated file above this size is data, not source.
const MAX_FILE_BYTES: u64 = 1 << 20;
/// The walk stops here and says so, rather than indexing a drive for minutes.
const MAX_FILES: usize = 5000;
/// A single file more than this many symbols is minified or generated.
const MAX_SYMBOLS_PER_FILE: usize = 2000;
const MAX_HITS: usize = 50;

pub(crate) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS code_index_files (
            path TEXT PRIMARY KEY, language TEXT NOT NULL, bytes INTEGER NOT NULL,
            lines INTEGER NOT NULL, digest TEXT NOT NULL, indexed_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS code_index_symbols (
            path TEXT NOT NULL, line INTEGER NOT NULL, kind TEXT NOT NULL,
            name TEXT NOT NULL,
            PRIMARY KEY (path, line, name)
        );
        CREATE INDEX IF NOT EXISTS code_index_symbol_name ON code_index_symbols(name);",
    )
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Symbol {
    pub line: usize,
    pub kind: String,
    pub name: String,
}

/// A stable content digest (FNV-1a 64). It exists to spot a changed file, not to
/// prove anything about it, so it is never called a checksum.
fn digest(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

/// Secret-shaped files are never indexed, whatever the exclusions say: an index
/// is local, but it is also searchable, and a key in a search result is a key in
/// a screenshot.
fn secret_shaped(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with(".env")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower == "id_rsa"
        || lower == "id_ed25519"
        || lower == ".npmrc"
        || lower == ".netrc"
}

/// Does a workspace-relative path fall under one of the user's exclusions? The
/// comparison mirrors the privacy filter: separators normalised, case folded on
/// Windows, prefix or equal.
fn excluded(relative: &str, exclusions: &[String]) -> bool {
    let path = relative.replace('\\', "/").to_ascii_lowercase();
    exclusions.iter().any(|rule| {
        let rule = rule.trim().replace('\\', "/").to_ascii_lowercase();
        let rule = rule
            .trim_start_matches("./")
            .trim_end_matches('/')
            .to_string();
        if rule.is_empty() {
            return false;
        }
        path == rule || path.starts_with(&format!("{rule}/"))
    })
}

/// Symbol extraction, per language family. Line-based on purpose: it needs no
/// parser, it cannot hang, and a missed symbol costs a search hit, not a wrong
/// answer. Markdown and data files declare nothing.
pub(crate) fn symbols_in(language: &str, text: &str) -> Vec<Symbol> {
    let keywords: &[(&str, &str)] = match language {
        "rust" => &[
            ("fn ", "fn"),
            ("struct ", "struct"),
            ("enum ", "enum"),
            ("trait ", "trait"),
            ("mod ", "mod"),
            ("impl ", "impl"),
            ("const ", "const"),
            ("static ", "static"),
            ("type ", "type"),
            ("macro_rules!", "macro"),
        ],
        "python" => &[("def ", "fn"), ("class ", "class")],
        "javascript" | "typescript" => &[
            ("function ", "fn"),
            ("class ", "class"),
            ("interface ", "interface"),
            ("type ", "type"),
            ("const ", "const"),
            ("let ", "const"),
            ("enum ", "enum"),
        ],
        "go" => &[("func ", "fn"), ("type ", "type")],
        "java" | "kotlin" | "csharp" => &[
            ("class ", "class"),
            ("interface ", "interface"),
            ("enum ", "enum"),
            ("record ", "record"),
            ("fun ", "fn"),
            ("struct ", "struct"),
        ],
        "ruby" => &[("def ", "fn"), ("class ", "class"), ("module ", "module")],
        "php" => &[("function ", "fn"), ("class ", "class")],
        "swift" => &[
            ("func ", "fn"),
            ("class ", "class"),
            ("struct ", "struct"),
            ("protocol ", "protocol"),
        ],
        "bash" => &[("function ", "fn")],
        _ => &[],
    };
    if keywords.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if out.len() >= MAX_SYMBOLS_PER_FILE {
            break;
        }
        let mut trimmed = line.trim_start();
        for prefix in [
            "pub(crate) ",
            "pub ",
            "export ",
            "async ",
            "public ",
            "private ",
            "protected ",
            "static ",
        ] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                trimmed = rest.trim_start();
            }
        }
        for (token, kind) in keywords {
            let Some(rest) = trimmed.strip_prefix(token) else {
                continue;
            };
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|ch| {
                    ch.is_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '<' | '!')
                })
                .collect();
            let name = name.trim_end_matches(['<', ':']).to_string();
            if name.is_empty() || name.chars().all(|ch| ch == '_') {
                continue;
            }
            // A `const`/`let` only declares a symbol when it is a function value;
            // a plain numeric constant is noise here.
            if *kind == "const" && !(rest.contains("=>") || rest.contains("function")) {
                continue;
            }
            out.push(Symbol {
                line: index + 1,
                kind: (*kind).to_string(),
                name,
            });
            break;
        }
    }
    out
}

#[derive(Debug, Default)]
pub(crate) struct Report {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub files: usize,
    pub symbols: usize,
    pub skipped_oversize: usize,
    pub skipped_binary: usize,
    pub skipped_secret: usize,
    pub skipped_excluded: usize,
    pub truncated: bool,
}

fn walk(
    conn: &Connection,
    dir: &FsPath,
    base: &str,
    exclusions: &[String],
    known: &mut HashMap<String, String>,
    seen: &mut HashSet<String>,
    report: &mut Report,
) -> Result<(), String> {
    if report.files >= MAX_FILES {
        report.truncated = true;
        return Ok(());
    }
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(_) => return Ok(()),
    };
    let mut items: Vec<fs::DirEntry> = read.flatten().collect();
    items.sort_by_key(|item| item.file_name());
    for item in items {
        if report.files >= MAX_FILES {
            report.truncated = true;
            return Ok(());
        }
        let name = item.file_name().to_string_lossy().to_string();
        let relative = if base.is_empty() {
            name.clone()
        } else {
            format!("{base}/{name}")
        };
        let file_type = match item.file_type() {
            Ok(kind) => kind,
            Err(_) => continue,
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if skip_dir(&name) {
                continue;
            }
            walk(
                conn,
                &dir.join(&name),
                &relative,
                exclusions,
                known,
                seen,
                report,
            )?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if secret_shaped(&name) {
            report.skipped_secret += 1;
            continue;
        }
        if excluded(&relative, exclusions) {
            report.skipped_excluded += 1;
            continue;
        }
        let size = item.metadata().map(|meta| meta.len()).unwrap_or(0);
        if size > MAX_FILE_BYTES {
            report.skipped_oversize += 1;
            continue;
        }
        let bytes = match fs::read(dir.join(&name)) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if bytes.contains(&0) {
            report.skipped_binary += 1;
            continue;
        }
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                report.skipped_binary += 1;
                continue;
            }
        };
        let language = crate::workspace::language_for(FsPath::new(&name));
        let content_digest = digest(&text);
        report.files += 1;
        seen.insert(relative.clone());
        match known.get(&relative) {
            Some(previous) if previous == &content_digest => {
                report.unchanged += 1;
                continue;
            }
            Some(_) => report.updated += 1,
            None => report.added += 1,
        }
        let now = crate::now_seconds();
        conn.execute(
            "INSERT INTO code_index_files(path,language,bytes,lines,digest,indexed_at)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(path) DO UPDATE SET language=?2, bytes=?3, lines=?4, digest=?5, indexed_at=?6",
            params![
                relative,
                language,
                text.len() as i64,
                text.lines().count() as i64,
                content_digest,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM code_index_symbols WHERE path=?1", [&relative])
            .map_err(|e| e.to_string())?;
        for symbol in symbols_in(&language, &text) {
            conn.execute(
                "INSERT OR IGNORE INTO code_index_symbols(path,line,kind,name) VALUES(?1,?2,?3,?4)",
                params![relative, symbol.line as i64, symbol.kind, symbol.name],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Bring the index in line with the folder. Unchanged files are left alone; a
/// changed file has its symbols replaced, never appended to.
pub(crate) fn refresh(db: &FsPath, root: &FsPath, exclusions: &[String]) -> Result<Report, String> {
    let conn = open_db(db).map_err(|e| e.to_string())?;
    let mut known: HashMap<String, String> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT path, digest FROM code_index_files")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            known.insert(row.0, row.1);
        }
    }
    let mut report = Report::default();
    let mut seen: HashSet<String> = HashSet::new();
    walk(
        &conn,
        root,
        "",
        exclusions,
        &mut known,
        &mut seen,
        &mut report,
    )?;
    if !report.truncated {
        for path in known.keys() {
            if !seen.contains(path) {
                conn.execute("DELETE FROM code_index_files WHERE path=?1", [path])
                    .map_err(|e| e.to_string())?;
                conn.execute("DELETE FROM code_index_symbols WHERE path=?1", [path])
                    .map_err(|e| e.to_string())?;
                report.removed += 1;
            }
        }
    }
    report.symbols = conn
        .query_row("SELECT COUNT(*) FROM code_index_symbols", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize;
    report.files = conn
        .query_row("SELECT COUNT(*) FROM code_index_files", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize;
    Ok(report)
}

fn report_json(report: &Report) -> Value {
    json!({
        "added": report.added,
        "updated": report.updated,
        "removed": report.removed,
        "unchanged": report.unchanged,
        "files": report.files,
        "symbols": report.symbols,
        "skipped": {
            "oversize": report.skipped_oversize,
            "binary": report.skipped_binary,
            "secret": report.skipped_secret,
            "excluded": report.skipped_excluded,
        },
        "truncated": report.truncated,
    })
}

/// Index state as it stands: counts, languages, the newest indexed file, and
/// which folder the index belongs to.
fn status_json(db: &FsPath, root: Option<&FsPath>) -> Value {
    let conn = match open_db(db) {
        Ok(conn) => conn,
        Err(error) => return json!({"ok": false, "error": error.to_string()}),
    };
    let files: i64 = conn
        .query_row("SELECT COUNT(*) FROM code_index_files", [], |row| {
            row.get(0)
        })
        .unwrap_or(0);
    let symbols: i64 = conn
        .query_row("SELECT COUNT(*) FROM code_index_symbols", [], |row| {
            row.get(0)
        })
        .unwrap_or(0);
    let indexed_at: Option<f64> = conn
        .query_row("SELECT MAX(indexed_at) FROM code_index_files", [], |row| {
            row.get(0)
        })
        .ok()
        .flatten();
    let mut languages = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT language, COUNT(*) FROM code_index_files GROUP BY language ORDER BY COUNT(*) DESC, language",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(json!({"language": row.get::<_, String>(0)?, "files": row.get::<_, i64>(1)?}))
        }) {
            languages = rows.flatten().collect();
        }
    }
    json!({
        "ok": true,
        "root": root.map(|path| path.display().to_string()).unwrap_or_default(),
        "files": files,
        "symbols": symbols,
        "indexed_at": indexed_at,
        "languages": languages,
    })
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

fn search(db: &FsPath, query: &str) -> Result<Vec<Value>, String> {
    let wanted = crate::skills::keywords(query);
    if wanted.is_empty() {
        return Err(
            "Type at least one word of two or more characters to search the index.".to_string(),
        );
    }
    let conn = open_db(db).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT s.path, s.line, s.kind, s.name, f.language
             FROM code_index_symbols s JOIN code_index_files f ON f.path = s.path",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let wanted_lower: Vec<String> = wanted.iter().cloned().collect();
    let mut hits: Vec<(usize, Value)> = Vec::new();
    let mut path_hits: HashMap<String, (usize, String)> = HashMap::new();
    for row in rows.flatten() {
        let (path, line, kind, name, language) = row;
        let lower_name = name.to_ascii_lowercase();
        let mut score = 0usize;
        for word in &wanted_lower {
            if lower_name == *word {
                score += 10;
            } else if lower_name.starts_with(word) {
                score += 6;
            } else if lower_name.contains(word) {
                score += 4;
            }
        }
        let lower_path = path.to_ascii_lowercase();
        for word in &wanted_lower {
            if lower_path.contains(word) {
                score += 2;
                let entry = path_hits
                    .entry(path.clone())
                    .or_insert((0, language.clone()));
                entry.0 += 2;
            }
        }
        if score == 0 {
            continue;
        }
        hits.push((
            score,
            json!({
                "path": path,
                "line": line,
                "kind": kind,
                "name": name,
                "language": language,
                "score": score,
            }),
        ));
    }
    // A file whose path matches is worth surfacing even when no symbol does.
    for (path, (score, language)) in path_hits {
        if hits
            .iter()
            .any(|(_, hit)| hit["path"].as_str() == Some(path.as_str()))
        {
            continue;
        }
        hits.push((
            score,
            json!({
                "path": path,
                "line": 0,
                "kind": "file",
                "name": "",
                "language": language,
                "score": score,
            }),
        ));
    }
    hits.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| {
                a.1["path"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b.1["path"].as_str().unwrap_or(""))
            })
            .then_with(|| {
                a.1["line"]
                    .as_i64()
                    .unwrap_or(0)
                    .cmp(&b.1["line"].as_i64().unwrap_or(0))
            })
    });
    Ok(hits
        .into_iter()
        .take(MAX_HITS)
        .map(|(_, hit)| hit)
        .collect())
}

async fn get_status(State(state): State<AppState>) -> Json<Value> {
    Json(status_json(
        &state.database_path,
        state.workspace.current_root().as_deref(),
    ))
}

async fn refresh_index(State(state): State<AppState>) -> Response<Body> {
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error.message().to_string()),
    };
    let cfg = load_config(&state);
    match refresh(&state.database_path, &root, &cfg.privacy_exclusions) {
        Ok(report) => Json(json!({
            "ok": true,
            "project": active_project_id(&state),
            "report": report_json(&report),
        }))
        .into_response(),
        Err(error) => json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn search_index(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Response<Body> {
    match search(&state.database_path, &query.q) {
        Ok(hits) => Json(json!({"ok": true, "hits": hits})).into_response(),
        Err(error) => json_error(StatusCode::BAD_REQUEST, error),
    }
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/index", get(get_status))
        .route("/api/index/refresh", post(refresh_index))
        .route("/api/index/search", get(search_index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "free-ai-studio-index-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The database lives beside the indexed folder, never inside it: a SQLite
    /// file in the tree would be walked and skipped as binary, which is not
    /// what these tests are measuring.
    fn database(dir: &FsPath) -> PathBuf {
        let path = dir.with_extension("db");
        let _ = fs::remove_file(&path);
        initialize(&open_db(&path).unwrap()).unwrap();
        path
    }

    fn write(root: &FsPath, relative: &str, text: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    #[test]
    fn symbols_are_found_per_language() {
        let rust = symbols_in(
            "rust",
            "pub fn open(path: &str) -> Self {}\nstruct Store;\n// fn not_this\n",
        );
        assert_eq!(
            rust,
            [
                Symbol {
                    line: 1,
                    kind: "fn".into(),
                    name: "open".into()
                },
                Symbol {
                    line: 2,
                    kind: "struct".into(),
                    name: "Store".into()
                },
            ]
        );
        let python = symbols_in("python", "async def fetch(url):\nclass Client:\n");
        assert_eq!(python.len(), 2);
        assert_eq!(python[0].name, "fetch");
        let ts = symbols_in(
            "typescript",
            "export function run() {}\nexport const helper = () => 1;\nconst answer = 42;\ninterface Options {}\n",
        );
        assert_eq!(
            ts.iter()
                .map(|symbol| symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["run", "helper", "Options"]
        );
        assert!(symbols_in("markdown", "fn not_code()").is_empty());
    }

    #[test]
    fn secrets_build_folders_and_oversize_files_are_skipped() {
        let dir = scratch("skips");
        let db = database(&dir);
        write(&dir, "src/main.rs", "fn main() {}\n");
        write(&dir, ".env", "SECRET=1\n");
        write(&dir, "certs/server.pem", "-----BEGIN-----\n");
        write(&dir, "node_modules/dep/index.js", "function dep() {}\n");
        write(&dir, "secret/notes.txt", "nothing to see\n");
        write(&dir, "bin.dat", "\u{0}\u{1}\u{2}");
        let report = refresh(&db, &dir, &["secret".to_string()]).unwrap();
        assert_eq!(report.added, 1, "{report:?}");
        assert_eq!(report.files, 1);
        assert_eq!(report.skipped_secret, 2, "the .env and the .pem");
        assert!(report.skipped_excluded >= 1, "the excluded folder");
        assert_eq!(report.skipped_binary, 1);
        let hits = search(&db, "main").unwrap();
        assert!(hits.iter().all(|hit| hit["path"] != json!(".env")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refresh_is_incremental() {
        let dir = scratch("incremental");
        let db = database(&dir);
        write(&dir, "a.rs", "fn alpha() {}\n");
        write(&dir, "b.py", "def beta():\n    pass\n");
        let first = refresh(&db, &dir, &[]).unwrap();
        assert_eq!((first.added, first.unchanged), (2, 0));
        assert_eq!(first.symbols, 2);
        let second = refresh(&db, &dir, &[]).unwrap();
        assert_eq!((second.added, second.updated, second.unchanged), (0, 0, 2));
        write(&dir, "a.rs", "fn gamma() {}\n");
        let third = refresh(&db, &dir, &[]).unwrap();
        assert_eq!((third.added, third.updated, third.unchanged), (0, 1, 1));
        let gamma = search(&db, "gamma").unwrap();
        assert_eq!(gamma.len(), 1, "{gamma:?}");
        assert!(
            search(&db, "alpha").unwrap().is_empty(),
            "the old symbol must be gone"
        );
        fs::remove_file(dir.join("b.py")).unwrap();
        let fourth = refresh(&db, &dir, &[]).unwrap();
        assert_eq!(fourth.removed, 1);
        assert!(search(&db, "beta").unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_ranks_names_above_paths_and_reports_the_line() {
        let dir = scratch("search");
        let db = database(&dir);
        write(&dir, "src/auth.rs", "fn login() {}\nfn logout() {}\n");
        write(&dir, "src/session.rs", "fn refresh_auth() {}\n");
        write(&dir, "src/other.rs", "fn unrelated() {}\n");
        refresh(&db, &dir, &[]).unwrap();
        let hits = search(&db, "login").unwrap();
        assert_eq!(hits[0]["path"], json!("src/auth.rs"));
        assert_eq!(hits[0]["line"], json!(1));
        assert_eq!(hits[0]["kind"], json!("fn"));
        let auth = search(&db, "auth").unwrap();
        assert!(auth.len() >= 2, "{auth:?}");
        assert!(search(&db, "unrelated").unwrap().len() == 1);
        assert!(search(&db, " ").is_err(), "an empty query is refused");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_reports_what_was_indexed() {
        let dir = scratch("status");
        let db = database(&dir);
        write(&dir, "main.go", "func main() {}\n");
        refresh(&db, &dir, &[]).unwrap();
        let status = status_json(&db, Some(dir.as_path()));
        assert_eq!(status["files"], json!(1));
        assert_eq!(status["symbols"], json!(1));
        assert_eq!(status["languages"][0]["language"], json!("go"));
        assert!(status["indexed_at"].as_f64().is_some());
        let _ = fs::remove_dir_all(&dir);
    }
}
