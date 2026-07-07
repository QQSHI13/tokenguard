//! Webhook notifications for limit events.

use crate::config::{Limit, LimitMetric, LimitScope};
use reqwest::Client;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
struct WebhookPayload {
    event: &'static str,
    limit_name: String,
    metric: String,
    used: f64,
    cap: f64,
    scope: String,
    timestamp: String,
}

fn metric_name(metric: LimitMetric) -> String {
    metric.as_db_str().to_string()
}

fn scope_name(scope: LimitScope) -> String {
    scope.as_db_str().to_string()
}

pub fn send_limit_event(
    client: &Client,
    url: &str,
    event: &'static str,
    limit: &Limit,
    used: f64,
    cap: f64,
) {
    if url.is_empty() {
        return;
    }
    let payload = WebhookPayload {
        event,
        limit_name: limit.name.clone(),
        metric: metric_name(limit.metric),
        used,
        cap,
        scope: scope_name(limit.scope),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    let client = client.clone();
    let url = url.to_string();
    tauri::async_runtime::spawn(async move {
        let res = client
            .post(&url)
            .timeout(Duration::from_secs(15))
            .json(&payload)
            .send()
            .await;
        if let Err(e) = res {
            tracing::warn!("webhook delivery failed: {e}");
        }
    });
}

#[derive(Debug, Clone, Serialize)]
struct TestWebhookPayload {
    event: &'static str,
    message: &'static str,
    timestamp: String,
}

pub async fn send_test(client: &Client, url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("webhook URL is empty".into());
    }
    let payload = TestWebhookPayload {
        event: "test",
        message: "Token Guard webhook test",
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    client
        .post(url)
        .timeout(Duration::from_secs(15))
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("webhook test failed: {e}"))?;
    Ok(())
}
