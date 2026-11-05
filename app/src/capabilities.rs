//! Model capabilities: what a provider actually says about a model.
//!
//! Nothing here infers a capability from a model's name. A flag is
//! `Some(true)`/`Some(false)` only when the provider published evidence for it
//! and `None` — serialised as `null` — when it did not, which is what lets the
//! UI say "unknown" instead of promising something the model may not do. This
//! is the Phase 3 table reduced to its only honest source: provider metadata.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Per-million-token prices, converted from whatever unit the provider used.
/// Only present when the provider published both halves. `Deserialize` exists
/// so a cached catalog row can be read back into the same shape for cost
/// accounting, without a second definition of what a price is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Price {
    pub input_per_million: f64,
    pub output_per_million: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ModelCapabilities {
    /// Where the facts came from: `openrouter`, `ollama` or `provider`.
    pub source: &'static str,
    /// 0 means the provider published no context limit.
    pub context_limit: u64,
    /// Empty means unknown; never assumed to be `text`.
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub streaming: Option<bool>,
    pub tools: Option<bool>,
    pub reasoning: Option<bool>,
    pub structured_output: Option<bool>,
    pub pricing: Option<Price>,
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `None` when the provider published no parameter list at all — an empty list
/// is evidence that *none* of the names are supported, which is not the same
/// answer.
fn supports(params: &[String], names: &[&str]) -> Option<bool> {
    if params.is_empty() {
        return None;
    }
    Some(
        names
            .iter()
            .any(|name| params.iter().any(|param| param == name)),
    )
}

/// `Some(true)` only when the name was published. Used where absence carries
/// no evidence: Ollama's `capabilities` enum describes model features
/// (`completion`, `tools`, `thinking`, `vision`, `embedding`), so it can
/// confirm a feature but never deny streaming or structured output.
fn listed(params: &[String], names: &[&str]) -> Option<bool> {
    if names
        .iter()
        .any(|name| params.iter().any(|param| param == name))
    {
        return Some(true);
    }
    None
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        // OpenRouter publishes prices as strings.
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn context_of(row: &Value) -> u64 {
    ["context_length", "context_window", "max_input_tokens"]
        .iter()
        .find_map(|key| row.get(key).and_then(Value::as_u64))
        .or_else(|| {
            // OpenRouter nests the limit under top_provider.
            row.pointer("/top_provider/context_length")
                .and_then(Value::as_u64)
        })
        .unwrap_or(0)
}

/// One `/models` row. OpenRouter-shaped rows carry the most metadata; a plain
/// OpenAI-compatible row carries only an id, so everything stays unknown.
pub(crate) fn from_catalog_row(row: &Value) -> ModelCapabilities {
    let params = strings(row.get("supported_parameters"));
    let architecture = row.get("architecture");
    let openrouter = architecture.is_some()
        || row.get("supported_parameters").is_some()
        || row.get("pricing").is_some();
    let pricing = match (
        number(row.pointer("/pricing/prompt")),
        number(row.pointer("/pricing/completion")),
    ) {
        (Some(input), Some(output)) => Some(Price {
            input_per_million: input * 1_000_000.0,
            output_per_million: output * 1_000_000.0,
        }),
        _ => None,
    };
    ModelCapabilities {
        source: if openrouter { "openrouter" } else { "provider" },
        context_limit: context_of(row),
        input_modalities: strings(architecture.and_then(|a| a.get("input_modalities"))),
        output_modalities: strings(architecture.and_then(|a| a.get("output_modalities"))),
        streaming: supports(&params, &["stream", "streaming"]),
        tools: supports(&params, &["tools", "tool_choice"]),
        reasoning: supports(
            &params,
            &["reasoning", "include_reasoning", "reasoning_effort"],
        ),
        structured_output: supports(
            &params,
            &["structured_outputs", "response_format", "json_schema"],
        ),
        pricing,
    }
}

/// Context limit out of Ollama's `model_info`, whose keys are namespaced by
/// architecture (`llama.context_length`, `qwen2.context_length`, …).
fn model_info_context(model_info: Option<&Value>) -> u64 {
    let Some(info) = model_info.and_then(Value::as_object) else {
        return 0;
    };
    info.iter()
        .filter(|(key, _)| key.ends_with(".context_length"))
        .filter_map(|(_, value)| value.as_u64())
        .max()
        .unwrap_or(0)
}

/// `POST /api/show` — the Ollama runtime's own description of an installed
/// model (`capabilities: ["completion","tools","thinking","vision", …]`).
pub(crate) fn from_show(show: &Value) -> ModelCapabilities {
    let caps = strings(show.get("capabilities"));
    let vision = caps.iter().any(|cap| cap == "vision");
    // Only the modalities the runtime names: a vision-capable model accepts
    // images as well as text, anything else stays `text` when the runtime
    // published a capability list at all.
    let input_modalities = if caps.is_empty() {
        Vec::new()
    } else if vision {
        vec!["text".to_string(), "image".to_string()]
    } else {
        vec!["text".to_string()]
    };
    let output_modalities = if caps.is_empty() {
        Vec::new()
    } else {
        vec!["text".to_string()]
    };
    ModelCapabilities {
        source: "ollama",
        context_limit: model_info_context(show.get("model_info")),
        input_modalities,
        output_modalities,
        streaming: listed(&caps, &["stream", "streaming"]),
        tools: supports(&caps, &["tools"]),
        reasoning: supports(&caps, &["thinking", "reasoning"]),
        structured_output: listed(&caps, &["structured_outputs", "structured_output"]),
        pricing: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_openrouter_row_yields_only_the_capabilities_it_states() {
        let row = json!({
            "id": "vendor/model",
            "context_length": 131072,
            "architecture": {
                "input_modalities": ["text", "image"],
                "output_modalities": ["text"]
            },
            "supported_parameters": ["tools", "response_format", "reasoning", "temperature"],
            "pricing": { "prompt": "0.000003", "completion": "0.000015" }
        });
        let caps = from_catalog_row(&row);
        assert_eq!(caps.source, "openrouter");
        assert_eq!(caps.context_limit, 131072);
        assert_eq!(caps.input_modalities, ["text", "image"]);
        assert_eq!(caps.tools, Some(true));
        assert_eq!(caps.reasoning, Some(true));
        assert_eq!(caps.structured_output, Some(true));
        // Not in supported_parameters, so the provider did not claim it.
        assert_eq!(caps.streaming, Some(false));
        assert_eq!(
            caps.pricing,
            Some(Price {
                input_per_million: 3.0,
                output_per_million: 15.0
            })
        );
    }

    #[test]
    fn a_row_without_metadata_is_unknown_rather_than_optimistic() {
        let caps = from_catalog_row(&json!({ "id": "gpt-4o", "owned_by": "openai" }));
        assert_eq!(caps.source, "provider");
        assert_eq!(caps.context_limit, 0);
        assert!(caps.input_modalities.is_empty());
        assert_eq!(caps.tools, None);
        assert_eq!(caps.reasoning, None);
        assert_eq!(caps.structured_output, None);
        assert_eq!(caps.streaming, None);
        assert_eq!(caps.pricing, None);
    }

    #[test]
    fn an_empty_parameter_list_means_none_of_them_not_unknown() {
        let caps = from_catalog_row(&json!({ "id": "vendor/plain", "supported_parameters": [] }));
        assert_eq!(caps.tools, None);
        // OpenRouter-shaped only because it published the (empty) list.
        assert_eq!(caps.source, "openrouter");
    }

    #[test]
    fn a_nested_top_provider_limit_is_read() {
        let caps = from_catalog_row(&json!({
            "id": "vendor/nested",
            "top_provider": { "context_length": 8192 }
        }));
        assert_eq!(caps.context_limit, 8192);
    }

    #[test]
    fn ollama_show_maps_its_capability_list_and_context() {
        let show = json!({
            "capabilities": ["completion", "tools", "thinking", "vision"],
            "model_info": { "llama.context_length": 32768, "llama.block_count": 32 }
        });
        let caps = from_show(&show);
        assert_eq!(caps.source, "ollama");
        assert_eq!(caps.context_limit, 32768);
        assert_eq!(caps.tools, Some(true));
        assert_eq!(caps.reasoning, Some(true));
        assert_eq!(caps.input_modalities, ["text", "image"]);
        assert_eq!(caps.output_modalities, ["text"]);
        // The runtime never reports streaming here, so it stays unknown.
        assert_eq!(caps.streaming, None);
        assert_eq!(caps.pricing, None);
    }

    #[test]
    fn ollama_show_without_a_capability_list_stays_unknown() {
        let caps = from_show(&json!({ "model_info": { "qwen2.context_length": 4096 } }));
        assert_eq!(caps.context_limit, 4096);
        assert!(caps.input_modalities.is_empty());
        assert_eq!(caps.tools, None);
        assert_eq!(caps.reasoning, None);
    }

    #[test]
    fn an_ollama_model_without_vision_does_not_claim_image_input() {
        let caps = from_show(&json!({ "capabilities": ["completion", "tools"] }));
        assert_eq!(caps.input_modalities, ["text"]);
    }

    #[test]
    fn unknown_flags_serialise_as_null_not_false() {
        let value = serde_json::to_value(from_catalog_row(&json!({ "id": "x" }))).unwrap();
        assert!(value["tools"].is_null());
        assert!(value["reasoning"].is_null());
        assert_eq!(value["context_limit"], 0);
        assert_eq!(value["source"], "provider");
    }
}
