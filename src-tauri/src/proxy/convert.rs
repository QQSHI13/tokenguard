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
        (ProviderFormat::OpenAI, ProviderFormat::Responses) => {
            openai_to_responses_request(body, remote_model)
        }
        (ProviderFormat::Anthropic, ProviderFormat::OpenAI) => {
            anthropic_to_openai_request(body, remote_model)
        }
        (ProviderFormat::Anthropic, ProviderFormat::Google) => anthropic_to_google_request(body),
        (ProviderFormat::Anthropic, ProviderFormat::Responses) => {
            openai_to_responses_request(&anthropic_to_openai_request(body, remote_model), remote_model)
        }
        (ProviderFormat::Google, ProviderFormat::OpenAI) => {
            google_to_openai_request(body, remote_model)
        }
        (ProviderFormat::Google, ProviderFormat::Anthropic) => {
            google_to_anthropic_request(body, remote_model)
        }
        (ProviderFormat::Google, ProviderFormat::Responses) => {
            openai_to_responses_request(&google_to_openai_request(body, remote_model), remote_model)
        }
        (ProviderFormat::Responses, ProviderFormat::OpenAI) => {
            responses_to_openai_request(body, remote_model)
        }
        (ProviderFormat::Responses, ProviderFormat::Anthropic) => {
            openai_to_anthropic_request(&responses_to_openai_request(body, remote_model), remote_model)
        }
        (ProviderFormat::Responses, ProviderFormat::Google) => {
            openai_to_google_request(&responses_to_openai_request(body, remote_model))
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
        (ProviderFormat::OpenAI, ProviderFormat::Responses) => openai_to_responses_response(body),
        (ProviderFormat::Anthropic, ProviderFormat::OpenAI) => anthropic_to_openai_response(body),
        (ProviderFormat::Anthropic, ProviderFormat::Google) => anthropic_to_google_response(body),
        (ProviderFormat::Anthropic, ProviderFormat::Responses) => {
            openai_to_responses_response(&anthropic_to_openai_response(body))
        }
        (ProviderFormat::Google, ProviderFormat::OpenAI) => google_to_openai_response(body),
        (ProviderFormat::Google, ProviderFormat::Anthropic) => google_to_anthropic_response(body),
        (ProviderFormat::Google, ProviderFormat::Responses) => {
            openai_to_responses_response(&google_to_openai_response(body))
        }
        (ProviderFormat::Responses, ProviderFormat::OpenAI) => responses_to_openai_response(body),
        (ProviderFormat::Responses, ProviderFormat::Anthropic) => {
            openai_to_anthropic_response(&responses_to_openai_response(body))
        }
        (ProviderFormat::Responses, ProviderFormat::Google) => {
            openai_to_google_response(&responses_to_openai_response(body))
        }
        _ => body.clone(),
    }
}

/// Build an error envelope in the client's format carrying the upstream
/// status code and message. Used for upstream error responses, which must not
/// go through the success-path `convert_response`.
pub fn error_envelope(format: ProviderFormat, status: u16, body: &[u8]) -> Value {
    let message = extract_error_message(body);
    match format {
        ProviderFormat::OpenAI => serde_json::json!({
            "error": {"message": message, "type": "upstream_error", "code": status}
        }),
        ProviderFormat::Anthropic => serde_json::json!({
            "type": "error",
            "error": {"type": "api_error", "message": message},
        }),
        ProviderFormat::Google => serde_json::json!({
            "error": {"code": status, "message": message, "status": "UNKNOWN"},
        }),
        ProviderFormat::Responses => serde_json::json!({
            "error": {"message": message, "type": "upstream_error", "code": status}
        }),
    }
}

/// Pull the message out of common provider error shapes (Anthropic, OpenAI,
/// and Google all use `error.message`); fall back to the truncated raw body.
fn extract_error_message(body: &[u8]) -> String {
    if let Ok(v) = serde_json::from_slice::<Value>(body) {
        if let Some(m) = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return m.to_string();
        }
    }
    String::from_utf8_lossy(body).chars().take(500).collect()
}

/// Determine whether the client asked for a streaming response. OpenAI and
/// Anthropic signal it with `"stream": true` in the body; Gemini signals it
/// via the URL (`:streamGenerateContent` suffix) or the `?alt=sse` query.
pub fn is_stream_request(format: ProviderFormat, client_path: &str, body: &Value) -> bool {
    if body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    if format == ProviderFormat::Google {
        let (path, query) = match client_path.split_once('?') {
            Some((p, q)) => (p, q),
            None => (client_path, ""),
        };
        if path.contains(":streamGenerateContent") {
            return true;
        }
        return query.split('&').any(|kv| kv == "alt=sse");
    }
    false
}

