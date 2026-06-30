//! Axum proxy server: routes /v1/* to providers by model name.

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tracing::Instrument;

use crate::config::{LimitAction, ProviderFormat};
use crate::notifications;
use crate::proxy::forwarder;
use crate::state::{remote_model_name, AppState};

/// Bind the loopback proxy and serve until the app exits.
pub async fn serve(
    state: Arc<AppState>,
    port: u16,
    shutdown: tokio::sync::watch::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!("Token Guard proxy listening on http://127.0.0.1:{port}");
    let app = router(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown))
        .await?;
    tracing::info!("Token Guard proxy shut down gracefully");
    Ok(())
}

async fn shutdown_signal(mut rx: tokio::sync::watch::Receiver<()>) {
    let _ = rx.changed().await;
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
    let req_id = state.next_request_id();
    let span = tracing::info_span!(
        "proxy_request",
        req_id,
        provider = tracing::field::Empty,
        model = tracing::field::Empty,
        project = tracing::field::Empty,
    );
    async move {
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
        let body_bytes = match axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024).await {
            Ok(b) => b,
            Err(e) => return super::error_resp(StatusCode::BAD_REQUEST, &e.to_string()),
        };

        // Parse once; provider requests on these endpoints are always JSON.
        let body_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                return super::error_resp(
                    StatusCode::BAD_REQUEST,
                    &format!("request body is not valid JSON: {e}"),
                );
            }
        };

        // Read model from the request body for routing (read-only).
        let model = body_json
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();

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
        tracing::Span::current().record("provider", &provider.name);
        tracing::Span::current().record("model", &model);
        tracing::Span::current().record("project", project_tag.as_deref().unwrap_or(""));

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
        let remote_model = remote_model_name(&provider, &model);
        let body_bytes = {
            let mut json = body_json.clone();
            if let Some(obj) = json.as_object_mut() {
                obj.insert(
                    "model".to_string(),
                    serde_json::Value::String(remote_model.clone()),
                );
            }
            serde_json::to_vec(&json).unwrap_or_else(|_| body_bytes.to_vec())
        };

        // Estimate cost/tokens for limit checking before spending anything.
        // Money/token limits are enforced reactively for the current request because
        // we only know the true cost after the response. Request limits are enforced
        // atomically via in-memory counters.
        let (estimated_cost, estimated_tokens) = crate::cost::estimate_request(
            &body_json,
            &model,
            &remote_model,
            provider.input_cost_per_1k,
            provider.output_cost_per_1k,
        );
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
                    if state.should_notify_block(v.limit.id) {
                        notifications::limit_blocked(
                            &state.app,
                            &v.limit.name,
                            v.used,
                            v.limit.cap,
                        );
                    }
                    state.release_request_limit(&v.limit);
                    return super::error_resp(
                        StatusCode::TOO_MANY_REQUESTS,
                        &format!(
                            "limit exceeded: {} ({:.0}/{:.0})",
                            v.limit.name, v.used, v.limit.cap
                        ),
                    );
                }
                LimitAction::Pause => {
                    if state.should_notify_block(v.limit.id) {
                        notifications::limit_paused(&state.app, &v.limit.name, v.used, v.limit.cap);
                    }
                    state.release_request_limit(&v.limit);
                    state.toggle_pause();
                    return super::error_resp(
                        StatusCode::SERVICE_UNAVAILABLE,
                        &format!("limit exceeded: {} — proxy paused", v.limit.name),
                    );
                }
                LimitAction::Warn => {
                    notifications::limit_warning(&state.app, &v.limit.name, v.used, v.limit.cap);
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
            body_bytes.into(),
            req_headers,
            provider,
            api_key,
            project_tag,
        )
        .await
    }
    .instrument(span)
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
