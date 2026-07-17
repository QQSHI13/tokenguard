//! SSE stream parser: extracts token usage while forwarding bytes unchanged.
//!
//! One parser handles both OpenAI and Anthropic streaming formats:
//! - OpenAI: a chunk's `data:` JSON may carry `usage` with `prompt_tokens`
//!   and `completion_tokens` (only present when `stream_options.include_usage`
//!   was injected by the forwarder).
//! - Anthropic: `message_start` carries `usage.input_tokens`, `message_delta`
//!   carries `usage.output_tokens`.

use crate::config::ProviderFormat;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub prompt: u64,
    pub completion: u64,
    pub cached: u64,
}

pub fn extract_from_usage_object(u: &Value, into: &mut Usage) {
    if let Some(p) = u.get("prompt_tokens").and_then(|x| x.as_u64()) {
        into.prompt = p;
    }
    if let Some(c) = u.get("completion_tokens").and_then(|x| x.as_u64()) {
        into.completion = c;
    }
    // Anthropic / OpenAI Responses also use input/output naming.
    if let Some(p) = u.get("input_tokens").and_then(|x| x.as_u64()) {
        into.prompt = p;
    }
    if let Some(c) = u.get("output_tokens").and_then(|x| x.as_u64()) {
        into.completion = c;
    }
    // Google Gemini naming.
    if let Some(p) = u.get("promptTokenCount").and_then(|x| x.as_u64()) {
        into.prompt = p;
    }
    if let Some(c) = u.get("candidatesTokenCount").and_then(|x| x.as_u64()) {
        into.completion = c;
    }

    // Cached input tokens.
    // OpenAI: usage.prompt_tokens_details.cached_tokens
    // Anthropic: usage.cache_read_input_tokens + usage.cache_creation_input_tokens
    // Google Gemini: usageMetadata.promptTokensDetails[].tokenCount where modality is not set,
    // plus cacheTokensDetails[].tokenCount.
    let mut cached = 0u64;
    if let Some(details) = u.get("prompt_tokens_details") {
        if let Some(c) = details.get("cached_tokens").and_then(|x| x.as_u64()) {
            cached += c;
        }
    }
    if let Some(c) = u.get("cache_read_input_tokens").and_then(|x| x.as_u64()) {
        cached += c;
    }
    if let Some(c) = u
        .get("cache_creation_input_tokens")
        .and_then(|x| x.as_u64())
    {
        cached += c;
    }
    if let Some(details) = u.get("promptTokensDetails").and_then(|x| x.as_array()) {
        cached += details
            .iter()
            .filter_map(|d| d.get("tokenCount").and_then(|x| x.as_u64()))
            .sum::<u64>();
    }
    if let Some(details) = u.get("cacheTokensDetails").and_then(|x| x.as_array()) {
        cached += details
            .iter()
            .filter_map(|d| d.get("tokenCount").and_then(|x| x.as_u64()))
            .sum::<u64>();
    }
    into.cached = cached;
}

