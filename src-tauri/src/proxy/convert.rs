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
        (ProviderFormat::OpenAI, ProviderFormat::Anthropic) => openai_to_anthropic_response(body),
        (ProviderFormat::OpenAI, ProviderFormat::Google) => openai_to_google_response(body),
        (ProviderFormat::Anthropic, ProviderFormat::OpenAI) => anthropic_to_openai_response(body),
        (ProviderFormat::Anthropic, ProviderFormat::Google) => anthropic_to_google_response(body),
        (ProviderFormat::Google, ProviderFormat::OpenAI) => google_to_openai_response(body),
        (ProviderFormat::Google, ProviderFormat::Anthropic) => google_to_anthropic_response(body),
        _ => body.clone(),
    }
}

/// Convert a single SSE streaming chunk from provider format (`from`) to the
/// client format (`to`). `event` is the SSE event name when one was supplied by
/// the upstream (Anthropic/Google). Returns `None` when the chunk should be
/// passed through unchanged.
pub fn convert_sse_data(
    from: ProviderFormat,
    to: ProviderFormat,
    event: Option<&str>,
    data: &Value,
) -> Option<(Option<String>, Value)> {
    if from == to {
        return Some((event.map(str::to_string), data.clone()));
    }

    match (from, to) {
        (ProviderFormat::OpenAI, ProviderFormat::Anthropic) => openai_to_anthropic_sse_data(data),
        (ProviderFormat::Anthropic, ProviderFormat::OpenAI) => {
            anthropic_to_openai_sse_data(event, data)
        }
        (ProviderFormat::Google, ProviderFormat::OpenAI) => {
            google_to_openai_sse_data(data).map(|v| (None, v))
        }
        (ProviderFormat::Google, ProviderFormat::Anthropic) => {
            google_to_openai_sse_data(data).and_then(|openai| openai_to_anthropic_sse_data(&openai))
        }
        (ProviderFormat::OpenAI, ProviderFormat::Google) => {
            openai_to_google_sse_data(data).map(|v| (None, v))
        }
        (ProviderFormat::Anthropic, ProviderFormat::Google) => {
            anthropic_to_openai_sse_data(event, data)
                .and_then(|(_, openai)| openai_to_google_sse_data(&openai).map(|v| (None, v)))
        }
        _ => Some((event.map(str::to_string), data.clone())),
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
    stream: bool,
) -> String {
    if from == to {
        return client_path.to_string();
    }
    match to {
        ProviderFormat::OpenAI => "/v1/chat/completions".to_string(),
        ProviderFormat::Anthropic => "/v1/messages".to_string(),
        ProviderFormat::Google => {
            let suffix = if stream {
                "streamGenerateContent"
            } else {
                "generateContent"
            };
            format!("/v1beta/models/{remote_model}:{suffix}")
        }
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

fn copy_f64(
    out: &mut serde_json::Map<String, Value>,
    src: &serde_json::Map<String, Value>,
    key: &str,
) {
    if let Some(v) = get_f64(src, key) {
        out.insert(
            key.to_string(),
            Value::Number(serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into())),
        );
    }
}

fn copy_u64(
    out: &mut serde_json::Map<String, Value>,
    src: &serde_json::Map<String, Value>,
    key: &str,
) {
    if let Some(v) = get_u64(src, key) {
        out.insert(key.to_string(), Value::Number(v.into()));
    }
}

fn copy_stream(out: &mut serde_json::Map<String, Value>, src: &serde_json::Map<String, Value>) {
    if src.get("stream").and_then(|v| v.as_bool()).unwrap_or(false) {
        out.insert("stream".to_string(), Value::Bool(true));
    }
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

fn openai_stop_to_array(stop: Option<&Value>) -> Option<Vec<Value>> {
    stop.map(|v| match v {
        Value::String(s) => vec![Value::String(s.clone())],
        Value::Array(arr) => arr.clone(),
        _ => Vec::new(),
    })
    .filter(|v| !v.is_empty())
}

fn openai_tools_to_anthropic(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            if t.get("type").and_then(|v| v.as_str()) != Some("function") {
                return None;
            }
            let f = t.get("function")?;
            let name = f.get("name")?.as_str()?;
            let mut tool = serde_json::Map::new();
            tool.insert("name".to_string(), Value::String(name.to_string()));
            if let Some(desc) = f.get("description").cloned() {
                tool.insert("description".to_string(), desc);
            }
            tool.insert(
                "input_schema".to_string(),
                f.get("parameters")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
            );
            Some(Value::Object(tool))
        })
        .collect()
}

fn openai_tool_choice_to_anthropic(tc: &Value) -> Option<Value> {
    match tc {
        Value::String(s) if s == "auto" => Some(Value::String("auto".to_string())),
        Value::String(s) if s == "none" => Some(Value::String("none".to_string())),
        Value::Object(o) if o.get("type").and_then(|v| v.as_str()) == Some("function") => {
            let name = o
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())?;
            Some(serde_json::json!({"type": "tool", "name": name}))
        }
        _ => None,
    }
}

