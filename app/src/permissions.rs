//! One place that answers "may this action touch this target?", and one audit
//! trail of the answers.
//!
//! Before this module, permissions were per-run booleans (`allow_writes`,
//! `allow_commands`) plus route guards, so a rule like "never write under
//! `secrets/`" had nowhere to live. The policy store is that place: rules are
//! scoped to one use, the session, one project, or every project, and a rule
//! can only ever make an action *stricter or equally strict* — the run's own
//! flags still decide the ceiling.
//!
//! Resolution is deliberately boring: the longest matching target wins, then
//! deny-over-ask-over-allow, then the narrower scope. Nothing about a target is
//! guessed from a model's name or from a path that does not exist yet.

use std::fs;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Mutex, OnceLock};

use axum::extract::{Query as AxumQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::AppState;

/// The kinds of action the policy speaks about. The wire name is what the
/// store, the routes and the audit log all use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    FsRead,
    FsWrite,
    Command,
    Network,
    Git,
    RouterInstall,
    Update,
    Settings,
    McpTool,
    /// Calling a tool an installed extension declares. A different action from
    /// `McpTool` on purpose: the two are granted separately, because trusting a
    /// remote server's tool is not the same decision as trusting local code.
    ExtensionTool,
}

impl Action {
    pub(crate) const ALL: [Action; 10] = [
        Action::FsRead,
        Action::FsWrite,
        Action::Command,
        Action::Network,
        Action::Git,
        Action::RouterInstall,
        Action::Update,
        Action::Settings,
        Action::McpTool,
        Action::ExtensionTool,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::FsRead => "fs_read",
            Self::FsWrite => "fs_write",
            Self::Command => "command",
            Self::Network => "network",
            Self::Git => "git",
            Self::RouterInstall => "router_install",
            Self::Update => "update",
            Self::Settings => "settings",
            Self::McpTool => "mcp_tool",
            Self::ExtensionTool => "extension_tool",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|action| action.as_str() == value.trim().to_ascii_lowercase())
    }

    pub(crate) fn describe(self) -> &'static str {
        match self {
            Self::FsRead => "Read files inside the open folder",
            Self::FsWrite => "Create, change or delete files",
            Self::Command => "Run shell commands in the workspace",
            Self::Network => "Make outbound network requests",
            Self::Git => "Stage, commit, revert or branch",
            Self::RouterInstall => "Install, update or remove a router",
            Self::Update => "Replace installed software",
            Self::Settings => "Change application settings and keys",
            Self::McpTool => "Call a tool exposed by an MCP server",
            Self::ExtensionTool => "Call a tool declared by an installed extension",
        }
    }

    /// Reads are allowed by default; everything that can destroy, spend or
    /// expose something has to be asked about. Destructive defaults are not a
    /// grant — the per-run flags and approval gates still apply.
    pub(crate) fn default_decision(self) -> Decision {
        match self {
            Self::FsRead => Decision::Allow,
            _ => Decision::Ask,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Decision {
    Allow,
    Ask,
    Deny,
}

impl Decision {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "allow" => Some(Self::Allow),
            "ask" => Some(Self::Ask),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Deny => 2,
            Self::Ask => 1,
            Self::Allow => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Scope {
    Once,
    Session,
    Project,
    Always,
}

impl Scope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Session => "session",
            Self::Project => "project",
            Self::Always => "always",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "once" => Some(Self::Once),
            "session" => Some(Self::Session),
            "project" => Some(Self::Project),
            "always" => Some(Self::Always),
            _ => None,
        }
    }

    /// `once` and `session` live in memory and are gone when the app closes.
    pub(crate) fn persists(self) -> bool {
        matches!(self, Self::Project | Self::Always)
    }

    fn rank(self) -> u8 {
        match self {
            Self::Once => 3,
            Self::Session => 2,
            Self::Project => 1,
            Self::Always => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Rule {
    pub action: Action,
    /// "" covers every target for the action; otherwise a path or command
    /// prefix.
    #[serde(default)]
    pub target: String,
    pub decision: Decision,
    pub scope: Scope,
    /// Project the rule belongs to, "" for every project.
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub created_at: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Effective {
    pub decision: Decision,
    pub scope: Scope,
}

impl Effective {
    fn to_json(self) -> Value {
        json!({ "decision": self.decision.as_str(), "scope": self.scope.as_str() })
    }
}

/// Paths reach this module with either separator: Windows callers build them
/// with `Path::join` (backslashes), the UI and the audit write `/`. A rule and
/// a target are compared after this normalisation, so a rule written one way
/// still matches the other — a mismatch would silently disable the rule.
fn normalise_target(value: &str) -> String {
    let mut out = value.trim().replace('\\', "/");
    while out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

fn applies(rule: &Rule, action: Action, target: &str, project: &str) -> bool {
    if rule.action != action || !(rule.project.is_empty() || rule.project == project) {
        return false;
    }
    let rule_target = normalise_target(&rule.target);
    rule_target.is_empty() || normalise_target(target).starts_with(&rule_target)
}

/// Most specific rule wins: longer target, then deny > ask > allow, then the
/// narrower scope. Two rules that tie on all three are equivalent; the first
/// one read wins, which is stable because persisted rules keep their order.
fn prefer(candidate: &Rule, current: &Rule) -> bool {
    let score = |rule: &Rule| (rule.target.len(), rule.decision.rank(), rule.scope.rank());
    score(candidate) > score(current)
}

pub(crate) fn resolve<'a>(
    rules: impl Iterator<Item = &'a Rule>,
    action: Action,
    target: &str,
    project: &str,
) -> Option<Effective> {
    let mut best: Option<&Rule> = None;
    for rule in rules {
        if !applies(rule, action, target, project) {
            continue;
        }
        best = match best {
            Some(current) if !prefer(rule, current) => Some(current),
            _ => Some(rule),
        };
    }
    best.map(|rule| Effective {
        decision: rule.decision,
        scope: rule.scope,
    })
}

// ---------------------------------------------------------------------------
// Storage: rules on disk plus the session's own list

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    rules: Vec<Rule>,
}

fn store_path(state: &AppState) -> PathBuf {
    state.config_path.with_file_name("permissions.json")
}

/// Rules from disk. A file that cannot be parsed is treated as empty rather
/// than failing every action: the defaults are the safe answer.
pub(crate) fn read_rules(path: &FsPath) -> Vec<Rule> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StoreFile>(&bytes).ok())
        .map(|file| file.rules)
        .unwrap_or_default()
}

/// Atomic write with private permissions, like the config file: a half-written
/// policy file must never be able to widen what an action may do.
pub(crate) fn write_rules(path: &FsPath, rules: &[Rule]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&StoreFile {
        rules: rules.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    crate::set_private_permissions(&temp, false).map_err(|e| e.to_string())?;
    fs::rename(&temp, path).map_err(|e| e.to_string())?;
    crate::set_private_permissions(path, false).map_err(|e| e.to_string())
}

fn session_rules() -> &'static Mutex<Vec<Rule>> {
    static SESSION: OnceLock<Mutex<Vec<Rule>>> = OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(Vec::new()))
}