/// Convert a single SSE streaming chunk from provider format (`from`) to the
/// client format (`to`). `event` is the SSE event name when one was supplied by
/// the upstream (Anthropic/Google). Returns `None` when the chunk has no
/// equivalent in the client format and should be dropped.
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
        (ProviderFormat::OpenAI, ProviderFormat::Google) => {
            openai_to_google_sse_data(data).map(|v| (None, v))
        }
        (ProviderFormat::OpenAI, ProviderFormat::Responses) => {
            openai_to_responses_sse_data(data).map(|v| {
                let event = v.get("type").and_then(|t| t.as_str()).map(str::to_string);
                (event, v)
            })
        }
        (ProviderFormat::Anthropic, ProviderFormat::OpenAI) => {
            anthropic_to_openai_sse_data(event, data)
        }
        (ProviderFormat::Anthropic, ProviderFormat::Google) => {
            anthropic_to_openai_sse_data(event, data)
                .and_then(|(_, openai)| openai_to_google_sse_data(&openai).map(|v| (None, v)))
        }
        (ProviderFormat::Anthropic, ProviderFormat::Responses) => anthropic_to_openai_sse_data(event, data)
            .and_then(|(_, openai)| {
                openai_to_responses_sse_data(&openai).map(|v| {
                    let event = v.get("type").and_then(|t| t.as_str()).map(str::to_string);
                    (event, v)
                })
            }),
        (ProviderFormat::Google, ProviderFormat::OpenAI) => {
            google_to_openai_sse_data(data).map(|v| (None, v))
        }
        (ProviderFormat::Google, ProviderFormat::Anthropic) => {
            google_to_openai_sse_data(data).and_then(|openai| openai_to_anthropic_sse_data(&openai))
        }
        (ProviderFormat::Google, ProviderFormat::Responses) => google_to_openai_sse_data(data)
            .and_then(|openai| {
                openai_to_responses_sse_data(&openai).map(|v| {
                    let event = v.get("type").and_then(|t| t.as_str()).map(str::to_string);
                    (event, v)
                })
            }),
        (ProviderFormat::Responses, ProviderFormat::OpenAI) => {
            responses_to_openai_sse_data(data).map(|v| (None, v))
        }
        (ProviderFormat::Responses, ProviderFormat::Anthropic) => responses_to_openai_sse_data(data)
            .and_then(|openai| openai_to_anthropic_sse_data(&openai)),
        (ProviderFormat::Responses, ProviderFormat::Google) => responses_to_openai_sse_data(data)
            .and_then(|openai| openai_to_google_sse_data(&openai).map(|v| (None, v))),
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
        if to == ProviderFormat::Google {
            return rewrite_google_model_segment(client_path, remote_model);
        }
        return client_path.to_string();
    }
    match to {
        ProviderFormat::OpenAI => "/v1/chat/completions".to_string(),
        ProviderFormat::Anthropic => "/v1/messages".to_string(),
        ProviderFormat::Responses => "/v1/responses".to_string(),
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

/// Replace the model segment of a Gemini-style path
/// (`/v1beta/models/{model}:{method}?query`) with the remote model name,
/// keeping the method suffix and query string.
fn rewrite_google_model_segment(client_path: &str, remote_model: &str) -> String {
    let (path, query) = match client_path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (client_path, None),
    };
    let rewritten = match path.rsplit_once("/models/") {
        Some((prefix, rest)) => match rest.split_once(':') {
            Some((_, method)) => format!("{prefix}/models/{remote_model}:{method}"),
            None => format!("{prefix}/models/{remote_model}"),
        },
        None => path.to_string(),
    };
    match query {
        Some(q) => format!("{rewritten}?{q}"),
        None => rewritten,
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

/// Mint a unique tool-call ID for a function name. Google has no tool-call
/// IDs, so they are synthesized per occurrence within one request/response,
/// letting parallel calls to the same function (and their results, mapped
/// back positionally) be told apart.
fn next_tool_call_id(name: &str, counts: &mut std::collections::HashMap<String, usize>) -> String {
    let n = counts.entry(name.to_string()).or_insert(0);
    let id = format!("call_{name}_{n}");
    *n += 1;
    id
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
            .filter_map(|p| {
                if p.get("type").and_then(|t| t.as_str()) == Some("text") {
                    p.get("text").and_then(|t| t.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Cross-format message content (text + images)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum ContentPart {
    Text(String),
    Image {
        mime: Option<String>,
        data: ImageData,
    },
}

#[derive(Debug, Clone)]
enum ImageData {
    Base64(String),
    Url(String),
}

fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (mime, b64) = rest.split_once(";base64,")?;
    Some((mime.to_string(), b64.to_string()))
}

fn to_data_url(mime: &str, data: &str) -> String {
    format!("data:{};base64,{}", mime, data)
}

fn openai_content_to_parts(content: &Value) -> Vec<ContentPart> {
    match content {
        Value::String(s) => vec![ContentPart::Text(s.clone())],
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                let t = p.get("type").and_then(|v| v.as_str())?;
                match t {
                    "text" => p
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| ContentPart::Text(s.to_string())),
                    "image_url" => {
                        let url = p
                            .get("image_url")
                            .and_then(|u| u.get("url"))
                            .and_then(|v| v.as_str())?;
                        if let Some((mime, data)) = parse_data_url(url) {
                            Some(ContentPart::Image {
                                mime: Some(mime),
                                data: ImageData::Base64(data),
                            })
                        } else {
                            Some(ContentPart::Image {
                                mime: None,
                                data: ImageData::Url(url.to_string()),
                            })
                        }
                    }
                    _ => None,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn anthropic_content_to_parts(content: &Value) -> Vec<ContentPart> {
    match content {
        Value::String(s) => vec![ContentPart::Text(s.clone())],
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                let t = p.get("type").and_then(|v| v.as_str())?;
                match t {
                    "text" => p
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|s| ContentPart::Text(s.to_string())),
                    "image" => {
                        let source = p.get("source")?;
                        let source_type = source.get("type").and_then(|v| v.as_str())?;
                        match source_type {
                            "base64" => {
                                let data = source.get("data").and_then(|v| v.as_str())?;
                                let mime = source
                                    .get("media_type")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string);
                                Some(ContentPart::Image {
                                    mime,
                                    data: ImageData::Base64(data.to_string()),
                                })
                            }
                            "url" => {
                                let url = source.get("url").and_then(|v| v.as_str())?;
                                Some(ContentPart::Image {
                                    mime: None,
                                    data: ImageData::Url(url.to_string()),
                                })
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn google_content_to_parts(content: &Value) -> Vec<ContentPart> {
    match content {
        Value::String(s) => vec![ContentPart::Text(s.clone())],
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                if let Some(text) = p.get("text").and_then(|v| v.as_str()) {
                    return Some(ContentPart::Text(text.to_string()));
                }
                if let Some(inline) = p.get("inlineData") {
                    let mime = inline
                        .get("mimeType")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let data = inline.get("data").and_then(|v| v.as_str())?;
                    return Some(ContentPart::Image {
                        mime,
                        data: ImageData::Base64(data.to_string()),
                    });
                }
                if let Some(file) = p.get("fileData") {
                    let mime = file
                        .get("mimeType")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    let url = file.get("fileUri").and_then(|v| v.as_str())?;
                    return Some(ContentPart::Image {
                        mime,
                        data: ImageData::Url(url.to_string()),
                    });
                }
                None
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parts_to_openai(parts: &[ContentPart]) -> Value {
    if parts.len() == 1 {
        if let ContentPart::Text(s) = &parts[0] {
            return Value::String(s.clone());
        }
    }
    Value::Array(
        parts
            .iter()
            .map(|p| match p {
                ContentPart::Text(s) => serde_json::json!({"type": "text", "text": s}),
                ContentPart::Image { mime, data } => match data {
                    ImageData::Base64(d) => {
                        let url = mime
                            .as_ref()
                            .map(|m| to_data_url(m, d))
                            .unwrap_or_else(|| d.clone());
                        serde_json::json!({"type": "image_url", "image_url": {"url": url}})
                    }
                    ImageData::Url(u) => {
                        serde_json::json!({"type": "image_url", "image_url": {"url": u}})
                    }
                },
            })
            .collect(),
    )
}

fn parts_to_anthropic(parts: &[ContentPart]) -> Value {
    if parts.len() == 1 {
        if let ContentPart::Text(s) = &parts[0] {
            return Value::String(s.clone());
        }
    }
    Value::Array(
        parts
            .iter()
            .map(|p| match p {
                ContentPart::Text(s) => serde_json::json!({"type": "text", "text": s}),
                ContentPart::Image { mime, data } => match data {
                    ImageData::Base64(d) => {
                        let mut source = serde_json::Map::new();
                        source.insert("type".to_string(), Value::String("base64".to_string()));
                        source.insert("data".to_string(), Value::String(d.clone()));
                        if let Some(m) = mime {
                            source.insert("media_type".to_string(), Value::String(m.clone()));
                        }
                        serde_json::json!({"type": "image", "source": source})
                    }
                    ImageData::Url(u) => serde_json::json!({
                        "type": "image",
                        "source": {"type": "url", "url": u}
                    }),
                },
            })
            .collect(),
    )
}

fn parts_to_google(parts: &[ContentPart]) -> Vec<Value> {
    parts
        .iter()
        .map(|p| match p {
            ContentPart::Text(s) => serde_json::json!({"text": s}),
            ContentPart::Image { mime, data } => match data {
                ImageData::Base64(d) => {
                    let mut inline = serde_json::Map::new();
                    inline.insert("data".to_string(), Value::String(d.clone()));
                    if let Some(m) = mime {
                        inline.insert("mimeType".to_string(), Value::String(m.clone()));
                    }
                    serde_json::json!({"inlineData": inline})
                }
                ImageData::Url(u) => {
                    let mut file = serde_json::Map::new();
                    file.insert("fileUri".to_string(), Value::String(u.clone()));
                    if let Some(m) = mime {
                        file.insert("mimeType".to_string(), Value::String(m.clone()));
                    }
                    serde_json::json!({"fileData": file})
                }
            },
        })
        .collect()
}

fn responses_content_to_parts(content: &Value) -> Vec<ContentPart> {
    match content {
        Value::String(s) => vec![ContentPart::Text(s.clone())],
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                match p.get("type").and_then(|t| t.as_str()) {
                    Some("input_text") | Some("output_text") => p
                        .get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| ContentPart::Text(s.to_string())),
                    Some("input_image") => {
                        let url = p.get("image_url").and_then(|u| {
                            if let Some(s) = u.as_str() {
                                Some(s.to_string())
                            } else {
                                u.get("url").and_then(|v| v.as_str()).map(str::to_string)
                            }
                        })?;
                        let mime = if url.starts_with("data:") {
                            parse_data_url(&url).map(|(m, _)| m)
                        } else {
                            None
                        };
                        Some(ContentPart::Image {
                            mime,
                            data: ImageData::Url(url),
                        })
                    }
                    _ => None,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parts_to_responses(parts: &[ContentPart], is_input: bool) -> Value {
    let text_type = if is_input { "input_text" } else { "output_text" };
    let arr: Vec<Value> = parts
        .iter()
        .map(|p| match p {
            ContentPart::Text(s) => serde_json::json!({"type": text_type, "text": s}),
            ContentPart::Image { mime, data } => match data {
                ImageData::Base64(d) => {
                    let url = mime
                        .as_ref()
                        .map(|m| to_data_url(m, d))
                        .unwrap_or_else(|| d.clone());
                    serde_json::json!({"type": "input_image", "image_url": {"url": url}})
                }
                ImageData::Url(u) => {
                    serde_json::json!({"type": "input_image", "image_url": {"url": u}})
                }
            },
        })
        .collect();
    if arr.len() == 1 {
        if let Value::Object(o) = &arr[0] {
            if o.get("type").and_then(|t| t.as_str()) == Some(text_type) {
                return o
                    .get("text")
                    .cloned()
                    .unwrap_or_else(|| Value::String(String::new()));
            }
        }
    }
    Value::Array(arr)
}

fn convert_message_content(content: &Value, from: ProviderFormat, to: ProviderFormat) -> Value {
    if from == to {
        return content.clone();
    }
    let parts = match from {
        ProviderFormat::OpenAI => openai_content_to_parts(content),
        ProviderFormat::Anthropic => anthropic_content_to_parts(content),
        ProviderFormat::Google => google_content_to_parts(content),
        ProviderFormat::Responses => responses_content_to_parts(content),
    };
    match to {
        ProviderFormat::OpenAI => parts_to_openai(&parts),
        ProviderFormat::Anthropic => parts_to_anthropic(&parts),
        ProviderFormat::Google => Value::Array(parts_to_google(&parts)),
        ProviderFormat::Responses => parts_to_responses(&parts, true),
    }
}

/// Look backwards through the message history to find the function/tool name
/// associated with a tool_use_id. Used when converting tool-result messages to
/// formats that require a name (e.g. Google `functionResponse`).
fn find_tool_name_by_id(messages: &[Value], tool_use_id: &str) -> Option<String> {
    for m in messages.iter().rev() {
        if m.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        // OpenAI-style assistant tool_calls.
        if let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) {
            for tc in tcs {
                if tc.get("id").and_then(|v| v.as_str()) == Some(tool_use_id) {
                    return tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                }
            }
        }
        // Anthropic-style assistant tool_use blocks.
        if let Some(blocks) = m.get("content").and_then(|v| v.as_array()) {
            for b in blocks {
                if b.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                    && b.get("id").and_then(|v| v.as_str()) == Some(tool_use_id)
                {
                    return b.get("name").and_then(|v| v.as_str()).map(str::to_string);
                }
            }
        }
    }
    None
}

/// Split an Anthropic user message content into plain text and tool-result pairs.
/// Returns `None` when the content contains anything other than text and
/// tool_result blocks (e.g. images), so the normal content converter can handle it.
fn split_anthropic_user_content(
    content: Option<&Value>,
) -> Option<(String, Vec<(String, String)>)> {
    let arr = content?.as_array()?;
    let mut text_parts = Vec::new();
    let mut tool_results = Vec::new();
    for block in arr {
        match block.get("type").and_then(|v| v.as_str()) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(t);
                }
            }
            Some("tool_result") => {
                let id = block.get("tool_use_id").and_then(|v| v.as_str())?;
                let content = match block.get("content") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Array(parts)) => parts
                        .iter()
                        .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
                        .collect::<Vec<_>>()
                        .join(""),
                    _ => String::new(),
                };
                tool_results.push((id.to_string(), content));
            }
            _ => return None,
        }
    }
    if text_parts.is_empty() && tool_results.is_empty() {
        return None;
    }
    Some((text_parts.join(""), tool_results))
}

/// Split an Anthropic assistant message content into plain text and OpenAI-style tool_calls.
/// Returns `None` for non-text/non-tool_use content so images are preserved.
fn split_anthropic_assistant_content(content: Option<&Value>) -> Option<(String, Vec<Value>)> {
    let arr = content?.as_array()?;
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    for block in arr {
        match block.get("type").and_then(|v| v.as_str()) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(t);
                }
            }
            Some("tool_use") => {
                if let Some(tc) = anthropic_tool_use_to_openai(block) {
                    tool_calls.push(tc);
                }
            }
            _ => return None,
        }
    }
    if text_parts.is_empty() && tool_calls.is_empty() {
        return None;
    }
    Some((text_parts.join(""), tool_calls))
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
// Responses API helpers and conversions
// ---------------------------------------------------------------------------

fn responses_content_part_to_openai(part: &Value) -> Option<Value> {
    match part.get("type").and_then(|v| v.as_str()) {
        Some("input_text") | Some("output_text") => part
            .get("text")
            .map(|t| Value::String(t.as_str().unwrap_or("").to_string())),
        Some("input_image") => {
            let url = part
                .get("image_url")
                .and_then(|u| {
                    if let Some(s) = u.as_str() {
                        Some(s.to_string())
                    } else {
                        u.get("url").and_then(|v| v.as_str()).map(str::to_string)
                    }
                })
                .unwrap_or_default();
            let detail = part.get("image_url").and_then(|u| u.get("detail")).cloned();
            let mut image_url = serde_json::Map::new();
            image_url.insert("url".to_string(), Value::String(url));
            if let Some(d) = detail {
                image_url.insert("detail".to_string(), d);
            }
            Some(serde_json::json!({"type": "image_url", "image_url": image_url}))
        }
        // input_file and unknown parts have no direct OpenAI equivalent; pass
        // them through so the caller can decide what to do with them.
        _ => Some(part.clone()),
    }
}

fn openai_content_part_to_responses(part: &Value) -> Value {
    match part.get("type").and_then(|v| v.as_str()) {
        Some("text") => serde_json::json!({
            "type": "input_text",
            "text": part.get("text").and_then(|v| v.as_str()).unwrap_or(""),
        }),
        Some("image_url") => {
            let url = part
                .get("image_url")
                .and_then(|u| {
                    if let Some(s) = u.as_str() {
                        Some(s.to_string())
                    } else {
                        u.get("url").and_then(|v| v.as_str()).map(str::to_string)
                    }
                })
                .unwrap_or_default();
            let detail = part.get("image_url").and_then(|u| u.get("detail")).cloned();
            let mut image_url = serde_json::Map::new();
            image_url.insert("url".to_string(), Value::String(url));
            if let Some(d) = detail {
                image_url.insert("detail".to_string(), d);
            }
            serde_json::json!({"type": "input_image", "image_url": image_url})
        }
        _ => part.clone(),
    }
}

fn responses_input_item_to_openai_message(item: &Value) -> Option<Value> {
    match item.get("type").and_then(|v| v.as_str()) {
        Some("message") => {
            let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let role = if role == "developer" { "system" } else { role };
            let content = item
                .get("content")
                .cloned()
                .unwrap_or_else(|| Value::String(String::new()));
            let converted = match content {
                Value::String(s) => Value::String(s),
                Value::Array(parts) => Value::Array(
                    parts
                        .iter()
                        .filter_map(responses_content_part_to_openai)
                        .collect(),
                ),
                _ => content.clone(),
            };
            Some(serde_json::json!({"role": role, "content": converted}))
        }
        Some("function_call_output") => {
            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let output = item
                .get("output")
                .cloned()
                .unwrap_or_else(|| Value::String(String::new()));
            let output_str = match output {
                Value::String(s) => s,
                _ => output.to_string(),
            };
            Some(serde_json::json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output_str,
            }))
        }
        _ => None,
    }
}

fn openai_message_to_responses_input_item(msg: &Value) -> Option<Value> {
    let role = msg.get("role").and_then(|v| v.as_str())?;
    let content = msg
        .get("content")
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));
    let converted = match content {
        Value::String(s) => Value::String(s),
        Value::Array(parts) => {
            Value::Array(parts.iter().map(openai_content_part_to_responses).collect())
        }
        _ => content.clone(),
    };
    match role {
        "system" | "developer" => Some(serde_json::json!({
            "type": "message",
            "role": "system",
            "content": converted,
        })),
        "user" => Some(serde_json::json!({
            "type": "message",
            "role": "user",
            "content": converted,
        })),
        "assistant" => Some(serde_json::json!({
            "type": "message",
            "role": "assistant",
            "content": converted,
        })),
        "tool" => {
            let call_id = msg.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
            Some(serde_json::json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": converted,
            }))
        }
        _ => None,
    }
}

fn responses_tool_to_openai(tool: &Value) -> Option<Value> {
    if tool.get("type").and_then(|v| v.as_str()) != Some("function") {
        return None;
    }
    let name = tool.get("name").and_then(|v| v.as_str())?;
    let mut function = serde_json::Map::new();
    function.insert("name".to_string(), Value::String(name.to_string()));
    if let Some(desc) = tool.get("description").cloned() {
        function.insert("description".to_string(), desc);
    }
    function.insert(
        "parameters".to_string(),
        tool.get("parameters")
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
    );
    Some(serde_json::json!({"type": "function", "function": function}))
}

fn openai_tool_to_responses(tool: &Value) -> Option<Value> {
    if tool.get("type").and_then(|v| v.as_str()) != Some("function") {
        return None;
    }
    let function = tool.get("function")?;
    let name = function.get("name").and_then(|v| v.as_str())?;
    let mut out = serde_json::Map::new();
    out.insert("type".to_string(), Value::String("function".to_string()));
    out.insert("name".to_string(), Value::String(name.to_string()));
    if let Some(desc) = function.get("description").cloned() {
        out.insert("description".to_string(), desc);
    }
    out.insert(
        "parameters".to_string(),
        function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
    );
    out.insert("strict".to_string(), Value::Bool(false));
    Some(Value::Object(out))
}

fn responses_tool_choice_to_openai(tc: &Value) -> Option<Value> {
    match tc {
        Value::String(s) if s == "auto" || s == "none" => Some(Value::String(s.clone())),
        Value::Object(o) if o.get("type").and_then(|v| v.as_str()) == Some("function") => {
            let name = o
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())?;
            Some(serde_json::json!({"type": "function", "function": {"name": name}}))
        }
        _ => None,
    }
}

fn openai_tool_choice_to_responses(tc: &Value) -> Option<Value> {
    match tc {
        Value::String(s) if s == "auto" || s == "none" => Some(Value::String(s.clone())),
        Value::Object(o) if o.get("type").and_then(|v| v.as_str()) == Some("function") => {
            let name = o
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())?;
            Some(serde_json::json!({"type": "function", "function": {"name": name}}))
        }
        _ => None,
    }
}