fn anthropic_tools_to_openai(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?;
            let mut function = serde_json::Map::new();
            function.insert("name".to_string(), Value::String(name.to_string()));
            if let Some(desc) = t.get("description").cloned() {
                function.insert("description".to_string(), desc);
            }
            function.insert(
                "parameters".to_string(),
                t.get("input_schema")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
            );
            Some(serde_json::json!({"type": "function", "function": function}))
        })
        .collect()
}

fn anthropic_tool_choice_to_openai(tc: &Value) -> Option<Value> {
    match tc {
        Value::String(s) if s == "auto" || s == "none" => Some(Value::String(s.clone())),
        Value::Object(o) => match o.get("type").and_then(|v| v.as_str()) {
            Some("tool") => {
                let name = o.get("name").and_then(|v| v.as_str())?;
                Some(serde_json::json!({"type": "function", "function": {"name": name}}))
            }
            Some("any") => Some(Value::String("auto".to_string())),
            _ => None,
        },
        _ => None,
    }
}

fn openai_tools_to_google(tools: &[Value]) -> Vec<Value> {
    let decls: Vec<Value> = tools
        .iter()
        .filter_map(|t| {
            if t.get("type").and_then(|v| v.as_str()) != Some("function") {
                return None;
            }
            let f = t.get("function")?;
            let name = f.get("name")?.as_str()?;
            let mut decl = serde_json::Map::new();
            decl.insert("name".to_string(), Value::String(name.to_string()));
            if let Some(desc) = f.get("description").cloned() {
                decl.insert("description".to_string(), desc);
            }
            decl.insert(
                "parameters".to_string(),
                f.get("parameters")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
            );
            Some(Value::Object(decl))
        })
        .collect();
    if decls.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({"functionDeclarations": decls})]
    }
}

fn google_tools_to_openai(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| t.get("functionDeclarations").and_then(|d| d.as_array()))
        .flat_map(|decls| {
            decls.iter().filter_map(|f| {
                let name = f.get("name")?.as_str()?;
                let mut function = serde_json::Map::new();
                function.insert("name".to_string(), Value::String(name.to_string()));
                if let Some(desc) = f.get("description").cloned() {
                    function.insert("description".to_string(), desc);
                }
                function.insert(
                    "parameters".to_string(),
                    f.get("parameters")
                        .cloned()
                        .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
                );
                Some(serde_json::json!({"type": "function", "function": function}))
            })
        })
        .collect()
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
        out.insert("system".to_string(), Value::String(system));
    }
    out.insert("messages".to_string(), messages);
    copy_u64(&mut out, &obj, "max_tokens");
    copy_f64(&mut out, &obj, "temperature");
    copy_f64(&mut out, &obj, "top_p");
    copy_stream(&mut out, &obj);
    if let Some(stop) = openai_stop_to_array(obj.get("stop")) {
        out.insert("stop_sequences".to_string(), Value::Array(stop));
    }
    if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
        let anthropic_tools = openai_tools_to_anthropic(tools);
        if !anthropic_tools.is_empty() {
            out.insert("tools".to_string(), Value::Array(anthropic_tools));
        }
    }
    if let Some(tc) = obj.get("tool_choice") {
        if let Some(converted) = openai_tool_choice_to_anthropic(tc) {
            out.insert("tool_choice".to_string(), converted);
        }
    }
    Value::Object(out)
}