/// Every rule in force right now, persisted first so a session rule can
/// override one saved earlier.
pub(crate) fn rules(state: &AppState) -> Vec<Rule> {
    let mut all = read_rules(&store_path(state));
    if let Ok(session) = session_rules().lock() {
        all.extend(session.iter().cloned());
    }
    all
}

pub(crate) fn grant(state: &AppState, rule: Rule) -> Result<Rule, String> {
    if rule.scope.persists() {
        let path = store_path(state);
        let mut rules = read_rules(&path);
        rules.push(rule.clone());
        write_rules(&path, &rules)?;
        return Ok(rule);
    }
    session_rules()
        .lock()
        .map_err(|_| "The session rule list is unavailable.".to_string())?
        .push(rule.clone());
    Ok(rule)
}

/// Remove matching rules and report how many were removed. A `scope` of `None`
/// removes the action/target from every scope, which is what a "revoke" button
/// should do.
pub(crate) fn revoke(
    state: &AppState,
    action: Action,
    target: &str,
    scope: Option<Scope>,
    project: &str,
) -> usize {
    let wanted_target = normalise_target(target);
    let matches = |rule: &Rule| {
        rule.action == action
            && normalise_target(&rule.target) == wanted_target
            && scope.map_or(true, |wanted| rule.scope == wanted)
            && (project.is_empty() || rule.project == project)
    };
    let path = store_path(state);
    let mut persisted = read_rules(&path);
    let before = persisted.len();
    persisted.retain(|rule| !matches(rule));
    let removed_persisted = before - persisted.len();
    if removed_persisted > 0 {
        let _ = write_rules(&path, &persisted);
    }
    let mut removed_session = 0;
    if let Ok(mut session) = session_rules().lock() {
        let before = session.len();
        session.retain(|rule| !matches(rule));
        removed_session = before - session.len();
    }
    removed_persisted + removed_session
}

