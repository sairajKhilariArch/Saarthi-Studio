//! Skills: prompt-level capabilities discovered from folders containing a
//! `SKILL.md` manifest.
//!
//! A skill is never executed — its instructions are text the user reads and
//! that (once picked) rides along with the next chat request. Roots are
//! scanned in precedence order:
//!
//! 1. `FREE_AI_STUDIO_SKILLS_DIR` (tests and packagers), when set,
//! 2. the app's bundled `static/skills/`, next to the UI it already serves,
//! 3. `skills/` inside the folder the user opened (project skills),
//! 4. `~/.config/free-ai-studio/skills/` (user skills),
//! 5. `~/.agents/skills/` (the agent skill packs installed on this machine).
//!
//! Front matter is a small `key: value` block between `---` fences:
//!
//! ```text
//! ---
//! name: code-review
//! description: Review a file for bugs, security problems and readability.
//! version: 1.2.0
//! ---
//! Instructions go here, in plain prose...
//! ```
//!
//! Enable/disable state lives in `config.json`, so it survives restarts and is
//! shared by the desktop app and the VS Code extension. Install metadata lives in
//! `skills.json` beside it: the version that was installed, when it was installed
//! and updated, and a fingerprint of what was written. Usage history is rows in
//! `skill_usage`, because "used 4 times, last Tuesday" is history, not state.
//!
//! Only the user root is written to. A project, built-in or `~/.agents` skill is
//! reported as not writable rather than edited behind the repository's back, and
//! removal moves a folder into `<root>/.removed/` instead of deleting it.

use crate::{home_dir, load_config, open_db, AppState};
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path as FsPath, PathBuf},
};

const MANIFEST: &str = "SKILL.md";
/// Instructions beyond this many tokens are truncated: the composer meter must
/// stay honest, and no single skill may eat the whole context budget.
const MAX_BODY_TOKENS: usize = 900;
/// A skill installed without a stated version still gets one, so the record can
/// show that it predates the first explicit update.
const DEFAULT_VERSION: &str = "0.1.0";
/// A message must share at least this much with a skill before the skill is
/// attached automatically (see [`select_for`]).
const AUTO_MIN_SCORE: usize = 2;
/// Ceiling on automatically attached skills: two related ones can help, five
/// bury the message under instructions.
const AUTO_MAX_SKILLS: usize = 2;
/// Ceiling on the instructions automatic selection may add to one turn.
const AUTO_MAX_TOKENS: usize = 1200;

#[derive(Clone, Debug)]
pub(crate) struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub body: String,
    pub path: String,
    pub source: &'static str,
    /// Fingerprint of the manifest as it is now, so a hand edit is visible
    /// against what was installed.
    pub fingerprint: String,
    pub error: Option<String>,
}

/// What `skills.json` remembers about a skill this app installed or updated.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Installed {
    id: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    installed_at: f64,
    #[serde(default)]
    updated_at: f64,
    /// Fingerprint of the manifest written at install/update time. A mismatch
    /// means someone edited the file since; the app reports that instead of
    /// silently overwriting their words on the next update.
    #[serde(default)]
    fingerprint: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Registry {
    #[serde(default)]
    skills: Vec<Installed>,
}

/// Search roots in precedence order: the first root that claims a skill name
/// wins, so a project skill can shadow a built-in one.
pub(crate) fn roots(
    static_dir: &FsPath,
    workspace_root: Option<&FsPath>,
) -> Vec<(String, PathBuf)> {
    if let Some(dir) = std::env::var_os("FREE_AI_STUDIO_SKILLS_DIR") {
        return vec![("built-in".to_string(), PathBuf::from(dir))];
    }
    let mut roots = vec![("built-in".to_string(), static_dir.join("skills"))];
    if let Some(root) = workspace_root {
        roots.push(("project".to_string(), root.join("skills")));
    }
    roots.push((
        "user".to_string(),
        home_dir().join(".config/free-ai-studio/skills"),
    ));
    roots.push(("agents".to_string(), home_dir().join(".agents/skills")));
    roots
}

pub(crate) fn discover(roots: &[(String, PathBuf)]) -> Vec<Skill> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (source, root) in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let manifest = dir.join(MANIFEST);
            if !manifest.is_file() {
                continue;
            }
            let skill = parse_manifest(&manifest, source.as_str());
            if !seen.insert(skill.id.clone()) {
                continue; // an earlier root already provides this skill
            }
            found.push(skill);
        }
    }
    found.sort_by_key(|a| a.name.to_lowercase());
    found
}

fn parse_manifest(manifest: &FsPath, source: &str) -> Skill {
    let folder = manifest
        .parent()
        .and_then(FsPath::file_name)
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    match fs::read_to_string(manifest) {
        Ok(text) => {
            let mut skill = parse_text(&folder, &text, source);
            skill.path = manifest.display().to_string();
            skill
        }
        Err(error) => Skill {
            id: folder.clone(),
            name: folder.replace(['_', '-'], " "),
            description: String::new(),
            version: String::new(),
            body: String::new(),
            path: manifest.display().to_string(),
            source: label(source),
            fingerprint: String::new(),
            error: Some(format!("SKILL.md could not be read: {error}")),
        },
    }
}

fn label(source: &str) -> &'static str {
    match source {
        "project" => "project",
        "user" => "user",
        "agents" => "agents",
        _ => "built-in",
    }
}

