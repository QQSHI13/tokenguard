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

pub struct SseUsageParser {
    _format: ProviderFormat,
    buf: Vec<u8>,
    pub usage: Usage,
}

impl SseUsageParser {
    pub fn new(format: ProviderFormat) -> Self {
        Self {
            _format: format,
            buf: Vec::new(),
            usage: Usage::default(),
        }
    }

    /// Feed a raw chunk of the stream. Complete `data:` lines are parsed.
    pub fn feed(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
        while let Some(nl) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=nl).collect();
            self.handle_line(&line);
        }
    }

    fn handle_line(&mut self, line: &[u8]) {
        let s = std::str::from_utf8(line).unwrap_or("").trim_end();
        let Some(rest) = s.strip_prefix("data:") else {
            return;
        };
        let data = rest.trim();
        if data.is_empty() || data == "[DONE]" {
            return;
        }
        if let Ok(v) = serde_json::from_str::<Value>(data) {
            self.extract(&v);
        }
    }

    fn extract(&mut self, v: &Value) {
        // Usage can live at the top level (OpenAI chat/Anthropic), nested under
        // `response` (OpenAI Responses API streaming final event), or nested
        // under `message` (Anthropic `message_start` event).
        let usage = v
            .get("usage")
            .or_else(|| v.get("response").and_then(|r| r.get("usage")))
            .or_else(|| v.get("message").and_then(|m| m.get("usage")));
        if let Some(u) = usage {
            extract_from_usage_object(u, &mut self.usage);
        }
    }
}

fn extract_from_usage_object(u: &Value, into: &mut Usage) {
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

    // Cached input tokens.
    // OpenAI: usage.prompt_tokens_details.cached_tokens
    // Anthropic: usage.cache_read_input_tokens + usage.cache_creation_input_tokens
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
    into.cached = cached;
}

/// Extract usage from a complete (non-streaming) JSON response body.
pub fn extract_json(body: &[u8], _format: ProviderFormat) -> Usage {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return Usage::default();
    };
    let mut u = Usage::default();
    let usage = v
        .get("usage")
        .or_else(|| v.get("response").and_then(|r| r.get("usage")));
    if let Some(usage) = usage {
        extract_from_usage_object(usage, &mut u);
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_openai_usage() {
        let mut parser = SseUsageParser::new(ProviderFormat::OpenAI);
        parser.feed(b"data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n");
        assert_eq!(parser.usage.prompt, 10);
        assert_eq!(parser.usage.completion, 5);
    }

    #[test]
    fn sse_anthropic_usage() {
        let mut parser = SseUsageParser::new(ProviderFormat::Anthropic);
        parser.feed(b"data: {\"usage\":{\"input_tokens\":20,\"output_tokens\":7}}\n\n");
        assert_eq!(parser.usage.prompt, 20);
        assert_eq!(parser.usage.completion, 7);
    }

    #[test]
    fn sse_done_is_ignored() {
        let mut parser = SseUsageParser::new(ProviderFormat::OpenAI);
        parser.feed(b"data: [DONE]\n\n");
        assert_eq!(parser.usage.prompt, 0);
        assert_eq!(parser.usage.completion, 0);
    }

    #[test]
    fn sse_chunks_may_span_lines() {
        let mut parser = SseUsageParser::new(ProviderFormat::OpenAI);
        parser.feed(b"data: {\"usage\":{\"prompt_tokens\":");
        parser.feed(b"3,\"completion_tokens\":2}}\n");
        assert_eq!(parser.usage.prompt, 3);
        assert_eq!(parser.usage.completion, 2);
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
    fn sse_openai_responses_streaming_usage() {
        let mut parser = SseUsageParser::new(ProviderFormat::OpenAI);
        parser.feed(b"data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":9}}}\n\n");
        assert_eq!(parser.usage.prompt, 12);
        assert_eq!(parser.usage.completion, 9);
    }

    #[test]
    fn sse_anthropic_message_start_input_tokens() {
        let mut parser = SseUsageParser::new(ProviderFormat::Anthropic);
        parser.feed(
            b"data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":17}}}\n\n",
        );
        assert_eq!(parser.usage.prompt, 17);
    }

    #[test]
    fn sse_anthropic_message_delta_output_tokens() {
        let mut parser = SseUsageParser::new(ProviderFormat::Anthropic);
        parser.feed(
            b"data: {\"type\":\"message_delta\",\"delta\":{},\"usage\":{\"output_tokens\":21}}\n\n",
        );
        assert_eq!(parser.usage.completion, 21);
    }

    #[test]
    fn sse_openai_cached_tokens() {
        let mut parser = SseUsageParser::new(ProviderFormat::OpenAI);
        parser.feed(
            b"data: {\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":5,\"prompt_tokens_details\":{\"cached_tokens\":40}}}\n\n",
        );
        assert_eq!(parser.usage.prompt, 100);
        assert_eq!(parser.usage.completion, 5);
        assert_eq!(parser.usage.cached, 40);
    }

    #[test]
    fn sse_anthropic_cached_tokens() {
        let mut parser = SseUsageParser::new(ProviderFormat::Anthropic);
        parser.feed(
            b"data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":200,\"cache_read_input_tokens\":150,\"cache_creation_input_tokens\":25}}}\n\n",
        );
        assert_eq!(parser.usage.prompt, 200);
        assert_eq!(parser.usage.cached, 175);
    }
}