fn responses_to_openai_request(body: &Value, remote_model: &str) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut messages = Vec::new();

    if let Some(instructions) = obj.get("instructions").and_then(|v| v.as_str()) {
        if !instructions.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": instructions}));
        }
    }

    match obj
        .get("input")
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()))
    {
        Value::String(s) if !s.is_empty() => {
            messages.push(serde_json::json!({"role": "user", "content": s}));
        }
        Value::Array(items) => {
            for item in items {
                if let Some(msg) = responses_input_item_to_openai_message(&item) {
                    messages.push(msg);
                }
            }
        }
        _ => {}
    }

    let mut out = serde_json::Map::new();
    out.insert("model".to_string(), Value::String(remote_model.to_string()));
    out.insert("messages".to_string(), Value::Array(messages));

    if let Some(v) = get_u64(&obj, "max_output_tokens") {
        out.insert("max_tokens".to_string(), Value::Number(v.into()));
    }
    copy_f64(&mut out, &obj, "temperature");
    copy_f64(&mut out, &obj, "top_p");
    copy_stream(&mut out, &obj);

    if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
        let openai_tools: Vec<Value> = tools.iter().filter_map(responses_tool_to_openai).collect();
        if !openai_tools.is_empty() {
            out.insert("tools".to_string(), Value::Array(openai_tools));
        }
    }
    if let Some(tc) = obj.get("tool_choice") {
        if let Some(converted) = responses_tool_choice_to_openai(tc) {
            out.insert("tool_choice".to_string(), converted);
        }
    }
    Value::Object(out)
}

