//! Forward requests to the real provider with per-format auth and
//! bidirectional request/response conversion. Logs usage + cost after the
//! response completes. Supports one fallback-provider retry for 5xx / 429 /
//! network failures.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;
use std::sync::Arc;
use tokio_stream::StreamExt;

use crate::config::{AuthScheme, Provider, ProviderFormat};
use crate::cost;
use crate::proxy::{convert, sse};
use crate::state::{
    cached_input_cost_per_1k, input_output_cost_per_1k, remote_model_name, AppState,
};

/// Forward a request to the chosen provider, retrying transient failures with
/// exponential backoff, then optionally falling back to another configured
/// provider.
#[allow(clippy::too_many_arguments)]
pub async fn forward(
    state: Arc<AppState>,
    start: std::time::Instant,
    client_path: String,
    body: Bytes,
    req_headers: HeaderMap,
    client_format: ProviderFormat,
    provider: Provider,
    api_key: String,
    project_tag: Option<String>,
    model: String,
) -> Response {
    // Retry the primary provider up to 2 extra times with exponential backoff.
    const BACKOFFS: [u64; 3] = [0, 200, 500];
    let mut used_provider = provider.clone();
    let mut used_key = api_key.clone();
    let mut final_resp: Option<reqwest::Response> = None;

    for delay_ms in BACKOFFS {
        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        match attempt_forward(
            &state,
            &client_path,
            &body,
            &req_headers,
            client_format,
            &provider,
            &api_key,
            &model,
        )
        .await
        {
            Ok(resp) => {
                let retryable = is_retryable_status(resp.status());
                final_resp = Some(resp);
                if !retryable {
                    break;
                }
            }
            Err(_) => {
                // network failure: continue to next retry
            }
        }
    }

    // If the last primary attempt was still retryable, try the fallback once.
    let should_fallback = match &final_resp {
        Some(resp) => is_retryable_status(resp.status()),
        None => true,
    };

    if should_fallback {
        if let Some(fallback) = find_fallback_provider(&state, &provider, &model) {
            if let Ok(key) = crate::secrets::get(&fallback.name) {
                if let Ok(resp) = attempt_forward(
                    &state,
                    &client_path,
                    &body,
                    &req_headers,
                    client_format,
                    &fallback,
                    &key,
                    &model,
                )
                .await
                {
                    used_provider = fallback;
                    used_key = key;
                    final_resp = Some(resp);
                }
            }
        }
    }

    // Finalize the response (stream + log) for whichever provider we ended up using.
    match final_resp {
        Some(resp) => {
            finalize_forward(
                state,
                start,
                resp,
                client_format,
                used_provider,
                used_key,
                &model,
                project_tag,
            )
            .await
        }
        None => super::error_resp(
            StatusCode::BAD_GATEWAY,
            "upstream request failed and no fallback succeeded",
        ),
    }
}

/// Send one attempt to a provider and return the raw upstream response.
#[allow(clippy::too_many_arguments)]
async fn attempt_forward(
    state: &Arc<AppState>,
    client_path: &str,
    body: &Bytes,
    req_headers: &HeaderMap,
    client_format: ProviderFormat,
    provider: &Provider,
    api_key: &str,
    model: &str,
) -> Result<reqwest::Response, ()> {
    let remote_model = remote_model_name(provider, model);

    // Google path-routed providers may have an empty body (e.g. GET /v1beta/models).
    let body_json: Value = if body.is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_slice(body).unwrap_or_else(|_| Value::Object(serde_json::Map::new()))
    };

    let upstream_body =
        convert::convert_request(client_format, provider.format, &body_json, &remote_model);
    let is_stream = convert::is_stream_request(client_format, client_path, &body_json);
    let upstream_path = convert::target_path(
        client_format,
        provider.format,
        &remote_model,
        client_path,
        is_stream,
    );

    // Ensure OpenAI streaming responses include usage so we can log it.
    let mut final_body_json = upstream_body;
    if provider.format == ProviderFormat::OpenAI
        && is_stream
        && is_chat_or_completions(&final_body_json)
    {
        let mut opts = final_body_json
            .get("stream_options")
            .and_then(|o| o.as_object().cloned())
            .unwrap_or_default();
        opts.insert("include_usage".to_string(), serde_json::json!(true));
        final_body_json["stream_options"] = Value::Object(opts);
    }

    let final_body = serde_json::to_vec(&final_body_json)
        .map(Bytes::from)
        .unwrap_or_else(|_| body.clone());

    let base = provider.base_url.trim_end_matches('/');
    let url = format!("{base}{upstream_path}");

    let mut req = state.client.post(&url);
    req = apply_auth(req, provider.auth, api_key);
    for (k, v) in req_headers.iter() {
        if is_passthrough_header(k) {
            req = req.header(k, v.clone());
        }
    }
    for (k, v) in &provider.extra_headers {
        if let Ok(name) = HeaderName::from_bytes(k.as_bytes()) {
            req = req.header(name, v);
        }
    }

    req.body(final_body).send().await.map_err(|_| ())
}