fn openai_to_anthropic_response(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let choice = obj
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first());
    let message = choice.and_then(|c| c.get("message"));

    let mut content = Vec::new();
    if let Some(text) = message
        .and_then(|m| m.get("content"))
        .map(text_content_to_string)
    {
        if !text.is_empty() {
            content.push(serde_json::json!({"type": "text", "text": text}));
        }
    }
    if let Some(tools) = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|v| v.as_array())
    {
        for tc in tools {
            if let Some(tool) = openai_tool_call_to_anthropic(tc) {
                content.push(tool);
            }
        }
    }

    let stop = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .map(translate_finish_reason_o2a)
        .unwrap_or_else(|| "end_turn".to_string());

    let mut out = serde_json::Map::new();
    if !content.is_empty() {
        out.insert("content".to_string(), Value::Array(content));
    }
    out.insert("stop_reason".to_string(), Value::String(stop));
    out.insert("role".to_string(), Value::String("assistant".to_string()));
    if let Some((prompt, completion, _)) = obj.get("usage").and_then(extract_usage_openai) {
        out.insert(
            "usage".to_string(),
            build_usage_anthropic(prompt, completion),
        );
    }
    Value::Object(out)
}

fn openai_tool_call_to_anthropic(tc: &Value) -> Option<Value> {
    let obj = tc.as_object()?;
    let id = obj.get("id")?.as_str()?;
    let function = obj.get("function")?;
    let name = function.get("name")?.as_str()?;
    let args_str = function
        .get("arguments")
        .and_then(|v| v.as_str())
        .unwrap_or("{}");
    let input: Value =
        serde_json::from_str(args_str).unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
    Some(serde_json::json!({
        "type": "tool_use",
        "id": id,
        "name": name,
        "input": input,
    }))
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
    let mut messages = obj
        .get("messages")
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));
    if let Some(system) = obj.get("system") {
        let system_msg = serde_json::json!({"role": "system", "content": system});
        if let Some(arr) = messages.as_array_mut() {
            arr.insert(0, system_msg);
        }
    }

    let mut out = serde_json::Map::new();
    out.insert("model".to_string(), Value::String(remote_model.to_string()));
    out.insert("messages".to_string(), messages);
    copy_u64(&mut out, &obj, "max_tokens");
    copy_f64(&mut out, &obj, "temperature");
    copy_f64(&mut out, &obj, "top_p");
    copy_stream(&mut out, &obj);
    if let Some(stop) = obj.get("stop_sequences").and_then(|v| v.as_array()) {
        out.insert("stop".to_string(), Value::Array(stop.clone()));
    }
    if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
        let openai_tools = anthropic_tools_to_openai(tools);
        if !openai_tools.is_empty() {
            out.insert("tools".to_string(), Value::Array(openai_tools));
        }
    }
    if let Some(tc) = obj.get("tool_choice") {
        if let Some(converted) = anthropic_tool_choice_to_openai(tc) {
            out.insert("tool_choice".to_string(), converted);
        }
    }
    Value::Object(out)
}

fn anthropic_to_openai_response(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    if let Some(content) = obj.get("content").and_then(|v| v.as_array()) {
        for block in content {
            match block.get("type").and_then(|v| v.as_str()) {
                Some("text") => {
                    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        text_parts.push(t);
                    }
                }
                Some("tool_use") => {
                    if let Some(tool) = anthropic_tool_use_to_openai(block) {
                        tool_calls.push(tool);
                    }
                }
                _ => {}
            }
        }
    }

    let stop = obj
        .get("stop_reason")
        .and_then(|v| v.as_str())
        .map(translate_finish_reason_a2o)
        .unwrap_or_else(|| "stop".to_string());

    let mut message = serde_json::Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert("content".to_string(), Value::String(text_parts.join("")));
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    let mut out = serde_json::Map::new();
    out.insert(
        "choices".to_string(),
        Value::Array(vec![serde_json::json!({
            "index": 0,
            "message": message,
            "finish_reason": stop,
        })]),
    );
    if let Some((prompt, completion)) = obj.get("usage").and_then(extract_usage_anthropic) {
        out.insert(
            "usage".to_string(),
            build_usage_openai(prompt, completion, prompt + completion),
        );
    }
    Value::Object(out)
}

