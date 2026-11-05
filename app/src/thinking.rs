//! Thinking control: how much reasoning a turn asks for, and where that answer
//! came from.
//!
//! Precedence is fixed and reported to the UI: message > agent > project >
//! global > provider default. A level is never inferred from a model's name —
//! the caller decides whether the chosen model has published reasoning support
//! (`capabilities.rs`) and simply sends no parameter when it has not.
//!
//! `@Thinking` is the composer shortcut. It is a directive only at the very
//! start of a message, and `\@Thinking` writes the token literally, so prose
//! about the feature never changes what the request does.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The four levels the UI offers. "Model default" is the absence of a level,
/// not a fifth value: nothing is sent and the provider's own default applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ThinkingLevel {
    Off,
    Low,
    Medium,
    High,
}

impl ThinkingLevel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Case- and space-insensitive parse of a stored or submitted level.
    /// `None` for anything else, including "default" — an unset level is the
    /// provider default, and an unrecognised word is not silently promoted to
    /// one of the four.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => Some(Self::Off),
            "low" | "minimal" => Some(Self::Low),
            "medium" | "med" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    /// The `reasoning.effort` value the OpenAI API documents. `Off` has no
    /// effort value on purpose: several models answer HTTP 400 to
    /// `effort: "none"`, so "off" means "send no reasoning parameter at all".
    pub(crate) fn effort(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
        }
    }

    /// What the level means, for a UI that must not promise more than it does.
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Self::Off => "No reasoning parameter is sent; the model's own default applies.",
            Self::Low => "Asks for a short reasoning pass — fastest, cheapest.",
            Self::Medium => "Asks for a balanced reasoning pass.",
            Self::High => "Asks for a deeper reasoning pass — slowest, most tokens.",
        }
    }
}

/// Where the level in force came from. Serialised so the composer can say
/// "high, from this message" instead of showing an unexplained setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Scope {
    Message,
    Agent,
    Project,
    Global,
}

impl Scope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Agent => "agent",
            Self::Project => "project",
            Self::Global => "global",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub level: ThinkingLevel,
    pub scope: Scope,
}

/// First configured scope wins, exactly as the specification orders it.
/// `None` means no scope set a level, so the provider default applies.
pub(crate) fn resolve(
    message: Option<ThinkingLevel>,
    agent: Option<&str>,
    project: Option<&str>,
    global: Option<&str>,
) -> Option<Resolved> {
    if let Some(level) = message {
        return Some(Resolved {
            level,
            scope: Scope::Message,
        });
    }
    for (value, scope) in [
        (agent, Scope::Agent),
        (project, Scope::Project),
        (global, Scope::Global),
    ] {
        if let Some(level) = value.and_then(ThinkingLevel::parse) {
            return Some(Resolved { level, scope });
        }
    }
    None
}

/// The `reasoning` object a Responses API request carries. `None` when the
/// level asks for no reasoning, so an "off" turn is byte-for-byte the request
/// it was before Thinking existed.
pub(crate) fn responses_reasoning(level: ThinkingLevel) -> Option<Value> {
    level.effort().map(|effort| {
        json!({
            "effort": effort,
            // Concise user-visible summaries, never raw chain-of-thought:
            // the API's own `summary` channel is what it is willing to expose.
            "summary": "auto",
        })
    })
}

/// The Chat Completions equivalent, for reasoning-capable models reached
/// through a router that publishes `reasoning_effort` in its parameter list.
pub(crate) fn completions_reasoning(level: ThinkingLevel) -> Option<&'static str> {
    level.effort()
}

/// One parsed composer directive.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Directive {
    /// The level the message itself asked for (`@Thinking` with no level
    /// leaves this `None`: the command is on, the level is unchanged).
    pub level: Option<ThinkingLevel>,
    /// True only when the token was consumed as a command.
    pub command: bool,
    /// The message to send: the command removed, or the escaped token restored.
    pub text: String,
}

const TOKEN: &str = "@thinking";

