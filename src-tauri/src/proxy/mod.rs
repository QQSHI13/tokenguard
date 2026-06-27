pub mod forwarder;
pub mod server;
pub mod sse;

/// Build a JSON error response (OpenAI-style shape for client compatibility).
pub fn error_resp(code: axum::http::StatusCode, msg: &str) -> Response {
    let body = serde_json::json!({"error": {"message": msg, "type": "tokenguard"}});
    axum::response::Response::builder()
        .status(code)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap()
}

use axum::response::Response;
