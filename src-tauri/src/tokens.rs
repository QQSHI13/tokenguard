//! Token counting fallback when a provider does not report usage.
//!
//! Uses tiktoken-rs for OpenAI-compatible models and cl100k_base as a
//! reasonable approximation for Anthropic / unknown models.

use crate::config::ProviderFormat;
use serde_json::Value;

/// Pick an encoding based on the model name.
fn encoding_for(model: &str) -> tiktoken_rs::CoreBPE {
    let m = model.to_lowercase();
    if m.contains("gpt-4o") || m.contains("o1-") || m.contains("o3-") || m.contains("gpt-4.5") {
        tiktoken_rs::o200k_base_singleton().clone()
    } else {
        // cl100k_base covers gpt-4-turbo, gpt-3.5-turbo, text-embedding, and is a
        // close enough stand-in for Claude models when no usage is provided.
        tiktoken_rs::cl100k_base_singleton().clone()
    }
}

pub fn count_text(model: &str, text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    encoding_for(model).encode_with_special_tokens(text).len() as u64
}

fn content_from_message(msg: &Value) -> String {
    let mut out = String::new();
    if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
        // vision / multi-modal content array
        for part in arr {
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                out.push_str(text);
                out.push('\n');
            } else if let Some(text) = part.as_str() {
                out.push_str(text);
                out.push('\n');
            }
        }
    } else if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
        out.push_str(text);
    }
    out
}

fn count_messages(model: &str, messages: &[Value]) -> u64 {
    let enc = encoding_for(model);
    let mut total = 0u64;
    for msg in messages {
        let text = content_from_message(msg);
        total += enc.encode_with_special_tokens(&text).len() as u64;
        // Per-message overhead (role + separators). Approximate but good enough.
        total += 3;
    }
    total
}

/// Count prompt tokens from a request body.
pub fn count_prompt(model: &str, format: ProviderFormat, body: &Value) -> u64 {
    match format {
        ProviderFormat::OpenAI => {
            if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
                count_messages(model, messages)
            } else if let Some(prompt) = body.get("prompt").and_then(|p| p.as_str()) {
                count_text(model, prompt)
            } else if let Some(input) = body.get("input").and_then(|i| i.as_array()) {
                count_messages(model, input)
            } else {
                0
            }
        }
        ProviderFormat::Anthropic => {
            if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
                count_messages(model, messages)
            } else {
                0
            }
        }
    }
}

fn openai_completion_text(body: &Value) -> String {
    let mut out = String::new();
    if let Some(choices) = body.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(text) = choice
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                out.push_str(text);
            } else if let Some(text) = choice.get("text").and_then(|t| t.as_str()) {
                out.push_str(text);
            }
        }
    }
    // OpenAI Responses API output
    if let Some(arr) = body.get("output").and_then(|o| o.as_array()) {
        for item in arr {
            if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
                for part in content {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        out.push_str(text);
                        out.push('\n');
                    }
                }
            }
        }
    }
    out
}

fn anthropic_completion_text(body: &Value) -> String {
    let mut out = String::new();
    if let Some(content) = body.get("content").and_then(|c| c.as_array()) {
        for block in content {
            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                out.push_str(text);
                out.push('\n');
            }
        }
    }
    out
}

/// Count completion tokens from a non-streaming response body.
pub fn count_completion_json(model: &str, format: ProviderFormat, body: &Value) -> u64 {
    let text = match format {
        ProviderFormat::OpenAI => openai_completion_text(body),
        ProviderFormat::Anthropic => anthropic_completion_text(body),
    };
    count_text(model, &text)
}

/// Extract a snippet of assistant content from one SSE data line.
pub fn extract_stream_delta(line: &Value) -> String {
    let mut out = String::new();
    // OpenAI / OpenRouter deltas
    if let Some(delta) = line.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("delta")) {
        if let Some(text) = delta.get("content").and_then(|c| c.as_str()) {
            out.push_str(text);
        }
        if let Some(text) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
            out.push_str(text);
        }
    }
    // Anthropic content_block_delta
    if let Some(delta) = line.get("delta") {
        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
            out.push_str(text);
        }
    }
    out
}