fn anthropic_tool_use_to_openai(block: &Value) -> Option<Value> {
    let obj = block.as_object()?;
    let id = obj.get("id")?.as_str()?;
    let name = obj.get("name")?.as_str()?;
    let input = obj
        .get("input")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    Some(serde_json::json!({
        "id": id,
        "type": "function",
        "function": {"name": name, "arguments": input.to_string()},
    }))
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
            let text = m
                .get("content")
                .map(text_content_to_string)
                .unwrap_or_default();
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
        gen_config.insert(
            "temperature".to_string(),
            Value::Number(serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into())),
        );
    }
    if let Some(v) = get_f64(&obj, "top_p") {
        gen_config.insert(
            "topP".to_string(),
            Value::Number(serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into())),
        );
    }
    if let Some(stop) = openai_stop_to_array(obj.get("stop")) {
        gen_config.insert("stopSequences".to_string(), Value::Array(stop));
    }
    if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
        let decls = openai_tools_to_google(tools);
        if !decls.is_empty() {
            out.insert("tools".to_string(), Value::Array(decls));
        }
    }
    if let Some(fmt) = obj.get("response_format") {
        if fmt.get("type").and_then(|v| v.as_str()) == Some("json_object") {
            gen_config.insert(
                "responseMimeType".to_string(),
                Value::String("application/json".to_string()),
            );
        }
    }
    if !gen_config.is_empty() {
        out.insert("generationConfig".to_string(), Value::Object(gen_config));
    }
    Value::Object(out)
}

fn openai_to_google_response(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let choice = obj
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first());
    let message = choice.and_then(|c| c.get("message"));

    let mut parts = Vec::new();
    if let Some(text) = message
        .and_then(|m| m.get("content"))
        .map(text_content_to_string)
    {
        if !text.is_empty() {
            parts.push(serde_json::json!({"text": text}));
        }
    }
    if let Some(tools) = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|v| v.as_array())
    {
        for tc in tools {
            if let Some(call) = openai_tool_call_to_google(tc) {
                parts.push(call);
            }
        }
    }

    let stop = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .map(translate_finish_reason_o2g)
        .unwrap_or_else(|| "STOP".to_string());

    let mut out = serde_json::Map::new();
    out.insert(
        "candidates".to_string(),
        Value::Array(vec![serde_json::json!({
            "content": {"role": "model", "parts": parts},
            "finishReason": stop,
        })]),
    );
    if let Some((prompt, completion, total)) = obj.get("usage").and_then(extract_usage_openai) {
        out.insert(
            "usageMetadata".to_string(),
            build_usage_google(prompt, completion, total),
        );
    }
    Value::Object(out)
}

fn openai_tool_call_to_google(tc: &Value) -> Option<Value> {
    let obj = tc.as_object()?;
    let function = obj.get("function")?;
    let name = function.get("name")?.as_str()?;
    let args_str = function
        .get("arguments")
        .and_then(|v| v.as_str())
        .unwrap_or("{}");
    let args: Value =
        serde_json::from_str(args_str).unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
    Some(serde_json::json!({"functionCall": {"name": name, "args": args}}))
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

    let gen_config = obj
        .get("generationConfig")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    if let Some(v) = get_u64(&gen_config, "maxOutputTokens") {
        out.insert("max_tokens".to_string(), Value::Number(v.into()));
    }
    if let Some(v) = get_f64(&gen_config, "temperature") {
        out.insert(
            "temperature".to_string(),
            Value::Number(serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into())),
        );
    }
    if let Some(v) = get_f64(&gen_config, "topP") {
        out.insert(
            "top_p".to_string(),
            Value::Number(serde_json::Number::from_f64(v).unwrap_or_else(|| 0.into())),
        );
    }
    if let Some(stop) = gen_config.get("stopSequences").and_then(|v| v.as_array()) {
        out.insert("stop".to_string(), Value::Array(stop.clone()));
    }
    if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
        let openai_tools = google_tools_to_openai(tools);
        if !openai_tools.is_empty() {
            out.insert("tools".to_string(), Value::Array(openai_tools));
        }
    }
    if let Some(mime) = gen_config.get("responseMimeType").and_then(|v| v.as_str()) {
        if mime.contains("json") {
            out.insert(
                "response_format".to_string(),
                serde_json::json!({"type": "json_object"}),
            );
        }
    }
    Value::Object(out)
}

