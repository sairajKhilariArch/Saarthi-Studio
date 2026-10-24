//! Token-budgeted request packing.
//!
//! Every chat request is assembled under a token budget so a long thread with
//! big file attachments does not resend the whole history each turn:
//!
//! * the system prompt is short and optional workspace outline is compact,
//! * old messages that carried an attached code block are replaced by a one
//!   line marker (the attachment is only sent in the turn that references it),
//! * older messages are cropped head/tail and dropped once the budget is used,
//! * the current message is always sent, cropped to the attachment cap.

use serde_json::{json, Value};

/// Rough characters-per-token ratio used everywhere in this app.
const CHARS_PER_TOKEN: usize = 4;
/// Per-message cap for history messages, in tokens.
const HISTORY_MESSAGE_CAP_TOKENS: usize = 1_200;
/// Cap for the message being sent right now, in tokens.
const CURRENT_MESSAGE_CAP_TOKENS: usize = 3_000;

pub(crate) const DEFAULT_BUDGET_TOKENS: usize = 6_000;
pub(crate) const MIN_BUDGET_TOKENS: usize = 1_000;
pub(crate) const MAX_BUDGET_TOKENS: usize = 32_000;
pub(crate) const DEFAULT_HISTORY_MESSAGES: usize = 6;
pub(crate) const MIN_HISTORY_MESSAGES: usize = 2;
pub(crate) const MAX_HISTORY_MESSAGES: usize = 24;

/// Cheap token estimate (~4 characters per token for mixed prose/code).
pub(crate) fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(CHARS_PER_TOKEN)
}

/// Keep a long text inside `max_tokens`, preserving both ends.
pub(crate) fn crop(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens.saturating_mul(CHARS_PER_TOKEN);
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    let head = max_chars * 2 / 3;
    let tail = max_chars.saturating_sub(head + 48);
    let omitted = chars.len().saturating_sub(head + tail);
    format!(
        "{}\n… [{} characters omitted] …\n{}",
        chars[..head].iter().collect::<String>(),
        omitted,
        chars[chars.len() - tail..].iter().collect::<String>()
    )
}

/// Replace a previously attached code block with a marker so old attachments
/// are not re-sent with every request.
pub(crate) fn strip_attached_code(content: &str) -> String {
    if let Some(index) = content.find("Here is the code from my editor") {
        let head = &content[..index];
        let truncated = head.trim_end();
        if truncated.is_empty() {
            return "[the attachment from this message was omitted to save context]".to_string();
        }
        return format!("{truncated}\n[attachment omitted from context; ask to re-attach it]");
    }
    content.to_string()
}

pub(crate) struct PackedRequest {
    pub messages: Vec<Value>,
    pub estimated_tokens: usize,
    pub trimmed_messages: usize,
}

/// Assemble the message list sent to the router under `budget_tokens`.
///
/// `history` must end with the message being sent right now.
pub(crate) fn pack(
    system: &str,
    outline: Option<&str>,
    history: &[Value],
    budget_tokens: usize,
    history_messages: usize,
) -> PackedRequest {
    let budget = budget_tokens.clamp(MIN_BUDGET_TOKENS, MAX_BUDGET_TOKENS);
    let keep = history_messages.clamp(MIN_HISTORY_MESSAGES, MAX_HISTORY_MESSAGES);

    let system_text = match outline {
        Some(outline) if !outline.trim().is_empty() => format!(
            "{system}\n\nWorkspace files (names only, ask before assuming contents):\n{outline}"
        ),
        _ => system.to_string(),
    };
    let mut messages = vec![json!({ "role": "system", "content": system_text })];
    let mut used = estimate_tokens(&system_text);
    let mut trimmed = 0usize;

    // Walk backwards: the newest messages (including the current one) are
    // always kept, older ones are cropped, then dropped at the budget edge.
    let window = history.len().min(keep.max(1));
    let start = history.len() - window;
    for (offset, entry) in history[start..].iter().enumerate() {
        let is_current = offset + 1 == window;
        let role = entry
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .to_string();
        let raw = entry
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let content = if role == "system" {
            continue;
        } else if is_current {
            crop(&raw, CURRENT_MESSAGE_CAP_TOKENS)
        } else {
            crop(&strip_attached_code(&raw), HISTORY_MESSAGE_CAP_TOKENS)
        };
        let cost = estimate_tokens(&content);
        if !is_current && used + cost > budget {
            trimmed += 1;
            continue;
        }
        used += cost;
        messages.push(json!({ "role": role, "content": content }));
    }

    PackedRequest {
        estimated_tokens: used,
        trimmed_messages: trimmed,
        messages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: &str, content: &str) -> Value {
        json!({ "role": role, "content": content })
    }

    #[test]
    fn estimates_tokens_from_characters() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens(&"x".repeat(400)), 100);
    }

    #[test]
    fn crops_long_text_keeping_both_ends() {
        let text = "HEAD".to_string() + &"x".repeat(10_000) + "TAIL";
        let cropped = crop(&text, 100);
        assert!(
            estimate_tokens(&cropped) <= 130,
            "crop should stay near the cap"
        );
        assert!(cropped.starts_with("HEAD"));
        assert!(cropped.ends_with("TAIL"));
        assert!(cropped.contains("characters omitted"));
        // Short text is never touched.
        assert_eq!(crop("short", 100), "short");
    }

    #[test]
    fn old_attachments_are_replaced_by_markers() {
        let raw = "Review this\n\nHere is the code from my editor (language: rust):\n```rust\nfn big() {}\n```";
        let stripped = strip_attached_code(raw);
        assert!(stripped.starts_with("Review this"));
        assert!(!stripped.contains("fn big()"));
        assert!(stripped.contains("attachment omitted"));
        // Messages without attachments are untouched.
        assert_eq!(strip_attached_code("plain question"), "plain question");
    }

    #[test]
    fn pack_respects_the_budget_and_keeps_the_current_message() {
        let history = vec![
            message("user", &"old question ".repeat(200)),
            message("assistant", &"long answer ".repeat(400)),
            message("user", &"newer question ".repeat(200)),
            message("user", "current question"),
        ];
        let packed = pack("You are a concise assistant.", None, &history, 1_000, 6);
        assert!(packed.messages.first().unwrap()["role"] == "system");
        // The current message is always the last message sent.
        assert_eq!(
            packed.messages.last().unwrap()["content"],
            "current question"
        );
        assert!(packed.estimated_tokens <= 1_000 + CURRENT_MESSAGE_CAP_TOKENS * CHARS_PER_TOKEN);
        assert!(packed.trimmed_messages >= 1);
    }

    #[test]
    fn pack_includes_the_workspace_outline_in_the_system_prompt() {
        let history = vec![message("user", "hi")];
        let outline = "my-project/\nsrc/\nmain.rs";
        let packed = pack("System.", Some(outline), &history, 6_000, 6);
        let system = packed.messages[0]["content"].as_str().unwrap();
        assert!(system.contains("Workspace files"));
        assert!(system.contains("main.rs"));
    }

    #[test]
    fn pack_honours_the_history_window() {
        let history: Vec<Value> = (0..10)
            .map(|index| message("user", &format!("message {index}")))
            .collect();
        let packed = pack("System.", None, &history, 6_000, 4);
        assert_eq!(packed.messages.len(), 1 + 4);
        assert_eq!(packed.messages.last().unwrap()["content"], "message 9");
    }
}
