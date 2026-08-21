//! Axum proxy server: routes /v1/* to providers by model name.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tracing::Instrument;

use crate::config::{LimitAction, ProviderFormat};
use crate::proxy::forwarder;
use crate::state::{pricing_profile, remote_model_name, AppState};

/// Bind the loopback proxy and serve until the app exits.
///
/// Binding modes, in order of preference:
/// - `share_over_tailscale`: bind to the host's Tailscale IPv4 (100.64.0.0/10)
///   *and* loopback, so only devices on the same tailnet can reach the gateway.
/// - `expose_to_lan`: bind `0.0.0.0` (any interface).
/// - otherwise: loopback only.
pub async fn serve(
    state: Arc<AppState>,
    port: u16,
    expose_to_lan: bool,
    share_over_tailscale: bool,
    shutdown: tokio::sync::watch::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let app = router(state);
    if share_over_tailscale {
        if let Some(ip) = tailscale_ipv4() {
            tracing::info!("Token Guard proxy sharing over Tailscale on http://{ip}:{port}");
            let tailnet_listener = tokio::net::TcpListener::bind((ip, port)).await?;
            let loopback_listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
            let app2 = app.clone();
            let shutdown2 = shutdown.clone();
            let tailnet_srv = axum::serve(tailnet_listener, app)
                .with_graceful_shutdown(shutdown_signal(shutdown));
            let loopback_srv = axum::serve(loopback_listener, app2)
                .with_graceful_shutdown(shutdown_signal(shutdown2));
            tokio::try_join!(tailnet_srv, loopback_srv)?;
            return Ok(());
        }
        // Tailscale in userspace networking mode (e.g. WSL): there is no
        // tailnet interface to bind; `tailscale serve` (see crate::share)
        // forwards the `/tg` path to loopback instead.
        tracing::warn!(
            "Tailscale sharing enabled but no tailnet interface found — assuming `tailscale serve` mode (loopback only)"
        );
    }
    let bind_addr = if expose_to_lan {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };
    let listener = tokio::net::TcpListener::bind((bind_addr, port)).await?;
    tracing::info!("Token Guard proxy listening on http://{bind_addr}:{port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown))
        .await?;
    tracing::info!("Token Guard proxy shut down gracefully");
    Ok(())
}

/// Find the host's Tailscale IPv4 address (100.64.0.0/10 CGNAT range).
/// Returns `None` when Tailscale is not installed or not connected.
pub fn tailscale_ipv4() -> Option<std::net::IpAddr> {
    let interfaces = local_ip_address::list_afinet_netifas().ok()?;
    interfaces
        .into_iter()
        .map(|(_, ip)| ip)
        .find(|ip| is_tailscale_ip(*ip))
}

/// True for addresses in the Tailscale CGNAT range 100.64.0.0/10.
pub fn is_tailscale_ip(ip: std::net::IpAddr) -> bool {
    matches!(ip, std::net::IpAddr::V4(v4) if {
        let o = v4.octets();
        o[0] == 100 && o[1] & 0b1100_0000 == 0b0100_0000
    })
}

async fn shutdown_signal(mut rx: tokio::sync::watch::Receiver<()>) {
    let _ = rx.changed().await;
}

fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(handle_openai))
        .route("/v1/completions", post(handle_openai))
        .route("/v1/responses", post(handle_responses))
        .route("/v1/messages", post(handle_anthropic))
        .route("/v1beta/{*path}", get(handle_google))
        .route("/v1beta/{*path}", post(handle_google))
        .route("/v1/models", get(handle_models))
        .with_state(state)
}

async fn handle_openai(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    handle(ProviderFormat::OpenAI, state, req, None).await
}

async fn handle_responses(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    handle(ProviderFormat::Responses, state, req, None).await
}

async fn handle_anthropic(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    handle(ProviderFormat::Anthropic, state, req, None).await
}

