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
//! 4. `~/.config/free-ai-studio/skills/` (user skills).
//!
//! Front matter is a small `key: value` block between `---` fences:
//!
//! ```text
//! ---
//! name: code-review
//! description: Review a file for bugs, security problems and readability.
//! ---
//! Instructions go here, in plain prose...
//! ```
//!
//! Enable/disable state lives in `config.json`, so it survives restarts and is
//! shared by the desktop app and the VS Code extension.

use crate::{home_dir, load_config, AppState};
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    path::{Path as FsPath, PathBuf},
};

const MANIFEST: &str = "SKILL.md";
/// Instructions beyond this many tokens are truncated: the composer meter must
/// stay honest, and no single skill may eat the whole context budget.
const MAX_BODY_TOKENS: usize = 900;

#[derive(Clone, Debug)]
pub(crate) struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub body: String,
    pub path: String,
    pub source: &'static str,
    pub error: Option<String>,
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
    roots
}

pub(crate) fn discover(roots: &[(String, PathBuf)]) -> Vec<Skill> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (source, root) in roots {
        let Ok(entries) = fs::read_dir(root) else { continue };
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
    found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    found
}

fn parse_manifest(manifest: &FsPath, source: &str) -> Skill {
    let folder = manifest
        .parent()
        .and_then(FsPath::file_name)
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    let mut skill = Skill {
        id: folder.clone(),
        name: folder.replace(['_', '-'], " "),
        description: String::new(),
        body: String::new(),
        path: manifest.display().to_string(),
        source: match source {
            "project" => "project",
            "user" => "user",
            _ => "built-in",
        },
        error: None,
    };

    let text = match fs::read_to_string(manifest) {
        Ok(text) => text,
        Err(error) => {
            skill.error = Some(format!("SKILL.md could not be read: {error}"));
            return skill;
        }
    };
    let (front, body) = split_front_matter(&text);
    skill.body = body.trim().to_string();

    for line in front.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim().trim_matches('"').trim();
        match key.trim() {
            "name" if !value.is_empty() => {
                skill.id = value.to_string();
                skill.name = value.to_string();
            }
            "description" if !value.is_empty() => skill.description = value.to_string(),
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

async fn get_skills(State(state): State<AppState>) -> Json<Value> {
    let disabled = disabled_list(&state);
    let skills: Vec<Value> = all_skills(&state)
        .into_iter()
        .map(|skill| {
            json!({
                "id": skill.id,
                "name": skill.name,
                "description": skill.description,
                "path": skill.path,
                "source": skill.source,
                "error": skill.error,
                "enabled": !disabled.contains(&skill.id),
            })
        })
        .collect();
    Json(json!({ "ok": true, "skills": skills }))
}

async fn get_skill(State(state): State<AppState>, AxumPath(id): AxumPath<String>) -> Response<Body> {
    match instructions_for(&id, &all_skills(&state)) {
        Some(skill) if skill.error.is_none() => Json(json!({
            "ok": true,
            "skill": {
                "id": skill.id, "name": skill.name, "description": skill.description,
                "body": skill.body, "path": skill.path, "source": skill.source,
                "enabled": !disabled_list(&state).contains(&skill.id),
            }
        }))
        .into_response(),
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

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/skills", get(get_skills))
        .route("/api/skills/:id", get(get_skill))
        .route("/api/skills/:id/state", post(set_skill_enabled))
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
        write_skill(&root, "empty", "---\nname: empty\ndescription: Nothing here.\n---\n");
        let skills = discover(&[("built-in".into(), root)]);
        assert_eq!(skills.len(), 1);
        assert!(skills[0]
            .error
            .as_deref()
            .unwrap_or("")
            .contains("no instructions"));
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
        write_skill(&root, "a", "---\nname: a\ndescription: First skill.\n---\nGo.\n");
        write_skill(&root, "b", "---\nname: b\ndescription: Second skill.\n---\nStop.\n");
        let all = discover(&[("built-in".into(), root)]);
        let index = index_text(&["b".into()], &all).expect("index");
        assert!(index.contains("- a: First skill."));
        assert!(!index.contains("Second skill"));
    }
}