/// Stream the final upstream response back to the client and log usage.
#[allow(clippy::too_many_arguments)]
async fn finalize_forward(
    state: Arc<AppState>,
    start: std::time::Instant,
    resp: reqwest::Response,
    client_format: ProviderFormat,
    provider: Provider,
    _api_key: String,
    model: &str,
    project_tag: Option<String>,
) -> Response {
    let status = resp.status();
    let headers = resp.headers().clone();
    let remote_model = remote_model_name(&provider, model);
    let is_sse = headers
        .get(axum::http::header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or("").contains("text/event-stream"))
        .unwrap_or(false);

    if is_sse {
        let st = state.clone();
        let prov = provider.clone();
        let client_fmt = client_format;
        let model_owned = model.to_string();
        let remote_model_owned = remote_model.clone();
        let (tx, rx) = tokio::sync::mpsc::channel::<
            Result<Bytes, Box<dyn std::error::Error + Send + Sync>>,
        >(32);
        tauri::async_runtime::spawn(async move {
            let mut s = resp.bytes_stream();
            let mut converter = SseConverter::new(prov.format, client_fmt, model_owned.clone());
            while let Some(chunk) = s.next().await {
                match chunk {
                    Ok(bytes) => {
                        let out = converter.feed(&bytes);
                        if tx.send(Ok(out)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>))
                            .await;
                        break;
                    }
                }
            }
            // Flush any trailing unterminated line and emit terminal lifecycle
            // events the upstream format never sends.
            let tail = converter.finish();
            if !tail.is_empty() {
                let _ = tx.send(Ok(tail)).await;
            }
            let usage = converter.usage;
            let (input_cost, output_cost) = input_output_cost_per_1k(&prov, &model_owned);
            let cached_cost = cached_input_cost_per_1k(&prov, &model_owned);
            let c = cost::estimate(
                &model_owned,
                &remote_model_owned,
                usage.prompt,
                usage.completion,
                usage.cached,
                input_cost,
                output_cost,
                cached_cost,
            );
            let duration_ms = start.elapsed().as_millis() as u64;
            st.log_request(
                prov.clone(),
                model_owned,
                usage.prompt,
                usage.completion,
                c,
                duration_ms,
                project_tag.clone(),
                Some(status.as_u16()),
            )
            .await;
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        build_response(status, headers, Body::from_stream(stream))
    } else {
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return super::error_resp(
                    StatusCode::BAD_GATEWAY,
                    &format!("upstream response body failed: {e}"),
                )
            }
        };
        // Upstream errors must not go through the success-path converter;
        // re-envelope the real status + message in the client's format.
        let client_json = if status.is_client_error() || status.is_server_error() {
            convert::error_envelope(client_format, status.as_u16(), &bytes)
        } else {
            let upstream_json = serde_json::from_slice::<Value>(&bytes)
                .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
            convert::convert_response(provider.format, client_format, &upstream_json)
        };
        let client_bytes = serde_json::to_vec(&client_json)
            .map(Bytes::from)
            .unwrap_or(bytes);

        let usage = sse::extract_json(&client_bytes, client_format);
        let (input_cost, output_cost) = input_output_cost_per_1k(&provider, model);
        let cached_cost = cached_input_cost_per_1k(&provider, model);
        let c = cost::estimate(
            model,
            &remote_model,
            usage.prompt,
            usage.completion,
            usage.cached,
            input_cost,
            output_cost,
            cached_cost,
        );
        let duration_ms = start.elapsed().as_millis() as u64;
        state
            .log_request(
                provider.clone(),
                model.to_string(),
                usage.prompt,
                usage.completion,
                c,
                duration_ms,
                project_tag.clone(),
                Some(status.as_u16()),
            )
            .await;
        build_response(status, headers, Body::from(client_bytes))
    }
}

