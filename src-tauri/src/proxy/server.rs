//! Axum proxy server: routes /v1/* to providers by model name.

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::config::ProviderFormat;
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
    let path = req.uri().path().to_string();
    let req_headers = req.headers().clone();
    let project_tag = req_headers
        .get("x-tokenguard-project")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

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

    forwarder::forward(state, path, body, req_headers, provider, api_key, project_tag).await
}

async fn handle_models(State(state): State<Arc<AppState>>) -> Response {
    let cfg = state.config.read().unwrap();
    let data: Vec<serde_json::Value> = cfg
        .providers
        .iter()
        .flat_map(|p| {
            p.models
                .iter()
                .map(|m| serde_json::json!({"id": m, "object": "model", "owned_by": p.name}))
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
