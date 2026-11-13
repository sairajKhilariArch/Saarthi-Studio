//! Semantic search over the code index.
//!
//! The index stores symbols, not meaning: its own search matches words. This
//! module embeds those symbols, caches the vectors beside the index (keyed by
//! model, symbol and the file's digest) and ranks symbols by cosine similarity.
//! Two backends can embed: the configured router (an OpenAI-shaped
//! `/embeddings` call) or the runtime on this machine, chosen with a `local:`
//! prefix on the model name. Nothing here runs on its own: a semantic search
//! happens only when the user asks for one and an embedding model is configured,
//! and a provider failure is reported as an error rather than papered over with
//! the word ranking, so a result never claims more than it did.

use crate::AppState;
use axum::{
    body::Body,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path as FsPath;

/// One request embeds at most this many documents: providers cap the input count,
/// and one slow request beats a wall of small ones.
const BATCH: usize = 32;
/// How many documents a single search may embed before it stops and reports what
/// is still pending; the next search continues where this one left off.
const EMBED_PER_SEARCH: usize = 128;
/// A document is the symbol's identity, not its body: a whole file would cost
/// tokens for no extra signal.
const DOC_CHARS: usize = 400;
/// How many hits a semantic search returns.
const MAX_HITS: usize = 20;
/// A `local:` prefix sends the request to the Ollama runtime on this machine
/// instead of the router, so a semantic index needs no router and no API key.
/// The cache scope stays the model string as typed, so switching backend
/// re-embeds instead of comparing vectors from two different models.
pub(crate) const LOCAL_PREFIX: &str = "local:";

/// The runtime model name behind a `local:` prefix, `None` for a router model.
/// A bare `local:` is not a model: it is treated as a router name so the
/// request fails visibly instead of embedding with an empty model.
fn local_model(model: &str) -> Option<String> {
    let trimmed = model.trim();
    let head = trimmed.get(..LOCAL_PREFIX.len())?;
    if !head.eq_ignore_ascii_case(LOCAL_PREFIX) {
        return None;
    }
    let name = trimmed[LOCAL_PREFIX.len()..].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

pub(crate) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS embedding_cache (
            scope TEXT NOT NULL, key TEXT NOT NULL, digest TEXT NOT NULL,
            vector TEXT NOT NULL, created_at REAL NOT NULL,
            PRIMARY KEY (scope, key)
        );",
    )
}

fn open_db(db: &FsPath) -> rusqlite::Result<Connection> {
    let conn = Connection::open(db)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}

/// Cosine similarity, 0 when either side is empty or the dimensions differ — a
/// model swap must not compare the old vectors to the new query.
pub(crate) fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut norm_a, mut norm_b) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// Rank documents against a query vector: best first, ties broken by key so the
/// same index always answers the same order.
fn ranked(query: &[f32], docs: &[(String, Vec<f32>)]) -> Vec<(f32, String)> {
    let mut scored: Vec<(f32, String)> = docs
        .iter()
        .filter(|(_, vector)| vector.len() == query.len())
        .map(|(key, vector)| (cosine(query, vector), key.clone()))
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });
    scored.truncate(MAX_HITS);
    scored
}

fn cached(db: &FsPath, scope: &str, key: &str, digest: &str) -> Option<Vec<f32>> {
    let conn = open_db(db).ok()?;
    let text: String = conn
        .query_row(
            "SELECT vector FROM embedding_cache WHERE scope=?1 AND key=?2 AND digest=?3",
            params![scope, key, digest],
            |row| row.get(0),
        )
        .ok()?;
    serde_json::from_str(&text).ok()
}

