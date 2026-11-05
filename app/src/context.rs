//! Token-budgeted request packing and context compression.
//!
//! Every chat request is assembled under a token budget so a long thread with
//! big file attachments does not resend the whole history each turn:
//!
//! * the system prompt is short and the optional workspace outline is compact,
//! * old messages that carried an attached code block are replaced by a one
//!   line marker (the attachment is only sent in the turn that references it),
//! * whitespace noise is normalised away, and a repeated turn is sent once,
//! * a thread that fits the budget is sent **untouched** — compression only
//!   happens when it is needed,
//! * when it does not fit, the newest turns stay whole while they fit and the
//!   older ones become labelled excerpts in the system prompt, so old context is
//!   condensed rather than silently dropped, and anything that does not even fit
//!   as an excerpt is counted instead of disappearing quietly,
//! * the current message is always sent, cropped to the attachment cap.
//!
//! The excerpts are labelled excerpts, not summaries: this module makes no model
//! call, so it must not imply it rewrote anyone's words.

use serde_json::{json, Value};
use std::collections::HashSet;

/// Rough characters-per-token ratio used everywhere in this app.
const CHARS_PER_TOKEN: usize = 4;
/// Cap for the message being sent right now, in tokens.
const CURRENT_MESSAGE_CAP_TOKENS: usize = 3_000;
/// Characters of a message kept when it is folded into the digest.
const DIGEST_CHARS: usize = 160;

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

/// Collapse the whitespace that inflates a request without adding meaning:
/// trailing spaces on every line and runs of blank lines between real ones.
pub(crate) fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0usize;
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            blank_run += 1;
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
            if blank_run > 0 {
                out.push('\n');
            }
        }
        blank_run = 0;
        out.push_str(trimmed);
    }
    out
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

/// One line standing in for a message too old to send whole: its role and the
/// opening of what it said, plus how much longer it was. An excerpt, never a
/// paraphrase this module cannot vouch for.
fn digest_line(role: &str, content: &str) -> String {
    let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let total = flat.chars().count();
    let head: String = flat.chars().take(DIGEST_CHARS).collect();
    if total > DIGEST_CHARS {
        format!("- [{role}] {head}… ({total} characters in the full message)")
    } else {
        format!("- [{role}] {head}")
    }
}

/// What packing did to the request, so the caller can report it honestly.
pub(crate) struct PackedRequest {
    pub messages: Vec<Value>,
    pub estimated_tokens: usize,
    /// Messages that were not sent whole because they were folded into the
    /// digest (their opening is still in the system prompt).
    pub trimmed_messages: usize,
    /// Messages that are not represented in the request at all: repeated turns,
    /// and digest lines that did not fit the digest's own share of the budget.
    pub dropped_messages: usize,
    /// Estimated tokens the compression kept out of the request.
    pub tokens_saved: usize,
}

struct Entry {
    role: String,
    content: String,
    current: bool,
    tokens: usize,
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

    let mut system_text = match outline {
        Some(outline) if !outline.trim().is_empty() => format!(
            "{system}\n\nWorkspace files (names only, ask before assuming contents):\n{outline}"
        ),
        _ => system.to_string(),
    };

    // The window: the newest messages, because that is what the turn is about.
    let window = history.len().min(keep.max(1));
    let start = history.len() - window;

    let mut entries: Vec<Entry> = Vec::new();
    for (offset, entry) in history[start..].iter().enumerate() {
        let role = entry
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
            .to_string();
        if role == "system" {
            continue;
        }
        let is_current = offset + 1 == window;
        let raw = entry.get("content").and_then(Value::as_str).unwrap_or("");
        let content = if is_current {
            crop(&normalise(raw), CURRENT_MESSAGE_CAP_TOKENS)
        } else {
            normalise(&strip_attached_code(raw))
        };
        entries.push(Entry {
            tokens: estimate_tokens(&content),
            role,
            content,
            current: is_current,
        });
    }

    // A repeated turn adds nothing the model has not already been told. The
    // current message is exempt: the user may deliberately resend it.
    let mut dropped = 0usize;
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut deduped: Vec<Entry> = Vec::with_capacity(entries.len());
    for entry in entries {
        if !entry.current && !seen.insert((entry.role.clone(), entry.content.clone())) {
            dropped += 1;
            continue;
        }
        deduped.push(entry);
    }

