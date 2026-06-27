//! Provider configuration & routing types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProviderFormat {
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "anthropic")]
    Anthropic,
}

impl ProviderFormat {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
        }
    }
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "anthropic" => Self::Anthropic,
            _ => Self::OpenAI,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthScheme {
    Bearer,
    XApiKey,
    ApiKey,
}

impl AuthScheme {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Bearer => "bearer",
            Self::XApiKey => "x_api_key",
            Self::ApiKey => "api_key",
        }
    }
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "x_api_key" => Self::XApiKey,
            "api_key" => Self::ApiKey,
            _ => Self::Bearer,
        }
    }
}

/// A configured LLM provider. The API key is *never* stored in this struct or
/// the database — it lives only in the OS keychain, keyed by `name`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: i64,
    pub name: String,
    pub base_url: String,
    pub format: ProviderFormat,
    pub auth: AuthScheme,
    pub models: Vec<String>,
    pub input_cost_per_1k: Option<f64>,
    pub output_cost_per_1k: Option<f64>,
    pub is_default: bool,
}

/// Frontend-facing provider with a flag indicating whether a key is stored.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderDto {
    pub provider: Provider,
    pub api_key_set: bool,
}

/// Input for creating a provider (includes the API key once, for storage).
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInput {
    pub name: String,
    pub base_url: String,
    pub format: ProviderFormat,
    pub auth: AuthScheme,
    pub api_key: String,
    pub models: Vec<String>,
    pub input_cost_per_1k: Option<f64>,
    pub output_cost_per_1k: Option<f64>,
    pub is_default: bool,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub providers: Vec<Provider>,
    pub port: u16,
    pub budget: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            port: 3742,
            budget: 0.0,
        }
    }
}
