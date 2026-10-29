//! Outbound privacy policy: nothing that looks like a credential leaves the
//! machine inside model context.
//!
//! Two mechanisms, both surfaced to the user (never silently):
//! 1. content scan — obvious credential shapes are masked before the text is
//!    packed into any provider request;
//! 2. path exclusions — configured workspace paths are never sent at all.
//!
//! The scanner is deliberately regex-free line matching over `char`s: the
//! shapes it looks for are bounded and well known, and false positives only
//! cost a masking, while false negatives would leak. Anything it masks is also
//! reported with a human-readable notice so the user can see what was filtered.

const KEY_NAMES: [&str; 9] = [
    "api_key", "apikey", "api-key", "secret", "token", "password", "passwd", "access_key",
    "private_key",
];

/// Scan text and mask any value that looks like a credential. Returns the
/// masked text plus the distinct detector kinds that fired.
pub(crate) fn sanitize(text: &str) -> (String, Vec<String>) {
    let mut masked_lines: Vec<String> = Vec::with_capacity(text.lines().count());
    let mut findings = Vec::new();
    let mut inside_key_block = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("-----BEGIN") {
            inside_key_block = true;
            // Headers are boilerplate, not secret; the body that follows is.
            masked_lines.push(line.to_string());
            push_kind(&mut findings, "pem");
            continue;
        }
        if trimmed.starts_with("-----END") {
            inside_key_block = false;
            masked_lines.push(line.to_string());
            continue;
        }
        if inside_key_block {
            masked_lines.push("[masked: pem]".to_string());
            push_kind(&mut findings, "pem");
            continue;
        }
        let (line_out, kinds) = scan_plain_line(line);
        masked_lines.push(line_out);
        for kind in kinds {
            push_kind(&mut findings, kind);
        }
    }
    let out = masked_lines.join("\n");
    let out = if text.ends_with('\n') && !out.is_empty() {
        format!("{out}\n")
    } else {
        out
    };
    (out, findings)
}



fn push_kind(findings: &mut Vec<String>, kind: &str) {
    if !findings.iter().any(|existing| existing == kind) {
        findings.push(kind.to_string());
    }
}

/// One non-PEM line: return it (possibly masked) with the kinds that fired.
fn scan_plain_line(line: &str) -> (String, Vec<&'static str>) {
    let mut kinds = Vec::new();
    let lower = line.to_ascii_lowercase();
    let mut out = line.to_string();

    if lower.contains("private key") {
        kinds.push("pem");
    }
    if (lower.contains("bearer") || lower.contains("authorization")) && has_token_run(&lower) {
        out = mask_runs(&out);
        kinds.push("bearer");
    }
    if lower.contains("akia") && has_token_run_near(&lower, "akia") {
        out = mask_runs(&out);
        kinds.push("aws");
    }
    if lower.starts_with("eyj") {
        let eyj_count = lower.matches("eyj").count();
        if eyj_count >= 2 {
            out = mask_runs(&out);
            kinds.push("jwt");
        }
    }
    for name in KEY_NAMES {
        if assignments_key(&lower, name) {
            out = mask_assignments(&out, name);
            kinds.push("key-assignment");
            break;
        }
    }
    (out, kinds)
}

/// `key = value` / `key: value` shapes for a given key name, with a long value.
fn assignments_key(lower: &str, name: &str) -> bool {
    let mut rest = lower;
    while let Some(pos) = rest.find(name) {
        let after = &rest[pos + name.len()..];
        // "keyhole" is not a key named "key".
        let next = after.chars().next().unwrap_or('_');
        let partial_word = next.is_alphanumeric() || next == '_';
        let value = after
            .trim_start_matches(['\t', '\n', '\r', ' '])
            .trim_start_matches('"')
            .trim_start();
        if let Some(remainder) = value.strip_prefix(['=', ':']) {
            let value = remainder.trim_start_matches(['"']);
            let len = value.split('\n').next().unwrap_or("").chars().filter(|c| !c.is_whitespace()).count();
            if len >= 16 && !partial_word {
                return true;
            }
            return false;
        }
        rest = &rest[pos + name.len()..];
    }
    false
}

/// A plausibly-random long run of token characters.
fn has_token_run(text: &str) -> bool {
    let mut run = 0;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || "-_./+=#".contains(ch) {
            run += 1;
            if run >= 24 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// Long token-ish run that follows a trigger substring.
fn has_token_run_near(lower: &str, trigger: &str) -> bool {
    match lower.find(trigger) {
        Some(index) => has_token_run(&lower[index..]),
        None => false,
    }
}

/// Mask every ≥24-char token-looking run. Char based: byte slicing is never
/// safe on text that may contain Unicode.
fn mask_runs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || "-_./+=#".contains(ch) {
            run.push(ch);
        } else {
            flush_run(&mut out, &run);
            run.clear();
            out.push(ch);
        }
    }
    flush_run(&mut out, &run);
    out
}

fn flush_run(out: &mut String, run: &str) {
    if run.chars().count() >= 24 {
        out.push_str("[masked]");
    } else {
        out.push_str(run);
    }
}

/// Mask the value side of every `key = value` assignment whose value is 16
/// characters or longer (any of the KEY_NAMES keys).
fn mask_assignments(text: &str, _kind: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for name in KEY_NAMES {
        chars = mask_assignments_in(&chars, name);
    }
    chars.into_iter().collect()
}

