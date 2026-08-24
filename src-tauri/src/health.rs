//! Provider health checks.

use crate::config::{AuthScheme, Provider, ProviderFormat};
use reqwest::Client;
use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

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

/// Endpoints to probe, in order, for a provider of this dialect.
///
/// Split out from [`check_provider`] so it can be tested without a live server:
/// the URL shapes are dialect-specific and easy to break silently — a wrong path
/// makes a healthy provider report as down, which the UI shows as a red dot with
/// a 404 body.
fn probe_endpoints(base_url: &str, format: ProviderFormat) -> Vec<String> {
    let base = base_url.trim_end_matches('/');
    match format {
        // Google has no /v1/models; probing it would 404 on a healthy provider.
        ProviderFormat::Google => vec![format!("{base}/v1beta/models")],
        // /v1/health is not part of the OpenAI spec, but many gateways expose it,
        // so it is worth a second try when /v1/models is unavailable.
        ProviderFormat::OpenAI => {
            vec![format!("{base}/v1/models"), format!("{base}/v1/health")]
        }
        _ => vec![format!("{base}/v1/models")],
    }
}

pub async fn check_provider(client: &Client, provider: &Provider) -> ProviderHealth {
    let start = Instant::now();
    let endpoints = probe_endpoints(&provider.base_url, provider.format);

    // Read the key once, before the loop: the OS keychain can prompt or hit
    // D-Bus, and the value cannot change between two attempts at the same
    // provider, so re-reading it per endpoint only adds latency.
    let api_key = match crate::secrets::get(&provider.name) {
        Ok(k) => k,
        Err(e) => {
            return ProviderHealth {
                ok: false,
                latency_ms: start.elapsed().as_millis() as u64,
                error: Some(format!("could not read API key from keychain: {e}")),
                checked_at: chrono::Utc::now().to_rfc3339(),
            };
        }
    };

    let mut last_error: Option<String> = None;

    for url in endpoints {
        let mut req = client.get(&url).timeout(Duration::from_secs(10));
        req = match provider.auth {
            AuthScheme::Bearer => req.bearer_auth(&api_key),
            AuthScheme::XApiKey => req
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01"),
            AuthScheme::ApiKey => req.header("api-key", &api_key),
            AuthScheme::XGoogApiKey => req.header("x-goog-api-key", &api_key),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_slashes_do_not_double_up() {
        // A base URL pasted from a browser usually ends in '/', and '//v1/models'
        // 404s on some gateways — so a healthy provider would read as down.
        for base in ["https://api.example.com", "https://api.example.com/"] {
            assert_eq!(
                probe_endpoints(base, ProviderFormat::Anthropic),
                vec!["https://api.example.com/v1/models".to_string()],
                "base {base:?}"
            );
        }
    }

    #[test]
    fn google_probes_only_its_own_versioned_path() {
        // Google exposes /v1beta/models and no /v1/models: probing the latter
        // would report a 404 as the provider being unhealthy.
        assert_eq!(
            probe_endpoints(
                "https://generativelanguage.googleapis.com",
                ProviderFormat::Google
            ),
            vec!["https://generativelanguage.googleapis.com/v1beta/models".to_string()]
        );
    }

    #[test]
    fn openai_falls_back_to_health_after_models() {
        // Order matters: /v1/models is the spec endpoint, /v1/health is the
        // gateway-specific fallback, so the fallback must come second.
        assert_eq!(
            probe_endpoints("https://api.openai.com", ProviderFormat::OpenAI),
            vec![
                "https://api.openai.com/v1/models".to_string(),
                "https://api.openai.com/v1/health".to_string(),
            ]
        );
    }

    #[test]
    fn every_format_probes_at_least_one_endpoint() {
        // An empty list would skip the loop entirely and report ok: false with
        // no error at all — indistinguishable from a silent failure.
        for format in [
            ProviderFormat::OpenAI,
            ProviderFormat::Anthropic,
            ProviderFormat::Google,
            ProviderFormat::Responses,
        ] {
            let endpoints = probe_endpoints("https://x.test", format);
            assert!(!endpoints.is_empty(), "{format:?} has no probe endpoint");
            for url in endpoints {
                assert!(url.starts_with("https://x.test/"), "malformed url {url}");
            }
        }
    }

    #[test]
    fn cache_returns_the_latest_insert_per_provider() {
        // The UI reads `all()` on a timer, so a stale entry surviving a newer
        // insert would pin a provider to a red dot after it recovered.
        let mut cache = HealthCache::default();
        assert!(cache.all().is_empty());

        let down = ProviderHealth {
            ok: false,
            latency_ms: 10,
            error: Some("boom".into()),
            checked_at: "2026-01-01T00:00:00Z".into(),
        };
        let up = ProviderHealth {
            ok: true,
            latency_ms: 20,
            error: None,
            checked_at: "2026-01-01T00:01:00Z".into(),
        };
        cache.insert(1, down);
        cache.insert(1, up);
        cache.insert(
            2,
            ProviderHealth {
                ok: false,
                latency_ms: 30,
                error: None,
                checked_at: "2026-01-01T00:02:00Z".into(),
            },
        );

        let all = cache.all();
        assert_eq!(all.len(), 2);
        assert!(all[&1].ok, "the newer insert must win");
        assert_eq!(all[&1].latency_ms, 20);
        assert!(!all[&2].ok);
    }
}