fn build_response(status: reqwest::StatusCode, headers: HeaderMap, body: Body) -> Response {
    let mut builder = Response::builder().status(status);
    for (k, v) in headers.iter() {
        if is_passthrough_header(k) {
            builder = builder.header(k, v.clone());
        }
    }
    match builder.body(body) {
        Ok(r) => r,
        Err(e) => super::error_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("failed to build response: {e}"),
        ),
    }
}

/// Find a fallback provider that supports the requested model. Format is not a
/// constraint because conversion is handled downstream. Honors the primary's
/// configured `fallback_provider_id` first; otherwise picks the first *other*
/// provider serving the model.
fn find_fallback_provider(
    state: &Arc<AppState>,
    primary: &Provider,
    model: &str,
) -> Option<Provider> {
    let cfg = state.config.read().ok()?;
    if let Some(id) = primary.fallback_provider_id {
        if let Some(p) = cfg.providers.iter().find(|p| p.id == id) {
            return Some(p.clone());
        }
    }
    cfg.providers
        .iter()
        .find(|p| p.id != primary.id && p.models.iter().any(|m| m.local == model))
        .cloned()
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
}

fn is_chat_or_completions(body: &Value) -> bool {
    body.get("messages").is_some() || body.get("prompt").is_some()
}

fn apply_auth(
    req: reqwest::RequestBuilder,
    auth: AuthScheme,
    key: &str,
) -> reqwest::RequestBuilder {
    match auth {
        AuthScheme::Bearer => req.bearer_auth(key),
        AuthScheme::XApiKey => req
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01"),
        AuthScheme::ApiKey => req.header("api-key", key),
        AuthScheme::XGoogApiKey => req.header("x-goog-api-key", key),
    }
}

/// Forward client headers except hop-by-hop, auth (we set our own), and
/// content-length/encoding (let reqwest recompute; force identity so we can
/// parse the stream and the client receives plain bytes).
fn is_passthrough_header(name: &HeaderName) -> bool {
    let s = name.as_str().to_lowercase();
    !matches!(
        s.as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "authorization"
            | "x-api-key"
            | "api-key"
            | "x-goog-api-key"
            | "anthropic-version"
            | "content-encoding"
            | "accept-encoding"
    )
}

// ---------------------------------------------------------------------------
// SSE chunk converter
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StreamingToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

struct SseConverter {
    from: ProviderFormat,
    to: ProviderFormat,
    model: String,
    buf: Vec<u8>,
    usage: sse::Usage,
    pending_event: Option<String>,
    streaming_tools: std::collections::HashMap<usize, StreamingToolCall>,
    last_tool_index: Option<usize>,
    anthropic_started: bool,
    anthropic_text_open: bool,
    anthropic_stopped: bool,
    done_sent: bool,
}

impl SseConverter {
    fn new(from: ProviderFormat, to: ProviderFormat, model: String) -> Self {
        Self {
            from,
            to,
            model,
            buf: Vec::new(),
            usage: sse::Usage::default(),
            pending_event: None,
            streaming_tools: std::collections::HashMap::new(),
            last_tool_index: None,
            anthropic_started: false,
            anthropic_text_open: false,
            anthropic_stopped: false,
            done_sent: false,
        }
    }