    // The thread's first message states the task; when the window has moved past
    // it, keep a pointer to it instead of forgetting it.
    let first_line = if start > 0 {
        history.first().and_then(|entry| {
            let role = entry.get("role").and_then(Value::as_str).unwrap_or("user");
            if role == "system" {
                return None;
            }
            let content = normalise(&strip_attached_code(
                entry.get("content").and_then(Value::as_str).unwrap_or(""),
            ));
            if content.trim().is_empty() {
                return None;
            }
            if deduped
                .iter()
                .any(|existing| existing.role == role && existing.content == content)
            {
                return None;
            }
            Some(digest_line(&format!("{role}, first message"), &content))
        })
    } else {
        None
    };

    let full_tokens = estimate_tokens(&system_text)
        + deduped.iter().map(|entry| entry.tokens).sum::<usize>()
        + first_line.as_deref().map(estimate_tokens).unwrap_or(0);

    // It fits: send everything as it is. Compression is not a style, it is a
    // remedy for a request that would not otherwise fit.
    if full_tokens <= budget {
        let mut messages = vec![json!({ "role": "system", "content": system_text })];
        if let Some(line) = first_line {
            let system = messages[0]["content"].as_str().unwrap_or("").to_string();
            messages[0] = json!({
                "role": "system",
                "content": format!("{system}\n\nStart of this thread, kept as a pointer:\n{line}")
            });
        }
        for entry in &deduped {
            messages.push(json!({ "role": entry.role, "content": entry.content }));
        }
        let estimated = messages
            .iter()
            .map(|message| estimate_tokens(message["content"].as_str().unwrap_or("")))
            .sum();
        return PackedRequest {
            messages,
            estimated_tokens: estimated,
            trimmed_messages: 0,
            dropped_messages: dropped,
            tokens_saved: 0,
        };
    }

    // Over budget. The current message is always sent; the newest older turns
    // stay whole while they fit; everything older becomes one excerpt each.
    let mut current: Option<Entry> = None;
    let mut rest: Vec<Entry> = Vec::new();
    for entry in deduped {
        if entry.current {
            current = Some(entry);
        } else {
            rest.push(entry);
        }
    }
    let current_tokens = current.as_ref().map(|entry| entry.tokens).unwrap_or(0);
    // The digest gets a quarter of the budget at most, reserved before the recent
    // turns are placed, so old context can never crowd out the turns the user is
    // actually continuing.
    let digest_cap = (budget / 4).max(64);
    let mut room =
        budget.saturating_sub(estimate_tokens(&system_text) + current_tokens + digest_cap);
    let mut kept_rev: Vec<Entry> = Vec::new();
    let mut digested_rev: Vec<Entry> = Vec::new();
    for entry in rest.into_iter().rev() {
        if entry.tokens <= room {
            room -= entry.tokens;
            kept_rev.push(entry);
        } else {
            digested_rev.push(entry);
        }
    }

    let mut digest_lines: Vec<String> = Vec::new();
    if let Some(line) = first_line {
        digest_lines.push(line);
    }
    let mut folded = digested_rev.len();
    fold_digest(
        &mut digest_lines,
        digested_rev,
        digest_cap,
        &mut folded,
        &mut dropped,
    );

    if !digest_lines.is_empty() {
        system_text.push_str(
            "\n\nEarlier turns in this thread, condensed to fit the context budget (excerpts, not summaries):\n",
        );
        system_text.push_str(&digest_lines.join("\n"));
    }

    let mut messages = vec![json!({ "role": "system", "content": system_text })];
    for entry in kept_rev.into_iter().rev() {
        messages.push(json!({ "role": entry.role, "content": entry.content }));
    }
    if let Some(entry) = current {
        messages.push(json!({ "role": entry.role, "content": entry.content }));
    }
    let estimated: usize = messages
        .iter()
        .map(|message| estimate_tokens(message["content"].as_str().unwrap_or("")))
        .sum();

    PackedRequest {
        messages,
        estimated_tokens: estimated,
        trimmed_messages: folded,
        dropped_messages: dropped,
        tokens_saved: full_tokens.saturating_sub(estimated),
    }
}