fn store(db: &FsPath, scope: &str, key: &str, digest: &str, vector: &[f32]) -> Result<(), String> {
    let conn = open_db(db).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO embedding_cache(scope,key,digest,vector,created_at) VALUES(?1,?2,?3,?4,?5) \
         ON CONFLICT(scope,key) DO UPDATE SET digest=?3, vector=?4, created_at=?5",
        params![
            scope,
            key,
            digest,
            serde_json::to_string(vector).unwrap_or_default(),
            crate::now_seconds()
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// One indexed symbol as the thing to embed. The digest folds the file's digest
/// and the symbol's name together, so an edited file re-embeds and a rename is a
/// new document instead of a stale vector under an old name.
#[derive(Debug)]
struct Doc {
    key: String,
    digest: String,
    text: String,
    path: String,
    line: i64,
    kind: String,
    name: String,
    language: String,
}

fn documents(db: &FsPath) -> Result<Vec<Doc>, String> {
    let conn = open_db(db).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT s.path, s.line, s.kind, s.name, f.language, f.digest
             FROM code_index_symbols s JOIN code_index_files f ON f.path = s.path
             ORDER BY s.path, s.line, s.name",
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
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut docs = Vec::new();
    for row in rows.flatten() {
        let (path, line, kind, name, language, file_digest) = row;
        let mut text = format!("{kind} {name} in {path} ({language})");
        if text.chars().count() > DOC_CHARS {
            text = text.chars().take(DOC_CHARS).collect();
        }
        docs.push(Doc {
            key: format!("{path}:{line}:{name}"),
            digest: format!("{file_digest}:{name}"),
            text,
            path,
            line,
            kind,
            name,
            language,
        });
    }
    Ok(docs)
}

/// One embeddings request. The provider's answer must line up with the inputs:
/// a short or empty `data` array is an error, never a silently shorter list.
async fn embed(state: &AppState, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let cfg = crate::load_config(state);
    if let Some(name) = local_model(model) {
        let base = crate::local::effective_base_url(&cfg.local_base_url);
        return crate::local::embed_texts(&base, &name, texts).await;
    }
    if cfg.api_key.trim().is_empty() {
        return Err("Add your router's unified API key in Settings first.".to_string());
    }
    let body = json!({ "model": model, "input": texts });
    let response = state
        .client
        .post(crate::router_url(&cfg, "embeddings"))
        .bearer_auth(&cfg.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|error| crate::reach_error(&cfg.base_url, error.to_string()))?;
    let status = response.status();
    let parsed: Value =
        serde_json::from_str(&response.text().await.unwrap_or_default()).unwrap_or(Value::Null);
    if !status.is_success() {
        let detail = parsed
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("The provider rejected the embeddings request.");
        return Err(detail.to_string());
    }
    let items = parsed
        .get("data")
        .and_then(Value::as_array)
        .ok_or("The provider returned no embeddings.")?;
    if items.len() != texts.len() {
        return Err(format!(
            "The provider returned {} embeddings for {} inputs.",
            items.len(),
            texts.len()
        ));
    }
    let mut vectors = Vec::with_capacity(items.len());
    for item in items {
        let vector: Vec<f32> = item
            .get("embedding")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_f64)
                    .map(|value| value as f32)
                    .collect()
            })
            .unwrap_or_default();
        if vector.is_empty() {
            return Err("The provider returned an empty embedding.".to_string());
        }
        vectors.push(vector);
    }
    Ok(vectors)
}

async fn search(state: &AppState, query: &str) -> Result<Value, (StatusCode, String)> {
    if query.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Type a question or a few words to search by meaning.".to_string(),
        ));
    }
    let cfg = crate::load_config(state);
    let model = cfg.embedding_model.trim().to_string();
    if model.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Set an embedding model before searching by meaning — `text-embedding-3-small` \
             through the router, or `local:nomic-embed-text` through a runtime on this machine. \
             The word search needs none."
                .to_string(),
        ));
    }
    let docs = documents(&state.database_path)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    if docs.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "The code index is empty — refresh it first.".to_string(),
        ));
    }
    // Every symbol already embedded is used as it is; the rest are embedded up to
    // the per-search budget, so a large index fills in across a few searches
    // instead of blocking on one long request.
    let mut vectors: HashMap<String, Vec<f32>> = HashMap::new();
    let mut missing: Vec<&Doc> = Vec::new();
    for doc in &docs {
        match cached(&state.database_path, &model, &doc.key, &doc.digest) {
            Some(vector) => {
                vectors.insert(doc.key.clone(), vector);
            }
            None => missing.push(doc),
        }
    }
    let cached_count = vectors.len();
    let mut embedded = 0usize;
    let budget = missing.len().min(EMBED_PER_SEARCH);
    for chunk in missing[..budget].chunks(BATCH) {
        let texts: Vec<String> = chunk.iter().map(|doc| doc.text.clone()).collect();
        let fresh = embed(state, &model, &texts)
            .await
            .map_err(|error| (StatusCode::BAD_GATEWAY, error))?;
        for (doc, vector) in chunk.iter().zip(fresh) {
            store(&state.database_path, &model, &doc.key, &doc.digest, &vector)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
            vectors.insert(doc.key.clone(), vector);
            embedded += 1;
        }
    }
    let pending = docs.len() - vectors.len();
    let query_vector = embed(state, &model, &[query.trim().to_string()])
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error))?
        .into_iter()
        .next()
        .unwrap_or_default();
    let pool: Vec<(String, Vec<f32>)> = docs
        .iter()
        .filter_map(|doc| {
            vectors
                .get(&doc.key)
                .map(|vector| (doc.key.clone(), vector.clone()))
        })
        .collect();
    let hits: Vec<Value> = ranked(&query_vector, &pool)
        .into_iter()
        .filter_map(|(score, key)| {
            let doc = docs.iter().find(|doc| doc.key == key)?;
            Some(json!({
                "path": doc.path,
                "line": doc.line,
                "kind": doc.kind,
                "name": doc.name,
                "language": doc.language,
                "score": (score * 100.0).round() as i64,
            }))
        })
        .collect();
    Ok(json!({
        "ok": true,
        "model": model,
        "hits": hits,
        "embedded": embedded,
        "cached": cached_count,
        "pending": pending,
    }))
}

