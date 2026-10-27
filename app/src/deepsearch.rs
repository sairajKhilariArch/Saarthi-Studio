//! Deep search: research-style answers with visible steps and sources.
//!
//! Two paths, in order:
//!   1. **Provider tools** — when the active backend IS the provider
//!      (Gemini `google_search` grounding, or direct OpenAI `web_search`),
//!      ask it directly; it answers with its own citations.
//!   2. **Search-API loop** — otherwise plan a few queries, run them through
//!      a configured search API (Exa or Tavily), and hand the evidence to the
//!      normal router call with a citation instruction. The synthesis itself
//!      still streams through the regular chat path.
//!
//! Every step emits an event the UI renders as a live status list. All loops
//! are hard-bounded: queries, results per query, snippet length, evidence size.

use serde_json::{json, Value};

pub const MAX_QUERIES: usize = 3;
pub const MAX_RESULTS_PER_QUERY: usize = 6;
pub const MAX_TOTAL_SOURCES: usize = 12;
pub const MAX_SNIPPET_CHARS: usize = 500;
pub const MAX_EVIDENCE_CHARS: usize = 6_000;

/// What the chat handlers need after the deep-search phase.
pub struct Outcome {
    /// `Some`: a native provider already answered — skip the router call.
    pub answer: Option<String>,
    /// Prompt to send through the normal router call (evidence appended).
    pub augmented: Option<String>,
    pub sources: Vec<Value>,
}

pub struct Config<'a> {
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub model: &'a str,
    /// "", "exa" or "tavily".
    pub search_provider: &'a str,
    pub search_api_key: &'a str,
}

fn status_event(message: &str) -> Value {
    json!({ "type": "status", "message": message })
}

fn query_event(text: &str) -> Value {
    json!({ "type": "query", "text": text })
}

fn source_event(index: usize, title: &str, url: &str) -> Value {
    json!({ "type": "source", "id": format!("s{index}"), "title": title, "url": url })
}

/// Run the deep-search phase. `emit` receives step/source events as they
/// happen (the streaming handler forwards them verbatim as SSE frames).
pub async fn run(
    client: &reqwest::Client,
    cfg: &Config<'_>,
    question: &str,
    emit: &mut (dyn FnMut(Value) + Send),
) -> Result<Outcome, String> {
    emit(status_event("Checking for built-in web tools…"));

    // 1. Native provider tools first, when the backend is the provider itself.
    if cfg.base_url.contains("generativelanguage.googleapis.com") && !cfg.api_key.is_empty() {
        match gemini_grounding(client, cfg, question).await {
            Ok((answer, sources)) => return Ok(finished(answer, sources, emit)),
            Err(error) => emit(status_event(&format!(
                "Gemini grounding unavailable ({error}); falling back to search."
            ))),
        }
    }
    if cfg.base_url.contains("api.openai.com") && !cfg.api_key.is_empty() {
        match openai_web_search(client, cfg, question).await {
            Ok((answer, sources)) => return Ok(finished(answer, sources, emit)),
            Err(error) => emit(status_event(&format!(
                "OpenAI web_search unavailable ({error}); falling back to search."
            ))),
        }
    }

    // 2. Search-API loop.
    if cfg.search_provider.is_empty() || cfg.search_api_key.is_empty() {
        return Err(
            "Deep search needs a search key (Settings → Deep search: Exa or Tavily), \
             or a direct Gemini/OpenAI backend with built-in web tools."
                .into(),
        );
    }

    emit(status_event("Planning searches…"));
    let queries = plan_queries(client, cfg, question).await;
    let queries = normalize_queries(&queries);
    emit(status_event(&format!("Searching the web ({} queries)…", queries.len())));

    let mut sources: Vec<Value> = Vec::new();
    let mut succeeded = 0usize;
    for query in &queries {
        emit(query_event(query));
        let found = match cfg.search_provider {
            "exa" => search_exa(client, cfg.search_api_key, query).await,
            "tavily" => search_tavily(client, cfg.search_api_key, query).await,
            other => Err(format!("Unknown search provider \"{other}\".")),
        };
        match found {
            Ok(results) => {
                succeeded += 1;
                for result in results {
                    push_source(&mut sources, result);
                }
            }
            Err(error) => emit(status_event(&format!("Search failed: {error}"))),
        }
    }
    if succeeded == 0 {
        return Err("Every web search failed. Check the search key in Settings → Deep search."
            .into());
    }

    let sources = dedupe_by_url(&sources);
    for (index, source) in sources.iter().enumerate() {
        emit(source_event(
            index + 1,
            source["title"].as_str().unwrap_or("Untitled"),
            source["url"].as_str().unwrap_or(""),
        ));
    }
    if sources.is_empty() {
        emit(status_event("No results found — answering without sources."));
        return Ok(Outcome {
            answer: None,
            augmented: None,
            sources,
        });
    }

    emit(status_event("Writing the answer…"));
    Ok(Outcome {
        answer: None,
        augmented: Some(augment_prompt(question, &sources)),
        sources,
    })
}