/// Parse a manifest from text — the same parser the installer validates with, so
/// something that can be installed is exactly something that can be listed.
fn parse_text(folder: &str, text: &str, source: &str) -> Skill {
    let mut skill = Skill {
        id: folder.to_string(),
        name: folder.replace(['_', '-'], " "),
        description: String::new(),
        version: String::new(),
        body: String::new(),
        path: String::new(),
        source: label(source),
        fingerprint: fingerprint(text),
        error: None,
    };

    let (front, body) = split_front_matter(text);
    skill.body = body.trim().to_string();

    let mut lines = front.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((key, raw)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let mut value = raw.trim().trim_matches('"').trim().to_string();

        // `description: >` / `|` (folded or literal block): the value is the
        // indented lines that follow, not the indicator itself.
        if matches!(value.as_str(), ">" | ">-" | ">+" | "|" | "|-" | "|+") {
            let mut block = Vec::new();
            while let Some(next) = lines.peek() {
                if next.trim().is_empty() {
                    block.push(String::new());
                    lines.next();
                    continue;
                }
                if next.starts_with(' ') || next.starts_with('\t') {
                    block.push(lines.next().unwrap().trim().to_string());
                } else {
                    break;
                }
            }
            while block.last().is_some_and(|line| line.is_empty()) {
                block.pop();
            }
            value = block.join(" ");
        }

        match key {
            "name" if !value.is_empty() => {
                skill.id = value.clone();
                skill.name = value;
            }
            "description" if !value.is_empty() => skill.description = value,
            "version" if !value.is_empty() => skill.version = value,
            _ => {}
        }
    }

    if skill.description.is_empty() {
        // Fall back to a plain first line so a manifest without front matter
        // still reads sensibly instead of being skipped silently.
        let summary = skill
            .body
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string();
        if summary.is_empty() {
            skill.error = Some("SKILL.md has no description and no instructions.".into());
        } else {
            skill.description = crate::context::crop(&summary, 40);
        }
    }
    if skill.body.is_empty() && skill.error.is_none() {
        skill.error = Some("SKILL.md contains no instructions.".into());
    }
    skill
}

fn split_front_matter(text: &str) -> (String, String) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    if first.trim_end() != "---" {
        return (String::new(), text.to_string());
    }
    let mut front = String::new();
    let mut consumed = first.len();
    for line in lines {
        if line.trim_end() == "---" {
            return (front, text[consumed + line.len()..].to_string());
        }
        front.push_str(line);
        consumed += line.len();
    }
    (String::new(), text.to_string())
}

/// One line per enabled skill for the system prompt: cheap context that tells
/// the model what exists without sending any instructions.
pub(crate) fn index_text(disabled: &[String], all: &[Skill]) -> Option<String> {
    let lines: Vec<String> = all
        .iter()
        .filter(|skill| skill.error.is_none() && !disabled.contains(&skill.id))
        .map(|skill| format!("- {}: {}", skill.id, skill.description))
        .collect();
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "Available skills (the user can attach one to a message):\n{}",
        lines.join("\n")
    ))
}

/// Full instructions for one skill, cropped to a sane size.
pub(crate) fn instructions_for(id: &str, all: &[Skill]) -> Option<Skill> {
    all.iter().find(|skill| skill.id == id).map(|skill| {
        let mut skill = skill.clone();
        skill.body = crate::context::crop(&skill.body, MAX_BODY_TOKENS);
        skill
    })
}

/// Words that say nothing about which skill a message needs. They are dropped
/// before scoring so a sentence like "please review this for me" matches a
/// review skill on "review" and not on "this".
const NOISE: &[&str] = &[
    "the", "and", "for", "this", "that", "with", "from", "into", "about", "when", "then", "you",
    "your", "are", "was", "were", "have", "has", "had", "not", "but", "all", "any", "can", "how",
    "what", "why", "who", "its", "its", "use", "using", "please", "make", "need", "want", "let",
    "get", "one", "two", "also", "just", "like", "should", "would", "could", "will", "shall",
    "may", "might", "must", "here", "there", "some", "more", "very", "much", "them", "they",
    "than", "out", "off", "our", "his", "her", "they",
];

/// A word the message and a skill can agree on: lowercase alphanumeric runs of
/// three or more characters, minus the noise list. Numbers are dropped, so a
/// version or an issue id never selects a skill on its own.
fn keywords(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut word = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            word.extend(ch.to_lowercase());
        } else {
            if word.len() >= 3
                && !NOISE.contains(&word.as_str())
                && !word.chars().all(char::is_numeric)
            {
                out.insert(std::mem::take(&mut word));
            } else {
                word.clear();
            }
        }
    }
    if word.len() >= 3 && !NOISE.contains(&word.as_str()) && !word.chars().all(char::is_numeric) {
        out.insert(word);
    }
    out
}