fn google_to_openai_response(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let candidate = obj
        .get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first());
    let parts = candidate
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    for p in &parts {
        if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
            text_parts.push(t);
        } else if let Some(fc) = p.get("functionCall") {
            if let Some(tc) = google_function_call_to_openai(fc) {
                tool_calls.push(tc);
            }
        }
    }

    let stop = candidate
        .and_then(|c| c.get("finishReason"))
        .and_then(|v| v.as_str())
        .map(translate_finish_reason_g2o)
        .unwrap_or_else(|| "stop".to_string());

    let mut message = serde_json::Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert("content".to_string(), Value::String(text_parts.join("")));
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }

    let mut out = serde_json::Map::new();
    out.insert(
        "choices".to_string(),
        Value::Array(vec![serde_json::json!({
            "index": 0,
            "message": message,
            "finish_reason": stop,
        })]),
    );
    if let Some((prompt, completion, total)) =
        obj.get("usageMetadata").and_then(extract_usage_google)
    {
        out.insert(
            "usage".to_string(),
            build_usage_openai(prompt, completion, total),
        );
    }
    Value::Object(out)
}

fn google_function_call_to_openai(fc: &Value) -> Option<Value> {
    let obj = fc.as_object()?;
    let name = obj.get("name")?.as_str()?;
    let args = obj
        .get("args")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    Some(serde_json::json!({
        "id": format!("call_{}", name),
        "type": "function",
        "function": {"name": name, "arguments": args.to_string()},
    }))
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
    for m in messages
        .and_then(|v| v.as_array())
        .unwrap_or(&Vec::new())
        .iter()
    {
        if let Some(role) = m.get("role").and_then(|v| v.as_str()) {
            if role == "system" {
                system_parts.push(
                    m.get("content")
                        .map(text_content_to_string)
                        .unwrap_or_default(),
                );
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
// SSE chunk conversion
// ---------------------------------------------------------------------------

fn openai_to_anthropic_sse_data(data: &Value) -> Option<(Option<String>, Value)> {
    let choice = data.get("choices")?.as_array()?.first()?;
    let delta = choice.get("delta")?;

    if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
        return Some((
            Some("content_block_delta".to_string()),
            serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": text},
            }),
        ));
    }

    if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
        return Some((
            Some("message_delta".to_string()),
            serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": translate_finish_reason_o2a(reason)},
            }),
        ));
    }

    None
}

fn anthropic_to_openai_sse_data(
    event: Option<&str>,
    data: &Value,
) -> Option<(Option<String>, Value)> {
    let event = event?;
    match event {
        "content_block_delta" => {
            let text = data
                .get("delta")
                .and_then(|d| d.get("text"))
                .and_then(|v| v.as_str())?;
            Some((
                None,
                serde_json::json!({
                    "choices": [{"index": 0, "delta": {"content": text}}],
                }),
            ))
        }
        "message_delta" => {
            let stop = data
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|v| v.as_str())
                .map(translate_finish_reason_a2o)
                .unwrap_or_else(|| "stop".to_string());
            let mut out = serde_json::json!({
                "choices": [{"index": 0, "delta": {}, "finish_reason": stop}],
            });
            if let Some(usage) = data.get("usage") {
                out["usage"] = usage.clone();
            }
            Some((None, out))
        }
        "message_stop" => Some((
            None,
            serde_json::json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
        )),
        "message_start" => Some((
            None,
            serde_json::json!({"choices": [{"index": 0, "delta": {"role": "assistant"}}]}),
        )),
        _ => None,
    }
}

