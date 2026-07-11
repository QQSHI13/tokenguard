//! Bidirectional request/response conversion between OpenAI, Anthropic, and
//! Google (Gemini) API formats.
//!
//! This enables 3 x 3 calling: a client can use any of the three API shapes
//! while the configured provider speaks any of the three formats. Conversions
//! focus on text chat; advanced features (vision, tools, function calling) are
//! passed through when structurally compatible and ignored otherwise.

use crate::config::ProviderFormat;
use serde_json::Value;

/// Convert a client request body from `from` format to `to` format.
/// `remote_model` is the provider-side model name that should be inserted into
/// the outgoing body when the target format requires it in the body.
pub fn convert_request(
    from: ProviderFormat,
    to: ProviderFormat,
    body: &Value,
    remote_model: &str,
) -> Value {
    if from == to {
        let mut out = body.clone();
        ensure_model(&mut out, to, remote_model);
        return out;
    }

    match (from, to) {
        (ProviderFormat::OpenAI, ProviderFormat::Anthropic) => {
            openai_to_anthropic_request(body, remote_model)
        }
        (ProviderFormat::OpenAI, ProviderFormat::Google) => openai_to_google_request(body),
        (ProviderFormat::Anthropic, ProviderFormat::OpenAI) => {
            anthropic_to_openai_request(body, remote_model)
        }
        (ProviderFormat::Anthropic, ProviderFormat::Google) => anthropic_to_google_request(body),
        (ProviderFormat::Google, ProviderFormat::OpenAI) => {
            google_to_openai_request(body, remote_model)
        }
        (ProviderFormat::Google, ProviderFormat::Anthropic) => {
            google_to_anthropic_request(body, remote_model)
        }
        _ => {
            let mut out = body.clone();
            ensure_model(&mut out, to, remote_model);
            out
        }
    }
}

/// Convert an upstream response body from provider format (`from`) back to the
/// client format (`to`).
pub fn convert_response(from: ProviderFormat, to: ProviderFormat, body: &Value) -> Value {
    if from == to {
        return body.clone();
    }

    match (from, to) {
        (ProviderFormat::OpenAI, ProviderFormat::Anthropic) => {
            openai_to_anthropic_response(body)
        }
        (ProviderFormat::OpenAI, ProviderFormat::Google) => openai_to_google_response(body),
        (ProviderFormat::Anthropic, ProviderFormat::OpenAI) => {
            anthropic_to_openai_response(body)
        }
        (ProviderFormat::Anthropic, ProviderFormat::Google) => {
            anthropic_to_google_response(body)
        }
        (ProviderFormat::Google, ProviderFormat::OpenAI) => google_to_openai_response(body),
        (ProviderFormat::Google, ProviderFormat::Anthropic) => {
            google_to_anthropic_response(body)
        }
        _ => body.clone(),
    }
}

/// Return the upstream path for a request given the client format, provider
/// format, and provider model name. `client_path` is preserved when formats
/// match.
pub fn target_path(
    from: ProviderFormat,
    to: ProviderFormat,
    remote_model: &str,
    client_path: &str,
) -> String {
    if from == to {
        return client_path.to_string();
    }
    match to {
        ProviderFormat::OpenAI => "/v1/chat/completions".to_string(),
        ProviderFormat::Anthropic => "/v1/messages".to_string(),
        ProviderFormat::Google => format!("/v1beta/models/{remote_model}:generateContent"),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ensure_model(body: &mut Value, format: ProviderFormat, remote_model: &str) {
    if format != ProviderFormat::Google {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("model".to_string(), Value::String(remote_model.to_string()));
        }
    }
}

fn get_f64(obj: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    obj.get(key).and_then(|v| v.as_f64())
}

fn get_u64(obj: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    obj.get(key).and_then(|v| v.as_u64())
}

fn text_content_to_string(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn string_to_text_content(text: &str) -> Value {
    Value::String(text.to_string())
}

// ---------------------------------------------------------------------------
// OpenAI -> Anthropic
// ---------------------------------------------------------------------------

fn openai_to_anthropic_request(body: &Value, remote_model: &str) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let (system, messages) = split_openai_messages(obj.get("messages"));

    let mut out = serde_json::Map::new();
    out.insert("model".to_string(), Value::String(remote_model.to_string()));
    if let Some(system) = system {
        out.insert("system".to_string(), system);
    }
    out.insert("messages".to_string(), messages);
    if let Some(v) = get_u64(&obj, "max_tokens") {
        out.insert("max_tokens".to_string(), Value::Number(v.into()));
    }
    if let Some(v) = get_f64(&obj, "temperature") {
        out.insert("temperature".to_string(), Value::Number(serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into())));
    }
    if obj.get("stream").and_then(|v| v.as_bool()).unwrap_or(false) {
        out.insert("stream".to_string(), Value::Bool(true));
    }
    Value::Object(out)
}

