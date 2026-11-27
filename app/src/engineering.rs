//! Autonomous engineering: understand the project, turn a request into a
//! pipeline, and deliver the result through the tools that already exist.
//!
//! This module is a *reader and a gate*, not a second orchestrator. A pipeline
//! is a mission (`multi_agent`) whose tasks are created through
//! `agent::create_run_with`; git work uses the same guarded operations the
//! Source control pane calls; checks run the project's own commands behind the
//! command permission; metrics are counted from rows that already exist. What
//! this layer adds is project understanding, requirement intake with acceptance
//! criteria, the autonomy policy, and a delivery step that only ever stages the
//! files the pipeline itself wrote.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::{Path as FsPath, PathBuf};

use crate::AppState;

/// Files this scan will look at before it stops. A monorepo with a million
/// files still answers quickly, and the answer says it was bounded.
const MAX_SCAN_FILES: usize = 20_000;
const MAX_SCAN_DEPTH: usize = 12;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const PROFILE_TTL_SECONDS: f64 = 300.0;
const CHECK_OUTPUT_CAP: usize = 8_000;
const CHECK_TIMEOUT_SECONDS: u64 = 180;

const SKIP_DIRS: [&str; 15] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
    ".gradle",
    "coverage",
    ".turbo",
    ".cache",
    ".idea",
    "Pods",
];

// Pipeline states (mission states are reused verbatim for started pipelines).
const NEEDS_INPUT: &str = "needs-input";
const DELIVERED: &str = "delivered";

fn now() -> f64 {
    crate::now_seconds()
}

fn db(state: &AppState) -> Result<Connection, String> {
    crate::open_db(&state.database_path).map_err(|error| format!("Database error: {error}"))
}

pub(crate) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS engineering_settings(
            id INTEGER PRIMARY KEY CHECK(id = 1),
            autonomy INTEGER NOT NULL DEFAULT 2,
            allow_branch INTEGER NOT NULL DEFAULT 1,
            allow_commit INTEGER NOT NULL DEFAULT 1,
            updated_at REAL NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS engineering_projects(
            path TEXT PRIMARY KEY,
            fingerprint TEXT NOT NULL,
            profile TEXT NOT NULL,
            analyzed_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS engineering_pipelines(
            id TEXT PRIMARY KEY,
            mission_id TEXT NOT NULL DEFAULT '',
            goal TEXT NOT NULL,
            autonomy INTEGER NOT NULL,
            questions TEXT NOT NULL DEFAULT '',
            acceptance TEXT NOT NULL DEFAULT '',
            profile TEXT NOT NULL DEFAULT '',
            branch TEXT NOT NULL DEFAULT '',
            baseline TEXT NOT NULL DEFAULT '',
            delivery TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL,
            updated_at REAL NOT NULL
        );
        CREATE TABLE IF NOT EXISTS engineering_checks(
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            pipeline TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL,
            status TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '',
            output TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL
        );",
    )
}

// ---------------------------------------------------------------- autonomy

#[derive(Clone, Copy)]
pub(crate) struct Autonomy {
    pub(crate) level: i64,
}

impl Autonomy {
    fn label(self) -> &'static str {
        match self.level {
            1 => "assisted",
            2 => "supervised",
            _ => "autonomous",
        }
    }
}

fn autonomy_of(state: &AppState) -> Autonomy {
    let level = db(state)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT autonomy FROM engineering_settings WHERE id=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .ok()
            .flatten()
        })
        .unwrap_or(2)
        .clamp(1, 3);
    Autonomy { level }
}

/// Whether a task must wait for a person. Level 1 gates every task, level 2
/// gates the tasks that change files, level 3 starts what the plan allows.
fn task_needs_approval(autonomy: i64, writes: bool) -> bool {
    match autonomy {
        1 => true,
        2 => writes,
        _ => false,
    }
}

fn save_autonomy(state: &AppState, input: &Value) -> Result<Autonomy, String> {
    let level = input
        .get("autonomy")
        .and_then(Value::as_i64)
        .unwrap_or(2)
        .clamp(1, 3);
    let conn = db(state)?;
    conn.execute(
        "INSERT INTO engineering_settings(id, autonomy, updated_at) VALUES(1, ?1, ?2) \
         ON CONFLICT(id) DO UPDATE SET autonomy=excluded.autonomy, \
         updated_at=excluded.updated_at",
        params![level, now()],
    )
    .map_err(|error| error.to_string())?;
    Ok(Autonomy { level })
}

// ---------------------------------------------------------------- profile

#[derive(Clone)]
pub(crate) struct Profile {
    files: usize,
    source_files: usize,
    test_files: usize,
    doc_files: usize,
    languages: Vec<(String, usize)>,
    manifests: Vec<String>,
    frameworks: Vec<String>,
    entry_points: Vec<String>,
    dependencies: usize,
    bounded: bool,
}

fn extension_language(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "rs" => "Rust",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" => "Kotlin",
        "swift" => "Swift",
        "c" | "h" => "C",
        "cc" | "cpp" | "hpp" => "C++",
        "cs" => "C#",
        "rb" => "Ruby",
        "php" => "PHP",
        "sh" => "Shell",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" => "CSS",
        "sql" => "SQL",
        "md" | "mdx" => "Markdown",
        "toml" => "TOML",
        "json" | "jsonc" => "JSON",
        "yaml" | "yml" => "YAML",
        _ => return None,
    })
}

fn is_test_path(relative: &str) -> bool {
    let lower = relative.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    lower.contains("/tests/")
        || lower.starts_with("tests/")
        || name.starts_with("test_")
        || name.ends_with("_test.rs")
        || name.ends_with("_test.py")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.ends_with("test.java")
        || name.ends_with("test.kt")
        || lower.contains("/__tests__/")
}

fn is_doc_path(relative: &str) -> bool {
    let lower = relative.to_ascii_lowercase();
    lower.starts_with("docs/")
        || lower.contains("/docs/")
        || lower.ends_with(".md")
        || lower.ends_with(".mdx")
        || lower.ends_with(".adoc")
}

/// Framework hints from the manifests' text. This is a grep over declared
/// dependencies, not a claim about what the code imports.
fn frameworks_in(manifests: &[(String, String)]) -> Vec<String> {
    let mut found: Vec<&'static str> = Vec::new();
    let push = |hit: bool, name: &'static str, found: &mut Vec<&'static str>| {
        if hit && !found.contains(&name) {
            found.push(name);
        }
    };
    for (name, text) in manifests {
        let lower = text.to_ascii_lowercase();
        if name.ends_with("Cargo.toml") {
            push(lower.contains("axum"), "Axum", &mut found);
            push(lower.contains("tauri"), "Tauri", &mut found);
            push(lower.contains("tokio"), "Tokio", &mut found);
            push(
                lower.contains("diesel") || lower.contains("sqlx"),
                "SQL database",
                &mut found,
            );
        }
        if name.ends_with("package.json") {
            push(lower.contains("\"react\""), "React", &mut found);
            push(lower.contains("\"next\""), "Next.js", &mut found);
            push(lower.contains("\"vue\""), "Vue", &mut found);
            push(lower.contains("\"svelte\""), "Svelte", &mut found);
            push(lower.contains("\"express\""), "Express", &mut found);
            push(
                lower.contains("\"react-native\""),
                "React Native",
                &mut found,
            );
        }
        if name.ends_with("pyproject.toml") || name.ends_with("requirements.txt") {
            push(lower.contains("fastapi"), "FastAPI", &mut found);
            push(lower.contains("django"), "Django", &mut found);
            push(lower.contains("flask"), "Flask", &mut found);
        }
        if name.ends_with("go.mod") {
            push(lower.contains("gin-gonic"), "Gin", &mut found);
            push(lower.contains("echo"), "Echo", &mut found);
        }
    }
    found.into_iter().map(str::to_string).collect()
}

fn count_dependencies(manifests: &[(String, String)]) -> usize {
    let mut total = 0;
    for (name, text) in manifests {
        if name.ends_with("Cargo.toml") {
            let mut in_deps = false;
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') {
                    in_deps = trimmed.contains("dependencies");
                    continue;
                }
                if in_deps && trimmed.contains('=') && !trimmed.starts_with('#') {
                    total += 1;
                }
            }
        } else if name.ends_with("package.json") {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                for key in ["dependencies", "devDependencies"] {
                    if let Some(map) = parsed.get(key).and_then(Value::as_object) {
                        total += map.len();
                    }
                }
            }
        } else if name.ends_with("requirements.txt") {
            total += text
                .lines()
                .filter(|line| {
                    let line = line.trim();
                    !line.is_empty() && !line.starts_with('#') && !line.starts_with('-')
                })
                .count();
        }
    }
    total
}

fn manifest_name(file: &str) -> bool {
    matches!(
        file,
        "Cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "requirements.txt"
            | "go.mod"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "Gemfile"
            | "composer.json"
            | "Dockerfile"
            | "docker-compose.yml"
            | "docker-compose.yaml"
    )
}