/// Drop `once` rules that were just used, so a single-use grant really is one
/// use.
fn consume_once(state: &AppState, action: Action, target: &str, project: &str) {
    let wanted = normalise_target(target);
    let mine = |rule: &Rule| {
        let rule_target = normalise_target(&rule.target);
        rule.action == action
            && rule.scope == Scope::Once
            && (rule.project.is_empty() || rule.project == project)
            && (rule_target.is_empty() || wanted.starts_with(&rule_target))
    };
    let path = store_path(state);
    let mut persisted = read_rules(&path);
    let before = persisted.len();
    persisted.retain(|rule| !mine(rule));
    if persisted.len() != before {
        let _ = write_rules(&path, &persisted);
    }
    if let Ok(mut session) = session_rules().lock() {
        session.retain(|rule| !mine(rule));
    }
}

/// The decision for one action on one target. No rule means the action's own
/// default, and a `once` rule is used up by answering.
pub(crate) fn check(state: &AppState, action: Action, target: &str, project: &str) -> Effective {
    let all = rules(state);
    match resolve(all.iter(), action, target, project) {
        Some(effective) => {
            if effective.scope == Scope::Once {
                consume_once(state, action, target, project);
            }
            effective
        }
        None => Effective {
            decision: action.default_decision(),
            scope: Scope::Always,
        },
    }
}

/// What a route that a *person* is driving should do about a saved rule. Rules
/// are enforced exactly as written; when no rule covers the action, the click
/// itself is the consent, so the action's ask-by-default is not applied here —
/// an editor save or an auto-opened terminal must not turn into a prompt the
/// user never asked for. The agent path keeps those defaults.
pub(crate) fn explicit(
    state: &AppState,
    action: Action,
    target: &str,
    project: &str,
) -> Option<Effective> {
    let all = rules(state);
    let effective = resolve(all.iter(), action, target, project)?;
    if effective.scope == Scope::Once {
        consume_once(state, action, target, project);
    }
    Some(effective)
}

