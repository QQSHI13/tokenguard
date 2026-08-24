//! Webhook notifications for limit events.

use crate::config::{Limit, LimitGroup, LimitMetric, LimitScope};
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
    tokio::spawn(async move {
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

pub fn send_limit_group_event(
    client: &Client,
    url: &str,
    event: &'static str,
    group: &LimitGroup,
    used: f64,
    cap: f64,
) {
    if url.is_empty() {
        return;
    }
    let payload = WebhookPayload {
        event,
        limit_name: format!("{} (group)", group.name),
        metric: metric_name(group.metric),
        used,
        cap,
        scope: scope_name(group.scope),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    let client = client.clone();
    let url = url.to_string();
    tokio::spawn(async move {
        let res = client
            .post(&url)
            .timeout(Duration::from_secs(15))
            .json(&payload)
            .send()
            .await;
        if let Err(e) = res {
            tracing::warn!("group webhook delivery failed: {e}");
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
    let resp = client
        .post(url)
        .timeout(Duration::from_secs(15))
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("webhook test failed: {e}"))?;
    // A delivered request is not a successful test: report the endpoint's own
    // rejection instead of claiming success on a 4xx/5xx.
    let status = resp.status();
    if !status.is_success() {
        let body: String = resp
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect();
        return Err(format!("webhook endpoint returned {status}: {body}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LimitAction, LimitPeriod};
    use wiremock::matchers::{body_json_schema, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn limit() -> Limit {
        Limit {
            id: 1,
            name: "Daily spend".into(),
            metric: LimitMetric::Money,
            period: LimitPeriod::Daily,
            cap: 10.0,
            warning_threshold: 0.8,
            scope: LimitScope::Provider,
            scope_id: Some(2),
            action: LimitAction::Block,
            enabled: true,
            active_hours_start: None,
            active_hours_end: None,
            active_days: 0b1111111,
            model_pattern: None,
        }
    }

    fn group() -> LimitGroup {
        LimitGroup {
            id: 7,
            name: "Team budget".into(),
            metric: LimitMetric::Tokens,
            period: LimitPeriod::Monthly,
            cap: 1000.0,
            warning_threshold: 0.9,
            scope: LimitScope::Global,
            scope_id: None,
            action: LimitAction::Warn,
            enabled: true,
            active_hours_start: None,
            active_hours_end: None,
            active_days: 0b1111111,
            model_pattern: None,
            member_limit_ids: vec![1, 2],
        }
    }

    #[tokio::test]
    async fn limit_event_posts_the_documented_payload() {
        // The payload shape is a public contract with whatever the user pointed
        // the webhook at (Slack relay, script, SIEM). Renaming a field or
        // switching a metric's wire name silently breaks their integration.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/hook"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        send_limit_event(
            &Client::new(),
            &format!("{}/hook", server.uri()),
            "limit_exceeded",
            &limit(),
            9.5,
            10.0,
        );

        // Delivery is fire-and-forget on a spawned task; wait for the request.
        let received = wait_for_request(&server).await;
        let body: serde_json::Value = serde_json::from_slice(&received.body).unwrap();
        assert_eq!(body["event"], "limit_exceeded");
        assert_eq!(body["limit_name"], "Daily spend");
        assert_eq!(body["metric"], "money");
        assert_eq!(body["scope"], "provider");
        assert_eq!(body["used"], 9.5);
        assert_eq!(body["cap"], 10.0);
        // RFC3339, so downstream consumers can parse it.
        let ts = body["timestamp"].as_str().expect("timestamp string");
        chrono::DateTime::parse_from_rfc3339(ts).expect("timestamp must be RFC3339");
    }

    #[tokio::test]
    async fn group_event_marks_the_name_and_uses_group_fields() {
        // A group and a limit can share a name; without the suffix the receiver
        // cannot tell which one fired.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        send_limit_group_event(
            &Client::new(),
            &server.uri(),
            "limit_warning",
            &group(),
            900.0,
            1000.0,
        );

        let received = wait_for_request(&server).await;
        let body: serde_json::Value = serde_json::from_slice(&received.body).unwrap();
        assert_eq!(body["limit_name"], "Team budget (group)");
        assert_eq!(body["metric"], "tokens");
        assert_eq!(body["scope"], "global");
    }

    #[tokio::test]
    async fn empty_url_sends_nothing() {
        // The webhook URL is unset by default, so this is the common path: it
        // must not spawn a request to "" and log a warning on every limit event.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        send_limit_event(&Client::new(), "", "limit_exceeded", &limit(), 1.0, 2.0);
        send_limit_group_event(&Client::new(), "", "limit_warning", &group(), 1.0, 2.0);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn send_test_reports_endpoint_rejection_as_failure() {
        // A 4xx means the endpoint refused the payload. Reporting "success"
        // because the TCP request completed would tell the user their webhook
        // works when it does not.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden: bad token"))
            .mount(&server)
            .await;

        let err = send_test(&Client::new(), &server.uri())
            .await
            .expect_err("a 403 must not read as success");
        assert!(err.contains("403"), "status missing from {err:?}");
        assert!(
            err.contains("forbidden: bad token"),
            "endpoint's own message missing from {err:?}"
        );
    }

    #[tokio::test]
    async fn send_test_succeeds_on_2xx_and_posts_json() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(body_json_schema::<serde_json::Value>)
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        send_test(&Client::new(), &server.uri())
            .await
            .expect("2xx must read as success");

        let received = &server.received_requests().await.unwrap()[0];
        let body: serde_json::Value = serde_json::from_slice(&received.body).unwrap();
        assert_eq!(body["event"], "test");
        assert!(body["message"].as_str().unwrap().contains("Token Guard"));
    }

    #[tokio::test]
    async fn send_test_rejects_an_empty_url() {
        // Otherwise reqwest fails on a relative URL and the user sees a parse
        // error rather than "you have not configured a webhook yet".
        let err = send_test(&Client::new(), "").await.expect_err("empty URL");
        assert!(err.contains("empty"), "unhelpful message: {err:?}");
    }

    /// Poll until the fire-and-forget task has delivered, then return the request.
    async fn wait_for_request(server: &MockServer) -> wiremock::Request {
        for _ in 0..100 {
            let reqs = server.received_requests().await.unwrap();
            if let Some(r) = reqs.into_iter().next() {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("webhook was never delivered");
    }
}