/// Walk the workspace once, bounded, and describe what is actually there.
pub(crate) fn scan(root: &FsPath) -> Result<Profile, String> {
    let mut files = 0usize;
    let mut source_files = 0usize;
    let mut test_files = 0usize;
    let mut doc_files = 0usize;
    let mut bounded = false;
    let mut languages: Vec<(String, usize)> = Vec::new();
    let mut manifests: Vec<String> = Vec::new();
    let mut manifest_bodies: Vec<(String, String)> = Vec::new();
    let mut entry_candidates: Vec<String> = Vec::new();

    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_SCAN_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if files >= MAX_SCAN_FILES {
                bounded = true;
                break;
            }
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if file_type.is_dir() {
                if SKIP_DIRS.contains(&name.as_str())
                    || name.starts_with('.') && depth == 0 && name != ".github"
                {
                    continue;
                }
                if name == ".github" {
                    stack.push((path.clone(), depth + 1));
                    continue;
                }
                stack.push((path, depth + 1));
                continue;
            }
            files += 1;
            let relative = path
                .strip_prefix(root)
                .map(|rest| rest.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if relative.is_empty() {
                continue;
            }
            let extension = path
                .extension()
                .map(|value| value.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if let Some(language) = extension_language(&extension) {
                match languages.iter_mut().find(|(known, _)| known == language) {
                    Some((_, count)) => *count += 1,
                    None => languages.push((language.to_string(), 1)),
                }
                if !matches!(language, "Markdown" | "TOML" | "JSON" | "YAML") {
                    source_files += 1;
                }
            }
            if is_test_path(&relative) {
                test_files += 1;
            }
            if is_doc_path(&relative) {
                doc_files += 1;
            }
            if manifest_name(&name) {
                if !manifests.contains(&relative) {
                    manifests.push(relative.clone());
                }
                if let Ok(meta) = entry.metadata() {
                    if meta.len() <= MAX_MANIFEST_BYTES {
                        if let Ok(text) = std::fs::read_to_string(&path) {
                            manifest_bodies.push((name.clone(), text));
                        }
                    }
                }
            }
            if matches!(
                relative.as_str(),
                "src/main.rs"
                    | "src/lib.rs"
                    | "src/App.tsx"
                    | "src/App.jsx"
                    | "src/index.ts"
                    | "src/index.js"
                    | "index.js"
                    | "main.py"
                    | "app.py"
                    | "manage.py"
                    | "main.go"
                    | "cmd/main.go"
                    | "src/main.ts"
            ) {
                entry_candidates.push(relative);
            }
        }
        if bounded {
            break;
        }
    }
    languages.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    manifest_bodies.sort_by(|a, b| a.0.cmp(&b.0));
    let frameworks = frameworks_in(&manifest_bodies);
    let dependencies = count_dependencies(&manifest_bodies);
    entry_candidates.sort();
    Ok(Profile {
        files,
        source_files,
        test_files,
        doc_files,
        languages,
        manifests,
        frameworks,
        entry_points: entry_candidates,
        dependencies,
        bounded,
    })
}

fn profile_json(
    profile: &Profile,
    git: &(String, String, bool),
    analyzed_at: f64,
    cached: bool,
) -> Value {
    json!({
        "files": profile.files,
        "sourceFiles": profile.source_files,
        "testFiles": profile.test_files,
        "docFiles": profile.doc_files,
        "languages": profile.languages.iter().map(|(name, count)| json!({ "name": name, "files": count })).collect::<Vec<_>>(),
        "manifests": profile.manifests,
        "frameworks": profile.frameworks,
        "entryPoints": profile.entry_points,
        "dependencies": profile.dependencies,
        "bounded": profile.bounded,
        "branch": git.0,
        "commit": git.1,
        "dirty": git.2,
        "analyzedAt": analyzed_at,
        "cached": cached,
    })
}

fn fingerprint(git: &(String, String, bool), profile: &Profile) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        git.0, git.1, git.2, profile.files, profile.source_files
    )
}