/// Skills a message should carry when the user picked none.
///
/// A skill scores 4 for each word its id or name shares with the message and 2
/// for each word of its one-line description: the name is what the skill is
/// called, the description is what it is for, and both are short. The message
/// must reach [`AUTO_MIN_SCORE`] to select anything, so a single incidental
/// word in a long description is not enough. Ties break on id, and the result
/// is capped by [`AUTO_MAX_SKILLS`] and [`AUTO_MAX_TOKENS`], so the same message
/// always selects the same skills and never blows the context budget.
pub(crate) fn select_for(text: &str, disabled: &[String], all: &[Skill]) -> Vec<Skill> {
    let wanted = keywords(text);
    if wanted.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, &Skill)> = all
        .iter()
        .filter(|skill| skill.error.is_none() && !disabled.contains(&skill.id))
        .filter_map(|skill| {
            let title = keywords(&format!(
                "{} {}",
                skill.id.replace(['-', '_'], " "),
                skill.name
            ));
            let about = keywords(&skill.description);
            let score =
                title.intersection(&wanted).count() * 4 + about.intersection(&wanted).count() * 2;
            (score >= AUTO_MIN_SCORE).then_some((score, skill))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    let mut chosen: Vec<Skill> = Vec::new();
    let mut tokens = 0usize;
    for (_, skill) in scored {
        let mut skill = skill.clone();
        skill.body = crate::context::crop(&skill.body, MAX_BODY_TOKENS);
        let cost = crate::context::estimate_tokens(&skill.body);
        if tokens + cost > AUTO_MAX_TOKENS {
            continue;
        }
        tokens += cost;
        chosen.push(skill);
        if chosen.len() == AUTO_MAX_SKILLS {
            break;
        }
    }
    chosen
}

/* ---------------------------------------------------------------- */
/* install / update / remove                                         */
/* ---------------------------------------------------------------- */

/// A stable, dependency-free content fingerprint (FNV-1a 64). It exists to spot
/// a hand edit, not to prove integrity, so it is called a fingerprint and never
/// presented as a checksum.
fn fingerprint(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Turn a user-supplied name into a folder name. Anything that could escape the
/// root (`/`, `\`, `..`) simply is not accepted as a character.
fn slugify(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            // Every non-alphanumeric becomes one separator, so a path-like name
            // reads as words instead of running together.
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Build a manifest from the fields the installer collects. `description` and
/// `version` are optional; a description is never invented from the
/// instructions, because the list would then show words the user did not write.
fn manifest_text(name: &str, description: &str, version: &str, instructions: &str) -> String {
    let description = description.replace(['\n', '\r'], " ");
    let version = version.trim();
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", name.trim()));
    if !description.trim().is_empty() {
        out.push_str(&format!("description: {}\n", description.trim()));
    }
    if !version.is_empty() {
        out.push_str(&format!("version: {version}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(instructions.trim());
    out.push('\n');
    out
}

fn registry_path(state: &AppState) -> PathBuf {
    state.config_path.with_file_name("skills.json")
}

fn load_registry_at(path: &FsPath) -> Registry {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_registry_at(path: &FsPath, registry: &Registry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(registry).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

/// Record an install/update, keeping the original `installed_at`.
fn remember_at(path: &FsPath, id: &str, version: &str, fingerprint: &str) -> Result<(), String> {
    let mut registry = load_registry_at(path);
    let now = crate::now_seconds();
    match registry.skills.iter_mut().find(|entry| entry.id == id) {
        Some(entry) => {
            entry.version = version.to_string();
            entry.updated_at = now;
            entry.fingerprint = fingerprint.to_string();
        }
        None => registry.skills.push(Installed {
            id: id.to_string(),
            version: version.to_string(),
            installed_at: now,
            updated_at: now,
            fingerprint: fingerprint.to_string(),
        }),
    }
    save_registry_at(path, &registry)
}

fn forget_at(path: &FsPath, id: &str) -> Result<(), String> {
    let mut registry = load_registry_at(path);
    registry.skills.retain(|entry| entry.id != id);
    save_registry_at(path, &registry)
}

fn installed_record(path: &FsPath, id: &str) -> Option<Installed> {
    load_registry_at(path)
        .skills
        .into_iter()
        .find(|entry| entry.id == id)
}

/// Write `document` as `<dir>/SKILL.md`. Validation happens before the disk is
/// touched, and the write goes through a temp file in the same folder so an
/// interrupted install cannot leave a half manifest behind.
fn write_manifest(dir: &FsPath, document: &str, replace: bool) -> Result<PathBuf, String> {
    let parsed = parse_text(
        dir.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default()
            .as_str(),
        document,
        "user",
    );
    if let Some(error) = parsed.error {
        return Err(error);
    }
    let manifest = dir.join(MANIFEST);
    if manifest.is_file() && !replace {
        return Err(format!("A skill named \"{}\" already exists.", parsed.id));
    }
    fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let temp = dir.join(format!("{MANIFEST}.tmp"));
    fs::write(&temp, document).map_err(|e| format!("Could not write {}: {e}", temp.display()))?;
    // Windows will not rename onto an existing file, so the replace path removes
    // the old manifest first; the temp file is already complete at this point.
    if manifest.exists() {
        fs::remove_file(&manifest)
            .map_err(|e| format!("Could not replace {}: {e}", manifest.display()))?;
    }
    fs::rename(&temp, &manifest).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("Could not install {}: {e}", manifest.display())
    })?;
    Ok(manifest)
}

/// The one root this app may write to: the user's own skills folder.
fn writable_root(state: &AppState) -> Option<PathBuf> {
    let workspace_root = state.workspace.current_root();
    roots(&state.static_dir, workspace_root.as_deref())
        .into_iter()
        .find(|(source, _)| source == "user")
        .map(|(_, path)| path)
}

fn folder_of(skill: &Skill) -> Option<PathBuf> {
    PathBuf::from(&skill.path).parent().map(FsPath::to_path_buf)
}

/// Is `dir` strictly inside `root`? Canonical paths, so `..` or a symlink cannot
/// talk the app into rewriting a skill that is not in the writable root.
fn inside(root: &FsPath, dir: &FsPath) -> bool {
    match (root.canonicalize(), dir.canonicalize()) {
        (Ok(root), Ok(dir)) => dir != root && dir.starts_with(&root),
        _ => false,
    }
}

/// Remove a skill by moving its folder into `<root>/.removed/`, which is
/// reversible; deleting a user's own files is not something a Remove button
/// should do.
fn move_out_of_the_way(root: &FsPath, dir: &FsPath) -> Result<PathBuf, String> {
    let trash = root.join(".removed");
    fs::create_dir_all(&trash).map_err(|e| format!("Could not create {}: {e}", trash.display()))?;
    let folder = dir
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    let stamp = crate::now_seconds() as i64;
    let target = trash.join(format!("{folder}-{stamp}"));
    fs::rename(dir, &target).map_err(|e| format!("Could not move {} aside: {e}", dir.display()))?;
    Ok(target)
}

fn disabled_list(state: &AppState) -> Vec<String> {
    load_config(state).disabled_skills
}

fn set_enabled(state: &AppState, id: &str, enabled: bool) -> Result<(), String> {
    let mut cfg = load_config(state);
    let before = cfg.disabled_skills.len();
    cfg.disabled_skills.retain(|entry| entry != id);
    if !enabled && cfg.disabled_skills.len() == before {
        cfg.disabled_skills.push(id.to_string());
    }
    crate::save_config_file(&state.config_path, &cfg).map_err(|e| e.to_string())
}

fn all_skills(state: &AppState) -> Vec<Skill> {
    let workspace_root = state.workspace.current_root();
    discover(&roots(&state.static_dir, workspace_root.as_deref()))
}

/* ---------------------------------------------------------------- */
/* usage history                                                     */
/* ---------------------------------------------------------------- */

/// `skill_usage` is created here as well as at boot so a database made by an
/// older build grows the table on first use instead of failing the insert.
pub(crate) fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS skill_usage (
            id INTEGER PRIMARY KEY AUTOINCREMENT, skill_id TEXT NOT NULL,
            used_at REAL NOT NULL, conversation_id TEXT NOT NULL DEFAULT '',
            tokens INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS skill_usage_skill ON skill_usage(skill_id, used_at DESC);",
    )
}

/// Record that a skill rode along with a message. Fire-and-forget: a lost usage
/// row must never fail the turn it describes.
pub(crate) fn record_use(db: &FsPath, id: &str, conversation: &str, tokens: usize) {
    let _ = open_db(db).and_then(|conn| {
        initialize(&conn)?;
        conn.execute(
            "INSERT INTO skill_usage(skill_id, used_at, conversation_id, tokens) VALUES(?1,?2,?3,?4)",
            rusqlite::params![id, crate::now_seconds(), conversation, tokens as i64],
        )?;
        Ok(())
    });
}

/// Record a pick from a chat request, sizing the instruction text the same way
/// the request itself does.
pub(crate) fn record_pick(state: &AppState, id: &str, conversation: &str) {
    let tokens = instructions_for(id, &all_skills(state))
        .map(|skill| crate::context::estimate_tokens(&skill.body))
        .unwrap_or(0);
    record_use(&state.database_path, id, conversation, tokens);
}

/// Uses, last use and instruction tokens per skill, for the list route.
fn usage_summary(db: &FsPath) -> HashMap<String, (i64, f64, i64)> {
    let mut summary = HashMap::new();
    let Ok(conn) = open_db(db) else {
        return summary;
    };
    let _ = initialize(&conn);
    let Ok(mut stmt) = conn.prepare(
        "SELECT skill_id, COUNT(*), MAX(used_at), COALESCE(SUM(tokens),0) FROM skill_usage GROUP BY skill_id",
    ) else {
        return summary;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (
                row.get::<_, i64>(1)?,
                row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                row.get::<_, i64>(3)?,
            ),
        ))
    }) else {
        return summary;
    };
    for row in rows.flatten() {
        summary.insert(row.0, row.1);
    }
    summary
}

fn usage_detail(db: &FsPath, id: &str, limit: usize) -> Value {
    let mut recent = Vec::new();
    let mut uses = 0i64;
    let mut last_used_at = 0.0f64;
    let mut tokens = 0i64;
    if let Ok(conn) = open_db(db) {
        let _ = initialize(&conn);
        if let Ok((count, last, total)) = conn.query_row(
            "SELECT COUNT(*), COALESCE(MAX(used_at),0), COALESCE(SUM(tokens),0) FROM skill_usage WHERE skill_id=?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        ) {
            uses = count;
            last_used_at = last;
            tokens = total;
        }
        if let Ok(mut stmt) = conn.prepare(
            "SELECT used_at, conversation_id, tokens FROM skill_usage WHERE skill_id=?1 ORDER BY used_at DESC LIMIT ?2",
        ) {
            if let Ok(rows) = stmt.query_map(rusqlite::params![id, limit as i64], |row| {
                Ok(json!({
                    "used_at": row.get::<_, f64>(0)?,
                    "conversation_id": row.get::<_, String>(1)?,
                    "tokens": row.get::<_, i64>(2)?,
                }))
            }) {
                recent = rows.flatten().collect();
            }
        }
    }
    json!({
        "ok": true, "id": id, "uses": uses, "last_used_at": last_used_at,
        "tokens": tokens, "recent": recent,
    })
}

/* ---------------------------------------------------------------- */
/* routes                                                            */
/* ---------------------------------------------------------------- */

/// The list entry: discovery plus what this app knows about installing it.
fn skill_json(state: &AppState, skill: &Skill, usage: &HashMap<String, (i64, f64, i64)>) -> Value {
    let record = installed_record(&registry_path(state), &skill.id);
    let usage = usage.get(&skill.id).copied().unwrap_or((0, 0.0, 0));
    json!({
        "id": skill.id,
        "name": skill.name,
        "description": skill.description,
        "path": skill.path,
        "source": skill.source,
        "error": skill.error,
        "enabled": !disabled_list(state).contains(&skill.id),
        "version": if skill.version.is_empty() {
            record.as_ref().map(|entry| entry.version.clone()).unwrap_or_default()
        } else {
            skill.version.clone()
        },
        "installed_at": record.as_ref().map(|entry| entry.installed_at),
        "updated_at": record.as_ref().map(|entry| entry.updated_at),
        "writable": skill.source == "user",
        // A fingerprint mismatch means the file was edited after installation;
        // the UI says so instead of implying the installed version is current.
        "modified": record
            .as_ref()
            .map(|entry| !entry.fingerprint.is_empty() && entry.fingerprint != skill.fingerprint)
            .unwrap_or(false),
        "uses": usage.0,
        "last_used_at": if usage.0 > 0 { Some(usage.1) } else { None },
        "tokens": usage.2,
    })
}

async fn get_skills(State(state): State<AppState>) -> Json<Value> {
    let usage = usage_summary(&state.database_path);
    let skills: Vec<Value> = all_skills(&state)
        .iter()
        .map(|skill| skill_json(&state, skill, &usage))
        .collect();
    Json(json!({ "ok": true, "skills": skills }))
}

async fn get_skill(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response<Body> {
    match instructions_for(&id, &all_skills(&state)) {
        Some(skill) if skill.error.is_none() => {
            let usage = usage_summary(&state.database_path);
            let mut entry = skill_json(&state, &skill, &usage);
            entry["body"] = json!(skill.body);
            Json(json!({ "ok": true, "skill": entry })).into_response()
        }
        Some(skill) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "ok": false, "error": skill.error.unwrap_or_default() })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": "No such skill." })),
        )
            .into_response(),
    }
}