/// Gate for a privileged route the UI calls. A saved deny is answered with 403,
/// a saved ask with 409 + `needs_approval` unless the request carries
/// `approve: true` — the person's second request is the yes. Every refusal and
/// every approved ask is audited.
pub(crate) fn guard_user(
    state: &AppState,
    action: Action,
    target: &str,
    project: &str,
    approve: bool,
    detail: &str,
) -> Option<axum::response::Response> {
    let effective = explicit(state, action, target, project)?;
    let scope = effective.scope;
    let shown = if target.is_empty() {
        "everything"
    } else {
        target
    };
    match (effective.decision, approve) {
        (Decision::Allow, _) => None,
        (Decision::Ask, true) => {
            record(
                state,
                action,
                target,
                Decision::Allow,
                scope,
                project,
                "approved by the user",
            );
            None
        }
        (Decision::Ask, false) => {
            record(state, action, target, Decision::Ask, scope, project, detail);
            let message = format!(
                "{} on \"{shown}\" needs approval before it runs. Approve it, or change the rule in Settings → Permissions.",
                action.as_str()
            );
            Some(axum::response::IntoResponse::into_response((
                axum::http::StatusCode::CONFLICT,
                axum::Json(serde_json::json!({ "error": message, "needs_approval": true })),
            )))
        }
        (Decision::Deny, _) => {
            record(
                state,
                action,
                target,
                Decision::Deny,
                scope,
                project,
                detail,
            );
            let message = format!(
                "A saved permission rule denies {} on \"{shown}\". Change it in Settings → Permissions.",
                action.as_str()
            );
            Some(axum::response::IntoResponse::into_response((
                axum::http::StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({ "error": message })),
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// Audit trail

pub(crate) fn initialize(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS permission_audit (
            id INTEGER PRIMARY KEY AUTOINCREMENT, created_at REAL NOT NULL,
            action TEXT NOT NULL, target TEXT NOT NULL, decision TEXT NOT NULL,
            scope TEXT NOT NULL, project TEXT NOT NULL DEFAULT '',
            detail TEXT NOT NULL DEFAULT '',
            request_id TEXT NOT NULL DEFAULT ''
        );",
    )
}

/// Record what was decided. A failed write must never fail the operation it
/// describes — the caller's action already happened or was refused — so this
/// deliberately ignores the error.
pub(crate) fn record(
    state: &AppState,
    action: Action,
    target: &str,
    decision: Decision,
    scope: Scope,
    project: &str,
    detail: &str,
) {
    // A decision that lets the action through is not news; a refusal or a prompt
    // is exactly what someone reading the log after the fact is looking for.
    // Editing the rule list is bookkeeping, not a decision about an action, so
    // it stays in the audit table and the editor rather than the process log —
    // otherwise "a deny rule was added" reads like something was denied.
    let bookkeeping = detail == "rule added" || detail.starts_with("rule revoked");
    if decision != Decision::Allow && !bookkeeping {
        let project_note = if project.is_empty() {
            String::new()
        } else {
            format!(" in {project}")
        };
        crate::oplog::record(
            if decision == Decision::Deny { "warn" } else { "info" },
            "permission",
            decision.as_str(),
            format!("{}{project_note}: {detail}", action.as_str()),
        );
    }
    // The id of the HTTP call that asked for this, when there was one. A
    // background sweep has none, and an empty value says so rather than
    // borrowing an id from whatever ran next.
    let request_id = crate::trace::current();
    let _ = crate::open_db(&state.database_path).and_then(|conn| {
        conn.execute(
            "INSERT INTO permission_audit(created_at,action,target,decision,scope,project,detail,request_id) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                crate::now_seconds(),
                action.as_str(),
                target.chars().take(1000).collect::<String>(),
                decision.as_str(),
                scope.as_str(),
                project,
                detail.chars().take(500).collect::<String>(),
                request_id,
            ],
        )
    });
}

pub(crate) fn recent(state: &AppState, limit: usize) -> Vec<Value> {
    let limit = limit.clamp(1, 500) as i64;
    let Ok(conn) = crate::open_db(&state.database_path) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT created_at, action, target, decision, scope, project, detail \
         FROM permission_audit ORDER BY id DESC LIMIT ?1",
    ) else {
        return Vec::new();
    };
    stmt.query_map([limit], |row| {
        Ok(json!({
            "created_at": row.get::<_, f64>(0)?,
            "action": row.get::<_, String>(1)?,
            "target": row.get::<_, String>(2)?,
            "decision": row.get::<_, String>(3)?,
            "scope": row.get::<_, String>(4)?,
            "project": row.get::<_, String>(5)?,
            "detail": row.get::<_, String>(6)?,
        }))
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Routes

pub(crate) fn router(port: u16) -> Router<AppState> {
    let read_only = Router::new()
        .route("/api/permissions", get(list_permissions))
        .route("/api/permissions/audit", get(audit));
    let mutations = Router::new()
        .route("/api/permissions/grant", post(grant_rule))
        .route("/api/permissions/revoke", post(revoke_rule))
        .route("/api/emergency-stop", post(emergency_stop))
        .route_layer(axum::middleware::from_fn_with_state(
            crate::workspace::LocalOriginGuard::new(port),
            crate::workspace::guard_local_origin,
        ));
    read_only.merge(mutations)
}

async fn list_permissions(State(state): State<AppState>) -> Response {
    let rules = rules(&state)
        .into_iter()
        .map(|rule| {
            json!({
                "action": rule.action.as_str(),
                "target": rule.target,
                "decision": rule.decision.as_str(),
                "scope": rule.scope.as_str(),
                "project": rule.project,
                "created_at": rule.created_at,
            })
        })
        .collect::<Vec<_>>();
    let actions = Action::ALL
        .iter()
        .map(|action| {
            json!({
                "id": action.as_str(),
                "description": action.describe(),
                "default": action.default_decision().as_str(),
            })
        })
        .collect::<Vec<_>>();
    Json(json!({ "ok": true, "actions": actions, "rules": rules })).into_response()
}

async fn audit(
    State(state): State<AppState>,
    AxumQuery(params): AxumQuery<std::collections::HashMap<String, String>>,
) -> Response {
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(50);
    Json(json!({ "ok": true, "entries": recent(&state, limit) })).into_response()
}

/// Global emergency stop: cancel every live agent run and close every terminal
/// session in one call. This route is deliberately NOT policy-gated — a deny
/// rule must never be able to hold the safety release closed — but it is the
/// one privileged operation recorded with an unconditional allow in the audit
/// trail, so every firing is visible in the Permissions panel even when it
/// stops nothing.
async fn emergency_stop(State(state): State<AppState>) -> Response {
    let reason = "Cancelled by the global emergency stop.";
    let runs = crate::agent::cancel_all_active(&state, reason);
    let terminals = crate::terminal::stop_all(&state.terminal);
    let detail = format!(
        "stopped {} live run{} and {} terminal session{}",
        runs.len(),
        if runs.len() == 1 { "" } else { "s" },
        terminals.len(),
        if terminals.len() == 1 { "" } else { "s" },
    );
    record(
        &state,
        Action::Command,
        "emergency-stop",
        Decision::Allow,
        Scope::Session,
        "",
        &detail,
    );
    Json(json!({
        "ok": true,
        "runs": runs,
        "terminals": terminals,
    }))
    .into_response()
}

fn clean_target(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .chars()
        .take(1000)
        .collect()
}

/// Add a rule. Every field is validated here, so the store only ever holds
/// values the resolver understands.
async fn grant_rule(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let Some(action) = input
        .get("action")
        .and_then(Value::as_str)
        .and_then(Action::parse)
    else {
        let names = Action::ALL
            .iter()
            .map(|action| action.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            format!("Name one of these actions: {names}."),
        );
    };
    let Some(decision) = input
        .get("decision")
        .and_then(Value::as_str)
        .and_then(Decision::parse)
    else {
        return crate::json_error(
            StatusCode::BAD_REQUEST,
            "Decision must be allow, ask or deny.",
        );
    };
    let scope = match input.get("scope").and_then(Value::as_str) {
        Some(value) => match Scope::parse(value) {
            Some(scope) => scope,
            None => {
                return crate::json_error(
                    StatusCode::BAD_REQUEST,
                    "Scope must be once, session, project or always.",
                )
            }
        },
        None => Scope::Session,
    };
    let rule = Rule {
        action,
        target: clean_target(input.get("target")),
        decision,
        scope,
        project: clean_target(input.get("project")),
        created_at: crate::now_seconds(),
    };
    record(
        &state,
        action,
        &rule.target,
        decision,
        scope,
        &rule.project,
        "rule added",
    );
    match grant(&state, rule.clone()) {
        Ok(stored) => Json(json!({
            "ok": true,
            "rule": {
                "action": stored.action.as_str(),
                "target": stored.target,
                "decision": stored.decision.as_str(),
                "scope": stored.scope.as_str(),
                "project": stored.project,
                "created_at": stored.created_at,
            }
        }))
        .into_response(),
        Err(error) => crate::json_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

async fn revoke_rule(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    let Some(action) = input
        .get("action")
        .and_then(Value::as_str)
        .and_then(Action::parse)
    else {
        return crate::json_error(StatusCode::BAD_REQUEST, "Name an action to revoke.");
    };
    let target = clean_target(input.get("target"));
    let scope = input
        .get("scope")
        .and_then(Value::as_str)
        .and_then(Scope::parse);
    let project = clean_target(input.get("project"));
    let removed = revoke(&state, action, &target, scope, &project);
    record(
        &state,
        action,
        &target,
        Decision::Deny,
        scope.unwrap_or(Scope::Always),
        &project,
        &format!("rule revoked ({removed})"),
    );
    Json(json!({ "ok": true, "removed": removed, "effective": check(&state, action, &target, &project).to_json() }))
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(action: Action, target: &str, decision: Decision, scope: Scope) -> Rule {
        Rule {
            action,
            target: target.to_string(),
            decision,
            scope,
            project: String::new(),
            created_at: 0.0,
        }
    }

    #[test]
    fn path_targets_match_across_separators_and_trailing_slashes() {
        // The bug this guards: a Windows caller builds its target with
        // `Path::join` (backslashes) while the rule was written with `/`.
        let file = rule(Action::FsWrite, "C:/work/ws/.env", Decision::Deny, Scope::Always);
        assert!(applies(&file, Action::FsWrite, r"C:\work\ws\.env", ""));
        assert!(applies(&file, Action::FsWrite, "C:/work/ws/.env", ""));
        assert!(!applies(&file, Action::FsWrite, r"C:\work\ws\other.txt", ""));
        let directory = rule(Action::FsWrite, "C:/work/ws/", Decision::Deny, Scope::Always);
        assert!(applies(
            &directory,
            Action::FsWrite,
            r"C:\work\ws\.env",
            ""
        ));
        assert_eq!(normalise_target(r"C:\work\ws\"), "C:/work/ws");
    }

    #[test]
    fn every_action_round_trips_through_its_wire_name() {
        for action in Action::ALL {
            assert_eq!(Action::parse(action.as_str()), Some(action));
        }
        assert_eq!(Action::parse("MCP_TOOL"), Some(Action::McpTool));
        assert_eq!(Action::parse("root"), None);
    }

    #[test]
    fn decisions_and_scopes_reject_unknown_words() {
        assert_eq!(Decision::parse("DENY"), Some(Decision::Deny));
        assert_eq!(Decision::parse("maybe"), None);
        for scope in [Scope::Once, Scope::Session, Scope::Project, Scope::Always] {
            assert_eq!(Scope::parse(scope.as_str()), Some(scope));
        }
        assert_eq!(Scope::parse("forever"), None);
        assert!(Scope::Always.persists());
        assert!(!Scope::Once.persists());
    }

    #[test]
    fn reads_default_to_allow_and_destructive_actions_to_ask() {
        assert_eq!(Action::FsRead.default_decision(), Decision::Allow);
        for action in [
            Action::FsWrite,
            Action::Command,
            Action::RouterInstall,
            Action::Update,
            Action::Settings,
            Action::McpTool,
        ] {
            assert_eq!(action.default_decision(), Decision::Ask);
        }
    }

    #[test]
    fn no_matching_rule_falls_back_to_the_default() {
        let rules = [rule(Action::FsRead, "", Decision::Deny, Scope::Always)];
        assert_eq!(
            resolve(rules.iter(), Action::FsWrite, "src/main.rs", ""),
            None
        );
    }

    #[test]
    fn the_longest_target_wins() {
        let rules = [
            rule(Action::FsWrite, "", Decision::Allow, Scope::Always),
            rule(Action::FsWrite, "secrets/", Decision::Deny, Scope::Always),
        ];
        let effective = resolve(rules.iter(), Action::FsWrite, "secrets/token.txt", "").unwrap();
        assert_eq!(effective.decision, Decision::Deny);
        let other = resolve(rules.iter(), Action::FsWrite, "src/main.rs", "").unwrap();
        assert_eq!(other.decision, Decision::Allow);
    }

    #[test]
    fn deny_beats_allow_at_the_same_specificity() {
        let rules = [
            rule(Action::Command, "rm", Decision::Allow, Scope::Always),
            rule(Action::Command, "rm", Decision::Deny, Scope::Always),
        ];
        let effective = resolve(rules.iter(), Action::Command, "rm -rf build", "").unwrap();
        assert_eq!(effective.decision, Decision::Deny);
    }

    #[test]
    fn the_narrower_scope_breaks_a_remaining_tie() {
        let rules = [
            rule(Action::FsWrite, "src/", Decision::Deny, Scope::Always),
            rule(Action::FsWrite, "src/", Decision::Deny, Scope::Once),
        ];
        let effective = resolve(rules.iter(), Action::FsWrite, "src/a.rs", "").unwrap();
        assert_eq!(effective.scope, Scope::Once);
    }

    #[test]
    fn a_project_rule_does_not_leak_into_another_project() {
        let mut scoped = rule(Action::FsWrite, "", Decision::Deny, Scope::Project);
        scoped.project = "alpha".to_string();
        let rules = [scoped];
        assert!(resolve(rules.iter(), Action::FsWrite, "a.txt", "beta").is_none());
        assert_eq!(
            resolve(rules.iter(), Action::FsWrite, "a.txt", "alpha")
                .unwrap()
                .decision,
            Decision::Deny
        );
    }

    #[test]
    fn a_rules_file_round_trips_and_unreadable_input_is_empty() {
        let dir = std::env::temp_dir().join(format!("saarthi-perm-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("permissions.json");
        let rules = vec![
            rule(Action::FsWrite, "secrets/", Decision::Deny, Scope::Always),
            rule(Action::Command, "", Decision::Ask, Scope::Session),
        ];
        write_rules(&path, &rules).unwrap();
        assert_eq!(read_rules(&path), rules);
        // A corrupt file is empty rules, never a panic or a wider policy.
        fs::write(&path, b"{not json").unwrap();
        assert!(read_rules(&path).is_empty());
        assert!(read_rules(&dir.join("missing.json")).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_command_prefix_rule_only_covers_commands_that_start_with_it() {
        let rules = [rule(
            Action::Command,
            "npm test",
            Decision::Allow,
            Scope::Session,
        )];
        assert!(resolve(rules.iter(), Action::Command, "npm test -- --watch", "").is_some());
        assert!(resolve(rules.iter(), Action::Command, "rm -rf /", "").is_none());
    }
}
