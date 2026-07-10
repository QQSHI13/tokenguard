//! Provider configuration & routing types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProviderFormat {
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "google")]
    Google,
}

impl ProviderFormat {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Google => "google",
        }
    }
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "anthropic" => Self::Anthropic,
            "google" => Self::Google,
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
    XGoogApiKey,
}

impl AuthScheme {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Bearer => "bearer",
            Self::XApiKey => "x_api_key",
            Self::ApiKey => "api_key",
            Self::XGoogApiKey => "x_goog_api_key",
        }
    }
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "x_api_key" => Self::XApiKey,
            "api_key" => Self::ApiKey,
            "x_goog_api_key" => Self::XGoogApiKey,
            _ => Self::Bearer,
        }
    }
}

/// Mapping between the model name the user sees/sends locally and the model
/// name the remote provider expects. Both default to the same value when not
/// explicitly configured.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMapping {
    pub local: String,
    pub remote: String,
    #[serde(default)]
    pub input_cost_per_1k: Option<f64>,
    #[serde(default)]
    pub output_cost_per_1k: Option<f64>,
    #[serde(default)]
    pub cached_input_cost_per_1k: Option<f64>,
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
    pub models: Vec<ModelMapping>,
    pub is_default: bool,
    pub fallback_provider_id: Option<i64>,
    #[serde(default)]
    pub extra_headers: Vec<(String, String)>,
}

/// Frontend-facing provider with a flag indicating whether a key is stored.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderDto {
    pub provider: Provider,
    pub api_key_set: bool,
    pub key_error: Option<String>,
    pub key_created_at: Option<String>,
}

/// Input for creating a provider (includes the API key once, for storage).
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInput {
    pub name: String,
    pub base_url: String,
    pub format: ProviderFormat,
    pub auth: AuthScheme,
    pub api_key: String,
    pub models: Vec<ModelMapping>,
    pub is_default: bool,
    /// On update: delete the stored key (api_key is ignored then).
    #[serde(default)]
    pub clear_key: bool,
    #[serde(default)]
    pub fallback_provider_id: Option<i64>,
    #[serde(default)]
    pub extra_headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BudgetPeriod {
    Daily,
    Weekly,
    Monthly,
}

impl BudgetPeriod {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "weekly" => Self::Weekly,
            "monthly" => Self::Monthly,
            _ => Self::Daily,
        }
    }
    /// Approximate seconds for the budget window. Monthly is treated as 30 days.
    pub fn seconds(self) -> u64 {
        match self {
            Self::Daily => 86_400,
            Self::Weekly => 604_800,
            Self::Monthly => 2_592_000,
        }
    }
}

/// A project workspace. `label_key` is the throwaway value the user sets as
/// OPENAI_API_KEY (or x-api-key) in their coding agent; the proxy maps it to
/// `name` for tagging. The real provider key stays in the keychain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub label_key: String,
    #[serde(default)]
    pub budget: f64,
    #[serde(default = "default_budget_period")]
    pub budget_period: BudgetPeriod,
    #[serde(default = "default_budget_action")]
    pub budget_action: LimitAction,
}

fn default_budget_period() -> BudgetPeriod {
    BudgetPeriod::Daily
}