async fn handle_google(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    req: Request<Body>,
) -> Response {
    let model = extract_model_from_path(&path);
    handle(ProviderFormat::Google, state, req, model).await
}

async fn handle(
    family: ProviderFormat,
    state: Arc<AppState>,
    req: Request<Body>,
    model_override: Option<String>,
) -> Response {
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
        let req_headers = req.headers().clone();
        let query = req.uri().query();

        // Project tagging by the client's API key: the user sets a project's
        // label_key as OPENAI_API_KEY in their agent. We never forward this key —
        // the real provider key comes from the keychain in forward().
        let client_key = extract_client_key(&req_headers, query);
        let project_tag = client_key.as_ref().and_then(|k| state.project_for_key(k));

        // Forward the query string upstream (Gemini needs e.g. ?alt=sse), but
        // strip our `key` auth param so it never leaks to the provider.
        let path = match query.map(strip_key_param) {
            Some(q) if !q.is_empty() => format!("{}?{q}", req.uri().path()),
            _ => req.uri().path().to_string(),
        };

        // Every request must be tagged with a known project. The client's API
        // key is only a label; the real provider key comes from the keychain.
        if project_tag.is_none() {
            return super::error_resp(
                StatusCode::UNAUTHORIZED,
                "invalid or missing project key — create a project in Token Guard and set its label key as your API key",
            );
        }

        // Per-project budget enforcement.
        if let Some((used, budget, action, should_notify)) =
            project_tag.as_ref().and_then(|t| state.check_project_budget(t))
        {
            match action {
                LimitAction::Block => {
                    state.notify_limit_blocked(
                        project_tag.as_deref().unwrap_or(""),
                        used,
                        budget,
                    );
                    return super::error_resp(
                        StatusCode::TOO_MANY_REQUESTS,
                        &format!(
                            "project budget exceeded: {used:.2} / {budget:.2}",
                        ),
                    );
                }
                LimitAction::Pause => {
                    state.notify_limit_paused(
                        project_tag.as_deref().unwrap_or(""),
                        used,
                        budget,
                    );
                    state.set_paused(true);
                    return super::error_resp(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "project budget exceeded — proxy paused",
                    );
                }
                LimitAction::Warn => {
                    if should_notify {
                        state.notify_limit_warning(
                            project_tag.as_deref().unwrap_or(""),
                            used,
                            budget,
                        );
                        state.mark_budget_notified(project_tag.as_deref().unwrap_or(""));
                    }
                    tracing::warn!(
                        "project budget warning: {} ({:.2}/{:.2})",
                        project_tag.as_deref().unwrap_or(""),
                        used,
                        budget
                    );
                }
            }
        }

        // 32 MiB ceiling — large prompts happen.
        let body_bytes = match axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024).await {
            Ok(b) => b,
            Err(e) => return super::error_resp(StatusCode::BAD_REQUEST, &e.to_string()),
        };

        // OpenAI/Anthropic requests carry the model in the body. Gemini carries it
        // in the URL path (e.g. /v1beta/models/gemini-1.5-pro:generateContent).
        let (model, body_json) = if let Some(m) = model_override {
            // For path-routed providers, a missing/empty body is valid (e.g. GET
            // /v1beta/models). Default to an empty object for limit estimation.
            let body_json = serde_json::from_slice(&body_bytes)
                .unwrap_or_else(|_| serde_json::Value::Object(Default::default()));
            (m, body_json)
        } else {
            // Body-based providers require valid JSON with a model field.
            let body_json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
                Ok(v) => v,
                Err(e) => {
                    return super::error_resp(
                        StatusCode::BAD_REQUEST,
                        &format!("request body is not valid JSON: {e}"),
                    );
                }
            };
            let model = body_json
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            (model, body_json)
        };

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

        // Estimate cost/tokens for limit checking before spending anything.
        // Money/token limits are enforced reactively for the current request because
        // we only know the true cost after the response. Request limits are enforced
        // atomically via in-memory counters.
        let remote_model = remote_model_name(&provider, &model);
        let profile = pricing_profile(&provider, &model);
        let (estimated_cost, estimated_tokens) = crate::cost::estimate_request(
            &body_json,
            &model,
            &remote_model,
            &profile,
        );
        // Pre-flight limit check uses estimated cost/tokens. Time limits are
        // checked after the request completes because the real duration is
        // only known then.
        let check = state.check_limits(
            provider.id,
            project_tag.as_deref(),
            Some(&model),
            estimated_cost,
            estimated_tokens,
            0,
        );
        for v in &check.violations {
            match v.limit.action {
                LimitAction::Block => {
                    if v.should_notify {
                        state.notify_limit_blocked(
                            &v.limit.name,
                            v.used,
                            v.limit.cap,
                        );
                        state.mark_block_notified(v.limit.id);
                    }
                    state.release_request_limits(&check.reservations);
                    return super::error_resp(
                        StatusCode::TOO_MANY_REQUESTS,
                        &format!(
                            "limit exceeded: {} ({:.0}/{:.0})",
                            v.limit.name, v.used, v.limit.cap
                        ),
                    );
                }
                LimitAction::Pause => {
                    if v.should_notify {
                        state.notify_limit_paused(&v.limit.name, v.used, v.limit.cap);
                        state.mark_block_notified(v.limit.id);
                    }
                    state.release_request_limits(&check.reservations);
                    state.set_paused(true);
                    return super::error_resp(
                        StatusCode::SERVICE_UNAVAILABLE,
                        &format!("limit exceeded: {} — proxy paused", v.limit.name),
                    );
                }
                LimitAction::Warn => {
                    if v.should_notify {
                        state.notify_limit_warning(&v.limit.name, v.used, v.limit.cap);
                        state.mark_warning_notified(v.limit.id);
                    }
                    tracing::warn!(
                        "limit warning: {} ({:.0}/{:.0})",
                        v.limit.name,
                        v.used,
                        v.limit.cap
                    );
                }
            }
        }
        for v in &check.group_violations {
            match v.group.action {
                LimitAction::Block => {
                    if v.should_notify {
                        state.notify_limit_blocked(&v.group.name, v.used, v.group.cap);
                        state.mark_block_notified(v.group.id);
                    }
                    state.release_request_limits(&check.reservations);
                    state.release_group_limits(&check.group_reservations);
                    return super::error_resp(
                        StatusCode::TOO_MANY_REQUESTS,
                        &format!(
                            "limit group exceeded: {} ({:.0}/{:.0})",
                            v.group.name, v.used, v.group.cap
                        ),
                    );
                }
                LimitAction::Pause => {
                    if v.should_notify {
                        state.notify_limit_paused(&v.group.name, v.used, v.group.cap);
                        state.mark_block_notified(v.group.id);
                    }
                    state.release_request_limits(&check.reservations);
                    state.release_group_limits(&check.group_reservations);
                    state.set_paused(true);
                    return super::error_resp(
                        StatusCode::SERVICE_UNAVAILABLE,
                        &format!("limit group exceeded: {} — proxy paused", v.group.name),
                    );
                }
                LimitAction::Warn => {
                    if v.should_notify {
                        state.notify_limit_warning(&v.group.name, v.used, v.group.cap);
                        state.mark_warning_notified(v.group.id);
                    }
                    tracing::warn!(
                        "limit group warning: {} ({:.0}/{:.0})",
                        v.group.name,
                        v.used,
                        v.group.cap
                    );
                }
            }
        }

        let provider_id = provider.id;
        let response = forwarder::forward(
            state.clone(),
            start,
            path,
            body_bytes,
            req_headers,
            family,
            provider,
            api_key,
            project_tag.clone(),
            model.clone(),
        )
        .await;
        // The request completed and its usage is persisted; release the
        // in-flight reservations so it isn't counted twice.
        state.release_request_limits(&check.reservations);
        state.release_group_limits(&check.group_reservations);

        // Post-flight: time-based limits can only be evaluated now that the
        // real wall-clock duration is known. The current request is already
        // through, so Block/Pause here affects subsequent requests.
        let duration_ms = start.elapsed().as_millis() as u64;
        let time_check = state.check_time_limits(
            provider_id,
            project_tag.as_deref(),
            Some(&model),
            duration_ms,
        );
        for v in &time_check.violations {
            match v.limit.action {
                LimitAction::Block => {
                    if v.should_notify {
                        state.notify_limit_blocked(
                            &v.limit.name,
                            v.used,
                            v.limit.cap,
                        );
                        state.mark_block_notified(v.limit.id);
                    }
                }
                LimitAction::Pause => {
                    if v.should_notify {
                        state.notify_limit_paused(
                            &v.limit.name,
                            v.used,
                            v.limit.cap,
                        );
                        state.mark_block_notified(v.limit.id);
                    }
                    state.set_paused(true);
                }
                LimitAction::Warn => {
                    if v.should_notify {
                        state.notify_limit_warning(
                            &v.limit.name,
                            v.used,
                            v.limit.cap,
                        );
                        state.mark_warning_notified(v.limit.id);
                    }
                    tracing::warn!(
                        "time limit warning: {} ({:.0}/{:.0})",
                        v.limit.name,
                        v.used,
                        v.limit.cap
                    );
                }
            }
        }
        for v in &time_check.group_violations {
            match v.group.action {
                LimitAction::Block => {
                    if v.should_notify {
                        state.notify_limit_blocked(&v.group.name, v.used, v.group.cap);
                        state.mark_block_notified(v.group.id);
                    }
                }
                LimitAction::Pause => {
                    if v.should_notify {
                        state.notify_limit_paused(&v.group.name, v.used, v.group.cap);
                        state.mark_block_notified(v.group.id);
                    }
                    state.set_paused(true);
                }
                LimitAction::Warn => {
                    if v.should_notify {
                        state.notify_limit_warning(&v.group.name, v.used, v.group.cap);
                        state.mark_warning_notified(v.group.id);
                    }
                    tracing::warn!(
                        "time limit group warning: {} ({:.0}/{:.0})",
                        v.group.name,
                        v.used,
                        v.group.cap
                    );
                }
            }
        }

        response
    }
    .instrument(span)
    .await
}

