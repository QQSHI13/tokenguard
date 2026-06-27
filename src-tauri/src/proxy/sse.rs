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
        let Some(u) = v.get("usage") else {
            return;
        };
        if let Some(p) = u.get("prompt_tokens").and_then(|x| x.as_u64()) {
            self.usage.prompt = p;
        }
        if let Some(c) = u.get("completion_tokens").and_then(|x| x.as_u64()) {
            self.usage.completion = c;
        }
        // Anthropic
        if let Some(p) = u.get("input_tokens").and_then(|x| x.as_u64()) {
            self.usage.prompt = p;
        }
        if let Some(c) = u.get("output_tokens").and_then(|x| x.as_u64()) {
            self.usage.completion = c;
        }
    }
}

/// Extract usage from a complete (non-streaming) JSON response body.
pub fn extract_json(body: &[u8], _format: ProviderFormat) -> Usage {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return Usage::default();
    };
    let mut u = Usage::default();
    if let Some(usage) = v.get("usage") {
        if let Some(p) = usage.get("prompt_tokens").and_then(|x| x.as_u64()) {
            u.prompt = p;
        }
        if let Some(c) = usage.get("completion_tokens").and_then(|x| x.as_u64()) {
            u.completion = c;
        }
        if let Some(p) = usage.get("input_tokens").and_then(|x| x.as_u64()) {
            u.prompt = p;
        }
        if let Some(c) = usage.get("output_tokens").and_then(|x| x.as_u64()) {
            u.completion = c;
        }
    }
    u
}