fn openai_to_anthropic_response(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let text = obj
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .map(text_content_to_string)
        .unwrap_or_default();
    let stop = obj
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .map(translate_finish_reason_o2a)
        .unwrap_or_else(|| "end_turn".to_string());

    let mut out = serde_json::Map::new();
    out.insert(
        "content".to_string(),
        Value::Array(vec![serde_json::json!({"type": "text", "text": text})]),
    );
    out.insert("stop_reason".to_string(), Value::String(stop));
    out.insert("role".to_string(), Value::String("assistant".to_string()));
    if let Some(usage) = normalize_usage_openai(obj.get("usage")) {
        out.insert("usage".to_string(), usage);
    }
    Value::Object(out)
}

fn translate_finish_reason_o2a(reason: &str) -> String {
    match reason {
        "stop" => "end_turn".to_string(),
        "length" => "max_tokens".to_string(),
        "tool_calls" => "tool_use".to_string(),
        _ => "end_turn".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Anthropic -> OpenAI
// ---------------------------------------------------------------------------

fn anthropic_to_openai_request(body: &Value, remote_model: &str) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut messages = obj.get("messages").cloned().unwrap_or(Value::Array(vec![]));
    if let Some(system) = obj.get("system") {
        let system_msg = serde_json::json!({"role": "system", "content": system});
        if let Some(arr) = messages.as_array_mut() {
            arr.insert(0, system_msg);
        }
    }

    let mut out = serde_json::Map::new();
    out.insert("model".to_string(), Value::String(remote_model.to_string()));
    out.insert("messages".to_string(), messages);
    if let Some(v) = get_u64(&obj, "max_tokens") {
        out.insert("max_tokens".to_string(), Value::Number(v.into()));
    }
    if let Some(v) = get_f64(&obj, "temperature") {
        out.insert("temperature".to_string(), Value::Number(serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into())));
    }
    if obj.get("stream").and_then(|v| v.as_bool()).unwrap_or(false) {
        out.insert("stream".to_string(), Value::Bool(true));
    }
    Value::Object(out)
}

fn anthropic_to_openai_response(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let text = obj
        .get("content")
        .map(text_content_to_string)
        .unwrap_or_default();
    let stop = obj
        .get("stop_reason")
        .and_then(|v| v.as_str())
        .map(translate_finish_reason_a2o)
        .unwrap_or_else(|| "stop".to_string());

    let mut out = serde_json::Map::new();
    out.insert(
        "choices".to_string(),
        Value::Array(vec![serde_json::json!({
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": stop,
        })]),
    );
    if let Some(usage) = normalize_usage_anthropic(obj.get("usage")) {
        out.insert("usage".to_string(), usage);
    }
    Value::Object(out)
}

fn translate_finish_reason_a2o(reason: &str) -> String {
    match reason {
        "end_turn" => "stop".to_string(),
        "max_tokens" => "length".to_string(),
        "tool_use" => "tool_calls".to_string(),
        _ => "stop".to_string(),
    }
}

// ---------------------------------------------------------------------------
// OpenAI -> Google
// ---------------------------------------------------------------------------