/// Extract usage from a complete (non-streaming) JSON response body.
pub fn extract_json(body: &[u8], _format: ProviderFormat) -> Usage {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return Usage::default();
    };
    let mut u = Usage::default();
    // Buffered Google streaming (no alt=sse upstream) returns a top-level
    // array of response objects; usage is usually carried by the last one.
    if let Value::Array(items) = &v {
        for item in items {
            let usage = item.get("usage").or_else(|| item.get("usageMetadata"));
            if let Some(usage) = usage {
                extract_from_usage_object(usage, &mut u);
            }
        }
        return u;
    }
    let usage = v
        .get("usage")
        .or_else(|| v.get("response").and_then(|r| r.get("usage")))
        .or_else(|| v.get("usageMetadata"));
    if let Some(usage) = usage {
        extract_from_usage_object(usage, &mut u);
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_openai_usage() {
        let v = serde_json::json!({"usage": {"prompt_tokens": 10, "completion_tokens": 5}});
        let mut u = Usage::default();
        extract_from_usage_object(v.get("usage").unwrap(), &mut u);
        assert_eq!(u.prompt, 10);
        assert_eq!(u.completion, 5);
    }

    #[test]
    fn extract_anthropic_usage() {
        let v = serde_json::json!({"usage": {"input_tokens": 20, "output_tokens": 7}});
        let mut u = Usage::default();
        extract_from_usage_object(v.get("usage").unwrap(), &mut u);
        assert_eq!(u.prompt, 20);
        assert_eq!(u.completion, 7);
    }

    #[test]
    fn extract_json_openai() {
        let body = br#"{"usage":{"prompt_tokens":8,"completion_tokens":4}}"#;
        let usage = extract_json(body, ProviderFormat::OpenAI);
        assert_eq!(usage.prompt, 8);
        assert_eq!(usage.completion, 4);
    }

    #[test]
    fn extract_json_anthropic() {
        let body = br#"{"usage":{"input_tokens":9,"output_tokens":3}}"#;
        let usage = extract_json(body, ProviderFormat::Anthropic);
        assert_eq!(usage.prompt, 9);
        assert_eq!(usage.completion, 3);
    }

    #[test]
    fn extract_json_malformed_returns_zero() {
        let usage = extract_json(b"not json", ProviderFormat::OpenAI);
        assert_eq!(usage.prompt, 0);
        assert_eq!(usage.completion, 0);
    }

    #[test]
    fn extract_openai_responses_streaming_usage() {
        let v = serde_json::json!({"type": "response.completed", "response": {"usage": {"input_tokens": 12, "output_tokens": 9}}});
        let mut u = Usage::default();
        extract_from_usage_object(v.get("response").unwrap().get("usage").unwrap(), &mut u);
        assert_eq!(u.prompt, 12);
        assert_eq!(u.completion, 9);
    }

    #[test]
    fn extract_anthropic_message_start_input_tokens() {
        let v = serde_json::json!({"type": "message_start", "message": {"usage": {"input_tokens": 17}}});
        let mut u = Usage::default();
        extract_from_usage_object(v.get("message").unwrap().get("usage").unwrap(), &mut u);
        assert_eq!(u.prompt, 17);
    }

    #[test]
    fn extract_anthropic_message_delta_output_tokens() {
        let v = serde_json::json!({"type": "message_delta", "delta": {}, "usage": {"output_tokens": 21}});
        let mut u = Usage::default();
        extract_from_usage_object(v.get("usage").unwrap(), &mut u);
        assert_eq!(u.completion, 21);
    }

    #[test]
    fn extract_openai_cached_tokens() {
        let v = serde_json::json!({"usage": {"prompt_tokens": 100, "completion_tokens": 5, "prompt_tokens_details": {"cached_tokens": 40}}});
        let mut u = Usage::default();
        extract_from_usage_object(v.get("usage").unwrap(), &mut u);
        assert_eq!(u.prompt, 100);
        assert_eq!(u.completion, 5);
        assert_eq!(u.cached, 40);
    }

    #[test]
    fn extract_anthropic_cached_tokens() {
        let v = serde_json::json!({"type": "message_start", "message": {"usage": {"input_tokens": 200, "cache_read_input_tokens": 150, "cache_creation_input_tokens": 25}}});
        let mut u = Usage::default();
        extract_from_usage_object(v.get("message").unwrap().get("usage").unwrap(), &mut u);
        assert_eq!(u.prompt, 200);
        assert_eq!(u.cached, 175);
    }

    #[test]
    fn extract_google_usage() {
        let v = serde_json::json!({"usageMetadata": {"promptTokenCount": 12, "candidatesTokenCount": 9, "totalTokenCount": 21}});
        let mut u = Usage::default();
        extract_from_usage_object(v.get("usageMetadata").unwrap(), &mut u);
        assert_eq!(u.prompt, 12);
        assert_eq!(u.completion, 9);
    }

    #[test]
    fn extract_json_google() {
        let body = br#"{"usageMetadata":{"promptTokenCount":8,"candidatesTokenCount":4,"totalTokenCount":12}}"#;
        let usage = extract_json(body, ProviderFormat::Google);
        assert_eq!(usage.prompt, 8);
        assert_eq!(usage.completion, 4);
    }

    #[test]
    fn extract_json_google_buffered_stream_array() {
        let body = br#"[{"candidates":[{"content":{"parts":[{"text":"Hi"}]}}]},{"candidates":[{"content":{"parts":[{"text":"!"}]}}],"usageMetadata":{"promptTokenCount":11,"candidatesTokenCount":2,"totalTokenCount":13}}]"#;
        let usage = extract_json(body, ProviderFormat::Google);
        assert_eq!(usage.prompt, 11);
        assert_eq!(usage.completion, 2);
    }
}