async fn set_skill_enabled(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let enabled = input
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if let Err(error) = set_enabled(&state, &id, enabled) {
        return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error);
    }
    Json(json!({ "ok": true, "id": id, "enabled": enabled })).into_response()
}

async fn install_skill(State(state): State<AppState>, Json(input): Json<Value>) -> Response<Body> {
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let instructions = input
        .get("instructions")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if name.is_empty() || instructions.trim().is_empty() {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "A skill needs a name and instructions.",
        );
    }
    let slug = slugify(&name);
    if slug.is_empty() {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "Use a name with at least one letter or digit.",
        );
    }
    let Some(root) = writable_root(&state) else {
        return crate::json_error(
            StatusCode::CONFLICT,
            "There is no writable skills folder: FREE_AI_STUDIO_SKILLS_DIR replaces the roots while it is set.",
        );
    };
    let target = root.join(&slug).join(MANIFEST).display().to_string();
    // Installing writes a file, so it is gated on the same `fs_write` rule as
    // every other write in the app.
    if let Some(response) = crate::permissions::guard_user(
        &state,
        crate::permissions::Action::FsWrite,
        &target,
        "",
        approve,
        "install a skill",
    ) {
        return response;
    }
    let version = input
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let version = if version.is_empty() {
        DEFAULT_VERSION.to_string()
    } else {
        version
    };
    let description = input
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    // The manifest names the slug, not the typed title: the id is what every
    // route, the config's disabled list and the usage history key on.
    let document = manifest_text(&slug, description, &version, &instructions);
    let replace = input
        .get("replace")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match write_manifest(&root.join(&slug), &document, replace) {
        Ok(manifest) => {
            if let Err(error) = remember_at(
                &registry_path(&state),
                &slug,
                &version,
                &fingerprint(&document),
            ) {
                return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error);
            }
            Json(json!({
                "ok": true, "id": slug, "version": version,
                "path": manifest.display().to_string(), "replaced": replace,
            }))
            .into_response()
        }
        Err(error) => crate::json_error(StatusCode::CONFLICT, error),
    }
}

