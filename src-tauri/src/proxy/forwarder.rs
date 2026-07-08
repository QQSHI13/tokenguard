//! Forward requests to the real provider with per-format auth and
//! transparent SSE streaming. Logs usage + cost after the response completes.
//! Supports one fallback-provider retry for 5xx / 429 / network failures.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures::StreamExt;
use std::sync::Arc;

use crate::config::{AuthScheme, Provider, ProviderFormat};
use crate::cost;
use crate::proxy::sse;
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
    path: String,
    body: Bytes,
    req_headers: HeaderMap,
    provider: Provider,
    api_key: String,
    project_tag: Option<String>,
) -> Response {
    let model = extract_model(&body, provider.format);
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
        match attempt_forward(&state, &path, &body, &req_headers, &provider, &api_key, &model).await
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
                    &path,
                    &body,
                    &req_headers,
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
async fn attempt_forward(
    state: &Arc<AppState>,
    path: &str,
    body: &Bytes,
    req_headers: &HeaderMap,
    provider: &Provider,
    api_key: &str,
    model: &str,
) -> Result<reqwest::Response, ()> {
    let base = provider.base_url.trim_end_matches('/');
    let url = format!("{base}{path}");
    let final_body = rewrite_body_for_provider(body, provider, model);

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
        let model_owned = model.to_string();
        let remote_model_owned = remote_model.clone();
        let (tx, rx) = tokio::sync::mpsc::channel::<
            Result<Bytes, Box<dyn std::error::Error + Send + Sync>>,
        >(32);
        tauri::async_runtime::spawn(async move {
            let mut s = resp.bytes_stream();
            let mut parser = sse::SseUsageParser::new(prov.format);
            while let Some(chunk) = s.next().await {
                match chunk {
                    Ok(bytes) => {
                        parser.feed(&bytes);
                        if tx.send(Ok(bytes)).await.is_err() {
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
            let usage = parser.usage.clone();
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
        let response_body = maybe_string_body(&bytes);
        let usage = sse::extract_json(&bytes, provider.format);
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
        build_response(status, headers, Body::from(bytes))
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

/// Find a fallback provider that has the same format and supports the requested
/// model (by local name).
fn find_fallback_provider(
    state: &Arc<AppState>,
    primary: &Provider,
    model: &str,
) -> Option<Provider> {
    let fallback_id = primary.fallback_provider_id?;
    let cfg = state.config.read().ok()?;
    let fallback = cfg.providers.iter().find(|p| p.id == fallback_id)?;
    if fallback.format != primary.format {
        return None;
    }
    if !fallback.models.iter().any(|m| m.local == model) {
        return None;
    }
    Some(fallback.clone())
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

fn extract_model(body: &Bytes, _format: ProviderFormat) -> String {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return String::new();
    };
    v.get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string()
}

/// Rewrite the request body for the target provider: remap the model field to
/// the provider's remote model name, and inject OpenAI stream_options when
/// appropriate.
fn rewrite_body_for_provider(body: &Bytes, provider: &Provider, local_model: &str) -> Bytes {
    let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.clone();
    };

    let remote_model = remote_model_name(provider, local_model);
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "model".to_string(),
            serde_json::Value::String(remote_model),
        );
    }

    let is_stream = v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let is_chat_or_completions = v.get("messages").is_some() || v.get("prompt").is_some();
    if provider.format == ProviderFormat::OpenAI && is_stream && is_chat_or_completions {
        let mut opts = v
            .get("stream_options")
            .and_then(|o| o.as_object().cloned())
            .unwrap_or_default();
        opts.insert("include_usage".to_string(), serde_json::json!(true));
        v["stream_options"] = serde_json::Value::Object(opts);
    }

    serde_json::to_vec(&v).map(Bytes::from).unwrap_or_else(|_| body.clone())
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
            | "anthropic-version"
            | "content-encoding"
            | "accept-encoding"
    )
}