fn finished(answer: String, sources: Vec<Value>, emit: &mut (dyn FnMut(Value) + Send)) -> Outcome {
    for (index, source) in sources.iter().enumerate() {
        emit(source_event(
            index + 1,
            source["title"].as_str().unwrap_or("Untitled"),
            source["url"].as_str().unwrap_or(""),
        ));
    }
    emit(status_event("Writing the answer…"));
    Outcome {
        answer: Some(answer),
        augmented: None,
        sources,
    }
}

/* ------------------------------------------------------------------ */
/* pure helpers (tested)                                               */
/* ------------------------------------------------------------------ */

/// Cap/trim/dedupe the planner's query list. Falls back to the question.
pub fn normalize_queries(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for candidate in raw {
        let query = candidate.trim();
        if query.is_empty() {
            continue;
        }
        let query: String = query.chars().take(200).collect();
        if !out.iter().any(|existing| existing.eq_ignore_ascii_case(&query)) {
            out.push(query);
        }
        if out.len() >= MAX_QUERIES {
            break;
        }
    }
    out
}

/// Extract `[..]` JSON from a planner reply, tolerating code fences/prose.
pub fn parse_plan_json(reply: &str) -> Vec<String> {
    let start = reply.find('[');
    let end = reply.rfind(']');
    let (Some(start), Some(end)) = (start, end) else {
        return Vec::new();
    };
    if end <= start {
        return Vec::new();
    }
    serde_json::from_str::<Vec<Value>>(&reply[start..=end])
        .ok()
        .map(|items| {
            items
                .into_iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Append the evidence block the synthesis call must cite.
pub fn augment_prompt(question: &str, sources: &[Value]) -> String {
    let mut evidence = String::new();
    let mut used = 0usize;
    for (index, source) in sources.iter().enumerate() {
        if used >= MAX_EVIDENCE_CHARS {
            break;
        }
        let title = source["title"].as_str().unwrap_or("Untitled");
        let url = source["url"].as_str().unwrap_or("");
        let snippet: String = source["snippet"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(MAX_SNIPPET_CHARS)
            .collect();
        let entry = format!("\n[{}] {}\n    {}\n    {}", index + 1, title, url, snippet);
        used += entry.len();
        evidence.push_str(&entry);
    }
    format!(
        "{question}\n\n--- Web evidence — cite sources as [1], [2], … with their links \
         when you use them. Never cite a source that is not listed. ---{evidence}"
    )
}

/// Insert a result, skipping duplicate URLs until the total cap.
pub fn push_source(sources: &mut Vec<Value>, result: Value) {
    if sources.len() >= MAX_TOTAL_SOURCES {
        return;
    }
    let url = result["url"].as_str().unwrap_or("").to_string();
    if url.is_empty() {
        return;
    }
    if sources.iter().any(|existing| existing["url"].as_str() == Some(url.as_str())) {
        return;
    }
    sources.push(result);
}

pub fn dedupe_by_url(sources: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for source in sources {
        push_source(&mut out, source.clone());
    }
    out
}

fn source(title: &str, url: &str, snippet: &str) -> Value {
    json!({ "title": title, "url": url, "snippet": snippet })
}

/* ------------------------------------------------------------------ */
/* planner                                                             */
/* ------------------------------------------------------------------ */

/// Ask the router model for up to 3 search queries. Best effort: on any
/// failure we search the original question itself.
async fn plan_queries(client: &reqwest::Client, cfg: &Config<'_>, question: &str) -> Vec<String> {
    let body = json!({
        "model": if cfg.model.trim().is_empty() { "auto" } else { cfg.model },
        "stream": false,
        "messages": [
            { "role": "system", "content": "You plan web searches. Reply ONLY with a JSON array of at most 3 short, diverse search queries for the user's question. No prose, no code fences." },
            { "role": "user", "content": question },
        ],
    });
    let response = client
        .post(format!(
            "{}/chat/completions",
            cfg.base_url.trim_end_matches('/')
        ))
        .bearer_auth(cfg.api_key)
        .json(&body)
        .send()
        .await;
    match response {
        Ok(response) if response.status().is_success() => {
            let Ok(text) = response.text().await else {
                return vec![question.to_string()];
            };
            let reply = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| {
                    value
                        .pointer("/choices/0/message/content")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let queries = parse_plan_json(&reply);
            if queries.is_empty() {
                vec![question.to_string()]
            } else {
                queries
            }
        }
        _ => vec![question.to_string()],
    }
}

/* ------------------------------------------------------------------ */
/* search providers                                                    */
/* ------------------------------------------------------------------ */

async fn search_exa(
    client: &reqwest::Client,
    key: &str,
    query: &str,
) -> Result<Vec<Value>, String> {
    let body = json!({
        "query": query,
        "numResults": MAX_RESULTS_PER_QUERY,
        "contents": { "text": true, "highlights": true },
    });
    let response = client
        .post("https://api.exa.ai/search")
        .header("x-api-key", key)
        .json(&body)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|error| format!("bad response: {error}"))?;
    if !status.is_success() {
        let detail = payload
            .pointer("/error")
            .and_then(Value::as_str)
            .unwrap_or("request rejected");
        return Err(format!("Exa HTTP {}: {}", status.as_u16(), detail));
    }
    let results = payload
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(results
        .into_iter()
        .filter_map(|item| {
            let url = item.get("url").and_then(Value::as_str)?.to_string();
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled")
                .to_string();
            let snippet = item
                .pointer("/highlights/0")
                .and_then(Value::as_str)
                .or_else(|| item.get("text").and_then(Value::as_str))
                .unwrap_or("")
                .to_string();
            Some(source(&title, &url, &snippet))
        })
        .collect())
}

async fn search_tavily(
    client: &reqwest::Client,
    key: &str,
    query: &str,
) -> Result<Vec<Value>, String> {
    let body = json!({
        "query": query,
        "max_results": MAX_RESULTS_PER_QUERY,
    });
    let response = client
        .post("https://api.tavily.com/search")
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|error| format!("bad response: {error}"))?;
    if !status.is_success() {
        let detail = payload
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or("request rejected");
        return Err(format!("Tavily HTTP {}: {}", status.as_u16(), detail));
    }
    let results = payload
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(results
        .into_iter()
        .filter_map(|item| {
            let url = item.get("url").and_then(Value::as_str)?.to_string();
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled")
                .to_string();
            let snippet = item
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some(source(&title, &url, &snippet))
        })
        .collect())
}

/* ------------------------------------------------------------------ */
/* native provider tools                                               */
/* ------------------------------------------------------------------ */

/// Gemini's native `google_search` grounding through the direct API.
async fn gemini_grounding(
    client: &reqwest::Client,
    cfg: &Config<'_>,
    question: &str,
) -> Result<(String, Vec<Value>), String> {
    let prefix = cfg
        .base_url
        .split("/v1beta")
        .next()
        .unwrap_or("https://generativelanguage.googleapis.com");
    let model = if cfg.model.starts_with("gemini-") && cfg.model != "auto" {
        cfg.model
    } else {
        "gemini-2.5-flash"
    };
    let body = json!({
        "contents": [{ "role": "user", "parts": [{ "text": question }] }],
        "tools": [{ "google_search": {} }],
    });
    let url = format!("{prefix}/v1beta/models/{model}:generateContent?key={}", cfg.api_key);
    let response = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|error| format!("bad response: {error}"))?;
    if !status.is_success() {
        let detail = payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("request rejected");
        return Err(format!("HTTP {}: {}", status.as_u16(), detail));
    }
    Ok(parse_gemini_grounding(&payload))
}

fn parse_gemini_grounding(payload: &Value) -> (String, Vec<Value>) {
    let answer = payload
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let sources = payload
        .pointer("/candidates/0/groundingMetadata/groundingChunks")
        .and_then(Value::as_array)
        .map(|chunks| {
            chunks
                .iter()
                .filter_map(|chunk| {
                    let url = chunk.pointer("/web/uri")?.as_str()?.to_string();
                    let title = chunk
                        .pointer("/web/title")
                        .and_then(Value::as_str)
                        .unwrap_or("Untitled")
                        .to_string();
                    Some(source(&title, &url, ""))
                })
                .collect()
        })
        .unwrap_or_default();
    (answer, sources)
}

/// OpenAI's `web_search` tool through the Responses API.
async fn openai_web_search(
    client: &reqwest::Client,
    cfg: &Config<'_>,
    question: &str,
) -> Result<(String, Vec<Value>), String> {
    let model = if cfg.model == "auto" || cfg.model.is_empty() {
        "gpt-4o-mini"
    } else {
        cfg.model
    };
    let body = json!({
        "model": model,
        "input": question,
        "tools": [{ "type": "web_search" }],
    });
    let response = client
        .post(format!("{}/responses", cfg.base_url.trim_end_matches('/')))
        .bearer_auth(cfg.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|error| format!("bad response: {error}"))?;
    if !status.is_success() {
        let detail = payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("request rejected");
        return Err(format!("HTTP {}: {}", status.as_u16(), detail));
    }
    Ok(parse_openai_responses(&payload))
}

fn parse_openai_responses(payload: &Value) -> (String, Vec<Value>) {
    let mut answer = String::new();
    let mut sources: Vec<Value> = Vec::new();
    let Some(output) = payload.get("output").and_then(Value::as_array) else {
        return (answer, sources);
    };
    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in content {
            if part.get("type").and_then(Value::as_str) == Some("output_text") {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    answer.push_str(text);
                }
            }
            if let Some(annotations) = part.get("annotations").and_then(Value::as_array) {
                for annotation in annotations {
                    let (Some(url), Some(title)) = (
                        annotation.get("url").and_then(Value::as_str),
                        annotation.get("title").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    push_source(&mut sources, source(title, url, ""));
                }
            }
        }
    }
    (answer, sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_caps_trims_and_dedupes() {
        let raw: Vec<String> = ["  rust async  ", "RUST ASYNC", "", "sqlite fts", "tokio", "axum"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let queries = normalize_queries(&raw);
        assert_eq!(queries.len(), MAX_QUERIES);
        assert_eq!(queries[0], "rust async");
        assert_eq!(queries[1], "sqlite fts");
        assert_eq!(queries[2], "tokio");
    }

    #[test]
    fn parse_plan_json_survives_fences_and_prose() {
        let reply = "Here you go:\n```json\n[\"a query\", \"b query\"]\n```";
        assert_eq!(parse_plan_json(reply), vec!["a query".to_string(), "b query".to_string()]);
        assert!(parse_plan_json("no array here").is_empty());
        assert!(parse_plan_json("[not json]").is_empty());
    }

    #[test]
    fn augment_prompt_lists_every_source_with_citations() {
        let sources = vec![
            source("Rust book", "https://doc.rust-lang.org/book/", "ownership rules"),
            source("Tokio", "https://tokio.rs/", "async runtime"),
        ];
        let prompt = augment_prompt("how do I use async?", &sources);
        assert!(prompt.starts_with("how do I use async?"));
        assert!(prompt.contains("[1] Rust book"));
        assert!(prompt.contains("https://doc.rust-lang.org/book/"));
        assert!(prompt.contains("[2] Tokio"));
        assert!(prompt.contains("cite sources as [1], [2]"));
    }

    #[test]
    fn augment_prompt_caps_snippets_and_evidence() {
        let long: String = "x".repeat(MAX_SNIPPET_CHARS * 3);
        let sources: Vec<Value> = (0..20)
            .map(|i| source(&format!("t{i}"), &format!("https://e.com/{i}"), &long))
            .collect();
        let prompt = augment_prompt("q", &sources);
        // Evidence block stays bounded even with hostile inputs.
        assert!(prompt.len() < 20_000);
        // Snippets never exceed their per-source cap.
        for line in prompt.lines() {
            assert!(line.len() <= MAX_SNIPPET_CHARS + 40);
        }
    }

    #[test]
    fn push_source_skips_duplicates_and_caps() {
        let mut sources = Vec::new();
        push_source(&mut sources, source("a", "https://dup.com/", "one"));
        push_source(&mut sources, source("a again", "https://dup.com/", "two"));
        assert_eq!(sources.len(), 1);
        for i in 0..(MAX_TOTAL_SOURCES + 5) {
            push_source(&mut sources, source("t", &format!("https://e.com/{i}"), "s"));
        }
        assert_eq!(sources.len(), MAX_TOTAL_SOURCES);
        push_source(&mut sources, source("empty", "", "s"));
        assert_eq!(sources.len(), MAX_TOTAL_SOURCES);
    }

    #[test]
    fn dedupe_by_url_preserves_order() {
        let input = vec![
            source("first", "https://a.com/", ""),
            source("dupe", "https://a.com/", ""),
            source("second", "https://b.com/", ""),
        ];
        let out = dedupe_by_url(&input);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["url"].as_str(), Some("https://a.com/"));
        assert_eq!(out[1]["url"].as_str(), Some("https://b.com/"));
    }

    #[test]
    fn gemini_grounding_parses_answer_and_chunks() {
        let payload = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "grounded answer" }] },
                "groundingMetadata": { "groundingChunks": [
                    { "web": { "uri": "https://a.com/", "title": "A" } },
                    { "web": { "uri": "https://b.com/", "title": "B" } }
                ]}
            }]
        });
        let (answer, sources) = parse_gemini_grounding(&payload);
        assert_eq!(answer, "grounded answer");
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0]["url"].as_str(), Some("https://a.com/"));
    }

    #[test]
    fn openai_responses_parses_text_and_citations() {
        let payload = json!({
            "output": [
                { "type": "reasoning", "summary": [] },
                { "type": "message", "content": [
                    { "type": "output_text", "text": "answer with citation",
                      "annotations": [{ "type": "url_citation", "url": "https://c.com/", "title": "C" }] }
                ]}
            ]
        });
        let (answer, sources) = parse_openai_responses(&payload);
        assert_eq!(answer, "answer with citation");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0]["title"].as_str(), Some("C"));
    }
}