async fn update_skill(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<Value>,
) -> Response<Body> {
    let approve = input
        .get("approve")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let Some(skill) = all_skills(&state).into_iter().find(|entry| entry.id == id) else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such skill.");
    };
    if skill.source != "user" {
        return crate::json_error(
            StatusCode::CONFLICT,
            format!(
                "\"{id}\" is a {} skill; edit its SKILL.md where it lives instead of from here.",
                skill.source
            ),
        );
    }
    let Some(root) = writable_root(&state) else {
        return crate::json_error(StatusCode::CONFLICT, "There is no writable skills folder.");
    };
    let Some(dir) = folder_of(&skill) else {
        return crate::json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "That skill has no containing folder.",
        );
    };
    if !inside(&root, &dir) {
        return crate::json_error(
            StatusCode::CONFLICT,
            "That skill is outside the user skills folder and is not editable from here.",
        );
    }
    let instructions = match input.get("instructions").and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty() => text.to_string(),
        _ => {
            return crate::json_error(StatusCode::BAD_REQUEST, "Provide the instructions to save.")
        }
    };
    if let Some(response) = crate::permissions::guard_user(
        &state,
        crate::permissions::Action::FsWrite,
        &dir.join(MANIFEST).display().to_string(),
        "",
        approve,
        "update a skill",
    ) {
        return response;
    }
    // No version given means no version claim: keep the recorded one rather than
    // inventing a bump the user did not make.
    let recorded = installed_record(&registry_path(&state), &id)
        .map(|entry| entry.version)
        .filter(|version| !version.is_empty());
    let version = input
        .get("version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|version| !version.is_empty())
        .map(str::to_string)
        .or_else(|| Some(skill.version.clone()).filter(|version| !version.is_empty()))
        .or(recorded)
        .unwrap_or_else(|| DEFAULT_VERSION.to_string());
    let description = input
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| skill.description.clone());
    let document = manifest_text(&id, &description, &version, &instructions);
    match write_manifest(&dir, &document, true) {
        Ok(manifest) => {
            let target_id = parse_text(&id, &document, "user").id;
            if let Err(error) = remember_at(
                &registry_path(&state),
                &target_id,
                &version,
                &fingerprint(&document),
            ) {
                return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error);
            }
            Json(json!({
                "ok": true, "id": target_id, "version": version,
                "path": manifest.display().to_string(),
            }))
            .into_response()
        }
        Err(error) => crate::json_error(StatusCode::CONFLICT, error),
    }
}