#[derive(Deserialize)]
pub(crate) struct SemanticQuery {
    #[serde(default)]
    q: String,
}

pub(crate) async fn semantic_endpoint(
    State(state): State<AppState>,
    Query(query): Query<SemanticQuery>,
) -> Response<Body> {
    match search(&state, &query.q).await {
        Ok(payload) => Json(payload).into_response(),
        Err((status, message)) => crate::json_error(status, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn database(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "free-ai-studio-embeddings-test-{}-{name}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let conn = open_db(&path).unwrap();
        initialize(&conn).unwrap();
        // The documents come from the index's tables, so the test database needs
        // those as well as this module's cache.
        crate::index::initialize(&conn).unwrap();
        path
    }

    /// A real state for the open path: the config file names the model and the
    /// endpoint, and the router key is deliberately empty so a test that reaches
    /// a provider through it would fail.
    fn runtime_state(tag: &str, local_base_url: &str) -> (AppState, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "free-ai-studio-embedding-e2e-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let state = AppState {
            config_path: root.join("config.json"),
            database_path: root.join("studio.db"),
            static_dir: root.join("static"),
            workspace: crate::workspace::WorkspaceStore::load(root.join("workspace.json")),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap(),
            terminal: crate::terminal::hub(),
            events: crate::events::Hub::new(),
            models_cache: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        };
        crate::ensure_setup(&state).expect("setup");
        let mut cfg = crate::load_config(&state);
        cfg.local_base_url = local_base_url.to_string();
        cfg.embedding_model = "local:nomic-embed-text".to_string();
        cfg.api_key = String::new();
        crate::save_config_file(&state.config_path, &cfg).expect("config");
        (state, root)
    }

    fn indexed(db: &FsPath, path: &str, line: i64, name: &str, digest: &str) {
        let conn = open_db(db).unwrap();
        conn.execute(
            "INSERT INTO code_index_files(path,language,bytes,lines,digest,indexed_at) \
             VALUES(?1,'rust',10,1,?2,1.0)",
            params![path, digest],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO code_index_symbols(path,line,kind,name) VALUES(?1,?2,'fn',?3)",
            params![path, line, name],
        )
        .unwrap();
    }

    #[test]
    fn cosine_prefers_aligned_vectors_and_refuses_mismatched_dims() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
        assert_eq!(cosine(&[], &[]), 0.0);
    }

    #[test]
    fn ranking_orders_by_similarity_and_breaks_ties_by_key() {
        let query = vec![1.0, 0.0];
        let docs = vec![
            ("b".to_string(), vec![0.0, 1.0]),
            ("a".to_string(), vec![1.0, 0.0]),
            ("c".to_string(), vec![1.0, 0.0]),
            ("d".to_string(), vec![0.5, 0.5]),
        ];
        let order: Vec<String> = ranked(&query, &docs)
            .into_iter()
            .map(|(_, key)| key)
            .collect();
        assert_eq!(order, ["a", "c", "d", "b"]);
    }

    #[test]
    fn the_cache_answers_only_for_the_digest_it_stored() {
        let db = database("cache");
        store(&db, "model-a", "src/lib.rs:1:f", "d1", &[0.25, 0.5]).unwrap();
        assert_eq!(
            cached(&db, "model-a", "src/lib.rs:1:f", "d1"),
            Some(vec![0.25, 0.5])
        );
        // A changed file, a different model or an unembedded key all miss.
        assert_eq!(cached(&db, "model-a", "src/lib.rs:1:f", "d2"), None);
        assert_eq!(cached(&db, "model-b", "src/lib.rs:1:f", "d1"), None);
        assert_eq!(cached(&db, "model-a", "src/lib.rs:2:g", "d1"), None);
        // Re-storing the same key under a new digest replaces the old vector.
        store(&db, "model-a", "src/lib.rs:1:f", "d2", &[1.0, 0.0]).unwrap();
        assert_eq!(
            cached(&db, "model-a", "src/lib.rs:1:f", "d2"),
            Some(vec![1.0, 0.0])
        );
    }

    #[test]
    fn only_a_named_local_model_leaves_the_router() {
        assert_eq!(
            local_model("local:nomic-embed-text"),
            Some("nomic-embed-text".to_string())
        );
        assert_eq!(
            local_model("  LOCAL: nomic-embed-text  "),
            Some("nomic-embed-text".to_string())
        );
        // A bare prefix is not a model, and a router model is not local.
        assert_eq!(local_model("local:"), None);
        assert_eq!(local_model("local:   "), None);
        assert_eq!(local_model("text-embedding-3-small"), None);
        assert_eq!(local_model("mylocal:nomic"), None);
    }

    #[tokio::test]
    async fn a_local_model_embeds_through_the_runtime_with_no_router_key() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/api/embed",
            axum::routing::post(|Json(body): Json<Value>| async move {
                assert_eq!(body["model"], "nomic-embed-text");
                let inputs = body["input"].as_array().cloned().unwrap_or_default();
                let vectors: Vec<Value> = inputs
                    .iter()
                    .map(|input| {
                        if input.as_str().unwrap_or("").contains("login") {
                            json!([1.0, 0.0])
                        } else {
                            json!([0.0, 1.0])
                        }
                    })
                    .collect();
                Json(json!({ "embeddings": vectors }))
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let (state, root) = runtime_state("local", &format!("http://{address}"));
        indexed(&state.database_path, "src/auth.rs", 3, "login", "d1");
        indexed(&state.database_path, "src/session.rs", 9, "logout", "d2");

        let payload = search(&state, "auth login").await.expect("local search");
        assert_eq!(payload["model"], "local:nomic-embed-text");
        assert_eq!(payload["embedded"], 2, "{payload}");
        assert_eq!(payload["pending"], 0, "{payload}");
        let hits = payload["hits"].as_array().cloned().unwrap_or_default();
        assert_eq!(hits[0]["name"], "login", "{payload}");

        // The cache answers for an unchanged index: only the query is embedded.
        let again = search(&state, "auth login").await.expect("cached search");
        assert_eq!(again["cached"], 2, "{again}");
        assert_eq!(again["embedded"], 0, "{again}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_runtime_refusal_is_an_error_not_a_word_search() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/api/embed",
            axum::routing::post(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "the model failed to load" })),
                )
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let (state, root) = runtime_state("refusal", &format!("http://{address}"));
        indexed(&state.database_path, "src/auth.rs", 3, "login", "d1");
        let (status, message) = search(&state, "auth").await.expect_err("must fail");
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(message.contains("the model failed to load"), "{message}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn documents_name_the_symbol_its_file_and_its_digest() {
        let db = database("documents");
        indexed(&db, "src/auth.rs", 3, "login", "filedigest");
        let docs = documents(&db).unwrap();
        assert_eq!(docs.len(), 1, "{docs:?}");
        assert_eq!(docs[0].key, "src/auth.rs:3:login");
        assert_eq!(docs[0].digest, "filedigest:login");
        assert!(
            docs[0].text.contains("fn login in src/auth.rs"),
            "{:?}",
            docs[0].text
        );
    }
}