/// Extract the model name from a Gemini-style path such as
/// `models/gemini-1.5-pro:generateContent` or `models/gemini-2.0-flash`.
fn extract_model_from_path(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    while let Some(part) = parts.next() {
        if part == "models" {
            let model = parts.next()?;
            let model = model.split(':').next().unwrap_or(model);
            if model.is_empty() {
                return None;
            }
            return Some(model.to_string());
        }
    }
    None
}

fn extract_client_key(headers: &axum::http::HeaderMap, query: Option<&str>) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").map(str::to_string))
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
        .or_else(|| {
            headers
                .get("api-key")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
        .or_else(|| {
            headers
                .get("x-goog-api-key")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
        .or_else(|| query.and_then(query_key_param))
}

/// Extract the `key` query param (how Google's own SDKs authenticate). Label
/// keys are plain tokens, so the raw value is compared without decoding.
fn query_key_param(query: &str) -> Option<String> {
    query.split('&').find_map(|p| {
        let (name, value) = p.split_once('=')?;
        (name == "key" && !value.is_empty()).then(|| value.to_string())
    })
}

/// Remove the `key` query param (our client auth) so it never leaks upstream;
/// every other param is kept verbatim.
fn strip_key_param(query: &str) -> String {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .filter(|p| p.split('=').next() != Some("key"))
        .collect::<Vec<_>>()
        .join("&")
}

async fn handle_models(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    // Same label-key auth as every other route: the model inventory must not
    // leak to unauthenticated clients (e.g. when exposed to the LAN).
    let client_key = extract_client_key(req.headers(), req.uri().query());
    if client_key
        .as_ref()
        .and_then(|k| state.project_for_key(k))
        .is_none()
    {
        return super::error_resp(
            StatusCode::UNAUTHORIZED,
            "invalid or missing project key — create a project in Token Guard and set its label key as your API key",
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_client_key_reads_bearer_header() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "authorization",
            axum::http::HeaderValue::from_static("Bearer tg_project_key_123"),
        );
        assert_eq!(
            extract_client_key(&headers, None),
            Some("tg_project_key_123".to_string())
        );
    }

    #[test]
    fn extract_client_key_reads_x_api_key_header() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "x-api-key",
            axum::http::HeaderValue::from_static("anthropic_project_key"),
        );
        assert_eq!(
            extract_client_key(&headers, None),
            Some("anthropic_project_key".to_string())
        );
    }

    #[test]
    fn extract_client_key_reads_api_key_header() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "api-key",
            axum::http::HeaderValue::from_static("azure_project_key"),
        );
        assert_eq!(
            extract_client_key(&headers, None),
            Some("azure_project_key".to_string())
        );
    }

    #[test]
    fn extract_client_key_reads_x_goog_api_key_header() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "x-goog-api-key",
            axum::http::HeaderValue::from_static("gemini_project_key"),
        );
        assert_eq!(
            extract_client_key(&headers, None),
            Some("gemini_project_key".to_string())
        );
    }

    #[test]
    fn extract_client_key_reads_key_query_param() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(
            extract_client_key(&headers, Some("alt=sse&key=gemini_project_key")),
            Some("gemini_project_key".to_string())
        );
    }

    #[test]
    fn extract_client_key_returns_none_when_missing() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(extract_client_key(&headers, None), None);
        assert_eq!(extract_client_key(&headers, Some("alt=sse")), None);
    }

    #[test]
    fn strip_key_param_removes_only_key() {
        assert_eq!(strip_key_param("key=secret&alt=sse"), "alt=sse");
        assert_eq!(strip_key_param("alt=sse&key=secret"), "alt=sse");
        assert_eq!(strip_key_param("key=secret"), "");
        assert_eq!(strip_key_param("alt=sse"), "alt=sse");
        // Params merely containing "key" survive.
        assert_eq!(strip_key_param("monkey=1&alt=sse"), "monkey=1&alt=sse");
    }

    #[test]
    fn extract_model_from_path_strips_method_suffix() {
        assert_eq!(
            extract_model_from_path("models/gemini-1.5-pro:generateContent"),
            Some("gemini-1.5-pro".to_string())
        );
    }

    #[test]
    fn extract_model_from_path_without_suffix() {
        assert_eq!(
            extract_model_from_path("models/gemini-2.0-flash"),
            Some("gemini-2.0-flash".to_string())
        );
    }

    #[test]
    fn extract_model_from_path_returns_none_for_list() {
        assert_eq!(extract_model_from_path("models"), None);
    }

    #[test]
    fn tailscale_range_detection() {
        use std::net::IpAddr;
        assert!(is_tailscale_ip(
            "100.100.100.100".parse::<IpAddr>().unwrap()
        ));
        assert!(is_tailscale_ip("100.64.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_tailscale_ip(
            "100.127.255.254".parse::<IpAddr>().unwrap()
        ));
        assert!(!is_tailscale_ip(
            "100.63.255.255".parse::<IpAddr>().unwrap()
        ));
        assert!(!is_tailscale_ip("100.128.0.1".parse::<IpAddr>().unwrap()));
        assert!(!is_tailscale_ip("192.168.1.5".parse::<IpAddr>().unwrap()));
        assert!(!is_tailscale_ip("127.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(!is_tailscale_ip("::1".parse::<IpAddr>().unwrap()));
    }
}