/// The cached project profile, refreshed when the workspace's fingerprint
/// moved or the cached answer is old. A non-repository workspace is always
/// rescanned: without HEAD there is nothing stable to key the cache on.
fn project_profile(state: &AppState) -> Result<(Value, Profile), String> {
    let root = state
        .workspace
        .root()
        .map_err(|_| "Open a workspace folder first.".to_string())?;
    let git = crate::git::head_info(&root);
    let key = root.to_string_lossy().to_string();
    if let Ok(conn) = db(state) {
        if let Ok(Some((stored, body, analyzed_at))) = conn
            .query_row(
                "SELECT fingerprint, profile, analyzed_at FROM engineering_projects WHERE path=?1",
                [&key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, f64>(2)?,
                    ))
                },
            )
            .optional()
        {
            if !git.1.is_empty() && now() - analyzed_at < PROFILE_TTL_SECONDS {
                if let Ok(mut parsed) = serde_json::from_str::<Value>(&body) {
                    let profile = Profile {
                        files: parsed["files"].as_u64().unwrap_or(0) as usize,
                        source_files: parsed["sourceFiles"].as_u64().unwrap_or(0) as usize,
                        test_files: parsed["testFiles"].as_u64().unwrap_or(0) as usize,
                        doc_files: parsed["docFiles"].as_u64().unwrap_or(0) as usize,
                        languages: parsed["languages"]
                            .as_array()
                            .map(|entries| {
                                entries
                                    .iter()
                                    .map(|entry| {
                                        (
                                            entry["name"].as_str().unwrap_or("").to_string(),
                                            entry["files"].as_u64().unwrap_or(0) as usize,
                                        )
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        manifests: parsed["manifests"]
                            .as_array()
                            .map(|entries| {
                                entries
                                    .iter()
                                    .filter_map(|entry| entry.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        frameworks: parsed["frameworks"]
                            .as_array()
                            .map(|entries| {
                                entries
                                    .iter()
                                    .filter_map(|entry| entry.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        entry_points: Vec::new(),
                        dependencies: parsed["dependencies"].as_u64().unwrap_or(0) as usize,
                        bounded: parsed["bounded"].as_bool().unwrap_or(false),
                    };
                    let expected = fingerprint(&git, &profile);
                    if expected == stored {
                        parsed["cached"] = json!(true);
                        parsed["branch"] = json!(git.0);
                        parsed["commit"] = json!(git.1);
                        parsed["dirty"] = json!(git.2);
                        return Ok((parsed, profile));
                    }
                }
            }
        }
    }
    let profile = scan(&root)?;
    let body = profile_json(&profile, &git, now(), false);
    if let Ok(conn) = db(state) {
        let _ = conn.execute(
            "INSERT INTO engineering_projects(path, fingerprint, profile, analyzed_at) \
             VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET \
             fingerprint=excluded.fingerprint, profile=excluded.profile, \
             analyzed_at=excluded.analyzed_at",
            params![
                key,
                fingerprint(&git, &profile),
                serde_json::to_string(&body).unwrap_or_default(),
                now()
            ],
        );
    }
    Ok((body, profile))
}

// ---------------------------------------------------------------- intake

#[derive(Clone)]
pub(crate) struct Requirement {
    pub(crate) questions: Vec<String>,
    pub(crate) acceptance: Vec<String>,
    pub(crate) risks: Vec<String>,
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

fn looks_vague(goal: &str) -> bool {
    let lower = goal.to_ascii_lowercase();
    let vague = [
        "improve",
        "clean up",
        "make it better",
        "fix stuff",
        "refactor everything",
        "optimize everything",
        "modernize",
        "polish",
    ];
    vague.iter().any(|word| lower.contains(word))
        && ![
            "file",
            "page",
            "test",
            "api",
            "module",
            "function",
            "endpoint",
            "component",
            "docs",
        ]
        .iter()
        .any(|word| lower.contains(word))
}

/// What a request must say before an agent may change code. These are
/// heuristics about the sentence, not a model's opinion, and they are only
/// raised when the answer would genuinely change the plan.
pub(crate) fn analyse_requirement(goal: &str, profile: Option<&Profile>) -> Requirement {
    let trimmed = goal.trim();
    let mut questions = Vec::new();
    if word_count(trimmed) < 4 {
        questions.push(
            "What exactly should change, and where? A sentence naming the behaviour and the area is enough."
                .to_string(),
        );
    }
    if looks_vague(trimmed) {
        questions.push(
            "Which files or features does this cover? \"Improve\" needs a target before an agent can plan it."
                .to_string(),
        );
    }
    let mut acceptance = vec![
        "The requested behaviour is implemented in the open workspace.".to_string(),
        "The project's check/build command succeeds after the change.".to_string(),
        "The project's test command succeeds, or the delivery report says why it could not run."
            .to_string(),
        "Documentation covering the changed area is updated.".to_string(),
    ];
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("bug") || lower.contains("fix") || lower.contains("crash") {
        acceptance.push(
            "The failure is reproduced before the fix and no longer reproduces after it."
                .to_string(),
        );
    }
    if lower.contains("api") || lower.contains("endpoint") || lower.contains("route") {
        acceptance.push(
            "The public interface the request names still behaves as documented.".to_string(),
        );
    }
    let mut risks = Vec::new();
    if let Some(profile) = profile {
        if profile.test_files == 0 {
            risks.push(
                "No test files were detected, so verification depends on the project's own commands and a review pass."
                    .to_string(),
            );
        }
        if profile.source_files > 5_000 {
            risks
                .push("The workspace is large; analysis and review are bounded scans.".to_string());
        }
        if profile.manifests.is_empty() {
            risks.push(
                "No build manifest was found, so check commands may be unavailable.".to_string(),
            );
        }
    }
    Requirement {
        questions,
        acceptance,
        risks,
    }
}

// ---------------------------------------------------------------- pipelines

fn engineering_plan(goal: &str, profile: &Profile, autonomy: i64) -> Vec<Value> {
    let facts = format!(
        "Workspace facts: {} files ({} source, {} tests, {} docs); languages: {}; frameworks: {}; \
         manifests: {}; declared dependencies: {}.",
        profile.files,
        profile.source_files,
        profile.test_files,
        profile.doc_files,
        profile
            .languages
            .iter()
            .take(4)
            .map(|(name, count)| format!("{name} ({count})"))
            .collect::<Vec<_>>()
            .join(", "),
        if profile.frameworks.is_empty() {
            "none detected".to_string()
        } else {
            profile.frameworks.join(", ")
        },
        if profile.manifests.is_empty() {
            "none".to_string()
        } else {
            profile.manifests.join(", ")
        },
        profile.dependencies
    );
    let specs = vec![
        json!({
            "id": "analyse",
            "role": "architect",
            "title": "Analyse the project",
            "goal": format!("{facts} Read the files this request will touch and report the build and test commands, the current structure, and the risks of changing it."),
        }),
        json!({
            "id": "plan",
            "role": "planner",
            "title": "Plan the implementation",
            "dependsOn": ["analyse"],
            "goal": format!("Turn this request into ordered steps with the files to change: {goal}. Use the analysis in the previous task."),
        }),
        json!({
            "id": "implement",
            "role": "coding",
            "title": "Implement the change",
            "dependsOn": ["plan"],
        }),
        json!({
            "id": "verify",
            "role": "testing",
            "title": "Run the project's checks",
            "dependsOn": ["implement"],
        }),
        json!({
            "id": "document",
            "role": "documentation",
            "title": "Update the documentation",
            "dependsOn": ["implement"],
        }),
        json!({
            "id": "review",
            "role": "review",
            "title": "Review the change",
            "dependsOn": ["verify", "document"],
        }),
    ];
    let mut specs = specs;
    for task in specs.iter_mut() {
        let writes = matches!(
            task["role"].as_str().unwrap_or(""),
            "coding" | "documentation"
        );
        task["needsApproval"] = json!(task_needs_approval(autonomy, writes));
    }
    specs
}

fn pipeline_from_row(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "missionId": row.get::<_, String>(1)?,
        "goal": row.get::<_, String>(2)?,
        "autonomy": row.get::<_, i64>(3)?,
        "questions": serde_json::from_str::<Value>(&row.get::<_, String>(4)?).unwrap_or(json!([])),
        "acceptance": serde_json::from_str::<Value>(&row.get::<_, String>(5)?).unwrap_or(json!([])),
        "profile": serde_json::from_str::<Value>(&row.get::<_, String>(6)?).unwrap_or(Value::Null),
        "branch": row.get::<_, String>(7)?,
        "baseline": row.get::<_, String>(8)?,
        "delivery": serde_json::from_str::<Value>(&row.get::<_, String>(9)?).unwrap_or(Value::Null),
        "createdAt": row.get::<_, f64>(10)?,
        "updatedAt": row.get::<_, f64>(11)?,
    }))
}

const PIPELINE_SQL: &str =
    "SELECT id, mission_id, goal, autonomy, questions, acceptance, profile, \
 branch, baseline, delivery, created_at, updated_at FROM engineering_pipelines";

fn load_pipeline(state: &AppState, id: &str) -> Result<Option<Value>, String> {
    let conn = db(state)?;
    conn.query_row(
        &format!("{PIPELINE_SQL} WHERE id=?1"),
        [id],
        pipeline_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

fn store_pipeline(state: &AppState, row: &Value) -> Result<(), String> {
    let conn = db(state)?;
    conn.execute(
        "INSERT INTO engineering_pipelines(id, mission_id, goal, autonomy, questions, acceptance, \
         profile, branch, baseline, delivery, created_at, updated_at) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11) \
         ON CONFLICT(id) DO UPDATE SET mission_id=excluded.mission_id, questions=excluded.questions, \
         acceptance=excluded.acceptance, profile=excluded.profile, branch=excluded.branch, \
         baseline=excluded.baseline, delivery=excluded.delivery, updated_at=excluded.updated_at",
        params![
            row["id"].as_str().unwrap_or(""),
            row["missionId"].as_str().unwrap_or(""),
            row["goal"].as_str().unwrap_or(""),
            row["autonomy"].as_i64().unwrap_or(2),
            row["questions"].to_string(),
            row["acceptance"].to_string(),
            row["profile"].to_string(),
            row["branch"].as_str().unwrap_or(""),
            row["baseline"].as_str().unwrap_or(""),
            row["delivery"].to_string(),
            row["createdAt"].as_f64().unwrap_or_else(now),
        ],
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn pipeline_status(state: &AppState, pipeline: &Value) -> String {
    if !pipeline["delivery"].is_null() {
        return DELIVERED.to_string();
    }
    let mission_id = pipeline["missionId"].as_str().unwrap_or("");
    if mission_id.is_empty() {
        return NEEDS_INPUT.to_string();
    }
    match crate::multi_agent::mission_json_for(state, mission_id) {
        Some(mission) => mission["status"].as_str().unwrap_or("running").to_string(),
        None => "unknown".to_string(),
    }
}

/// Which acceptance criteria have evidence, and which are still promises.
/// "Verified" requires a real verifier or command step; a succeeded run alone
/// is not evidence that a command ran.
fn trace(state: &AppState, pipeline: &Value) -> Value {
    let mission = pipeline["missionId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .and_then(|id| crate::multi_agent::mission_json_for(state, id));
    let tasks: Vec<Value> = mission
        .as_ref()
        .and_then(|mission| mission["tasks"].as_array().cloned())
        .unwrap_or_default();
    let report = mission
        .as_ref()
        .map(|mission| mission["report"].clone())
        .unwrap_or(Value::Null);
    let commands: Vec<String> = report["commandsRun"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let checks: Vec<String> = report["checks"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let wrote_files = report["filesWritten"]
        .as_array()
        .map(|entries| !entries.is_empty())
        .unwrap_or(false);
    let mission_status = mission
        .as_ref()
        .and_then(|mission| mission["status"].as_str())
        .unwrap_or("planned")
        .to_string();
    let finished = !matches!(
        mission_status.as_str(),
        "planned" | "running" | "paused" | "awaiting-approval"
    );
    let running = tasks
        .iter()
        .filter(|task| task["status"] == json!("running"))
        .count();
    let all_done = !tasks.is_empty() && tasks.iter().all(|task| task["status"] == json!("done"));
    let done: Vec<&str> = tasks
        .iter()
        .filter(|task| task["status"] == json!("done"))
        .filter_map(|task| task["id"].as_str())
        .collect();
    let evidence_for = |criterion: &str| -> (String, String, Vec<String>) {
        let lower = criterion.to_ascii_lowercase();
        let mut source = Vec::new();
        if lower.contains("test") || lower.contains("check") || lower.contains("build") {
            source.extend(commands.clone());
            source.extend(checks.clone());
        }
        if lower.contains("documentation") {
            source.extend(
                tasks
                    .iter()
                    .filter(|task| {
                        task["role"] == json!("documentation")
                            && matches!(task["status"].as_str(), Some("done") | Some("running"))
                    })
                    .map(|task| {
                        format!(
                            "documentation task {}",
                            task["status"].as_str().unwrap_or("unknown")
                        )
                    }),
            );
        }
        if lower.contains("behaviour") || lower.contains("implemented") {
            source.extend(
                tasks
                    .iter()
                    .filter(|task| {
                        (task["role"] == json!("coding") || task["role"] == json!("review"))
                            && matches!(task["status"].as_str(), Some("done") | Some("running"))
                    })
                    .map(|task| {
                        format!(
                            "{} task {}",
                            task["roleName"].as_str().unwrap_or("agent"),
                            task["status"].as_str().unwrap_or("unknown")
                        )
                    }),
            );
        }
        if lower.contains("reproduced") {
            source.extend(
                tasks
                    .iter()
                    .filter(|task| {
                        task["role"] == json!("debugging")
                            && matches!(task["status"].as_str(), Some("done") | Some("running"))
                    })
                    .map(|task| {
                        format!(
                            "debugging task {}",
                            task["status"].as_str().unwrap_or("unknown")
                        )
                    }),
            );
        }
        let has_source = !source.is_empty();
        // Only a finished mission whose stages all succeeded can verify a
        // criterion, and only when it produced an artifact or ran a command. A
        // cancelled or failed pipeline says so instead of leaving a criterion
        // "in progress" for ever.
        let status = if finished && all_done && (wrote_files || !commands.is_empty()) {
            "verified"
        } else if finished && all_done {
            "no-artifact"
        } else if finished {
            "not-verified"
        } else if has_source || running > 0 {
            "in-progress"
        } else {
            "not-verified"
        };
        (
            status.to_string(),
            if has_source {
                "observed".to_string()
            } else {
                "none".to_string()
            },
            source,
        )
    };
    let criteria: Vec<Value> = pipeline["acceptance"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .map(|criterion| {
                    let (status, evidence_kind, mut sources) = evidence_for(&criterion);
                    sources.truncate(4);
                    json!({
                        "criterion": criterion,
                        "status": status,
                        "evidence": evidence_kind,
                        "sources": sources,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "acceptance": criteria,
        "doneTasks": done,
        "missionStatus": mission_status,
        "wroteFiles": wrote_files,
    })
}

fn pipeline_json(state: &AppState, pipeline: &Value) -> Value {
    let mut payload = pipeline.clone();
    let status = pipeline_status(state, pipeline);
    payload["status"] = json!(status);
    let mission = pipeline["missionId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .and_then(|id| crate::multi_agent::mission_json_for(state, id));
    if let Some(mission) = &mission {
        payload["mission"] = mission.clone();
        payload["counts"] = mission["counts"].clone();
        payload["report"] = mission["report"].clone();
        payload["tasks"] = mission["tasks"].clone();
    } else {
        payload["counts"] =
            json!({ "total": 0, "done": 0, "running": 0, "failed": 0, "blocked": 0 });
    }
    payload["trace"] = trace(state, pipeline);
    payload
}

fn written_paths(report: &Value) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    if let Some(entries) = report["filesWritten"].as_array() {
        for entry in entries {
            let Some(text) = entry.as_str() else {
                continue;
            };
            let Some(rest) = text.strip_prefix("write ") else {
                continue;
            };
            let path = match rest.rfind(" (") {
                Some(index) if rest[index..].ends_with(" bytes)") => &rest[..index],
                _ => rest,
            };
            let path = path.trim().to_string();
            if !path.is_empty() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    paths
}

fn commit_message(goal: &str, mission: &Value) -> String {
    let subject: String = goal
        .lines()
        .next()
        .unwrap_or("Engineering change")
        .trim()
        .chars()
        .take(68)
        .collect();
    let mut message = format!("{subject}\n\n");
    if let Some(tasks) = mission["tasks"].as_array() {
        for task in tasks {
            if task["status"] != json!("done") {
                continue;
            }
            let summary = task["summary"].as_str().unwrap_or("").trim();
            if summary.is_empty() {
                continue;
            }
            let role = task["roleName"].as_str().unwrap_or("agent");
            message.push_str(&format!("- {role}: {}\n", summary.replace('\n', " ")));
        }
    }
    if let Some(mission_id) = mission["id"].as_str() {
        message.push_str(&format!("\nMission: {mission_id}\n"));
    }
    message.trim_end().to_string()
}

/// A bounded repair loop: diagnose, fix, prove, review. The mission's retry
/// policy and the run step budget are the only loops; there is no unbounded
/// "try again" path.
fn repair_plan(autonomy: i64) -> Vec<Value> {
    let mut plan = vec![
        json!({ "id": "diagnose", "role": "debugging", "title": "Diagnose the failure" }),
        json!({ "id": "fix", "role": "coding", "title": "Fix the verified cause", "dependsOn": ["diagnose"] }),
        json!({ "id": "verify", "role": "testing", "title": "Prove the fix", "dependsOn": ["fix"] }),
        json!({ "id": "review", "role": "review", "title": "Review the fix", "dependsOn": ["fix", "verify"] }),
    ];
    for task in plan.iter_mut() {
        let writes = task["role"] == json!("coding");
        task["needsApproval"] = json!(task_needs_approval(autonomy, writes));
    }
    plan
}

// ---------------------------------------------------------------- checks

fn classify_failure(text: &str) -> (&'static str, &'static str) {
    let lower = text.to_ascii_lowercase();
    if lower.contains("cannot find package")
        || lower.contains("failed to resolve")
        || lower.contains("no matching package")
        || lower.contains("could not find `")
        || lower.contains("module not found")
    {
        (
            "dependency",
            "A dependency is missing or unresolvable. Fetch it or fix the manifest, then rerun the check.",
        )
    } else if lower.contains("error[e")
        || lower.contains("error: expected")
        || lower.contains("error[")
        || lower.contains("error ts")
        || lower.contains("syntaxerror")
    {
        (
            "compile",
            "The code does not compile. Fix the reported error at the file and line it names.",
        )
    } else if lower.contains("panicked at")
        || lower.contains("traceback")
        || lower.contains("exception")
    {
        (
            "runtime",
            "Something panicked or raised at runtime. Reproduce the failing case before changing code.",
        )
    } else if lower.contains("permission denied") || lower.contains("read-only file system") {
        (
            "environment",
            "A permission blocked the command. Fix the file mode or run in a writable folder.",
        )
    } else if lower.contains("not found") && lower.contains("command") {
        (
            "environment",
            "The toolchain for this project is not installed. Install it or point the project at one that exists.",
        )
    } else if lower.contains("timed out")
        || lower.contains("connection refused")
        || lower.contains("dns")
    {
        (
            "network",
            "The failure is outside the code: the network or a provider is unreachable.",
        )
    } else {
        (
            "unknown",
            "The failure was not recognised. Read the output before changing anything.",
        )
    }
}

fn first_lines(text: &str, limit: usize) -> String {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .take(limit)
        .collect::<Vec<_>>()
        .join("\n")
}

async fn run_command_check(root: &FsPath, program: &str, args: &[&str]) -> Value {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let started = std::time::Instant::now();
    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(CHECK_TIMEOUT_SECONDS),
        command.output(),
    )
    .await;
    match outcome {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let combined = format!("{stdout}\n{stderr}");
            let ok = output.status.success();
            let (kind, suggestion) = if ok {
                ("ok", "")
            } else {
                classify_failure(&combined)
            };
            json!({
                "command": format!("{program} {}", args.join(" ")),
                "ok": ok,
                "kind": kind,
                "suggestion": suggestion,
                "summary": first_lines(&combined, 12),
                "output": combined.chars().take(CHECK_OUTPUT_CAP).collect::<String>(),
                "elapsedMs": started.elapsed().as_millis() as i64,
            })
        }
        Ok(Err(error)) => json!({
            "command": format!("{program} {}", args.join(" ")),
            "ok": false,
            "kind": "environment",
            "suggestion": format!("The command could not start: {error}. Install the toolchain or run it yourself."),
            "summary": error.to_string(),
            "output": "",
            "elapsedMs": started.elapsed().as_millis() as i64,
        }),
        Err(_) => json!({
            "command": format!("{program} {}", args.join(" ")),
            "ok": false,
            "kind": "timeout",
            "suggestion": "The check did not finish in time. Run it yourself or raise the budget.",
            "summary": format!("timed out after {CHECK_TIMEOUT_SECONDS}s"),
            "output": "",
            "elapsedMs": started.elapsed().as_millis() as i64,
        }),
    }
}

async fn checks(state: &AppState, approve: bool) -> Result<Value, (StatusCode, String)> {
    let root = state.workspace.root().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "Open a workspace folder first.".to_string(),
        )
    })?;
    if let Some(response) = crate::permissions::guard_user(
        state,
        crate::permissions::Action::Command,
        "engineering checks",
        "",
        approve,
        "run the project's check command",
    ) {
        let _ = response;
        return Err((
            StatusCode::CONFLICT,
            "Running check commands asks first: save a command rule for \"engineering checks\" or pass approve."
                .to_string(),
        ));
    }
    let mut ran = Vec::new();
    let mut skipped = Vec::new();
    if root.join("Cargo.toml").is_file() {
        ran.push(run_command_check(&root, "cargo", &["check", "--quiet"]).await);
    } else {
        skipped.push("cargo check (no Cargo.toml)".to_string());
    }
    if root.join("package.json").is_file() {
        skipped.push(
            "npm checks (a JavaScript project's scripts are arbitrary commands; run them from the project's own tooling)"
                .to_string(),
        );
    }
    let issues: Vec<Value> = ran
        .iter()
        .filter(|result| result["ok"] == json!(false))
        .map(|result| {
            json!({
                "kind": result["kind"],
                "summary": result["summary"],
                "suggestion": result["suggestion"],
                "command": result["command"],
            })
        })
        .collect();
    Ok(json!({
        "ok": issues.is_empty(),
        "ran": ran,
        "skipped": skipped,
        "issues": issues,
        "workspace": root.to_string_lossy(),
    }))
}

// ---------------------------------------------------------------- metrics

fn count(conn: &Connection, sql: &str, since: f64) -> i64 {
    conn.query_row(sql, params![since], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
}

fn coverage_from(root: &FsPath) -> Option<(f64, String)> {
    for candidate in [
        "coverage/lcov.info",
        "lcov.info",
        "target/coverage/lcov.info",
        "coverage/lcov-report/lcov.info",
    ] {
        let path = root.join(candidate);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut found = 0f64;
        let mut hit = 0f64;
        for line in text.lines() {
            if let Some(value) = line.strip_prefix("LF:") {
                found += value.trim().parse::<f64>().unwrap_or(0.0);
            } else if let Some(value) = line.strip_prefix("LH:") {
                hit += value.trim().parse::<f64>().unwrap_or(0.0);
            }
        }
        if found > 0.0 {
            return Some((
                (hit / found * 100.0 * 10.0).round() / 10.0,
                candidate.to_string(),
            ));
        }
    }
    None
}

fn metrics(state: &AppState) -> Value {
    let (profile, scanned) = match project_profile(state) {
        Ok((json, profile)) => (json, profile),
        Err(_) => (
            Value::Null,
            Profile {
                files: 0,
                source_files: 0,
                test_files: 0,
                doc_files: 0,
                languages: Vec::new(),
                manifests: Vec::new(),
                frameworks: Vec::new(),
                entry_points: Vec::new(),
                dependencies: 0,
                bounded: false,
            },
        ),
    };
    let root = state.workspace.root().ok();
    let coverage = root.as_deref().and_then(coverage_from);
    let mut out = json!({
        "measured": {},
        "unavailable": [],
    });
    if let Ok(conn) = db(state) {
        let since = now() - 30.0 * 24.0 * 3600.0;
        let runs = count(
            &conn,
            "SELECT COUNT(*) FROM agent_runs WHERE created_at >= ?1",
            since,
        );
        let runs_ok = count(
            &conn,
            "SELECT COUNT(*) FROM agent_runs WHERE created_at >= ?1 AND status='succeeded'",
            since,
        );
        let runs_failed = count(
            &conn,
            "SELECT COUNT(*) FROM agent_runs WHERE created_at >= ?1 AND status IN ('failed','interrupted')",
            since,
        );
        let missions = count(
            &conn,
            "SELECT COUNT(*) FROM missions WHERE created_at >= ?1",
            since,
        );
        let missions_ok = count(
            &conn,
            "SELECT COUNT(*) FROM missions WHERE created_at >= ?1 AND status='succeeded'",
            since,
        );
        let tasks = count(
            &conn,
            "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id = mt.mission_id WHERE m.created_at >= ?1",
            since,
        );
        let tasks_done = count(
            &conn,
            "SELECT COUNT(*) FROM mission_tasks mt JOIN missions m ON m.id = mt.mission_id WHERE m.created_at >= ?1 AND mt.status='done'",
            since,
        );
        let retries = count(
            &conn,
            "SELECT COUNT(*) FROM mission_messages WHERE created_at >= ?1 AND body LIKE 'Retrying after failure%'",
            since,
        );
        let verifications = count(
            &conn,
            "SELECT COUNT(*) FROM agent_steps WHERE created_at >= ?1 AND action='verify'",
            since,
        );
        let test_commands = count(
            &conn,
            "SELECT COUNT(*) FROM agent_steps WHERE created_at >= ?1 AND action='command' AND \
             (lower(summary) LIKE '%test%' OR lower(summary) LIKE '%check%' OR lower(summary) LIKE '%build%')",
            since,
        );
        let failovers = count(
            &conn,
            "SELECT COUNT(*) FROM agent_steps WHERE created_at >= ?1 AND action IN ('failover','escalate')",
            since,
        );
        let pipelines = count(
            &conn,
            "SELECT COUNT(*) FROM engineering_pipelines WHERE created_at >= ?1",
            since,
        );
        out["measured"] = json!({
            "windowDays": 30,
            "runs": { "total": runs, "succeeded": runs_ok, "failed": runs_failed },
            "missions": { "total": missions, "succeeded": missions_ok },
            "tasks": { "total": tasks, "done": tasks_done },
            "retries": retries,
            "verifications": verifications,
            "testCommands": test_commands,
            "failovers": failovers,
            "pipelines": pipelines,
        });
    } else {
        out["unavailable"] = json!(["run, mission and step history (database unavailable)"]);
    }
    out["project"] = profile;
    out["coverage"] = match coverage {
        Some((percent, source)) => json!({ "percent": percent, "source": source }),
        None => {
            let mut unavailable: Vec<Value> =
                out["unavailable"].as_array().cloned().unwrap_or_default();
            unavailable.push(json!(
                "coverage (no lcov.info in coverage/, target/coverage/ or the workspace root)"
            ));
            out["unavailable"] = Value::Array(unavailable);
            Value::Null
        }
    };
    out["security"] = Value::Null;
    out["notes"] = json!([
        "Test command count is derived from command steps whose summary mentions test, check or build; it is not a parsed test report.",
        format!(
            "{} source files vs {} documentation files in the workspace (scan bounded at {}).",
            scanned.source_files, scanned.doc_files, MAX_SCAN_FILES
        ),
        "No security scanner is configured, so security findings are reported as unavailable rather than estimated.",
    ]);
    out
}

// ---------------------------------------------------------------- api

fn bad(message: impl Into<String>) -> Response {
    crate::json_error(StatusCode::BAD_REQUEST, message)
}

async fn project(State(state): State<AppState>) -> Response {
    match project_profile(&state) {
        Ok((profile, _)) => Json(json!({ "project": profile })).into_response(),
        Err(error) => bad(error),
    }
}

async fn analyse(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let goal = input
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if goal.is_empty() {
        return bad("Describe what you want built or fixed.");
    }
    let profile = project_profile(&state).ok().map(|(_, profile)| profile);
    let requirement = analyse_requirement(&goal, profile.as_ref());
    Json(json!({
        "goal": goal,
        "questions": requirement.questions,
        "acceptance": requirement.acceptance,
        "risks": requirement.risks,
        "ready": requirement.questions.is_empty(),
    }))
    .into_response()
}

async fn create_pipeline(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let goal = input
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let autonomy = input
        .get("autonomy")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| autonomy_of(&state).level)
        .clamp(1, 3);
    match create_for_goal(&state, &goal, autonomy).await {
        Ok((pipeline, needs_input, risks)) => Json(json!({
            "ok": true,
            "needsInput": needs_input,
            "pipeline": pipeline,
            "risks": risks,
        }))
        .into_response(),
        Err((code, message)) => crate::json_error(code, message),
    }
}

/// A pipeline created by another module (self-improvement) rather than the
/// route, with the same validation, profile, plan and audit.
pub(crate) async fn create_for_goal(
    state: &AppState,
    goal: &str,
    autonomy: i64,
) -> Result<(Value, bool, Vec<String>), (StatusCode, String)> {
    let goal = goal.trim().to_string();
    if goal.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Describe what you want built or fixed.".to_string(),
        ));
    }
    if goal.chars().count() > 5_000 {
        return Err((
            StatusCode::BAD_REQUEST,
            "The request is too long.".to_string(),
        ));
    }
    let (profile_json, profile) = project_profile(state).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            if error.contains("Open a workspace") {
                "Open a workspace folder first — a pipeline works on its files.".to_string()
            } else {
                error
            },
        )
    })?;
    if state.workspace.root().is_err() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Open a workspace folder first — a pipeline works on its files.".to_string(),
        ));
    }
    let autonomy = autonomy.clamp(1, 3);
    let requirement = analyse_requirement(&goal, Some(&profile));
    let git = crate::git::head_info(
        &state
            .workspace
            .root()
            .map(|root| root.to_path_buf())
            .unwrap_or_default(),
    );
    let baseline = format!("{}@{}", git.0, git.1);
    let id = uuid::Uuid::new_v4().to_string();
    let mut row = json!({
        "id": id,
        "missionId": "",
        "goal": goal,
        "autonomy": autonomy,
        "questions": requirement.questions,
        "acceptance": requirement.acceptance,
        "profile": profile_json,
        "branch": git.0,
        "baseline": baseline,
        "delivery": Value::Null,
        "createdAt": now(),
        "updatedAt": now(),
    });
    if !requirement.questions.is_empty() {
        store_pipeline(state, &row).ok();
        return Ok((pipeline_json(state, &row), true, requirement.risks));
    }
    start_pipeline(state, &mut row, &goal, autonomy, &profile, &requirement).await?;
    Ok((pipeline_json(state, &row), false, requirement.risks))
}

/// The cached project profile as the API sees it, for modules that must not
/// re-scan the workspace.
pub(crate) fn profile_view(state: &AppState) -> Option<Value> {
    project_profile(state).ok().map(|(json, _)| json)
}

/// A pipeline as the API sees it, for modules that must not reimplement the
/// store (the self-improvement proposals read its delivery commit).
pub(crate) fn pipeline_view(state: &AppState, id: &str) -> Option<Value> {
    load_pipeline(state, id)
        .ok()
        .flatten()
        .map(|row| pipeline_json(state, &row))
}

/// Record a proposal's delivery branch and commit by asking git, not by
/// guessing from the pipeline row.
pub(crate) fn delivery_of(pipeline: &Value) -> Option<(String, String)> {
    let delivery = pipeline.get("delivery")?.as_object()?;
    let commit = delivery.get("commit")?.as_str().unwrap_or("").to_string();
    let branch = delivery.get("branch")?.as_str().unwrap_or("").to_string();
    (!commit.is_empty()).then_some((commit, branch))
}

async fn start_pipeline(
    state: &AppState,
    row: &mut Value,
    goal: &str,
    autonomy: i64,
    profile: &Profile,
    requirement: &Requirement,
) -> Result<(), (StatusCode, String)> {
    let writes = json!(engineering_plan(goal, profile, autonomy))
        .as_array()
        .map(|tasks| {
            tasks.iter().any(|task| {
                matches!(
                    task["role"].as_str().unwrap_or(""),
                    "coding" | "documentation"
                )
            })
        })
        .unwrap_or(true);
    let input = json!({
        "goal": goal,
        "template": "custom",
        "tasks": engineering_plan(goal, profile, autonomy),
        "parallel": 2,
        "maxRetries": 1,
        "allowWrites": writes,
        "allowCommands": true,
        "autoApprove": autonomy >= 3,
    });
    match crate::multi_agent::create_mission_with(state, input).await {
        Ok(mission_id) => {
            row["missionId"] = json!(mission_id);
            row["acceptance"] = json!(requirement.acceptance);
            store_pipeline(state, row).map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("The pipeline started but could not be recorded: {error}"),
                )
            })?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn list_pipelines(State(state): State<AppState>) -> Response {
    let Ok(conn) = db(&state) else {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error.");
    };
    let mut stmt = match conn.prepare(&format!("{PIPELINE_SQL} ORDER BY created_at DESC LIMIT 30"))
    {
        Ok(stmt) => stmt,
        Err(error) => {
            return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
    };
    let rows = match stmt.query_map([], pipeline_from_row) {
        Ok(rows) => rows.filter_map(|row| row.ok()).collect::<Vec<_>>(),
        Err(error) => {
            return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
        }
    };
    let pipelines: Vec<Value> = rows.iter().map(|row| pipeline_json(&state, row)).collect();
    Json(json!({ "pipelines": pipelines })).into_response()
}

async fn get_pipeline(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match load_pipeline(&state, &id) {
        Ok(Some(row)) => Json(json!({ "pipeline": pipeline_json(&state, &row) })).into_response(),
        Ok(None) => crate::json_error(StatusCode::NOT_FOUND, "No such pipeline."),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn clarify(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    let Some(mut row) = load_pipeline(&state, &id).ok().flatten() else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such pipeline.");
    };
    if !row["missionId"].as_str().unwrap_or("").is_empty() {
        return bad("This pipeline already started; its questions were answered.");
    }
    let answer = input
        .get("answer")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if word_count(&answer) < 3 {
        return bad("Add the missing detail before the pipeline starts.");
    }
    let goal = format!(
        "{}\n\nAnswer to the missing requirement: {}",
        row["goal"].as_str().unwrap_or(""),
        answer
    );
    let (_, profile) = match project_profile(&state) {
        Ok(result) => result,
        Err(error) => return bad(error),
    };
    let requirement = analyse_requirement(&goal, Some(&profile));
    if !requirement.questions.is_empty() {
        row["questions"] = json!(requirement.questions);
        let _ = store_pipeline(&state, &row);
        return Json(json!({
            "ok": false,
            "needsInput": true,
            "questions": requirement.questions,
            "pipeline": pipeline_json(&state, &row),
        }))
        .into_response();
    }
    let autonomy = row["autonomy"].as_i64().unwrap_or(2);
    row["goal"] = json!(goal);
    row["questions"] = json!([]);
    row["acceptance"] = json!(requirement.acceptance);
    match start_pipeline(&state, &mut row, &goal, autonomy, &profile, &requirement).await {
        Ok(()) => {
            Json(json!({ "ok": true, "pipeline": pipeline_json(&state, &row) })).into_response()
        }
        Err((code, message)) => crate::json_error(code, message),
    }
}

async fn prepare_branch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    let Some(mut row) = load_pipeline(&state, &id).ok().flatten() else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such pipeline.");
    };
    let autonomy = row["autonomy"].as_i64().unwrap_or(2);
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if autonomy < 3 && !approve {
        return crate::json_error(
            StatusCode::CONFLICT,
            "Creating a branch changes git state: approve it (or raise the autonomy level to autonomous).",
        );
    }
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            let slug: String = row["goal"]
                .as_str()
                .unwrap_or("change")
                .to_lowercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            let slug: String = slug
                .split('-')
                .filter(|part| !part.is_empty())
                .take(4)
                .collect::<Vec<_>>()
                .join("-");
            format!("{slug}-{}", &id[..8])
        });
    if let Some(response) = crate::permissions::guard_user(
        &state,
        crate::permissions::Action::Git,
        &name,
        "",
        approve || autonomy >= 3,
        "create a branch",
    ) {
        let _ = response;
        return crate::json_error(
            StatusCode::CONFLICT,
            "Creating a branch is gated: approve it first.",
        );
    }
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(_) => return bad("Open a workspace folder first."),
    };
    match crate::git::create_branch_checked(&root, &name) {
        Ok(_) => {
            row["branch"] = json!(name);
            let _ = store_pipeline(&state, &row);
            Json(json!({ "ok": true, "branch": name, "pipeline": pipeline_json(&state, &row) }))
                .into_response()
        }
        Err(error) => crate::json_error(StatusCode::BAD_REQUEST, error),
    }
}

async fn deliver(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Response {
    let Some(mut row) = load_pipeline(&state, &id).ok().flatten() else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such pipeline.");
    };
    let mission_id = row["missionId"].as_str().unwrap_or("");
    let Some(mission) = crate::multi_agent::mission_json_for(&state, mission_id) else {
        return bad("This pipeline has not started a mission.");
    };
    if mission["status"] != json!("succeeded") {
        return bad("Only a pipeline whose every task succeeded can be delivered.");
    }
    let report = mission["report"].clone();
    let paths = written_paths(&report);
    if paths.is_empty() {
        return bad("This pipeline wrote no files, so there is nothing to commit.");
    }
    let autonomy = row["autonomy"].as_i64().unwrap_or(2);
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if autonomy < 3 && !approve {
        return crate::json_error(
            StatusCode::CONFLICT,
            "Committing is a git change: approve it (or raise the autonomy level to autonomous).",
        );
    }
    if let Some(response) = crate::permissions::guard_user(
        &state,
        crate::permissions::Action::Git,
        &paths.join(","),
        "",
        approve || autonomy >= 3,
        "commit the pipeline's changes",
    ) {
        let _ = response;
        return crate::json_error(
            StatusCode::CONFLICT,
            "Committing is gated: approve it first.",
        );
    }
    let root = match state.workspace.root() {
        Ok(root) => root,
        Err(_) => return bad("Open a workspace folder first."),
    };
    let message = match input.get("message").and_then(Value::as_str) {
        Some(message) if !message.trim().is_empty() => message.trim().to_string(),
        _ => commit_message(
            row["goal"].as_str().unwrap_or("Engineering change"),
            &mission,
        ),
    };
    match crate::git::commit_checked(&root, &message, &paths) {
        Ok(value) => {
            let head = crate::git::head_info(&root);
            row["delivery"] = json!({
                "files": paths,
                "message": message,
                "commit": head.1,
                "branch": head.0,
                "at": now(),
                "result": value,
            });
            let _ = store_pipeline(&state, &row);
            Json(json!({
                "ok": true,
                "delivery": row["delivery"],
                "pipeline": pipeline_json(&state, &row),
            }))
            .into_response()
        }
        Err(error) => crate::json_error(StatusCode::BAD_REQUEST, error),
    }
}

async fn checks_route(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let pipeline = input.get("pipeline").and_then(Value::as_str).unwrap_or("");
    match checks(&state, approve).await {
        Ok(mut payload) => {
            if let Ok(conn) = db(&state) {
                for issue in payload["issues"].as_array().cloned().unwrap_or_default() {
                    let _ = conn.execute(
                        "INSERT INTO engineering_checks(pipeline, kind, status, summary, output, created_at) \
                         VALUES(?1,?2,'failed',?3,?4,?5)",
                        params![
                            pipeline,
                            issue["kind"].as_str().unwrap_or("unknown"),
                            issue["summary"].as_str().unwrap_or(""),
                            issue["suggestion"].as_str().unwrap_or(""),
                            now()
                        ],
                    );
                }
            }
            if let Some(issues) = payload["issues"].as_array_mut() {
                for issue in issues.iter_mut() {
                    // The repair action is a real mission; it is offered, not run.
                    issue["repair"] = json!("Start a repair pipeline for this failure");
                }
            }
            Json(payload).into_response()
        }
        Err((code, message)) => crate::json_error(code, message),
    }
}

async fn repair(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let problem = input
        .get("problem")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if word_count(&problem) < 3 {
        return bad("Describe the failure the repair should target.");
    }
    let autonomy = autonomy_of(&state).level;
    let id = uuid::Uuid::new_v4().to_string();
    let mut row = json!({
        "id": id,
        "missionId": "",
        "goal": format!("Repair the failure: {problem}"),
        "autonomy": autonomy,
        "questions": [],
        "acceptance": [
            "The failure is reproduced before the fix and no longer reproduces after it.",
            "The project's test command succeeds, or the report says why it could not run.",
        ],
        "profile": project_profile(&state).map(|(json, _)| json).unwrap_or(Value::Null),
        "branch": "",
        "baseline": "",
        "delivery": Value::Null,
        "createdAt": now(),
        "updatedAt": now(),
    });
    let input = json!({
        "goal": row["goal"],
        "template": "custom",
        "tasks": repair_plan(autonomy),
        "parallel": 1,
        "maxRetries": 1,
        "allowWrites": true,
        "allowCommands": true,
        "autoApprove": autonomy >= 3,
    });
    match crate::multi_agent::create_mission_with(&state, input).await {
        Ok(mission_id) => {
            row["missionId"] = json!(mission_id);
            // A repair mission's tasks were planned by the template, so the
            // approval policy lives there; the pipeline records it either way.
            let _ = store_pipeline(&state, &row);
            Json(json!({ "ok": true, "pipeline": pipeline_json(&state, &row) })).into_response()
        }
        Err((code, message)) => crate::json_error(code, message),
    }
}

async fn settings(State(state): State<AppState>) -> Response {
    let autonomy = autonomy_of(&state);
    let (allow_branch, allow_commit) = db(&state)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT allow_branch, allow_commit FROM engineering_settings WHERE id=1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .ok()
            .flatten()
        })
        .unwrap_or((1, 1));
    Json(json!({
        "autonomy": autonomy.level,
        "label": autonomy.label(),
        "levels": [
            { "level": 1, "label": "assisted", "detail": "every task waits for approval" },
            { "level": 2, "label": "supervised", "detail": "tasks that change files wait for approval" },
            { "level": 3, "label": "autonomous", "detail": "the plan runs; git delivery still asks unless approved" }
        ],
        "allowBranch": allow_branch != 0,
        "allowCommit": allow_commit != 0,
    }))
    .into_response()
}