async fn remove_skill(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    input: Option<Json<Value>>,
) -> Response<Body> {
    let approve = input
        .as_ref()
        .and_then(|Json(body)| body.get("approve"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let Some(skill) = all_skills(&state).into_iter().find(|entry| entry.id == id) else {
        return crate::json_error(StatusCode::NOT_FOUND, "No such skill.");
    };
    if skill.source != "user" {
        return crate::json_error(
            StatusCode::CONFLICT,
            format!(
                "\"{id}\" is a {} skill; this app does not delete files it did not install.",
                skill.source
            ),
        );
    }
    let Some(root) = writable_root(&state) else {
        return crate::json_error(StatusCode::CONFLICT, "There is no writable skills folder.");
    };
    let Some(dir) = folder_of(&skill) else {
        return crate::json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "That skill has no containing folder.",
        );
    };
    if !inside(&root, &dir) {
        return crate::json_error(
            StatusCode::CONFLICT,
            "That skill is outside the user skills folder and is not removable from here.",
        );
    }
    if let Some(response) = crate::permissions::guard_user(
        &state,
        crate::permissions::Action::FsWrite,
        &dir.display().to_string(),
        "",
        approve,
        "remove a skill",
    ) {
        return response;
    }
    match move_out_of_the_way(&root, &dir) {
        Ok(target) => {
            if let Err(error) = forget_at(&registry_path(&state), &id) {
                return crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error);
            }
            Json(json!({
                "ok": true, "id": id, "moved_to": target.display().to_string(),
            }))
            .into_response()
        }
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn get_usage(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response<Body> {
    let limit = 20usize;
    Json(usage_detail(&state.database_path, &id, limit)).into_response()
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/skills", get(get_skills))
        .route("/api/skills/install", post(install_skill))
        .route("/api/skills/:id", get(get_skill))
        .route("/api/skills/:id/state", post(set_skill_enabled))
        .route("/api/skills/:id/update", post(update_skill))
        .route("/api/skills/:id/remove", post(remove_skill))
        .route("/api/skills/:id/usage", get(get_usage))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "free-ai-studio-skills-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_skill(root: &FsPath, folder: &str, content: &str) {
        let dir = root.join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MANIFEST), content).unwrap();
    }

    #[test]
    fn parses_front_matter_and_body() {
        let root = scratch("parse");
        write_skill(
            &root,
            "review",
            "---\nname: code-review\ndescription: Review code for bugs.\n---\nFind the bug.\n",
        );
        let skills = discover(&[("built-in".into(), root)]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "code-review");
        assert_eq!(skills[0].description, "Review code for bugs.");
        assert_eq!(skills[0].body, "Find the bug.");
        assert!(skills[0].error.is_none());
    }

    #[test]
    fn flags_a_manifest_without_instructions() {
        let root = scratch("broken");
        write_skill(
            &root,
            "empty",
            "---\nname: empty\ndescription: Nothing here.\n---\n",
        );
        let skills = discover(&[("built-in".into(), root)]);
        assert_eq!(skills.len(), 1);
        assert!(skills[0]
            .error
            .as_deref()
            .unwrap_or("")
            .contains("no instructions"));
    }

    #[test]
    fn agents_root_is_labelled_agents() {
        let root = scratch("agents-source");
        write_skill(
            &root,
            "ponytail",
            "---\nname: ponytail\ndescription: Lazy senior dev mode.\n---\nBe lazy.\n",
        );
        let skills = discover(&[("agents".into(), root)]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source, "agents");
        assert_eq!(skills[0].id, "ponytail");
    }

    #[test]
    fn folds_a_block_scalar_description() {
        let root = scratch("block-description");
        write_skill(
            &root,
            "folded",
            "---\nname: folded\ndescription: >\n  Lazy senior dev mode: the smallest change\n  that fully solves the task.\n---\nDo the small thing.\n",
        );
        let skills = discover(&[("agents".into(), root)]);
        assert_eq!(skills.len(), 1);
        assert_eq!(
            skills[0].description,
            "Lazy senior dev mode: the smallest change that fully solves the task."
        );
        assert_eq!(skills[0].body, "Do the small thing.");
    }

    #[test]
    fn earlier_roots_shadow_later_ones() {
        let builtin = scratch("shadow-a");
        let project = scratch("shadow-b");
        write_skill(
            &builtin,
            "dup",
            "---\nname: dup\ndescription: Built-in copy.\n---\nA.\n",
        );
        write_skill(
            &project,
            "dup",
            "---\nname: dup\ndescription: Project copy.\n---\nB.\n",
        );
        let skills = discover(&[("built-in".into(), builtin), ("project".into(), project)]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source, "built-in");
    }

    #[test]
    fn index_lists_enabled_skills_only() {
        let root = scratch("index");
        write_skill(
            &root,
            "a",
            "---\nname: a\ndescription: First skill.\n---\nGo.\n",
        );
        write_skill(
            &root,
            "b",
            "---\nname: b\ndescription: Second skill.\n---\nStop.\n",
        );
        let all = discover(&[("built-in".into(), root)]);
        let index = index_text(&["b".into()], &all).expect("index");
        assert!(index.contains("- a: First skill."));
        assert!(!index.contains("Second skill"));
    }

    #[test]
    fn install_writes_a_manifest_and_keeps_the_version() {
        let root = scratch("install");
        let document = manifest_text(
            "code-review",
            "Review a file for bugs.",
            "1.2.0",
            "Read the file, then list the bugs you can prove.",
        );
        let manifest =
            write_manifest(&root.join("code-review"), &document, false).expect("install");
        assert!(manifest.is_file());
        let skills = discover(&[("user".into(), root.clone())]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "code-review");
        assert_eq!(skills[0].version, "1.2.0");
        assert_eq!(skills[0].description, "Review a file for bugs.");
        assert!(skills[0].body.starts_with("Read the file"));
        assert!(skills[0].error.is_none());
    }

    #[test]
    fn install_refuses_to_overwrite_unless_asked() {
        let root = scratch("overwrite");
        let first = manifest_text("Review", "First copy.", "1.0.0", "First instructions.");
        let second = manifest_text("Review", "Second copy.", "2.0.0", "Second instructions.");
        write_manifest(&root.join("review"), &first, false).expect("first install");
        let error = write_manifest(&root.join("review"), &second, false)
            .expect_err("a second install without replace must not clobber");
        assert!(error.contains("already exists"), "{error}");
        // The refused write left the original untouched.
        let skills = discover(&[("user".into(), root.clone())]);
        assert_eq!(skills[0].body, "First instructions.");

        write_manifest(&root.join("review"), &second, true).expect("explicit replace");
        let skills = discover(&[("user".into(), root)]);
        assert_eq!(skills[0].version, "2.0.0");
        assert_eq!(skills[0].body, "Second instructions.");
    }

    #[test]
    fn an_install_with_no_instructions_creates_nothing() {
        let root = scratch("empty-install");
        let document = manifest_text("Empty", "Nothing to say.", "1.0.0", "   ");
        let error = write_manifest(&root.join("empty"), &document, false)
            .expect_err("an empty skill is refused");
        assert!(error.contains("no instructions"), "{error}");
        assert!(!root.join("empty").exists(), "nothing was written");
    }

    #[test]
    fn a_hand_edit_changes_the_fingerprint() {
        let installed = manifest_text("Edit", "Before.", "1.0.0", "Original instructions.");
        let edited = manifest_text("Edit", "Before.", "1.0.0", "Someone changed this.");
        assert_ne!(fingerprint(&installed), fingerprint(&edited));
        // Same text, same fingerprint: the comparison is about content only.
        assert_eq!(fingerprint(&edited), fingerprint(&edited.clone()));
    }

    #[test]
    fn the_registry_remembers_the_version_without_losing_the_install_time() {
        let dir = scratch("registry");
        let path = dir.join("skills.json");
        remember_at(&path, "review", "1.0.0", "aaaa").expect("first install");
        let first = installed_record(&path, "review").expect("record");
        assert_eq!(first.version, "1.0.0");
        assert_eq!(first.installed_at, first.updated_at);

        remember_at(&path, "review", "1.1.0", "bbbb").expect("update");
        let second = installed_record(&path, "review").expect("record");
        assert_eq!(second.version, "1.1.0");
        assert_eq!(second.fingerprint, "bbbb");
        assert_eq!(
            second.installed_at, first.installed_at,
            "an update does not reset the install time"
        );

        forget_at(&path, "review").expect("forget");
        assert!(installed_record(&path, "review").is_none());
    }

    #[test]
    fn slugs_cannot_escape_the_root() {
        assert_eq!(slugify("Code Review"), "code-review");
        assert_eq!(slugify("../../etc/passwd"), "etc-passwd");
        assert_eq!(slugify("already-slugged"), "already-slugged");
        assert_eq!(slugify("   "), "");
        assert_eq!(slugify("///"), "");
    }

    #[test]
    fn remove_moves_the_folder_into_the_trash_rather_than_deleting_it() {
        let root = scratch("remove");
        write_skill(
            &root,
            "temp",
            "---\nname: temp\ndescription: Temporary.\n---\nGo.\n",
        );
        let dir = root.join("temp");
        assert!(inside(&root, &dir));
        let moved = move_out_of_the_way(&root, &dir).expect("moved");
        assert!(!dir.exists(), "the skill left the list");
        assert!(
            moved.join(MANIFEST).is_file(),
            "its manifest is still on disk"
        );
        assert!(moved.starts_with(root.join(".removed")));
        // The trash is not a skill root entry: it has no manifest of its own.
        assert_eq!(discover(&[("user".into(), root.clone())]).len(), 0);
        // The root itself is never "inside" itself.
        assert!(!inside(&root, &root));
    }

    #[test]
    fn usage_history_counts_uses_and_reports_the_last_one() {
        let dir = scratch("usage");
        let db = dir.join("studio.db");
        record_use(&db, "review", "conversation-1", 40);
        record_use(&db, "review", "conversation-2", 60);
        record_use(&db, "other", "conversation-1", 5);

        let summary = usage_summary(&db);
        let (uses, last, tokens) = summary.get("review").copied().expect("review usage");
        assert_eq!(uses, 2);
        assert!(last > 0.0);
        assert_eq!(tokens, 100);

        let detail = usage_detail(&db, "review", 10);
        assert_eq!(detail["uses"], 2);
        assert_eq!(detail["tokens"], 100);
        let recent = detail["recent"].as_array().expect("recent rows");
        assert_eq!(recent.len(), 2);
        // Newest first.
        assert_eq!(recent[0]["conversation_id"], "conversation-2");

        // An unknown skill reads as never used rather than as an error.
        let none = usage_detail(&db, "missing", 10);
        assert_eq!(none["uses"], 0);
        assert_eq!(none["recent"].as_array().map(Vec::len), Some(0));
    }
    /// A skill with nothing on disk behind it, for the selection tests.
    fn built(id: &str, description: &str, body: &str) -> Skill {
        Skill {
            id: id.to_string(),
            name: id.replace('-', " "),
            description: description.to_string(),
            version: "1.0.0".to_string(),
            body: body.to_string(),
            path: format!("/skills/{id}/SKILL.md"),
            source: "user",
            fingerprint: String::new(),
            error: None,
        }
    }

    fn ids(skills: &[Skill]) -> Vec<String> {
        skills.iter().map(|skill| skill.id.clone()).collect()
    }

    #[test]
    fn a_message_selects_the_skill_whose_name_it_uses() {
        let all = vec![
            built(
                "code-review",
                "Review a file for bugs and security problems.",
                "Read it.",
            ),
            built(
                "changelog",
                "Write a changelog entry for a release.",
                "Write it.",
            ),
        ];
        let chosen = select_for("please review src/main.rs before I merge", &[], &all);
        assert_eq!(ids(&chosen), ["code-review"]);
        // The cropped body is what an attachment would carry.
        assert_eq!(chosen[0].body, "Read it.");
    }

    #[test]
    fn a_description_alone_can_select_a_skill() {
        let all = vec![built(
            "house-style",
            "Keep the writing plain and short.",
            "Be plain.",
        )];
        let chosen = select_for("make the writing plain here", &[], &all);
        assert_eq!(ids(&chosen), ["house-style"]);
    }

    #[test]
    fn an_unrelated_message_selects_nothing() {
        let all = vec![built("code-review", "Review a file for bugs.", "Read it.")];
        assert!(select_for("what is the weather today", &[], &all).is_empty());
        assert!(select_for("??", &[], &all).is_empty());
        assert!(select_for("", &[], &all).is_empty());
    }

    #[test]
    fn selection_skips_disabled_and_broken_skills() {
        let all = vec![built("code-review", "Review a file for bugs.", "Read it.")];
        assert!(select_for("review this", &["code-review".to_string()], &all).is_empty());
        let mut broken = built("broken-review", "Review a file for bugs.", "");
        broken.error = Some("no instructions".to_string());
        assert!(select_for("review this", &[], &[broken]).is_empty());
    }

    #[test]
    fn selection_is_capped_and_the_same_message_picks_the_same_skills() {
        let all = vec![
            built("zeta-review", "Review things carefully.", ""),
            built("alpha-review", "Review things carefully.", ""),
            built("beta-review", "Review things carefully.", ""),
        ];
        let first = select_for("review this change", &[], &all);
        let second = select_for("review this change", &[], &all);
        // Equal scores break on id, and only two may ride along.
        assert_eq!(ids(&first), ["alpha-review", "beta-review"]);
        assert_eq!(ids(&first), ids(&second));
    }

    #[test]
    fn selection_stays_inside_its_token_budget() {
        let long = "x ".repeat(4000);
        let all = vec![
            built("alpha-review", "Review things carefully.", &long),
            built("beta-review", "Review things carefully.", &long),
        ];
        let chosen = select_for("review this change", &[], &all);
        assert_eq!(chosen.len(), 1, "a second full skill would blow the budget");
        assert!(crate::context::estimate_tokens(&chosen[0].body) <= 1200);
    }
}
