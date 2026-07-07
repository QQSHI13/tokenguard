//! Provider health checks.

use crate::config::{AuthScheme, Provider};
use reqwest::Client;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
pub struct ProviderHealth {
    pub ok: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
    pub checked_at: String,
}

#[derive(Default)]
pub struct HealthCache {
    by_provider: HashMap<i64, ProviderHealth>,
}

impl HealthCache {
    pub fn all(&self) -> HashMap<i64, ProviderHealth> {
        self.by_provider.clone()
    }

    pub fn insert(&mut self, id: i64, health: ProviderHealth) {
        self.by_provider.insert(id, health);
    }
}

pub async fn check_provider(client: &Client, provider: &Provider) -> ProviderHealth {
    let start = Instant::now();
    let base = provider.base_url.trim_end_matches('/');

    let mut endpoints = vec![format!("{base}/v1/models")];
    if provider.format == crate::config::ProviderFormat::OpenAI {
        endpoints.push(format!("{base}/v1/health"));
    }

    let mut last_error: Option<String> = None;

    for url in endpoints {
        let api_key = crate::secrets::get(&provider.name).unwrap_or_default();
        let mut req = client.get(&url);
        req = match provider.auth {
            AuthScheme::Bearer => req.bearer_auth(&api_key),
            AuthScheme::XApiKey => req
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01"),
            AuthScheme::ApiKey => req.header("api-key", &api_key),
        };

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let latency_ms = start.elapsed().as_millis() as u64;
                if status.is_success() {
                    return ProviderHealth {
                        ok: true,
                        latency_ms,
                        error: None,
                        checked_at: chrono::Utc::now().to_rfc3339(),
                    };
                }
                let body: String = resp
                    .text()
                    .await
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect();
                last_error = Some(format!("{status}: {body}"));
            }
            Err(e) => {
                last_error = Some(e.to_string());
            }
        }
    }

    ProviderHealth {
        ok: false,
        latency_ms: start.elapsed().as_millis() as u64,
        error: last_error,
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// Run health checks for every configured provider and store the results.
pub async fn refresh_all(
    client: &Client,
    providers: &[Provider],
    cache: Arc<Mutex<HealthCache>>,
) {
    for provider in providers {
        let health = check_provider(client, provider).await;
        if let Ok(mut c) = cache.lock() {
            c.insert(provider.id, health);
        }
    }
}
