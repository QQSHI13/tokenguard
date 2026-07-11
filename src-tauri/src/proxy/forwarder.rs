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
    let log_bodies = state
        .config
        .read()
        .map(|cfg| cfg.log_bodies)
        .unwrap_or(false);
    let request_body = if log_bodies {
        maybe_string_body(&body)
    } else {
        None
    };

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
                request_body,
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
    let is_stream = body_json
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
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
    request_body: Option<String>,
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
            let mut converter = SseConverter::new(prov.format, client_fmt);
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
                request_body.clone(),
                None,
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
        let upstream_json = serde_json::from_slice::<Value>(&bytes)
            .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
        let client_json = convert::convert_response(provider.format, client_format, &upstream_json);
        let client_bytes = serde_json::to_vec(&client_json)
            .map(Bytes::from)
            .unwrap_or(bytes);

        let response_body = maybe_string_body(&client_bytes);
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
                request_body,
                response_body,
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
/// constraint because conversion is handled downstream.
fn find_fallback_provider(
    state: &Arc<AppState>,
    _primary: &Provider,
    model: &str,
) -> Option<Provider> {
    let cfg = state.config.read().ok()?;
    cfg.providers
        .iter()
        .find(|p| p.models.iter().any(|m| m.local == model))
        .cloned()
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
}

/// Convert a raw body to a UTF-8 string for logging, capping size so the DB
/// does not grow unbounded on large binary responses.
fn maybe_string_body(body: &Bytes) -> Option<String> {
    const MAX_LOG_BODY_BYTES: usize = 256 * 1024;
    if body.len() > MAX_LOG_BODY_BYTES {
        let truncated = &body[..MAX_LOG_BODY_BYTES];
        String::from_utf8(truncated.to_vec()).ok()
    } else {
        String::from_utf8(body.to_vec()).ok()
    }
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

struct SseConverter {
    from: ProviderFormat,
    to: ProviderFormat,
    buf: Vec<u8>,
    usage: sse::Usage,
    pending_event: Option<String>,
}

impl SseConverter {
    fn new(from: ProviderFormat, to: ProviderFormat) -> Self {
        Self {
            from,
            to,
            buf: Vec::new(),
            usage: sse::Usage::default(),
            pending_event: None,
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

    fn handle_line(&mut self, line: &[u8], out: &mut Vec<u8>) {
        let s = std::str::from_utf8(line).unwrap_or("").trim_end();

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
        if payload.is_empty() || payload == "[DONE]" {
            self.pending_event = None;
            out.extend_from_slice(line);
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
            if self.pending_event.is_some() {
                // The event line was already emitted above for same-format passthrough.
                out.extend_from_slice(line);
            } else {
                out.extend_from_slice(line);
            }
            self.pending_event = None;
            return;
        }

        match convert::convert_sse_data(self.from, self.to, self.pending_event.as_deref(), &value) {
            Some((event_name, converted)) => {
                if let Some(name) = event_name {
                    out.extend_from_slice(format!("event: {}\n", name).as_bytes());
                }
                if let Ok(json) = serde_json::to_string(&converted) {
                    out.extend_from_slice(format!("data: {}\n", json).as_bytes());
                }
            }
            None => {
                // Unrecognized chunk shape: pass through the original line.
                out.extend_from_slice(line);
            }
        }
        self.pending_event = None;
    }
}