fn mask_assignments_in(chars: &[char], name: &str) -> Vec<char> {
    let name_units: Vec<char> = name.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        // Case-insensitive check for the key name at this position.
        let mut matched = chars[index..].len() >= name_units.len();
        for (offset, key_ch) in name_units.iter().enumerate() {
            if matched && chars[index + offset].to_ascii_lowercase() != *key_ch {
                matched = false;
            }
        }
        if !matched {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        let after = index + name_units.len();
        // "keyhole" is not a key named "key": the next char must not extend
        // the word.
        let partial_word = after < chars.len()
            && (chars[after].is_alphanumeric() || chars[after] == '_');
        if partial_word {
            out.extend(&chars[index..after]);
            index = after;
            continue;
        }
        let mut cursor = after;
        while cursor < chars.len() && (chars[cursor].is_whitespace() || chars[cursor] == '"') {
            cursor += 1;
        }
        if !(cursor < chars.len() && (chars[cursor] == '=' || chars[cursor] == ':')) {
            out.extend(&chars[index..after]);
            index = after;
            continue;
        }
        cursor += 1;
        while cursor < chars.len() && (chars[cursor].is_whitespace() || chars[cursor] == '"') {
            cursor += 1;
        }
        let value: Vec<char> = chars[cursor..]
            .iter()
            .copied()
            .take_while(|ch| *ch != '\n')
            .collect();
        let value_len = value.iter().filter(|ch| !ch.is_whitespace()).count();
        if value_len < 16 {
            out.extend(&chars[index..after]);
            index = after;
            continue;
        }
        out.extend(&chars[index..cursor]);
        out.extend("[masked]".chars());
        index = cursor + value.len();
    }
    out
}

/// True when a workspace-relative path is on the user's exclusion list.
pub(crate) fn is_excluded(path: &str, exclusions: &[String]) -> bool {
    exclusions.iter().any(|rule| {
        let rule = rule.trim().trim_start_matches("./").trim_end_matches('/');
        if rule.is_empty() || rule == "." || rule.is_empty() {
            return false;
        }
        // Entry paths may arrive with a leading "./"; normalize before match.
        let path = path.trim_start_matches("./");
        path == rule || path.starts_with(&format!("{rule}/"))
    })
}

/// Human-readable summary of what the outbound policy changed, or None.
pub(crate) fn notice_text(findings: &[String], excluded_paths: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    if !findings.is_empty() {
        parts.push(format!(
            "possible credentials masked before sending ({}: {})",
            findings.len(),
            findings.join(", ")
        ));
    }
    if !excluded_paths.is_empty() {
        parts.push(format!(
            "excluded files were not sent: {}",
            excluded_paths.join(", ")
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_masked(masked: &str, sentinel: &str, kind: &str) -> (String, Vec<String>) {
        let (out, kinds) = sanitize(masked);
        assert!(!out.contains(sentinel), "sentinel leaked: {out}");
        assert!(
            kinds.iter().any(|k| k == kind),
            "kind {kind} missing from {kinds:?}"
        );
        (out, kinds)
    }

    #[test]
    fn masks_obvious_credentials() {
        let secret_env = "SERVICE_API_KEY=sk-totally-real-key-abcdef1234567890\nCOLOR=blue\n";
        let (out, _) = assert_masked(secret_env, "sk-totally-real", "key-assignment");
        assert!(out.contains("SERVICE_API_KEY="), "{out}");
        assert!(out.contains("COLOR=blue"), "ordinary lines untouched: {out}");
        assert!(out.contains("[masked]"), "{out}");

        let bearer = "Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123456789\n";
        assert_masked(bearer, "abcdefghijklmnopqrstuvwxyz0123456789", "bearer");

        let aws = "aws_access_key_id=AKIAfaketoken000000000123\n";
        assert_masked(aws, "AKIAfaketoken", "aws");

        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signaturepartsignaturepart\n";
        assert_masked(jwt, "signaturepartsignaturepart", "jwt");

        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIaaaabcdefghijklmno\n-----END RSA PRIVATE KEY-----\n";
        let (out, _) = assert_masked(pem, "MIIaaaabcdefghijklmno", "pem");
        assert!(out.contains("-----BEGIN"), "{out}");
        assert!(out.contains("-----END"), "{out}");
    }

    #[test]
    fn clean_text_passes_through_unchanged() {
        let text = "let config = Config::default();\nexport function api() { build(); }\nplain words here are fine\n";
        let (out, kinds) = sanitize(text);
        assert_eq!(out, text);
        assert!(kinds.is_empty(), "{kinds:?}");
    }

    #[test]
    fn short_values_are_not_masked() {
        let text = "token=short\npassword=1234\nkey='fourteen chars '\n";
        let (out, kinds) = sanitize(text);
        assert!(out.contains("token=short"));
        assert!(kinds.is_empty(), "{kinds:?}");
    }

    #[test]
    fn exclusion_rules_match_paths_and_prefixes_only() {
        let rules = vec![
            ".env".to_string(),
            "secrets/".to_string(),
            " certs ".to_string(),
        ];
        assert!(is_excluded(".env", &rules));
        assert!(is_excluded("secrets/prod.pem", &rules));
        assert!(is_excluded("certs/host.key", &rules), "trimmed rule");
        assert!(is_excluded("./.env", &rules));
        assert!(!is_excluded("src/main.rs", &rules));
        // Over-exclusion is the safe direction: a file literally named like
        // the excluded folder is also kept off the wire.
        assert!(is_excluded("secrets", &rules));
        assert!(!is_excluded("", &rules));
    }

    #[test]
    fn notice_text_summarizes_findings_and_exclusions() {
        assert!(notice_text(&[], &[]).is_none());
        let text = notice_text(
            &["bearer".to_string()],
            &["certs/host.key".to_string()],
        )
        .unwrap();
        assert!(text.contains("bearer"));
        assert!(text.contains("certs/host.key"));
    }
}