async fn save_settings(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    if input.get("autonomy").is_none() {
        return bad("Choose an autonomy level: 1 assisted, 2 supervised, 3 autonomous.");
    }
    match save_autonomy(&state, &input) {
        Ok(autonomy) => {
            Json(json!({ "ok": true, "autonomy": autonomy.level, "label": autonomy.label() }))
                .into_response()
        }
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn stop_everything(State(state): State<AppState>) -> Response {
    // Emergency stop is never gated: stopping is the safe direction.
    let (missions, runs) = crate::multi_agent::cancel_all(&state).await;
    let detached = crate::agent::cancel_all_active(&state, "Emergency stop from Engineering.");
    Json(json!({
        "ok": true,
        "missionsCancelled": missions,
        "runsStopped": runs.max(detached.len()),
    }))
    .into_response()
}

async fn metrics_route(State(state): State<AppState>) -> Response {
    Json(metrics(&state)).into_response()
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/engineering/project", get(project))
        .route("/api/engineering/analyse", post(analyse))
        .route(
            "/api/engineering/pipelines",
            get(list_pipelines).post(create_pipeline),
        )
        .route("/api/engineering/pipelines/:id", get(get_pipeline))
        .route("/api/engineering/pipelines/:id/clarify", post(clarify))
        .route(
            "/api/engineering/pipelines/:id/branch",
            post(prepare_branch),
        )
        .route("/api/engineering/pipelines/:id/deliver", post(deliver))
        .route("/api/engineering/checks", post(checks_route))
        .route("/api/engineering/repair", post(repair))
        .route(
            "/api/engineering/settings",
            get(settings).post(save_settings),
        )
        .route("/api/engineering/stop", post(stop_everything))
        .route("/api/engineering/metrics", get(metrics_route))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sarthi-engineering-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn repo(tag: &str) -> PathBuf {
        let dir = scratch(tag);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"demo\"\n\n[dependencies]\naxum = \"0.7\"\ntokio = \"1\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn demo() {}").unwrap();
        std::fs::write(dir.join("tests/flow.rs"), "#[test] fn ok() {}").unwrap();
        std::fs::write(dir.join("docs/README.md"), "# demo").unwrap();
        std::fs::write(dir.join("README.md"), "# demo").unwrap();
        std::fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        std::fs::write(dir.join("node_modules/pkg/index.js"), "module.exports = 1;").unwrap();
        dir
    }

    #[test]
    fn the_scan_finds_languages_tests_docs_and_frameworks_without_the_ignored_tree() {
        let dir = repo("scan");
        let profile = scan(&dir).expect("scan");
        assert!(profile.files > 0);
        assert!(
            profile
                .languages
                .iter()
                .any(|(name, count)| name == "Rust" && *count >= 3),
            "{:?}",
            profile.languages
        );
        assert_eq!(profile.test_files, 1);
        assert!(profile.doc_files >= 2);
        assert!(profile.frameworks.iter().any(|name| name == "Axum"));
        assert!(profile
            .entry_points
            .iter()
            .any(|name| name == "src/main.rs"));
        assert_eq!(profile.dependencies, 2);
        assert!(!profile.bounded);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_request_that_names_nothing_asks_instead_of_guessing() {
        let vague = analyse_requirement("improve it", None);
        assert_eq!(vague.questions.len(), 2, "{:?}", vague.questions);

        let concrete = analyse_requirement(
            "Fix the crash when importing an empty CSV file in the reports page",
            None,
        );
        assert!(concrete.questions.is_empty(), "{:?}", concrete.questions);
        assert!(concrete
            .acceptance
            .iter()
            .any(|line| line.contains("reproduced")));
    }

    #[test]
    fn a_project_without_tests_says_so_instead_of_inventing_verification() {
        let profile = Profile {
            files: 3,
            source_files: 2,
            test_files: 0,
            doc_files: 0,
            languages: vec![("Rust".to_string(), 2)],
            manifests: Vec::new(),
            frameworks: Vec::new(),
            entry_points: Vec::new(),
            dependencies: 0,
            bounded: false,
        };
        let requirement = analyse_requirement("Add a settings screen for the app", Some(&profile));
        assert!(requirement
            .risks
            .iter()
            .any(|risk| risk.contains("No test files")));
        assert!(requirement
            .risks
            .iter()
            .any(|risk| risk.contains("No build manifest")));
    }

    #[test]
    fn the_autonomy_level_decides_which_tasks_wait() {
        assert!(task_needs_approval(1, false));
        assert!(task_needs_approval(1, true));
        assert!(!task_needs_approval(2, false));
        assert!(task_needs_approval(2, true));
        assert!(!task_needs_approval(3, true));

        let profile = Profile {
            files: 1,
            source_files: 1,
            test_files: 0,
            doc_files: 0,
            languages: Vec::new(),
            manifests: Vec::new(),
            frameworks: Vec::new(),
            entry_points: Vec::new(),
            dependencies: 0,
            bounded: false,
        };
        let plan = engineering_plan("add a page", &profile, 1);
        assert!(plan.iter().all(|task| task["needsApproval"] == json!(true)));
        let plan = engineering_plan("add a page", &profile, 2);
        for task in &plan {
            let writes = matches!(task["role"].as_str().unwrap(), "coding" | "documentation");
            assert_eq!(task["needsApproval"], json!(writes), "{task}");
        }
        let plan = engineering_plan("add a page", &profile, 3);
        assert!(plan
            .iter()
            .all(|task| task["needsApproval"] == json!(false)));
        // Every engineering plan starts with analysis and ends with a review.
        assert_eq!(plan[0]["role"], json!("architect"));
        assert_eq!(plan.last().unwrap()["role"], json!("review"));
    }

    #[test]
    fn failures_are_classified_by_what_the_output_says() {
        assert_eq!(
            classify_failure("error[E0432]: unresolved import").0,
            "compile"
        );
        assert_eq!(
            classify_failure("failed to resolve: use of undeclared crate").0,
            "dependency"
        );
        assert_eq!(
            classify_failure("thread 'main' panicked at src/main.rs:3").0,
            "runtime"
        );
        assert_eq!(
            classify_failure("bash: cargo: command not found").0,
            "environment"
        );
        assert_eq!(
            classify_failure("error sending request: connection refused").0,
            "network"
        );
        assert_eq!(classify_failure("something odd happened").0, "unknown");
    }

    #[test]
    fn only_the_pipeline_s_own_writes_are_staged() {
        let report = json!({
            "filesWritten": [
                "write src/a.rs (12 bytes)",
                "write docs/x.md (9 bytes)",
                "write src/a.rs (12 bytes)"
            ]
        });
        assert_eq!(
            written_paths(&report),
            vec!["src/a.rs".to_string(), "docs/x.md".to_string()]
        );
        assert!(written_paths(&json!({ "filesWritten": [] })).is_empty());
        assert!(written_paths(&json!({ "filesWritten": ["command echo hi"] })).is_empty());
    }

    #[test]
    fn a_commit_message_is_the_request_plus_what_each_task_reported() {
        let mission = json!({
            "id": "m1",
            "tasks": [
                { "id": "implement", "roleName": "Coding", "status": "done", "summary": "wrote src/a.rs\nand tests" },
                { "id": "verify", "roleName": "Testing", "status": "done", "summary": "tests pass" },
                { "id": "review", "roleName": "Code review", "status": "failed", "summary": "ignored" }
            ]
        });
        let message = commit_message("Add CSV export to the reports page", &mission);
        assert!(message.starts_with("Add CSV export to the reports page\n"));
        assert!(message.contains("- Coding: wrote src/a.rs and tests"));
        assert!(message.contains("- Testing: tests pass"));
        assert!(!message.contains("ignored"));
        assert!(message.ends_with("Mission: m1"));
    }

    #[test]
    fn coverage_is_read_from_lcov_and_never_guessed() {
        let dir = scratch("coverage");
        assert_eq!(coverage_from(&dir), None);
        std::fs::create_dir_all(dir.join("coverage")).unwrap();
        std::fs::write(
            dir.join("coverage/lcov.info"),
            "TN:\nLF:200\nLH:150\nend_of_record\n",
        )
        .unwrap();
        let (percent, source) = coverage_from(&dir).expect("coverage");
        assert_eq!(percent, 75.0);
        assert_eq!(source, "coverage/lcov.info");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignored_directories_are_not_counted() {
        let dir = repo("ignored");
        let profile = scan(&dir).unwrap();
        assert!(!profile
            .manifests
            .iter()
            .any(|manifest| manifest.contains("node_modules")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod flow {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    struct Fixture {
        state: AppState,
        app: Router,
        root: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    async fn scripted(replies: &[&str], delay_ms: u64) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(
            replies
                .iter()
                .map(|reply| reply.to_string())
                .collect::<Vec<_>>(),
        )));
        let app = Router::new().route(
            "/chat/completions",
            axum::routing::post(move |_body: axum::Json<Value>| {
                let queue = queue.clone();
                async move {
                    if delay_ms > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                    let reply = queue.lock().unwrap().pop_front().unwrap_or_else(|| {
                        "{\"action\":\"fail\",\"reason\":\"mock out of replies\"}".to_string()
                    });
                    axum::Json(json!({
                        "choices": [{ "message": { "role": "assistant", "content": reply } }]
                    }))
                }
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}")
    }

    async fn boot(tag: &str, replies: &[&str], delay_ms: u64) -> Fixture {
        let base = scripted(replies, delay_ms).await;
        let root = std::env::temp_dir().join(format!(
            "sarthi-engineering-flow-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let ws = root.join("ws");
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(ws.join("README.md"), "# demo").unwrap();
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
            models_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };
        let _ = crate::ensure_setup(&state);
        let mut cfg = crate::load_config(&state);
        cfg.base_url = base;
        cfg.api_key = "test-key".to_string();
        cfg.local_chat = false;
        let _ = crate::save_config_file(&state.config_path, &cfg);
        state
            .workspace
            .open(ws.to_str().unwrap())
            .expect("workspace");
        let app = router().with_state(state.clone());
        Fixture { state, app, root }
    }

    async fn request(app: &Router, method: &str, uri: &str, body: Value) -> (u16, Value) {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn wait_for(state: &AppState, id: &str, expected: &str) -> Value {
        for _ in 0..400 {
            let row = load_pipeline(state, id).unwrap().expect("pipeline");
            if pipeline_status(state, &row) == expected {
                return pipeline_json(state, &row);
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let row = load_pipeline(state, id).unwrap().expect("pipeline");
        panic!(
            "pipeline never reached {expected}: {}",
            pipeline_status(state, &row)
        );
    }

    async fn settle(state: &AppState, id: &str) -> Value {
        for _ in 0..800 {
            let row = load_pipeline(state, id).unwrap().expect("pipeline");
            let status = pipeline_status(state, &row);
            if !matches!(status.as_str(), "planned" | "running" | "paused") {
                return pipeline_json(state, &row);
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let row = load_pipeline(state, id).unwrap().expect("pipeline");
        panic!("pipeline never settled: {}", pipeline_status(state, &row));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_supervised_pipeline_refuses_to_start_until_the_request_is_answerable() {
        let fixture = boot("intake", &[], 0).await;
        let (status, payload) = request(
            &fixture.app,
            "POST",
            "/api/engineering/pipelines",
            json!({ "goal": "improve it" }),
        )
        .await;
        assert_eq!(status, 200, "{payload}");
        assert_eq!(payload["needsInput"], json!(true));
        let questions = payload["pipeline"]["questions"].as_array().unwrap();
        assert!(!questions.is_empty());
        assert_eq!(payload["pipeline"]["status"], json!("needs-input"));
        // No mission was started, so no model call was spent.
        assert_eq!(payload["pipeline"]["missionId"], json!(""));

        let id = payload["pipeline"]["id"].as_str().unwrap();
        let (status, answered) = request(
            &fixture.app,
            "POST",
            &format!("/api/engineering/pipelines/{id}/clarify"),
            json!({ "answer": "the reports page should export an empty CSV instead of crashing" }),
        )
        .await;
        assert_eq!(status, 200, "{answered}");
        // The answer is enough to be concrete, but the workspace has no test
        // files and the pipeline still records that as a risk rather than
        // pretending verification exists. The supervisor settles the mission
        // on its next tick, so wait for it rather than reading a race.
        let waiting = wait_for(&fixture.state, id, "awaiting-approval").await;
        assert_eq!(waiting["status"], json!("awaiting-approval"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn autonomy_one_waits_on_every_task_and_gates_git() {
        let fixture = boot("assisted", &[], 0).await;
        let (status, _) = request(
            &fixture.app,
            "POST",
            "/api/engineering/settings",
            json!({ "autonomy": 1 }),
        )
        .await;
        assert_eq!(status, 200);
        let (status, payload) = request(
            &fixture.app,
            "POST",
            "/api/engineering/pipelines",
            json!({ "goal": "Add a CSV export control to the reports page" }),
        )
        .await;
        assert_eq!(status, 200, "{payload}");
        let id = payload["pipeline"]["id"].as_str().unwrap().to_string();
        let pipeline = wait_for(&fixture.state, &id, "awaiting-approval").await;
        assert_eq!(pipeline["status"], json!("awaiting-approval"));
        // Nothing started: every task still needs the person's approval.
        for task in pipeline["tasks"].as_array().unwrap() {
            assert_eq!(task["status"], json!("planned"), "{task}");
            assert_eq!(task["needsApproval"], json!(true), "{task}");
            assert_eq!(task["runId"], json!(""), "{task}");
        }
        // The git gate is independent of the mission and asks by default.
        let (status, refused) = request(
            &fixture.app,
            "POST",
            &format!("/api/engineering/pipelines/{id}/branch"),
            json!({ "name": "feature/csv-export" }),
        )
        .await;
        assert_eq!(status, 409, "{refused}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_autonomous_pipeline_runs_every_stage_and_never_claims_unverified_work() {
        let fixture = boot(
            "autonomous",
            &[
                "{\"action\":\"done\",\"summary\":\"structure: one Rust file, no tests\"}",
                "{\"action\":\"done\",\"summary\":\"plan: change src/main.rs\"}",
                "{\"action\":\"done\",\"summary\":\"implemented without writing a file\"}",
                "{\"action\":\"done\",\"summary\":\"no test command exists\"}",
                "{\"action\":\"done\",\"summary\":\"readme still accurate\"}",
                "{\"action\":\"done\",\"summary\":\"reviewed the plan against the files\"}",
                "APPROVE: the change is consistent with the plan",
            ],
            0,
        )
        .await;
        let (status, _) = request(
            &fixture.app,
            "POST",
            "/api/engineering/settings",
            json!({ "autonomy": 3 }),
        )
        .await;
        assert_eq!(status, 200);
        let (status, payload) = request(
            &fixture.app,
            "POST",
            "/api/engineering/pipelines",
            json!({ "goal": "Document how the demo binary starts" }),
        )
        .await;
        assert_eq!(status, 200, "{payload}");
        let id = payload["pipeline"]["id"].as_str().unwrap().to_string();
        let settled = settle(&fixture.state, &id).await;
        assert_eq!(settled["status"], json!("succeeded"), "{settled}");
        let tasks = settled["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 6);
        assert!(tasks.iter().all(|task| task["status"] == json!("done")));
        let trace = &settled["trace"];
        let criteria = trace["acceptance"].as_array().unwrap();
        assert!(criteria.iter().any(|entry| entry["criterion"]
            .as_str()
            .unwrap()
            .contains("test command")));
        // Nothing was written and no command ran, so no criterion may be
        // labelled verified: the pipeline reports what it observed.
        assert!(criteria
            .iter()
            .all(|entry| entry["status"] != json!("verified")));
        assert!(criteria
            .iter()
            .any(|entry| entry["status"] == json!("no-artifact")));
        assert_eq!(trace["wroteFiles"], json!(false));

        // Delivery is refused for the honest reason: there is nothing staged.
        let (status, refused) = request(
            &fixture.app,
            "POST",
            &format!("/api/engineering/pipelines/{id}/deliver"),
            json!({ "approve": true }),
        )
        .await;
        assert_eq!(status, 400, "{refused}");
        assert!(refused["error"]
            .as_str()
            .unwrap_or("")
            .contains("wrote no files"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_emergency_stop_leaves_live_pipelines_cancelled() {
        let replies: Vec<&str> = (0..20)
            .map(|_| "{\"action\":\"done\",\"summary\":\"working\"}")
            .collect();
        let fixture = boot("stop", &replies, 400).await;
        let (status, _) = request(
            &fixture.app,
            "POST",
            "/api/engineering/settings",
            json!({ "autonomy": 3 }),
        )
        .await;
        assert_eq!(status, 200);
        let (_, payload) = request(
            &fixture.app,
            "POST",
            "/api/engineering/pipelines",
            json!({ "goal": "Refactor the demo binary's entry point" }),
        )
        .await;
        let id = payload["pipeline"]["id"].as_str().unwrap().to_string();
        tokio::time::sleep(std::time::Duration::from_millis(220)).await;
        let (status, stopped) =
            request(&fixture.app, "POST", "/api/engineering/stop", json!({})).await;
        assert_eq!(status, 200, "{stopped}");
        assert!(stopped["missionsCancelled"].as_u64().unwrap_or(0) >= 1);
        let settled = settle(&fixture.state, &id).await;
        assert_eq!(settled["status"], json!("cancelled"), "{settled}");
        assert!(settled["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|task| task["status"] != json!("running")));
        // A cancelled stage is not evidence: no criterion may read as
        // verified or as still in progress after the stop.
        for entry in settled["trace"]["acceptance"].as_array().unwrap() {
            assert_eq!(entry["status"], json!("not-verified"), "{entry}");
        }
    }
}
