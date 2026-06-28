//! Axum proxy server: routes /v1/* to providers by model name.

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::config::{LimitAction, ProviderFormat};
use crate::proxy::forwarder;
use crate::state::AppState;

/// Bind the loopback proxy and serve until the app exits.
pub async fn serve(state: Arc<AppState>, port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("Token Guard proxy listening on http://127.0.0.1:{port}");
    let app = router(state);
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(handle_openai))
        .route("/v1/completions", post(handle_openai))
        .route("/v1/responses", post(handle_openai))
        .route("/v1/messages", post(handle_anthropic))
        .route("/v1/models", get(handle_models))
        .with_state(state)
}

async fn handle_openai(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    handle(ProviderFormat::OpenAI, state, req).await
}

async fn handle_anthropic(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    handle(ProviderFormat::Anthropic, state, req).await
}

async fn handle(family: ProviderFormat, state: Arc<AppState>, req: Request<Body>) -> Response {
    if state.paused.load(Ordering::Relaxed) {
        return super::error_resp(
            StatusCode::SERVICE_UNAVAILABLE,
            "Token Guard proxy is paused",
        );
    }
    let start = std::time::Instant::now();
    let path = req.uri().path().to_string();
    let req_headers = req.headers().clone();

    // Project tagging by the client's API key: the user sets a project's
    // label_key as OPENAI_API_KEY in their agent. We never forward this key —
    // the real provider key comes from the keychain in forward().
    let client_key = req_headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").map(str::to_string))
        .or_else(|| {
            req_headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
        .or_else(|| {
            req_headers
                .get("api-key")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        });
    let project_tag = client_key.as_ref().and_then(|k| state.project_for_key(k));

    // 32 MiB ceiling — large prompts happen.
    let body = match axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return super::error_resp(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    // Read model from the request body for routing (read-only).
    let model = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(String::from))
        .unwrap_or_default();

    let provider = match state.route_provider(family, &model) {
        Some(p) => p,
        None => {
            return super::error_resp(
                StatusCode::NOT_FOUND,
                &format!(
                "no provider configured for model '{model}' on this endpoint — add one in Settings"
            ),
            )
        }
    };

    let api_key = match crate::secrets::get(&provider.name) {
        Ok(k) => k,
        Err(_) => {
            return super::error_resp(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!(
                    "no API key stored for provider '{}' — add it in Settings",
                    provider.name
                ),
            )
        }
    };

    // Rewrite the model field to the provider's remote model name if an alias
    // is configured. This supports OpenAI/Anthropic model aliases and newer
    // provider-specific model slugs while keeping the local API clean.
    let body = if let Ok(mut json) = serde_json::from_slice::<serde_json::Value>(&body) {
        let remote = crate::state::remote_model_name(&provider, &model);
        if let Some(obj) = json.as_object_mut() {
            obj.insert("model".to_string(), serde_json::Value::String(remote));
        }
        serde_json::to_vec(&json).unwrap_or_else(|_| body.to_vec())
    } else {
        body.to_vec()
    };

    // Estimate cost/tokens for limit checking before spending anything.
    // Token limits are enforced reactively: a single request that pushes usage
    // over the cap will still go through, and the *next* request will be blocked.
    // This avoids parsing/tokenizing the request body twice.
    let estimated_cost = crate::cost::estimate(
        &model,
        0,
        0,
        provider.input_cost_per_1k,
        provider.output_cost_per_1k,
    );
    let estimated_tokens = 0;
    let duration_ms = start.elapsed().as_millis() as u64;
    let violations = state.check_limits(
        provider.id,
        project_tag.as_deref(),
        estimated_cost,
        estimated_tokens,
        duration_ms,
    );
    for v in &violations {
        match v.limit.action {
            LimitAction::Block => {
                return super::error_resp(
                    StatusCode::TOO_MANY_REQUESTS,
                    &format!(
                        "limit exceeded: {} ({:.0}/{:.0})",
                        v.limit.name, v.used, v.limit.cap
                    ),
                );
            }
            LimitAction::Pause => {
                state.toggle_pause();
                return super::error_resp(
                    StatusCode::SERVICE_UNAVAILABLE,
                    &format!("limit exceeded: {} — proxy paused", v.limit.name),
                );
            }
            LimitAction::Warn => {
                tracing::warn!(
                    "limit warning: {} ({:.0}/{:.0})",
                    v.limit.name,
                    v.used,
                    v.limit.cap
                );
            }
        }
    }

    forwarder::forward(
        state,
        start,
        path,
        body.into(),
        req_headers,
        provider,
        api_key,
        project_tag,
    )
    .await
}

async fn handle_models(State(state): State<Arc<AppState>>) -> Response {
    let Ok(cfg) = state.config.read() else {
        return super::error_resp(
            StatusCode::INTERNAL_SERVER_ERROR,
            "configuration lock poisoned",
        );
    };
    let data: Vec<serde_json::Value> = cfg
        .providers
        .iter()
        .flat_map(|p| {
            p.models
                .iter()
                .map(|m| serde_json::json!({"id": m.local, "object": "model", "owned_by": p.name}))
        })
        .collect();
    let body = serde_json::json!({"object": "list", "data": data});
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}