fn google_to_openai_sse_data(data: &Value) -> Option<Value> {
    let candidate = data.get("candidates")?.as_array()?.first()?;
    let text = candidate
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
                .next()
        })?;
    let finish = candidate
        .get("finishReason")
        .and_then(|v| v.as_str())
        .map(translate_finish_reason_g2o);
    let mut out = serde_json::json!({
        "choices": [{"index": 0, "delta": {"content": text}}],
    });
    if let Some(stop) = finish {
        out["choices"][0]["finish_reason"] = Value::String(stop);
    }
    if let Some((prompt, completion, total)) =
        data.get("usageMetadata").and_then(extract_usage_google)
    {
        out["usage"] = build_usage_openai(prompt, completion, total);
    }
    Some(out)
}

fn openai_to_google_sse_data(data: &Value) -> Option<Value> {
    let choice = data.get("choices")?.as_array()?.first()?;
    let text = choice
        .get("delta")
        .and_then(|d| d.get("content"))
        .and_then(|v| v.as_str())?;
    let finish = choice
        .get("finish_reason")
        .and_then(|v| v.as_str())
        .map(translate_finish_reason_o2g);
    let mut candidate = serde_json::json!({
        "content": {"role": "model", "parts": [{"text": text}]},
    });
    if let Some(stop) = finish {
        candidate["finishReason"] = Value::String(stop);
    }
    let mut out = serde_json::json!({"candidates": [candidate]});
    if let Some((prompt, completion, total)) = data.get("usage").and_then(extract_usage_openai) {
        out["usageMetadata"] = build_usage_google(prompt, completion, total);
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Usage normalization
// ---------------------------------------------------------------------------

fn extract_usage_openai(usage: &Value) -> Option<(u64, u64, u64)> {
    let obj = usage.as_object()?;
    let prompt = obj.get("prompt_tokens").and_then(|v| v.as_u64())?;
    let completion = obj.get("completion_tokens").and_then(|v| v.as_u64())?;
    let total = obj
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(prompt + completion);
    Some((prompt, completion, total))
}

fn extract_usage_anthropic(usage: &Value) -> Option<(u64, u64)> {
    let obj = usage.as_object()?;
    let prompt = obj.get("input_tokens").and_then(|v| v.as_u64())?;
    let completion = obj.get("output_tokens").and_then(|v| v.as_u64())?;
    Some((prompt, completion))
}

fn extract_usage_google(usage: &Value) -> Option<(u64, u64, u64)> {
    let obj = usage.as_object()?;
    let prompt = obj.get("promptTokenCount").and_then(|v| v.as_u64())?;
    let completion = obj.get("candidatesTokenCount").and_then(|v| v.as_u64())?;
    let total = obj
        .get("totalTokenCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(prompt + completion);
    Some((prompt, completion, total))
}

fn build_usage_openai(prompt: u64, completion: u64, total: u64) -> Value {
    serde_json::json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": total,
    })
}

fn build_usage_anthropic(prompt: u64, completion: u64) -> Value {
    serde_json::json!({
        "input_tokens": prompt,
        "output_tokens": completion,
    })
}

fn build_usage_google(prompt: u64, completion: u64, total: u64) -> Value {
    serde_json::json!({
        "promptTokenCount": prompt,
        "candidatesTokenCount": completion,
        "totalTokenCount": total,
    })
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
            target_path(
                ProviderFormat::OpenAI,
                ProviderFormat::Anthropic,
                "claude",
                "/v1/chat/completions",
                false
            ),
            "/v1/messages"
        );
        assert_eq!(
            target_path(
                ProviderFormat::Anthropic,
                ProviderFormat::Google,
                "gemini",
                "/v1/messages",
                false
            ),
            "/v1beta/models/gemini:generateContent"
        );
        assert_eq!(
            target_path(
                ProviderFormat::Anthropic,
                ProviderFormat::Google,
                "gemini",
                "/v1/messages",
                true
            ),
            "/v1beta/models/gemini:streamGenerateContent"
        );
        assert_eq!(
            target_path(
                ProviderFormat::OpenAI,
                ProviderFormat::OpenAI,
                "gpt",
                "/v1/chat/completions",
                false
            ),
            "/v1/chat/completions"
        );
    }
}