/// Fold the digested messages into excerpt lines inside `cap` tokens, oldest
/// first so the digest reads in order. A line that does not fit is counted as
/// dropped: its message is then genuinely absent, and the caller must be able to
/// say so rather than imply it was condensed.
fn fold_digest(
    digest_lines: &mut Vec<String>,
    digested_rev: Vec<Entry>,
    cap: usize,
    folded: &mut usize,
    dropped: &mut usize,
) {
    let mut used = 0usize;
    let mut kept: Vec<String> = Vec::new();
    for entry in &digested_rev {
        let line = digest_line(&entry.role, &entry.content);
        let cost = estimate_tokens(&line);
        if used + cost > cap {
            *dropped += 1;
            *folded = folded.saturating_sub(1);
            continue;
        }
        used += cost;
        kept.push(line);
    }
    kept.reverse();
    digest_lines.extend(kept);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: &str, content: &str) -> Value {
        json!({ "role": role, "content": content })
    }

    fn text(value: &Value) -> String {
        value["content"].as_str().unwrap_or("").to_string()
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
    fn normalise_drops_trailing_spaces_and_blank_runs_without_editing_words() {
        let raw = "first line   \n\n\n\nsecond line\t\n   \nthird   line  ";
        assert_eq!(normalise(raw), "first line\n\nsecond line\n\nthird   line");
        // Already-clean text comes back unchanged.
        assert_eq!(normalise("a\nb"), "a\nb");
        assert_eq!(normalise(""), "");
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
        assert_eq!(
            packed.messages.last().unwrap()["content"],
            "current question"
        );
        assert!(packed.estimated_tokens <= 1_000 + CURRENT_MESSAGE_CAP_TOKENS);
        assert!(packed.trimmed_messages >= 1);
        assert!(packed.tokens_saved > 0);
        // Old turns are represented as labelled excerpts, not silently erased.
        let system = text(&packed.messages[0]);
        assert!(
            system.contains("Earlier turns in this thread, condensed"),
            "{system}"
        );
        assert!(system.contains("- [assistant]"), "{system}");
    }

    #[test]
    fn a_thread_that_fits_is_sent_untouched() {
        let history = vec![
            message("user", "first question"),
            message("assistant", "first answer"),
            message("user", "follow-up"),
        ];
        let packed = pack("System.", None, &history, 6_000, 6);
        assert_eq!(packed.trimmed_messages, 0);
        assert_eq!(packed.dropped_messages, 0);
        assert_eq!(packed.tokens_saved, 0);
        assert_eq!(packed.messages.len(), 4);
        assert_eq!(text(&packed.messages[1]), "first question");
        // No compression language anywhere in a request that fits.
        assert!(!text(&packed.messages[0]).contains("condensed"));
    }

    #[test]
    fn pack_includes_the_workspace_outline_in_the_system_prompt() {
        let history = vec![message("user", "hi")];
        let outline = "my-project/\nsrc/\nmain.rs";
        let packed = pack("System.", Some(outline), &history, 6_000, 6);
        let system = text(&packed.messages[0]);
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
        // The first message is outside the window; it survives as a pointer.
        let system = text(&packed.messages[0]);
        assert!(system.contains("Start of this thread"), "{system}");
        assert!(system.contains("message 0"), "{system}");
    }

    #[test]
    fn a_repeated_turn_is_sent_once_and_counted() {
        let history = vec![
            message("user", "what does this do?"),
            message("assistant", "it reads the file"),
            message("user", "what does this do?"),
            message("assistant", "it reads the file"),
            message("user", "ok"),
        ];
        let packed = pack("System.", None, &history, 6_000, 6);
        assert_eq!(packed.dropped_messages, 2, "both repeats are dropped");
        assert_eq!(packed.trimmed_messages, 0);
        assert_eq!(packed.messages.len(), 4);
        assert_eq!(text(packed.messages.last().unwrap()), "ok");
    }

    #[test]
    fn the_current_message_is_never_condensed_away() {
        let huge = "now".repeat(20_000);
        let history = vec![
            message("user", &"older ".repeat(4_000)),
            message("assistant", &"reply ".repeat(4_000)),
            message("user", &huge),
        ];
        let packed = pack("System.", None, &history, 1_000, 6);
        let current = text(packed.messages.last().unwrap());
        assert!(current.starts_with("nownow"), "the current message is kept");
        assert!(
            estimate_tokens(&current) <= CURRENT_MESSAGE_CAP_TOKENS + 5,
            "and cropped to its own cap"
        );
    }
}