fn default_budget_action() -> LimitAction {
    LimitAction::Warn
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectInput {
    pub name: String,
    pub label_key: String,
    #[serde(default)]
    pub budget: f64,
    #[serde(default = "default_budget_period")]
    pub budget_period: BudgetPeriod,
    #[serde(default = "default_budget_action")]
    pub budget_action: LimitAction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LimitMetric {
    Money,
    Tokens,
    Requests,
    TimeSec,
    RequestsPerMinute,
    TokensPerMinute,
}

impl LimitMetric {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Money => "money",
            Self::Tokens => "tokens",
            Self::Requests => "requests",
            Self::TimeSec => "time_sec",
            Self::RequestsPerMinute => "requests_per_minute",
            Self::TokensPerMinute => "tokens_per_minute",
        }
    }
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "tokens" => Self::Tokens,
            "requests" => Self::Requests,
            "time_sec" => Self::TimeSec,
            "requests_per_minute" => Self::RequestsPerMinute,
            "tokens_per_minute" => Self::TokensPerMinute,
            _ => Self::Money,
        }
    }
    /// True for rate-based metrics that use a fixed rolling window.
    pub fn is_rate(self) -> bool {
        matches!(self, Self::RequestsPerMinute | Self::TokensPerMinute)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LimitPeriod {
    Once,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    CustomSec(u64),
}

impl LimitPeriod {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Hourly => "hourly",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::CustomSec(_) => "custom_sec",
        }
    }
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "once" => Self::Once,
            "hourly" => Self::Hourly,
            "weekly" => Self::Weekly,
            "monthly" => Self::Monthly,
            "custom_sec" => Self::CustomSec(0),
            _ => Self::Daily,
        }
    }
    pub fn seconds(self) -> Option<u64> {
        match self {
            Self::Once => None,
            Self::Hourly => Some(3600),
            Self::Daily => Some(86_400),
            Self::Weekly => Some(604_800),
            // Approximate month as 30 days. Calendar-month boundaries would require
            // a more complex cutoff query.
            Self::Monthly => Some(2_592_000),
            Self::CustomSec(s) => Some(s),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LimitScope {
    Global,
    Provider,
    Project,
}

impl LimitScope {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Provider => "provider",
            Self::Project => "project",
        }
    }
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "provider" => Self::Provider,
            "project" => Self::Project,
            _ => Self::Global,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LimitAction {
    Warn,
    Block,
    Pause,
}

impl LimitAction {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Warn => "warn",
            Self::Block => "block",
            Self::Pause => "pause",
        }
    }
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "block" => Self::Block,
            "pause" => Self::Pause,
            _ => Self::Warn,
        }
    }
}

fn all_days() -> u8 {
    0b1111111
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limit {
    pub id: i64,
    pub name: String,
    pub metric: LimitMetric,
    pub period: LimitPeriod,
    pub cap: f64,
    pub warning_threshold: f64,
    pub scope: LimitScope,
    pub scope_id: Option<i64>,
    pub action: LimitAction,
    pub enabled: bool,
    #[serde(default)]
    pub active_hours_start: Option<String>,
    #[serde(default)]
    pub active_hours_end: Option<String>,
    #[serde(default = "all_days")]
    pub active_days: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitInput {
    pub name: String,
    pub metric: LimitMetric,
    pub period: LimitPeriod,
    pub cap: f64,
    pub warning_threshold: f64,
    pub scope: LimitScope,
    pub scope_id: Option<i64>,
    pub action: LimitAction,
    pub enabled: bool,
    #[serde(default)]
    pub active_hours_start: Option<String>,
    #[serde(default)]
    pub active_hours_end: Option<String>,
    #[serde(default = "all_days")]
    pub active_days: u8,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub providers: Vec<Provider>,
    pub projects: Vec<Project>,
    pub limits: Vec<Limit>,
    pub port: u16,
    pub budget: f64,
    pub log_bodies: bool,
    pub auto_export_days: u32,
    pub auto_export_folder: Option<String>,
    pub webhook_url: Option<String>,
    pub auto_start: bool,
    pub key_rotation_days: u32,
    pub log_retention_days: u32,
    pub expose_to_lan: bool,
    pub lan_ip: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            projects: Vec::new(),
            limits: Vec::new(),
            port: 3742,
            budget: 0.0,
            log_bodies: false,
            auto_export_days: 0,
            auto_export_folder: None,
            webhook_url: None,
            auto_start: false,
            key_rotation_days: 90,
            log_retention_days: 0,
            expose_to_lan: false,
            lan_ip: None,
        }
    }
}