fn openai_to_responses_request(body: &Value, remote_model: &str) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut input_items = Vec::new();

    if let Some(messages) = obj.get("messages").and_then(|v| v.as_array()) {
        for m in messages {
            if let Some(item) = openai_message_to_responses_input_item(m) {
                input_items.push(item);
            }
        }
    }

    let mut out = serde_json::Map::new();
    out.insert("model".to_string(), Value::String(remote_model.to_string()));
    out.insert("input".to_string(), Value::Array(input_items));

    if let Some(v) = get_u64(&obj, "max_tokens") {
        out.insert("max_output_tokens".to_string(), Value::Number(v.into()));
    }
    copy_f64(&mut out, &obj, "temperature");
    copy_f64(&mut out, &obj, "top_p");
    copy_stream(&mut out, &obj);

    if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
        let responses_tools: Vec<Value> =
            tools.iter().filter_map(openai_tool_to_responses).collect();
        if !responses_tools.is_empty() {
            out.insert("tools".to_string(), Value::Array(responses_tools));
        }
    }
    if let Some(tc) = obj.get("tool_choice") {
        if let Some(converted) = openai_tool_choice_to_responses(tc) {
            out.insert("tool_choice".to_string(), converted);
        }
    }
    Value::Object(out)
}

fn responses_function_call_to_openai(fc: &Value) -> Option<Value> {
    let obj = fc.as_object()?;
    let id = obj.get("id").and_then(|v| v.as_str())?;
    let name = obj.get("name").and_then(|v| v.as_str())?;
    let arguments = obj.get("arguments").and_then(|v| v.as_str())?;
    Some(serde_json::json!({
        "id": id,
        "type": "function",
        "function": {"name": name, "arguments": arguments},
    }))
}