fn openai_to_google_request(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let (system, messages) = split_openai_messages(obj.get("messages"));

    let mut contents = Vec::new();
    for m in messages.as_array().unwrap_or(&Vec::new()).iter() {
        if let Some(role) = m.get("role").and_then(|v| v.as_str()) {
            let google_role = match role {
                "assistant" => "model",
                _ => "user",
            };
            let text = m.get("content").map(text_content_to_string).unwrap_or_default();
            contents.push(serde_json::json!({
                "role": google_role,
                "parts": [{"text": text}],
            }));
        }
    }

    let mut out = serde_json::Map::new();
    out.insert("contents".to_string(), Value::Array(contents));
    if let Some(system) = system {
        out.insert(
            "systemInstruction".to_string(),
            serde_json::json!({"parts": [{"text": system}]}),
        );
    }

    let mut gen_config = serde_json::Map::new();
    if let Some(v) = get_u64(&obj, "max_tokens") {
        gen_config.insert("maxOutputTokens".to_string(), Value::Number(v.into()));
    }
    if let Some(v) = get_f64(&obj, "temperature") {
        gen_config.insert("temperature".to_string(), Value::Number(serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into())));
    }
    if !gen_config.is_empty() {
        out.insert("generationConfig".to_string(), Value::Object(gen_config));
    }
    Value::Object(out)
}

fn openai_to_google_response(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let text = obj
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .map(text_content_to_string)
        .unwrap_or_default();
    let stop = obj
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .map(translate_finish_reason_o2g)
        .unwrap_or_else(|| "STOP".to_string());

    let mut out = serde_json::Map::new();
    out.insert(
        "candidates".to_string(),
        Value::Array(vec![serde_json::json!({
            "content": {"role": "model", "parts": [{"text": text}]},
            "finishReason": stop,
        })]),
    );
    if let Some(usage) = normalize_usage_openai(obj.get("usage")) {
        if let Some(u_obj) = usage.as_object() {
            let mut gm = serde_json::Map::new();
            if let Some(v) = u_obj.get("promptTokens").and_then(|v| v.as_u64()) {
                gm.insert("promptTokenCount".to_string(), Value::Number(v.into()));
            }
            if let Some(v) = u_obj.get("completionTokens").and_then(|v| v.as_u64()) {
                gm.insert("candidatesTokenCount".to_string(), Value::Number(v.into()));
            }
            if let Some(v) = u_obj.get("totalTokens").and_then(|v| v.as_u64()) {
                gm.insert("totalTokenCount".to_string(), Value::Number(v.into()));
            }
            out.insert("usageMetadata".to_string(), Value::Object(gm));
        }
    }
    Value::Object(out)
}

fn translate_finish_reason_o2g(reason: &str) -> String {
    match reason {
        "stop" => "STOP".to_string(),
        "length" => "MAX_TOKENS".to_string(),
        _ => "OTHER".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Anthropic -> Google
// ---------------------------------------------------------------------------

fn anthropic_to_google_request(body: &Value) -> Value {
    // Anthropic body is structurally similar to OpenAI after system extraction,
    // so normalize it to OpenAI shape first then convert.
    let openai = anthropic_to_openai_request(body, "");
    openai_to_google_request(&openai)
}

fn anthropic_to_google_response(body: &Value) -> Value {
    // Normalize Anthropic response to OpenAI shape, then to Google shape.
    let openai = anthropic_to_openai_response(body);
    openai_to_google_response(&openai)
}

// ---------------------------------------------------------------------------
// Google -> OpenAI
// ---------------------------------------------------------------------------

fn google_to_openai_request(body: &Value, remote_model: &str) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut messages = Vec::new();

    if let Some(system) = obj
        .get("systemInstruction")
        .and_then(|v| v.get("parts"))
        .and_then(|p| p.as_array())
    {
        let text = system
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("");
        if !text.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": text}));
        }
    }

    if let Some(contents) = obj.get("contents").and_then(|v| v.as_array()) {
        for c in contents {
            let role = c
                .get("role")
                .and_then(|v| v.as_str())
                .map(|r| if r == "model" { "assistant" } else { "user" })
                .unwrap_or("user");
            let text = c
                .get("parts")
                .and_then(|p| p.as_array())
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default();
            messages.push(serde_json::json!({"role": role, "content": text}));
        }
    }

    let mut out = serde_json::Map::new();
    out.insert("model".to_string(), Value::String(remote_model.to_string()));
    out.insert("messages".to_string(), Value::Array(messages));

    let gen_config = obj.get("generationConfig").and_then(|v| v.as_object()).cloned().unwrap_or_default();
    if let Some(v) = get_u64(&gen_config, "maxOutputTokens") {
        out.insert("max_tokens".to_string(), Value::Number(v.into()));
    }
    if let Some(v) = get_f64(&gen_config, "temperature") {
        out.insert("temperature".to_string(), Value::Number(serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into())));
    }
    Value::Object(out)
}

