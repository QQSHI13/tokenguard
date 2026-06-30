//! Runtime configuration fetched from a private gist at build time.
//!
//! This separates edition-specific behavior (banners, updater endpoints) from
//! the public source code. The gist URL is injected via the
//! `TOKENGUARD_CONFIG_GIST_URL` env var during CI builds. Local dev builds use
//! a default config with banners and updates disabled.

use reqwest::Client;
use serde::{Deserialize, Serialize};

const CONFIG_GIST_URL: &str = match option_env!("TOKENGUARD_CONFIG_GIST_URL") {
    Some(url) => url,
    None => "unset",
};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Edition {
    #[default]
    GithubFree,
    Paid,
    MicrosoftStore,
}

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
pub struct UpdaterConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub edition: Edition,
    pub banners: BannersConfig,
    pub updater: UpdaterConfig,
}

impl RuntimeConfig {
    /// Default config used for local development or when the gist is unreachable.
    pub fn dev_default() -> Self {
        Self {
            edition: Edition::GithubFree,
            banners: BannersConfig {
                enabled: false,
                interval_hours: 48,
                title: String::new(),
                body: String::new(),
                cta_url: String::new(),
                dismiss_duration_hours: 24,
            },
            updater: UpdaterConfig {
                enabled: false,
                endpoint: String::new(),
                public_key: String::new(),
            },
        }
    }
}

/// Fetch the runtime config from the configured gist, falling back to the dev
/// default on any error or when no URL is set.
pub async fn fetch_or_default(client: &Client, url: &str) -> RuntimeConfig {
    if url.trim().is_empty() || url == "unset" {
        return RuntimeConfig::dev_default();
    }
    match fetch(client, url).await {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::warn!("failed to load runtime config from {url}: {e}");
            RuntimeConfig::dev_default()
        }
    }
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
    fn parse_github_free_config() {
        let json = r#"{
            "edition": "github-free",
            "banners": {
                "enabled": true,
                "interval_hours": 24,
                "title": "Hi",
                "body": "Support us",
                "cta_url": "https://example.com",
                "dismiss_duration_hours": 12
            },
            "updater": { "enabled": false }
        }"#;
        let cfg: RuntimeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.edition, Edition::GithubFree);
        assert!(cfg.banners.enabled);
        assert_eq!(cfg.banners.title, "Hi");
    }

    #[test]
    fn parse_paid_config() {
        let json = r#"{
            "edition": "paid",
            "updater": {
                "enabled": true,
                "endpoint": "https://example.com/latest.json",
                "public_key": "abc123"
            }
        }"#;
        let cfg: RuntimeConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.edition, Edition::Paid);
        assert!(cfg.updater.enabled);
    }
}