/// Recognise `@Thinking` / `@Thinking:high` / `@Thinking high` at the start of
/// a message. Anything that would leave an empty prompt, carry an unknown
/// level, or appear mid-sentence is left exactly as typed — a message is never
/// silently truncated.
pub(crate) fn take_directive(message: &str) -> Directive {
    let literal = |text: String| Directive {
        level: None,
        command: false,
        text,
    };
    let trimmed = message.trim_start();

    // Escaped: the backslash is consumed and the token ships as text.
    if let Some(rest) = trimmed.strip_prefix('\\') {
        if rest.to_ascii_lowercase().starts_with(TOKEN) {
            return literal(rest.to_string());
        }
    }

    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with(TOKEN) {
        return literal(message.to_string());
    }
    let after = &trimmed[TOKEN.len()..];
    let mut level = None;
    let rest = if let Some(rest) = after.strip_prefix(':') {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        match ThinkingLevel::parse(&rest[..end]) {
            Some(parsed) => {
                level = Some(parsed);
                &rest[end..]
            }
            // `@Thinking:banana` is not a command — keep the text intact.
            None => return literal(message.to_string()),
        }
    } else if after.is_empty() {
        after
    } else if after.starts_with(char::is_whitespace) {
        let trimmed_rest = after.trim_start();
        let end = trimmed_rest
            .find(char::is_whitespace)
            .unwrap_or(trimmed_rest.len());
        match ThinkingLevel::parse(&trimmed_rest[..end]) {
            Some(parsed) => {
                level = Some(parsed);
                &trimmed_rest[end..]
            }
            // A bare token followed by prose: the command is on, the prose is
            // the prompt.
            None => after,
        }
    } else {
        // `@ThinkingXYZ` is not our token.
        return literal(message.to_string());
    };
    let text = rest.trim().to_string();
    if text.is_empty() {
        // Nothing to send: treat the whole input as prose rather than
        // rejecting the message after swallowing what the user typed.
        return literal(message.to_string());
    }
    Directive {
        level,
        command: true,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_levels_round_trip_and_aliases_are_accepted() {
        for level in [
            ThinkingLevel::Off,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
        ] {
            assert_eq!(ThinkingLevel::parse(level.as_str()), Some(level));
        }
        assert_eq!(ThinkingLevel::parse("  HIGH "), Some(ThinkingLevel::High));
        assert_eq!(ThinkingLevel::parse("med"), Some(ThinkingLevel::Medium));
        assert_eq!(ThinkingLevel::parse("minimal"), Some(ThinkingLevel::Low));
        assert_eq!(ThinkingLevel::parse("none"), Some(ThinkingLevel::Off));
    }

    #[test]
    fn an_unknown_or_default_level_is_not_a_level() {
        assert_eq!(ThinkingLevel::parse("default"), None);
        assert_eq!(ThinkingLevel::parse(""), None);
        assert_eq!(ThinkingLevel::parse("banana"), None);
    }

    #[test]
    fn off_sends_no_reasoning_parameter_and_the_rest_map_to_effort() {
        assert_eq!(ThinkingLevel::Off.effort(), None);
        assert!(responses_reasoning(ThinkingLevel::Off).is_none());
        for (level, effort) in [
            (ThinkingLevel::Low, "low"),
            (ThinkingLevel::Medium, "medium"),
            (ThinkingLevel::High, "high"),
        ] {
            let body = responses_reasoning(level).expect("effort level carries a parameter");
            assert_eq!(body["effort"], effort);
            assert_eq!(body["summary"], "auto");
            assert_eq!(completions_reasoning(level), Some(effort));
        }
    }

    #[test]
    fn precedence_is_message_then_agent_then_project_then_global() {
        let message = resolve(
            Some(ThinkingLevel::High),
            Some("low"),
            Some("medium"),
            Some("off"),
        )
        .unwrap();
        assert_eq!(message.level, ThinkingLevel::High);
        assert_eq!(message.scope, Scope::Message);

        let agent = resolve(None, Some("low"), Some("medium"), Some("off")).unwrap();
        assert_eq!(agent.level, ThinkingLevel::Low);
        assert_eq!(agent.scope, Scope::Agent);

        let project = resolve(None, None, Some("medium"), Some("off")).unwrap();
        assert_eq!(project.level, ThinkingLevel::Medium);
        assert_eq!(project.scope, Scope::Project);

        let global = resolve(None, None, None, Some("off")).unwrap();
        assert_eq!(global.level, ThinkingLevel::Off);
        assert_eq!(global.scope, Scope::Global);
    }

    #[test]
    fn nothing_configured_means_the_provider_default() {
        assert_eq!(resolve(None, None, None, None), None);
        // An unparseable stored value is "unset", not a silent promotion.
        assert_eq!(resolve(None, Some("default"), Some(""), Some("  ")), None);
    }

    #[test]
    fn a_bare_directive_turns_thinking_on_without_changing_the_level() {
        let directive = take_directive("@Thinking summarise this repo");
        assert!(directive.command);
        assert_eq!(directive.level, None);
        assert_eq!(directive.text, "summarise this repo");
    }

    #[test]
    fn a_level_rides_the_directive_in_both_spellings() {
        let colon = take_directive("@Thinking:high explain the build");
        assert_eq!(colon.level, Some(ThinkingLevel::High));
        assert_eq!(colon.text, "explain the build");

        let space = take_directive("  @thinking medium  explain the build");
        assert_eq!(space.level, Some(ThinkingLevel::Medium));
        assert_eq!(space.text, "explain the build");
    }

    #[test]
    fn prose_that_mentions_the_token_is_left_alone() {
        let mid = take_directive("How do I use @Thinking?");
        assert!(!mid.command);
        assert_eq!(mid.text, "How do I use @Thinking?");

        let prefixy = take_directive("@ThinkingTool is a different word");
        assert!(!prefixy.command);
    }

    #[test]
    fn a_backslash_writes_the_token_literally() {
        let escaped = take_directive(r"\@Thinking is the shortcut name");
        assert!(!escaped.command);
        assert_eq!(escaped.text, "@Thinking is the shortcut name");
    }

    #[test]
    fn an_unknown_level_or_empty_prompt_is_never_swallowed() {
        let unknown = take_directive("@Thinking:banana what now");
        assert!(!unknown.command);
        assert_eq!(unknown.text, "@Thinking:banana what now");

        let only = take_directive("@Thinking");
        assert!(!only.command);
        assert_eq!(only.text, "@Thinking");
    }
}