fn google_to_openai_response(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let text = obj
        .get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    let stop = obj
        .get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("finishReason"))
        .and_then(|v| v.as_str())
        .map(translate_finish_reason_g2o)
        .unwrap_or_else(|| "stop".to_string());

    let mut out = serde_json::Map::new();
    out.insert(
        "choices".to_string(),
        Value::Array(vec![serde_json::json!({
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": stop,
        })]),
    );
    if let Some(usage) = normalize_usage_google(obj.get("usageMetadata")) {
        out.insert("usage".to_string(), usage);
    }
    Value::Object(out)
}

fn translate_finish_reason_g2o(reason: &str) -> String {
    match reason {
        "STOP" => "stop".to_string(),
        "MAX_TOKENS" => "length".to_string(),
        _ => "stop".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Google -> Anthropic
// ---------------------------------------------------------------------------

fn google_to_anthropic_request(body: &Value, remote_model: &str) -> Value {
    // Normalize Google -> OpenAI, then OpenAI -> Anthropic.
    let openai = google_to_openai_request(body, remote_model);
    openai_to_anthropic_request(&openai, remote_model)
}

fn google_to_anthropic_response(body: &Value) -> Value {
    let openai = google_to_openai_response(body);
    openai_to_anthropic_response(&openai)
}

// ---------------------------------------------------------------------------
// Message helpers
// ---------------------------------------------------------------------------

/// Split an OpenAI-style messages array into an optional system string and a
/// messages array with system messages removed.
fn split_openai_messages(messages: Option<&Value>) -> (Option<String>, Value) {
    let mut system_parts = Vec::new();
    let mut out = Vec::new();
    for m in messages.and_then(|v| v.as_array()).unwrap_or(&Vec::new()).iter() {
        if let Some(role) = m.get("role").and_then(|v| v.as_str()) {
            if role == "system" {
                system_parts.push(m.get("content").map(text_content_to_string).unwrap_or_default());
                continue;
            }
        }
        out.push(m.clone());
    }
    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n"))
    };
    (system, Value::Array(out))
}

// ---------------------------------------------------------------------------
// Usage normalization
// ---------------------------------------------------------------------------

fn normalize_usage_openai(usage: Option<&Value>) -> Option<Value> {
    let obj = usage?.as_object()?;
    let prompt = obj.get("prompt_tokens").and_then(|v| v.as_u64())?;
    let completion = obj.get("completion_tokens").and_then(|v| v.as_u64())?;
    let total = obj
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(prompt + completion);
    Some(serde_json::json!({
        "promptTokens": prompt,
        "completionTokens": completion,
        "totalTokens": total,
    }))
}

fn normalize_usage_anthropic(usage: Option<&Value>) -> Option<Value> {
    let obj = usage?.as_object()?;
    let prompt = obj.get("input_tokens").and_then(|v| v.as_u64())?;
    let completion = obj.get("output_tokens").and_then(|v| v.as_u64())?;
    Some(serde_json::json!({
        "promptTokens": prompt,
        "completionTokens": completion,
        "totalTokens": prompt + completion,
    }))
}

fn normalize_usage_google(usage: Option<&Value>) -> Option<Value> {
    let obj = usage?.as_object()?;
    let prompt = obj.get("promptTokenCount").and_then(|v| v.as_u64())?;
    let completion = obj.get("candidatesTokenCount").and_then(|v| v.as_u64())?;
    let total = obj
        .get("totalTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(prompt + completion);
    Some(serde_json::json!({
        "promptTokens": prompt,
        "completionTokens": completion,
        "totalTokens": total,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn openai_body() -> Value {
        serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Hello"},
            ],
            "max_tokens": 100,
            "temperature": 0.5,
        })
    }

    fn anthropic_body() -> Value {
        serde_json::json!({
            "model": "claude-3-5-sonnet",
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 100,
            "temperature": 0.5,
        })
    }

    fn google_body() -> Value {
        serde_json::json!({
            "contents": [
                {"role": "user", "parts": [{"text": "Hello"}]},
            ],
            "systemInstruction": {"parts": [{"text": "You are a helpful assistant."}]},
            "generationConfig": {"maxOutputTokens": 100, "temperature": 0.5},
        })
    }

    #[test]
    fn openai_to_anthropic_request_round_trip() {
        let out = convert_request(
            ProviderFormat::OpenAI,
            ProviderFormat::Anthropic,
            &openai_body(),
            "claude-3-5-sonnet",
        );
        assert_eq!(out["model"], "claude-3-5-sonnet");
        assert_eq!(out["system"], "You are a helpful assistant.");
        assert_eq!(out["messages"].as_array().unwrap().len(), 1);
        assert_eq!(out["max_tokens"], 100);
    }

    #[test]
    fn anthropic_to_openai_request_round_trip() {
        let out = convert_request(
            ProviderFormat::Anthropic,
            ProviderFormat::OpenAI,
            &anthropic_body(),
            "gpt-4o",
        );
        assert_eq!(out["model"], "gpt-4o");
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn openai_to_google_request() {
        let out = convert_request(
            ProviderFormat::OpenAI,
            ProviderFormat::Google,
            &openai_body(),
            "gemini-1.5-pro",
        );
        assert!(out.get("model").is_none());
        assert_eq!(
            out["systemInstruction"]["parts"][0]["text"],
            "You are a helpful assistant."
        );
        assert_eq!(out["contents"].as_array().unwrap().len(), 1);
        assert_eq!(out["generationConfig"]["maxOutputTokens"], 100);
    }

    #[test]
    fn google_to_openai_request() {
        let out = convert_request(
            ProviderFormat::Google,
            ProviderFormat::OpenAI,
            &google_body(),
            "gpt-4o",
        );
        assert_eq!(out["model"], "gpt-4o");
        assert_eq!(out["messages"].as_array().unwrap().len(), 2);
        assert_eq!(out["max_tokens"], 100);
    }

    #[test]
    fn openai_to_anthropic_response() {
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "Hi"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15},
        });
        let out = convert_response(ProviderFormat::OpenAI, ProviderFormat::Anthropic, &body);
        assert_eq!(out["content"][0]["text"], "Hi");
        assert_eq!(out["stop_reason"], "end_turn");
        assert_eq!(out["usage"]["input_tokens"], 10);
    }

    #[test]
    fn anthropic_to_openai_response() {
        let body = serde_json::json!({
            "content": [{"type": "text", "text": "Hi"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        });
        let out = convert_response(ProviderFormat::Anthropic, ProviderFormat::OpenAI, &body);
        assert_eq!(out["choices"][0]["message"]["content"], "Hi");
        assert_eq!(out["choices"][0]["finish_reason"], "stop");
        assert_eq!(out["usage"]["prompt_tokens"], 10);
    }

    #[test]
    fn google_to_openai_response() {
        let body = serde_json::json!({
            "candidates": [{"content": {"parts": [{"text": "Hi"}]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5, "totalTokenCount": 15},
        });
        let out = convert_response(ProviderFormat::Google, ProviderFormat::OpenAI, &body);
        assert_eq!(out["choices"][0]["message"]["content"], "Hi");
        assert_eq!(out["usage"]["prompt_tokens"], 10);
    }

    #[test]
    fn target_paths() {
        assert_eq!(
            target_path(ProviderFormat::OpenAI, ProviderFormat::Anthropic, "claude", "/v1/chat/completions"),
            "/v1/messages"
        );
        assert_eq!(
            target_path(ProviderFormat::Anthropic, ProviderFormat::Google, "gemini", "/v1/messages"),
            "/v1beta/models/gemini:generateContent"
        );
        assert_eq!(
            target_path(ProviderFormat::OpenAI, ProviderFormat::OpenAI, "gpt", "/v1/chat/completions"),
            "/v1/chat/completions"
        );
    }
}
