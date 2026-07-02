//! Runtime configuration fetched from a private gist at build time.
//!
//! The gist URL is injected via the `TOKENGUARD_CONFIG_GIST_URL` env var during
//! builds. The app refuses to start if the URL is missing or the config cannot
//! be loaded.

use reqwest::Client;
use serde::{Deserialize, Serialize};

const CONFIG_GIST_URL: &str = match option_env!("TOKENGUARD_CONFIG_GIST_URL") {
    Some(url) => url,
    None => "unset",
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct BannersConfig {
    pub enabled: bool,
    pub interval_hours: u64,
    pub title: String,
    pub body: String,
    pub cta_url: String,
    pub dismiss_duration_hours: u64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub banners: BannersConfig,
}

/// Fetch the runtime config from the configured gist.
///
/// Returns an error if no URL is set, the request fails, or the response is not
/// valid JSON.
pub async fn fetch_required(client: &Client, url: &str) -> Result<RuntimeConfig, String> {
    if url.trim().is_empty() || url == "unset" {
        return Err(
            "TOKENGUARD_CONFIG_GIST_URL is not set; a runtime config gist is required".into(),
        );
    }
    fetch(client, url)
        .await
        .map_err(|e| format!("failed to load runtime config from {url}: {e}"))
}

async fn fetch(client: &Client, url: &str) -> Result<RuntimeConfig, String> {
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let text = resp.text().await.map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Build-time helper: returns the configured gist URL so the app can reload the
/// config later if needed.
pub fn gist_url() -> &'static str {
    CONFIG_GIST_URL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_banner_config() {
        let json = r#"{
            "banners": {
                "enabled": true,
                "interval_hours": 24,
                "title": "Hi",
                "body": "Support us",
                "cta_url": "https://example.com",
                "dismiss_duration_hours": 12
            }
        }"#;
        let cfg: RuntimeConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.banners.enabled);
        assert_eq!(cfg.banners.title, "Hi");
    }
}
