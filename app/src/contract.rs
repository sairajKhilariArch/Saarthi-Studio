//! The local capability contract: what this build can actually do.
//!
//! A local client — the desktop UI, the VS Code extension, any future embedding
//! host — used to assume a feature existed because a route was compiled in. This
//! module is the one answer to that question: a stable id per capability, a
//! version for the shape of the answer, and the limits a client has to respect.
//! An id is added only once the feature behind it works end to end; the list is
//! not aspirational.
//!
//! The Android companion negotiates its own v1 handshake over the Host API
//! (`/api/host/v1/capabilities`); this contract covers the local API, which is
//! what the desktop UI and the VS Code extension talk to.

use serde_json::{json, Map, Value};

/// The contract's own version. Bumped when an id changes meaning or is removed;
/// a client that does not read this number must fall back to what it was built
/// with instead of guessing.
pub(crate) const CONTRACT: u32 = 1;

/// Every capability this build implements, once. Adding one here is a claim that
/// the route or module behind it exists and is tested.
const FEATURES: &[&str] = &[
    "chat",
    "chat_streaming",
    "agents",
    "agent_queue",
    "router_management",
    "terminal",
    "git",
    "git_worktrees",
    "git_reflog_undo",
    "editor_format",
    "editor_complete",
    "saved_commands",
    "watch",
    "skills",
    "mcp",
    "memory",
    "code_index",
    "workflows",
    "extensions",
    "thinking",
    "openai_platform",
    "permissions",
    "deep_search",
    "usage_cost",
    "diagnostics",
];

/// The answer `GET /api/capabilities` serves. `limits` names numbers a client
/// must not exceed, read from the same constants the routes enforce so the
/// contract cannot drift from the behaviour.
pub(crate) fn capabilities() -> Value {
    let mut features = Map::new();
    for id in FEATURES {
        features.insert((*id).to_string(), json!(true));
    }
    json!({
        "contract": CONTRACT,
        "product": crate::APP_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "features": features,
        "limits": {
            "max_concurrent_runs": crate::MAX_CONCURRENT_RUNS,
            "max_saved_commands": crate::runner::MAX_CUSTOM_PRESETS,
            "max_reflog_entries": crate::git::MAX_REFLOG_ENTRIES,
            "max_completions": crate::index::MAX_COMPLETIONS,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_contract_names_this_build_and_every_feature_exactly_once() {
        let value = capabilities();
        assert_eq!(value["contract"], json!(CONTRACT));
        assert_eq!(value["product"], json!(crate::APP_NAME));
        assert_eq!(value["version"], json!(env!("CARGO_PKG_VERSION")));
        assert!(!value["platform"].as_str().unwrap_or("").is_empty());
        assert!(!value["arch"].as_str().unwrap_or("").is_empty());
        let features = value["features"].as_object().expect("features is a map");
        assert_eq!(
            features.len(),
            FEATURES.len(),
            "a duplicate id would shrink the map"
        );
        for (id, supported) in features {
            assert_eq!(supported, &json!(true), "{id} is supported");
            assert!(
                id.chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'),
                "{id} is not a stable snake_case id"
            );
        }
        // The two newest capabilities are present, so the table cannot silently
        // fall behind the tree.
        assert!(features.contains_key("saved_commands"));
        assert!(features.contains_key("git_reflog_undo"));
        assert!(features.contains_key("editor_complete"));
    }

    #[test]
    fn the_limits_come_from_the_enforced_constants() {
        let value = capabilities();
        assert_eq!(
            value["limits"]["max_concurrent_runs"],
            json!(crate::MAX_CONCURRENT_RUNS)
        );
        assert_eq!(
            value["limits"]["max_saved_commands"],
            json!(crate::runner::MAX_CUSTOM_PRESETS)
        );
        assert_eq!(
            value["limits"]["max_reflog_entries"],
            json!(crate::git::MAX_REFLOG_ENTRIES)
        );
        assert_eq!(
            value["limits"]["max_completions"],
            json!(crate::index::MAX_COMPLETIONS)
        );
    }
}