fn openai_tool_call_to_responses_function_call(tc: &Value) -> Option<Value> {
    let obj = tc.as_object()?;
    let id = obj.get("id").and_then(|v| v.as_str())?;
    let function = obj.get("function")?;
    let name = function.get("name").and_then(|v| v.as_str())?;
    let arguments = function.get("arguments").and_then(|v| v.as_str())?;
    Some(serde_json::json!({
        "type": "function_call",
        "id": id,
        "call_id": id,
        "name": name,
        "arguments": arguments,
        "status": "completed",
    }))
}

fn extract_usage_responses(usage: &Value) -> Option<(u64, u64, u64)> {
    let obj = usage.as_object()?;
    let prompt = obj.get("input_tokens").and_then(|v| v.as_u64())?;
    let completion = obj.get("output_tokens").and_then(|v| v.as_u64())?;
    let total = obj
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(prompt + completion);
    Some((prompt, completion, total))
}

fn build_usage_responses(prompt: u64, completion: u64, total: u64) -> Value {
    serde_json::json!({
        "input_tokens": prompt,
        "output_tokens": completion,
        "total_tokens": total,
        "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 0},
        "output_tokens_details": {"reasoning_tokens": 0},
    })
}

fn responses_to_openai_response(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    if let Some(output) = obj.get("output").and_then(|v| v.as_array()) {
        for item in output {
            match item.get("type").and_then(|v| v.as_str()) {
                Some("message") => {
                    if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                        for part in content {
                            if part.get("type").and_then(|v| v.as_str()) == Some("output_text") {
                                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                                    text_parts.push(t);
                                }
                            }
                        }
                    }
                }
                Some("function_call") => {
                    if let Some(tc) = responses_function_call_to_openai(item) {
                        tool_calls.push(tc);
                    }
                }
                _ => {}
            }
        }
    }

    let finish_reason = if obj.get("error").is_some() {
        "stop"
    } else if obj.get("incomplete_details").is_some() {
        "length"
    } else if !tool_calls.is_empty() && text_parts.is_empty() {
        "tool_calls"
    } else {
        "stop"
    };

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
            "finish_reason": finish_reason,
        })]),
    );

    if let Some(usage) = obj.get("usage") {
        if let Some((prompt, completion, total)) = extract_usage_responses(usage) {
            out.insert("usage".to_string(), build_usage_openai(prompt, completion, total));
        }
    }
    Value::Object(out)
}

fn openai_to_responses_response(body: &Value) -> Value {
    let obj = body.as_object().cloned().unwrap_or_default();
    let choice = obj
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first());
    let message = choice.and_then(|c| c.get("message"));

    let mut output = Vec::new();
    let mut text_parts = Vec::new();
    if let Some(content) = message.and_then(|m| m.get("content")).map(text_content_to_string) {
        if !content.is_empty() {
            text_parts.push(content);
        }
    }
    if !text_parts.is_empty() {
        output.push(serde_json::json!({
            "type": "message",
            "id": "msg_tokenguard",
            "role": "assistant",
            "status": "completed",
            "content": text_parts
                .iter()
                .map(|t| serde_json::json!({"type": "output_text", "text": t, "annotations": []}))
                .collect::<Vec<_>>(),
        }));
    }

    if let Some(tcs) = message
        .and_then(|m| m.get("tool_calls"))
        .and_then(|v| v.as_array())
    {
        for tc in tcs {
            if let Some(fc) = openai_tool_call_to_responses_function_call(tc) {
                output.push(fc);
            }
        }
    }

    let finish_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str());
    let status = match finish_reason {
        Some("length") => "incomplete",
        _ => "completed",
    };

    let mut out = serde_json::Map::new();
    out.insert("id".to_string(), Value::String("resp_tokenguard".to_string()));
    out.insert("object".to_string(), Value::String("response".to_string()));
    out.insert("status".to_string(), Value::String(status.to_string()));
    out.insert("output".to_string(), Value::Array(output));

    if let Some(usage) = obj.get("usage") {
        if let Some((prompt, completion, total)) = extract_usage_openai(usage) {
            out.insert("usage".to_string(), build_usage_responses(prompt, completion, total));
        }
    }
    Value::Object(out)
}

fn responses_to_openai_sse_data(data: &Value) -> Option<Value> {
    let event_type = data.get("type").and_then(|v| v.as_str())?;
    match event_type {
        "response.output_text.delta" => {
            let delta = data.get("delta").and_then(|v| v.as_str())?;
            Some(serde_json::json!({
                "choices": [{"index": 0, "delta": {"content": delta}}],
            }))
        }
        "response.function_call_arguments.delta" => {
            let delta = data.get("delta").and_then(|v| v.as_str())?;
            Some(serde_json::json!({
                "choices": [{
                    "index": 0,
                    "delta": {"tool_calls": [{"index": 0, "function": {"arguments": delta}}]},
                }],
            }))
        }
        "response.completed" => {
            let mut out = serde_json::json!({"choices": [{"index": 0, "delta": {}}]});
            if let Some(response) = data.get("response") {
                if let Some(usage) = response.get("usage") {
                    if let Some((prompt, completion, total)) = extract_usage_responses(usage) {
                        out["usage"] = build_usage_openai(prompt, completion, total);
                    }
                }
            }
            Some(out)
        }
        "response.incomplete" => Some(serde_json::json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": "length"}],
        })),
        "response.failed" | "error" => Some(serde_json::json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        })),
        _ => None,
    }
}

