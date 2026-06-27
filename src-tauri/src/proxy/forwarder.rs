//! Forward requests to the real provider with per-format auth and
//! transparent SSE streaming. Logs usage + cost after the response completes.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures::StreamExt;
use std::sync::Arc;

use crate::config::{AuthScheme, Provider, ProviderFormat};
use crate::cost;
use crate::proxy::sse;
use crate::state::AppState;

pub async fn forward(
    state: Arc<AppState>,
    path: String,
    body: Bytes,
    req_headers: HeaderMap,
    provider: Provider,
    api_key: String,
) -> Response {
    let base = provider.base_url.trim_end_matches('/');
    let url = format!("{base}{path}");
    let client = state.client.clone();

    let (final_body, model, _is_stream) = prepare_body(&body, provider.format);

    let mut req = client.post(&url);
    req = apply_auth(req, provider.auth, &api_key);
    for (k, v) in req_headers.iter() {
        if is_passthrough_header(k) {
            req = req.header(k, v.clone());
        }
    }

    let resp = match req.body(final_body).send().await {
        Ok(r) => r,
        Err(e) => {
            return super::error_resp(
                StatusCode::BAD_GATEWAY,
                &format!("upstream request failed: {e}"),
            )
        }
    };

    let status = resp.status();
    let headers = resp.headers().clone();
    let is_sse = headers
        .get(axum::http::header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or("").contains("text/event-stream"))
        .unwrap_or(false);

    if is_sse {
        // Stream bytes to the client unchanged; parse usage as they pass.
        let prov = provider.clone();
        let model_owned = model.clone();
        let st = state.clone();
        let stream = async_stream::stream! {
            let mut s = resp.bytes_stream();
            let mut parser = sse::SseUsageParser::new(prov.format);
            while let Some(chunk) = s.next().await {
                match chunk {
                    Ok(bytes) => {
                        parser.feed(&bytes);
                        yield Ok::<Bytes, Box<dyn std::error::Error + Send + Sync + 'static>>(bytes);
                    }
                    Err(e) => {
                        yield Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync + 'static>);
                        return;
                    }
                }
            }
            let usage = parser.usage.clone();
            let c = cost::estimate(
                &model_owned,
                usage.prompt,
                usage.completion,
                prov.input_cost_per_1k,
                prov.output_cost_per_1k,
            );
            st.log_request(prov.clone(), model_owned, usage.prompt, usage.completion, c)
                .await;
        };

        let mut builder = Response::builder().status(status);
        for (k, v) in headers.iter() {
            if is_passthrough_header(k) {
                builder = builder.header(k, v.clone());
            }
        }
        builder.body(Body::from_stream(stream)).unwrap()
    } else {
        let bytes = resp.bytes().await.unwrap_or_default();
        let usage = sse::extract_json(&bytes, provider.format);
        let c = cost::estimate(
            &model,
            usage.prompt,
            usage.completion,
            provider.input_cost_per_1k,
            provider.output_cost_per_1k,
        );
        state
            .log_request(provider.clone(), model.clone(), usage.prompt, usage.completion, c)
            .await;
        let mut builder = Response::builder().status(status);
        for (k, v) in headers.iter() {
            if is_passthrough_header(k) {
                builder = builder.header(k, v.clone());
            }
        }
        builder.body(Body::from(bytes)).unwrap()
    }
}

/// Parse the request body to read `model` and, for OpenAI streaming requests,
/// inject `stream_options: {"include_usage": true}` (the one documented,
/// opt-out exception to "no request modification"). Bytes are otherwise
/// forwarded unchanged.
fn prepare_body(body: &Bytes, format: ProviderFormat) -> (Bytes, String, bool) {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return (body.clone(), String::new(), false);
    };
    let model = v
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let is_stream = v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    if format == ProviderFormat::OpenAI && is_stream && v.get("stream_options").is_none() {
        let mut v = v;
        v["stream_options"] = serde_json::json!({"include_usage": true});
        if let Ok(new_body) = serde_json::to_vec(&v) {
            return (Bytes::from(new_body), model, true);
        }
    }
    (body.clone(), model, is_stream)
}

fn apply_auth(req: reqwest::RequestBuilder, auth: AuthScheme, key: &str) -> reqwest::RequestBuilder {
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