    fn feed(&mut self, data: &[u8]) -> Bytes {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        while let Some(nl) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=nl).collect();
            self.handle_line(&line, &mut out);
        }
        Bytes::from(out)
    }

    /// Flush any remaining buffered bytes as a final line and emit terminal
    /// lifecycle events the upstream format never sends.
    fn finish(&mut self) -> Bytes {
        let mut out = Vec::new();
        if !self.buf.is_empty() {
            let mut line: Vec<u8> = std::mem::take(&mut self.buf);
            line.push(b'\n');
            self.handle_line(&line, &mut out);
        }
        if self.to == ProviderFormat::OpenAI && !self.done_sent {
            out.extend_from_slice(b"data: [DONE]\n");
            self.done_sent = true;
        }
        if self.to == ProviderFormat::Anthropic
            && self.from != ProviderFormat::Anthropic
            && !self.anthropic_stopped
        {
            self.ensure_anthropic_message(&mut out);
            self.close_anthropic_blocks(&mut out);
            self.emit_anthropic_event(
                &mut out,
                "message_stop",
                serde_json::json!({"type": "message_stop"}),
            );
            self.anthropic_stopped = true;
        }
        Bytes::from(out)
    }

    fn handle_line(&mut self, line: &[u8], out: &mut Vec<u8>) {
        let s = match std::str::from_utf8(line) {
            Ok(s) => s.trim_end(),
            Err(_) => {
                // Undecodable bytes: forward the line verbatim instead of
                // blanking it.
                out.extend_from_slice(line);
                return;
            }
        };

        if s.is_empty() {
            self.pending_event = None;
            out.extend_from_slice(b"\n");
            return;
        }

        if let Some(event) = s.strip_prefix("event:") {
            let name = event.trim().to_string();
            self.pending_event = Some(name);
            if self.from == self.to {
                out.extend_from_slice(line);
            }
            return;
        }

        let Some(rest) = s.strip_prefix("data:") else {
            out.extend_from_slice(line);
            return;
        };

        let payload = rest.trim();
        if payload.is_empty() {
            self.pending_event = None;
            out.extend_from_slice(line);
            return;
        }
        if payload == "[DONE]" {
            self.pending_event = None;
            self.done_sent = true;
            if self.from == ProviderFormat::OpenAI && self.to == ProviderFormat::Anthropic {
                self.ensure_anthropic_message(out);
                self.close_anthropic_blocks(out);
                self.emit_anthropic_event(
                    out,
                    "message_stop",
                    serde_json::json!({"type": "message_stop"}),
                );
                self.anthropic_stopped = true;
            } else {
                out.extend_from_slice(line);
            }
            return;
        }

        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            out.extend_from_slice(line);
            return;
        };

        // Extract usage from the original upstream chunk before converting it.
        let usage_obj = value
            .get("usage")
            .or_else(|| value.get("response").and_then(|r| r.get("usage")))
            .or_else(|| value.get("message").and_then(|m| m.get("usage")))
            .or_else(|| value.get("usageMetadata"));
        if let Some(u) = usage_obj {
            sse::extract_from_usage_object(u, &mut self.usage);
        }

        if self.from == self.to {
            out.extend_from_slice(line);
            self.pending_event = None;
            return;
        }

        // Tool-call streaming needs stateful accumulation, so handle it before
        // the generic chunk converter.
        if self.from == ProviderFormat::OpenAI
            && self.to == ProviderFormat::Anthropic
            && self.handle_openai_tool_chunk(&value, out)
        {
            self.pending_event = None;
            return;
        }
        if self.from == ProviderFormat::Anthropic
            && self.to == ProviderFormat::OpenAI
            && self.handle_anthropic_tool_chunk(&value, out)
        {
            self.pending_event = None;
            return;
        }

        match convert::convert_sse_data(self.from, self.to, self.pending_event.as_deref(), &value) {
            Some((event_name, converted)) => {
                // Synthesize the mandatory Anthropic lifecycle events around
                // converted chunks.
                if self.to == ProviderFormat::Anthropic {
                    match event_name.as_deref() {
                        Some("content_block_delta") => {
                            self.ensure_anthropic_message(out);
                            self.ensure_anthropic_text_block(out);
                        }
                        Some("message_delta") => {
                            self.close_anthropic_blocks(out);
                        }
                        _ => {}
                    }
                }
                if let Some(name) = event_name {
                    out.extend_from_slice(format!("event: {}\n", name).as_bytes());
                }
                if let Ok(json) = serde_json::to_string(&converted) {
                    out.extend_from_slice(format!("data: {}\n", json).as_bytes());
                }
            }
            None => {
                // No equivalent in the client format: drop the chunk instead
                // of leaking provider-format events.
            }
        }
        self.pending_event = None;
    }

    /// Emit message_start once, before the first content block.
    fn ensure_anthropic_message(&mut self, out: &mut Vec<u8>) {
        if self.anthropic_started {
            return;
        }
        self.anthropic_started = true;
        self.emit_anthropic_event(
            out,
            "message_start",
            serde_json::json!({
                "type": "message_start",
                "message": {
                    "id": "msg_tokenguard",
                    "type": "message",
                    "role": "assistant",
                    "model": self.model,
                    "content": [],
                    "stop_reason": null,
                    "usage": {"input_tokens": 0, "output_tokens": 0},
                },
            }),
        );
    }

    /// Open the index-0 text block once, before the first text delta.
    fn ensure_anthropic_text_block(&mut self, out: &mut Vec<u8>) {
        if self.anthropic_text_open {
            return;
        }
        self.anthropic_text_open = true;
        self.emit_anthropic_event(
            out,
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""},
            }),
        );
    }

    /// Close any open text/tool content blocks before message_delta/stop.
    fn close_anthropic_blocks(&mut self, out: &mut Vec<u8>) {
        if self.anthropic_text_open {
            self.anthropic_text_open = false;
            self.emit_anthropic_event(
                out,
                "content_block_stop",
                serde_json::json!({"type": "content_block_stop", "index": 0}),
            );
        }
        let mut indices: Vec<usize> = self.streaming_tools.keys().copied().collect();
        indices.sort_unstable();
        for idx in indices {
            self.emit_anthropic_event(
                out,
                "content_block_stop",
                serde_json::json!({"type": "content_block_stop", "index": idx}),
            );
        }
        self.streaming_tools.clear();
    }

    fn emit_anthropic_event(&self, out: &mut Vec<u8>, event: &str, data: Value) {
        out.extend_from_slice(format!("event: {}\n", event).as_bytes());
        if let Ok(json) = serde_json::to_string(&data) {
            out.extend_from_slice(format!("data: {}\n", json).as_bytes());
        }
    }

    fn handle_openai_tool_chunk(&mut self, value: &Value, out: &mut Vec<u8>) -> bool {
        let delta = value
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("delta"));
        let tool_calls = match delta
            .and_then(|d| d.get("tool_calls"))
            .and_then(|v| v.as_array())
        {
            Some(tcs) => tcs,
            None => return false,
        };

        self.ensure_anthropic_message(out);
        for tc in tool_calls {
            let idx = match tc.get("index").and_then(|v| v.as_u64()).map(|u| u as usize) {
                Some(i) => i,
                None => continue,
            };
            {
                let entry = self.streaming_tools.entry(idx).or_default();
                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                    entry.id = Some(id.to_string());
                }
                if let Some(name) = tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|v| v.as_str())
                {
                    entry.name = Some(name.to_string());
                }
            }

            if let Some(name) = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
            {
                let id = self
                    .streaming_tools
                    .get(&idx)
                    .and_then(|e| e.id.clone())
                    .unwrap_or_else(|| format!("call_{}", idx));
                self.emit_anthropic_event(
                    out,
                    "content_block_start",
                    serde_json::json!({
                        "type": "content_block_start",
                        "index": idx,
                        "content_block": {
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": {},
                        },
                    }),
                );
            }
            if let Some(args) = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str())
            {
                {
                    let entry = self.streaming_tools.entry(idx).or_default();
                    entry.arguments.push_str(args);
                }
                self.emit_anthropic_event(
                    out,
                    "content_block_delta",
                    serde_json::json!({
                        "type": "content_block_delta",
                        "index": idx,
                        "delta": {
                            "type": "input_json_delta",
                            "partial_json": args,
                        },
                    }),
                );
            }
        }
        true
    }

    fn handle_anthropic_tool_chunk(&mut self, value: &Value, out: &mut Vec<u8>) -> bool {
        let event = match self.pending_event.as_deref() {
            Some(e) => e,
            None => return false,
        };
        match event {
            "content_block_start" => {
                if value
                    .get("content_block")
                    .and_then(|b| b.get("type"))
                    .and_then(|t| t.as_str())
                    != Some("tool_use")
                {
                    return false;
                }
                let idx = value.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                self.last_tool_index = Some(idx);
                let block = &value["content_block"];
                let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let openai = serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "role": "assistant",
                            "tool_calls": [{
                                "index": idx,
                                "id": id,
                                "type": "function",
                                "function": {"name": name},
                            }],
                        },
                    }],
                });
                if let Ok(json) = serde_json::to_string(&openai) {
                    out.extend_from_slice(format!("data: {}\n", json).as_bytes());
                }
                true
            }
            "content_block_delta" => {
                let partial = value
                    .get("delta")
                    .and_then(|d| d.get("partial_json"))
                    .and_then(|v| v.as_str());
                let partial = match partial {
                    Some(p) => p,
                    None => return false,
                };
                // The event carries its own block index; attributing deltas to
                // the last-started block corrupts interleaved tool calls.
                let idx = value
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|u| u as usize)
                    .or(self.last_tool_index)
                    .unwrap_or(0);
                let openai = serde_json::json!({
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": idx,
                                "function": {"arguments": partial},
                            }],
                        },
                    }],
                });
                if let Ok(json) = serde_json::to_string(&openai) {
                    out.extend_from_slice(format!("data: {}\n", json).as_bytes());
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn converter(from: ProviderFormat, to: ProviderFormat) -> SseConverter {
        SseConverter::new(from, to, "test-model".to_string())
    }

    #[test]
    fn finish_flushes_trailing_unterminated_line() {
        let mut c = converter(ProviderFormat::OpenAI, ProviderFormat::OpenAI);
        let first = c.feed(b"data: {\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}");
        assert!(first.is_empty());
        let tail = c.finish();
        let s = String::from_utf8(tail.to_vec()).unwrap();
        assert!(s.contains("\"prompt_tokens\":3"));
        assert!(s.contains("data: [DONE]"));
        assert_eq!(c.usage.prompt, 3);
        assert_eq!(c.usage.completion, 2);
    }

    #[test]
    fn openai_to_anthropic_synthesizes_lifecycle_events() {
        let input = b"data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        let mut c = converter(ProviderFormat::OpenAI, ProviderFormat::Anthropic);
        let mut out = c.feed(input).to_vec();
        out.extend_from_slice(&c.finish());
        let s = String::from_utf8(out).unwrap();

        // The provider-format role chunk must not leak to the client.
        assert!(!s.contains("choices"));
        assert!(s.contains("Hello"));

        let ms = s.find("event: message_start").unwrap();
        let cbs = s.find("event: content_block_start").unwrap();
        let cbd = s.find("event: content_block_delta").unwrap();
        let cbstop = s.find("event: content_block_stop").unwrap();
        let md = s.find("event: message_delta").unwrap();
        let mstop = s.find("event: message_stop").unwrap();
        assert!(ms < cbs && cbs < cbd && cbd < cbstop && cbstop < md && md < mstop);
    }

    #[test]
    fn anthropic_to_openai_emits_done_and_drops_unknown_events() {
        let input = b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10}}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\nevent: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\nevent: ping\ndata: {\"type\":\"ping\"}\n\n";
        let mut c = converter(ProviderFormat::Anthropic, ProviderFormat::OpenAI);
        let mut out = c.feed(input).to_vec();
        out.extend_from_slice(&c.finish());
        let s = String::from_utf8(out).unwrap();

        assert!(s.contains("data: [DONE]"));
        assert!(s.contains("Hi"));
        assert!(!s.contains("ping"));
        assert!(!s.contains("content_block_start"));
        assert_eq!(c.usage.prompt, 10);
        assert_eq!(c.usage.completion, 5);
    }

    #[test]
    fn google_to_openai_emits_done_on_finish() {
        let input = b"data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hi\"}]}}],\"usageMetadata\":{\"promptTokenCount\":7,\"candidatesTokenCount\":1}}\n\n";
        let mut c = converter(ProviderFormat::Google, ProviderFormat::OpenAI);
        let mut out = c.feed(input).to_vec();
        out.extend_from_slice(&c.finish());
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Hi"));
        assert!(s.contains("data: [DONE]"));
        assert_eq!(c.usage.prompt, 7);
    }

    #[test]
    fn invalid_utf8_line_forwarded_verbatim() {
        let mut c = converter(ProviderFormat::OpenAI, ProviderFormat::Anthropic);
        let out = c.feed(b"data: \xff\xfe\n");
        assert_eq!(&out[..], &b"data: \xff\xfe\n"[..]);
    }

    #[test]
    fn anthropic_tool_delta_uses_its_own_index() {
        let input = b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_0\",\"name\":\"a\"}}\n\nevent: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"b\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n";
        let mut c = converter(ProviderFormat::Anthropic, ProviderFormat::OpenAI);
        let out = c.feed(input);
        let s = String::from_utf8(out.to_vec()).unwrap();
        let chunks: Vec<Value> = s
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(str::trim)
            .filter_map(|p| serde_json::from_str::<Value>(p).ok())
            .collect();
        let last = chunks.last().unwrap();
        assert_eq!(last["choices"][0]["delta"]["tool_calls"][0]["index"], 0);
    }
}