fn openai_to_responses_sse_data(data: &Value) -> Option<Value> {
    let choice = data.get("choices")?.as_array()?.first()?;
    let delta = choice.get("delta")?;

    if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
        return Some(serde_json::json!({
            "type": "response.output_text.delta",
            "output_index": 0,
            "content_index": 0,
            "delta": text,
        }));
    }

    if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
        let status = if reason == "length" { "incomplete" } else { "completed" };
        let mut out = serde_json::json!({
            "type": "response.completed",
            "response": {"id": "resp_tokenguard", "object": "response", "status": status, "output": []},
        });
        if let Some(usage) = data.get("usage") {
            if let Some((prompt, completion, total)) = extract_usage_openai(usage) {
                out["response"]["usage"] = build_usage_responses(prompt, completion, total);
            }
        }
        return Some(out);
    }

    None
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

    let openai_messages = messages.as_array().cloned().unwrap_or_default();
    let mut anthropic_messages = Vec::new();
    for m in &openai_messages {
        if let Some(role) = m.get("role").and_then(|v| v.as_str()) {
            if role == "tool" {
                if let Some(id) = m.get("tool_call_id").and_then(|v| v.as_str()) {
                    let content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    anthropic_messages.push(serde_json::json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": content,
                        }],
                    }));
                }
                continue;
            }
            let anthropic_role = match role {
                "assistant" => "assistant",
                _ => "user",
            };
            let mut content = m
                .get("content")
                .map(|c| {
                    convert_message_content(c, ProviderFormat::OpenAI, ProviderFormat::Anthropic)
                })
                .unwrap_or_else(|| Value::String(String::new()));
            if role == "assistant" {
                if let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) {
                    let mut blocks = match content {
                        Value::String(s) if !s.is_empty() => {
                            vec![serde_json::json!({"type": "text", "text": s})]
                        }
                        Value::Array(arr) => arr,
                        _ => Vec::new(),
                    };
                    for tc in tcs {
                        if let Some(tool) = openai_tool_call_to_anthropic(tc) {
                            blocks.push(tool);
                        }
                    }
                    content = Value::Array(blocks);
                }
            }
            anthropic_messages
                .push(serde_json::json!({"role": anthropic_role, "content": content}));
        }
    }
    out.insert("messages".to_string(), Value::Array(anthropic_messages));

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
    let messages = obj
        .get("messages")
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));

    let mut openai_messages = Vec::new();
    if let Some(system) = obj.get("system") {
        let system_content =
            convert_message_content(system, ProviderFormat::Anthropic, ProviderFormat::OpenAI);
        openai_messages.push(serde_json::json!({"role": "system", "content": system_content}));
    }
    for m in messages.as_array().unwrap_or(&Vec::new()).iter() {
        if let Some(role) = m.get("role").and_then(|v| v.as_str()) {
            if role == "user" {
                if let Some((text, tool_results)) = split_anthropic_user_content(m.get("content")) {
                    if !text.is_empty() {
                        openai_messages.push(serde_json::json!({"role": "user", "content": text}));
                    }
                    for (id, content) in tool_results {
                        openai_messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": id,
                            "content": content,
                        }));
                    }
                    continue;
                }
            }
            if role == "assistant" {
                if let Some((text, tool_calls)) =
                    split_anthropic_assistant_content(m.get("content"))
                {
                    let mut msg = serde_json::Map::new();
                    msg.insert("role".to_string(), Value::String("assistant".to_string()));
                    msg.insert("content".to_string(), Value::String(text));
                    if !tool_calls.is_empty() {
                        msg.insert("tool_calls".to_string(), Value::Array(tool_calls));
                    }
                    openai_messages.push(Value::Object(msg));
                    continue;
                }
            }
            let content = m
                .get("content")
                .map(|c| {
                    convert_message_content(c, ProviderFormat::Anthropic, ProviderFormat::OpenAI)
                })
                .unwrap_or_else(|| Value::String(String::new()));
            openai_messages.push(serde_json::json!({"role": role, "content": content}));
        }
    }

    let mut out = serde_json::Map::new();
    out.insert("model".to_string(), Value::String(remote_model.to_string()));
    out.insert("messages".to_string(), Value::Array(openai_messages));
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

    let openai_messages = messages.as_array().cloned().unwrap_or_default();
    let mut contents = Vec::new();
    for m in &openai_messages {
        if let Some(role) = m.get("role").and_then(|v| v.as_str()) {
            if role == "tool" {
                if let Some(id) = m.get("tool_call_id").and_then(|v| v.as_str()) {
                    let name = find_tool_name_by_id(&openai_messages, id)
                        .unwrap_or_else(|| id.to_string());
                    let response = m
                        .get("content")
                        .and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str::<Value>(s).ok())
                        .unwrap_or_else(|| {
                            m.get("content")
                                .cloned()
                                .unwrap_or_else(|| Value::String(String::new()))
                        });
                    contents.push(serde_json::json!({
                        "role": "user",
                        "parts": [{"functionResponse": {"name": name, "response": response}}],
                    }));
                }
                continue;
            }
            let google_role = match role {
                "assistant" => "model",
                _ => "user",
            };
            let mut parts = m
                .get("content")
                .map(|c| convert_message_content(c, ProviderFormat::OpenAI, ProviderFormat::Google))
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_else(|| vec![serde_json::json!({"text": ""})]);
            if role == "assistant" {
                if let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tcs {
                        if let Some(call) = openai_tool_call_to_google(tc) {
                            parts.push(call);
                        }
                    }
                }
            }
            contents.push(serde_json::json!({
                "role": google_role,
                "parts": parts,
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

    let mut call_id_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut result_id_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    if let Some(contents) = obj.get("contents").and_then(|v| v.as_array()) {
        for c in contents {
            let role = c
                .get("role")
                .and_then(|v| v.as_str())
                .map(|r| if r == "model" { "assistant" } else { "user" })
                .unwrap_or("user");
            let parts = c
                .get("parts")
                .cloned()
                .unwrap_or_else(|| Value::Array(vec![]));

            if role == "user" {
                let mut user_parts = Vec::new();
                let mut tool_results = Vec::new();
                for part in parts.as_array().unwrap_or(&Vec::new()).iter() {
                    if part.get("functionResponse").is_some() {
                        if let Some(fr) = part.get("functionResponse") {
                            let name = fr.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let id = next_tool_call_id(name, &mut result_id_counts);
                            let response = fr.get("response").cloned().unwrap_or(Value::Null);
                            tool_results.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": id,
                                "content": response.to_string(),
                            }));
                        }
                    } else {
                        user_parts.push(part.clone());
                    }
                }
                if !user_parts.is_empty() {
                    let content = convert_message_content(
                        &Value::Array(user_parts),
                        ProviderFormat::Google,
                        ProviderFormat::OpenAI,
                    );
                    messages.push(serde_json::json!({"role": "user", "content": content}));
                }
                messages.extend(tool_results);
            } else {
                let mut assistant_parts = Vec::new();
                let mut tool_calls = Vec::new();
                for part in parts.as_array().unwrap_or(&Vec::new()).iter() {
                    if let Some(fc) = part.get("functionCall") {
                        let name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let args = fc.get("args").cloned().unwrap_or(Value::Null);
                        let id = next_tool_call_id(name, &mut call_id_counts);
                        tool_calls.push(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": {"name": name, "arguments": args.to_string()},
                        }));
                    } else {
                        assistant_parts.push(part.clone());
                    }
                }
                let mut msg = serde_json::Map::new();
                msg.insert("role".to_string(), Value::String("assistant".to_string()));
                let content = convert_message_content(
                    &Value::Array(assistant_parts),
                    ProviderFormat::Google,
                    ProviderFormat::OpenAI,
                );
                msg.insert("content".to_string(), content);
                if !tool_calls.is_empty() {
                    msg.insert("tool_calls".to_string(), Value::Array(tool_calls));
                }
                messages.push(Value::Object(msg));
            }
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
    let mut call_id_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for p in &parts {
        if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
            text_parts.push(t);
        } else if let Some(fc) = p.get("functionCall") {
            let name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let id = next_tool_call_id(name, &mut call_id_counts);
            if let Some(tc) = google_function_call_to_openai(fc, &id) {
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

fn google_function_call_to_openai(fc: &Value, id: &str) -> Option<Value> {
    let obj = fc.as_object()?;
    let name = obj.get("name")?.as_str()?;
    let args = obj
        .get("args")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    Some(serde_json::json!({
        "id": id,
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
    fn openai_image_to_anthropic() {
        let body = serde_json::json!({
            "model": "claude-3-5-sonnet",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,ABC"}},
                ],
            }],
        });
        let out = convert_request(
            ProviderFormat::OpenAI,
            ProviderFormat::Anthropic,
            &body,
            "claude-3-5-sonnet",
        );
        let content = out["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["type"], "base64");
        assert_eq!(content[1]["source"]["data"], "ABC");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
    }

    #[test]
    fn anthropic_image_to_openai() {
        let body = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "XYZ"}},
                ],
            }],
        });
        let out = convert_request(
            ProviderFormat::Anthropic,
            ProviderFormat::OpenAI,
            &body,
            "gpt-4o",
        );
        let content = out["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "data:image/png;base64,XYZ");
    }

    #[test]
    fn openai_tool_result_to_anthropic() {
        let body = serde_json::json!({
            "model": "claude-3-5-sonnet",
            "messages": [
                {"role": "assistant", "content": "ok", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{}"}}]},
                {"role": "tool", "tool_call_id": "call_1", "content": "{\"temperature\": 72}"},
            ],
        });
        let out = convert_request(
            ProviderFormat::OpenAI,
            ProviderFormat::Anthropic,
            &body,
            "claude-3-5-sonnet",
        );
        let tool_result = &out["messages"][1]["content"].as_array().unwrap()[0];
        assert_eq!(tool_result["type"], "tool_result");
        assert_eq!(tool_result["tool_use_id"], "call_1");
        assert_eq!(tool_result["content"], "{\"temperature\": 72}");
    }

    #[test]
    fn anthropic_tool_result_to_openai() {
        let body = serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "assistant", "content": [{"type": "text", "text": "ok"}, {"type": "tool_use", "id": "tu_1", "name": "get_weather", "input": {}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "tu_1", "content": "sunny"}]},
            ],
        });
        let out = convert_request(
            ProviderFormat::Anthropic,
            ProviderFormat::OpenAI,
            &body,
            "gpt-4o",
        );
        let tool_msg = &out["messages"][1];
        assert_eq!(tool_msg["role"], "tool");
        assert_eq!(tool_msg["tool_call_id"], "tu_1");
        assert_eq!(tool_msg["content"], "sunny");
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

    #[test]
    fn target_path_google_passthrough_rewrites_model() {
        assert_eq!(
            target_path(
                ProviderFormat::Google,
                ProviderFormat::Google,
                "gemini-remote",
                "/v1beta/models/gemini-local:streamGenerateContent?alt=sse",
                true
            ),
            "/v1beta/models/gemini-remote:streamGenerateContent?alt=sse"
        );
        assert_eq!(
            target_path(
                ProviderFormat::Google,
                ProviderFormat::Google,
                "gemini-remote",
                "/v1beta/models/gemini-local:generateContent",
                false
            ),
            "/v1beta/models/gemini-remote:generateContent"
        );
        // Non-method paths are left untouched.
        assert_eq!(
            target_path(
                ProviderFormat::Google,
                ProviderFormat::Google,
                "gemini-remote",
                "/v1beta/models",
                false
            ),
            "/v1beta/models"
        );
    }

    #[test]
    fn stream_detection() {
        assert!(is_stream_request(
            ProviderFormat::OpenAI,
            "/v1/chat/completions",
            &serde_json::json!({"stream": true})
        ));
        assert!(!is_stream_request(
            ProviderFormat::OpenAI,
            "/v1/chat/completions",
            &serde_json::json!({})
        ));
        assert!(is_stream_request(
            ProviderFormat::Google,
            "/v1beta/models/g:streamGenerateContent",
            &serde_json::json!({})
        ));
        assert!(is_stream_request(
            ProviderFormat::Google,
            "/v1beta/models/g:generateContent?alt=sse",
            &serde_json::json!({})
        ));
        assert!(!is_stream_request(
            ProviderFormat::Google,
            "/v1beta/models/g:generateContent?alt=json",
            &serde_json::json!({})
        ));
        assert!(!is_stream_request(
            ProviderFormat::Google,
            "/v1beta/models/g:generateContent",
            &serde_json::json!({})
        ));
    }

    #[test]
    fn error_envelopes() {
        let anthropic_err =
            br#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#;
        let out = error_envelope(ProviderFormat::OpenAI, 429, anthropic_err);
        assert_eq!(out["error"]["message"], "slow down");
        assert_eq!(out["error"]["type"], "upstream_error");
        assert_eq!(out["error"]["code"], 429);

        let out = error_envelope(ProviderFormat::Anthropic, 500, anthropic_err);
        assert_eq!(out["type"], "error");
        assert_eq!(out["error"]["type"], "api_error");
        assert_eq!(out["error"]["message"], "slow down");

        let google_err =
            br#"{"error":{"code":429,"message":"quota","status":"RESOURCE_EXHAUSTED"}}"#;
        let out = error_envelope(ProviderFormat::Google, 429, google_err);
        assert_eq!(out["error"]["message"], "quota");
        assert_eq!(out["error"]["code"], 429);
        assert_eq!(out["error"]["status"], "UNKNOWN");
    }

    #[test]
    fn error_envelope_unparseable_truncates() {
        let body = vec![b'x'; 1000];
        let out = error_envelope(ProviderFormat::OpenAI, 502, &body);
        let msg = out["error"]["message"].as_str().unwrap();
        assert_eq!(msg.len(), 500);
    }

    #[test]
    fn google_parallel_tool_calls_get_unique_ids() {
        let body = serde_json::json!({
            "contents": [
                {"role": "model", "parts": [
                    {"functionCall": {"name": "get_weather", "args": {"city": "Paris"}}},
                    {"functionCall": {"name": "get_weather", "args": {"city": "London"}}},
                ]},
                {"role": "user", "parts": [
                    {"functionResponse": {"name": "get_weather", "response": {"temp": 20}}},
                    {"functionResponse": {"name": "get_weather", "response": {"temp": 15}}},
                ]},
            ],
        });
        let out = convert_request(
            ProviderFormat::Google,
            ProviderFormat::OpenAI,
            &body,
            "gpt-4o",
        );
        let msgs = out["messages"].as_array().unwrap();
        let tcs = msgs[0]["tool_calls"].as_array().unwrap();
        assert_eq!(tcs[0]["id"], "call_get_weather_0");
        assert_eq!(tcs[1]["id"], "call_get_weather_1");
        assert_eq!(msgs[1]["tool_call_id"], "call_get_weather_0");
        assert_eq!(msgs[2]["tool_call_id"], "call_get_weather_1");
    }

    #[test]
    fn google_response_parallel_tool_calls_get_unique_ids() {
        let body = serde_json::json!({
            "candidates": [{"content": {"parts": [
                {"functionCall": {"name": "f", "args": {"a": 1}}},
                {"functionCall": {"name": "f", "args": {"a": 2}}},
            ]}, "finishReason": "STOP"}],
        });
        let out = convert_response(ProviderFormat::Google, ProviderFormat::OpenAI, &body);
        let tcs = out["choices"][0]["message"]["tool_calls"]
            .as_array()
            .unwrap();
        assert_eq!(tcs[0]["id"], "call_f_0");
        assert_eq!(tcs[1]["id"], "call_f_1");
    }

    fn responses_body() -> Value {
        serde_json::json!({
            "model": "gpt-5.4",
            "input": [
                {"type": "message", "role": "system", "content": "You are a helpful assistant."},
                {"type": "message", "role": "user", "content": "Hello"},
            ],
            "max_output_tokens": 100,
            "temperature": 0.5,
        })
    }

    #[test]
    fn responses_to_openai_request_round_trip() {
        let out = convert_request(
            ProviderFormat::Responses,
            ProviderFormat::OpenAI,
            &responses_body(),
            "gpt-4o",
        );
        assert_eq!(out["model"], "gpt-4o");
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(out["max_tokens"], 100);
    }

    #[test]
    fn responses_string_input_becomes_user_message() {
        let body = serde_json::json!({
            "model": "gpt-5.4",
            "input": "Hello",
            "instructions": "Be brief.",
        });
        let out = convert_request(
            ProviderFormat::Responses,
            ProviderFormat::OpenAI,
            &body,
            "gpt-4o",
        );
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be brief.");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "Hello");
    }

    #[test]
    fn responses_function_call_output_to_openai_tool_message() {
        let body = serde_json::json!({
            "model": "gpt-5.4",
            "input": [
                {"type": "function_call_output", "call_id": "call_1", "output": "sunny"},
            ],
        });
        let out = convert_request(
            ProviderFormat::Responses,
            ProviderFormat::OpenAI,
            &body,
            "gpt-4o",
        );
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "tool");
        assert_eq!(msgs[0]["tool_call_id"], "call_1");
        assert_eq!(msgs[0]["content"], "sunny");
    }

    #[test]
    fn openai_to_responses_request_round_trip() {
        let out = convert_request(
            ProviderFormat::OpenAI,
            ProviderFormat::Responses,
            &openai_body(),
            "gpt-5.4",
        );
        assert_eq!(out["model"], "gpt-5.4");
        let input = out["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "system");
        assert_eq!(input[1]["role"], "user");
        assert_eq!(out["max_output_tokens"], 100);
    }

    #[test]
    fn responses_to_anthropic_request_round_trip() {
        let out = convert_request(
            ProviderFormat::Responses,
            ProviderFormat::Anthropic,
            &responses_body(),
            "claude-3-5-sonnet",
        );
        assert_eq!(out["model"], "claude-3-5-sonnet");
        assert_eq!(out["system"], "You are a helpful assistant.");
        assert_eq!(out["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn anthropic_to_responses_request_round_trip() {
        let out = convert_request(
            ProviderFormat::Anthropic,
            ProviderFormat::Responses,
            &anthropic_body(),
            "gpt-5.4",
        );
        assert_eq!(out["model"], "gpt-5.4");
        let input = out["input"].as_array().unwrap();
        // Anthropic body has a system message plus one user message.
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "system");
        assert_eq!(input[1]["role"], "user");
    }

    #[test]
    fn responses_to_google_request_round_trip() {
        let out = convert_request(
            ProviderFormat::Responses,
            ProviderFormat::Google,
            &responses_body(),
            "gemini-1.5-pro",
        );
        assert!(out.get("model").is_none());
        assert_eq!(
            out["systemInstruction"]["parts"][0]["text"],
            "You are a helpful assistant."
        );
        assert_eq!(out["contents"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn google_to_responses_request_round_trip() {
        let out = convert_request(
            ProviderFormat::Google,
            ProviderFormat::Responses,
            &google_body(),
            "gpt-5.4",
        );
        assert_eq!(out["model"], "gpt-5.4");
        let input = out["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "system");
        assert_eq!(input[1]["role"], "user");
    }

    #[test]
    fn responses_tools_map_to_openai_tools() {
        let body = serde_json::json!({
            "model": "gpt-5.4",
            "input": "What's the weather?",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object", "properties": {}},
                "strict": true,
            }],
            "tool_choice": "auto",
        });
        let out = convert_request(
            ProviderFormat::Responses,
            ProviderFormat::OpenAI,
            &body,
            "gpt-4o",
        );
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
        assert_eq!(out["tool_choice"], "auto");
    }

    #[test]
    fn responses_to_openai_response_text() {
        let body = serde_json::json!({
            "id": "resp_1",
            "object": "response",
            "status": "completed",
            "model": "gpt-5.4",
            "output": [{
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "status": "completed",
                "content": [{"type": "output_text", "text": "Hi there!"}],
            }],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15},
        });
        let out = convert_response(ProviderFormat::Responses, ProviderFormat::OpenAI, &body);
        assert_eq!(out["choices"][0]["message"]["content"], "Hi there!");
        assert_eq!(out["choices"][0]["finish_reason"], "stop");
        assert_eq!(out["usage"]["prompt_tokens"], 10);
        assert_eq!(out["usage"]["completion_tokens"], 5);
    }

    #[test]
    fn responses_to_openai_response_function_call() {
        let body = serde_json::json!({
            "id": "resp_1",
            "object": "response",
            "status": "completed",
            "model": "gpt-5.4",
            "output": [{
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "get_weather",
                "arguments": "{\"city\":\"Boston\"}",
                "status": "completed",
            }],
            "usage": {"input_tokens": 20, "output_tokens": 8, "total_tokens": 28},
        });
        let out = convert_response(ProviderFormat::Responses, ProviderFormat::OpenAI, &body);
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
        let tcs = out["choices"][0]["message"]["tool_calls"].as_array().unwrap();
        assert_eq!(tcs[0]["id"], "fc_1");
        assert_eq!(tcs[0]["function"]["name"], "get_weather");
    }

    #[test]
    fn openai_to_responses_response_round_trip() {
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "Hi"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15},
        });
        let out = convert_response(ProviderFormat::OpenAI, ProviderFormat::Responses, &body);
        assert_eq!(out["status"], "completed");
        assert_eq!(out["output"][0]["type"], "message");
        assert_eq!(out["output"][0]["content"][0]["text"], "Hi");
        assert_eq!(out["usage"]["input_tokens"], 10);
    }

    #[test]
    fn responses_to_openai_sse_text_delta() {
        let data = serde_json::json!({
            "type": "response.output_text.delta",
            "output_index": 0,
            "content_index": 0,
            "delta": "Hi",
        });
        let (event, out) = convert_sse_data(
            ProviderFormat::Responses,
            ProviderFormat::OpenAI,
            Some("response.output_text.delta"),
            &data,
        )
        .unwrap();
        assert!(event.is_none());
        assert_eq!(out["choices"][0]["delta"]["content"], "Hi");
    }

    #[test]
    fn responses_to_openai_sse_completed_with_usage() {
        let data = serde_json::json!({
            "type": "response.completed",
            "response": {
                "usage": {"input_tokens": 8, "output_tokens": 4, "total_tokens": 12},
            },
        });
        let (event, out) = convert_sse_data(
            ProviderFormat::Responses,
            ProviderFormat::OpenAI,
            Some("response.completed"),
            &data,
        )
        .unwrap();
        assert!(event.is_none());
        assert_eq!(out["usage"]["prompt_tokens"], 8);
        assert_eq!(out["usage"]["completion_tokens"], 4);
    }

    #[test]
    fn openai_to_responses_sse_text_delta() {
        let data = serde_json::json!({
            "choices": [{"index": 0, "delta": {"content": "Hi"}}],
        });
        let (event, out) = convert_sse_data(
            ProviderFormat::OpenAI,
            ProviderFormat::Responses,
            None,
            &data,
        )
        .unwrap();
        assert_eq!(event.as_deref(), Some("response.output_text.delta"));
        assert_eq!(out["type"], "response.output_text.delta");
        assert_eq!(out["delta"], "Hi");
    }

    #[test]
    fn openai_to_responses_sse_finish_event() {
        let data = serde_json::json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 8, "completion_tokens": 4, "total_tokens": 12},
        });
        let (event, out) = convert_sse_data(
            ProviderFormat::OpenAI,
            ProviderFormat::Responses,
            None,
            &data,
        )
        .unwrap();
        assert_eq!(event.as_deref(), Some("response.completed"));
        assert_eq!(out["type"], "response.completed");
        assert_eq!(out["response"]["usage"]["input_tokens"], 8);
    }

    #[test]
    fn target_path_responses() {
        assert_eq!(
            target_path(
                ProviderFormat::OpenAI,
                ProviderFormat::Responses,
                "gpt-5.4",
                "/v1/chat/completions",
                false,
            ),
            "/v1/responses"
        );
    }

    #[test]
    fn stream_detection_responses() {
        assert!(is_stream_request(
            ProviderFormat::Responses,
            "/v1/responses",
            &serde_json::json!({"stream": true})
        ));
        assert!(!is_stream_request(
            ProviderFormat::Responses,
            "/v1/responses",
            &serde_json::json!({})
        ));
    }

    #[test]
    fn error_envelope_responses() {
        let out = error_envelope(ProviderFormat::Responses, 429, br#"{"error":{"message":"rate limit"}}"#);
        assert_eq!(out["error"]["message"], "rate limit");
        assert_eq!(out["error"]["code"], 429);
    }
}
