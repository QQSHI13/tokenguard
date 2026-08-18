//! Interactive prompts for a friendlier CLI experience.

use dialoguer::{Input, Select};

fn required(text: &str, value: Option<String>) -> Result<String, anyhow::Error> {
    match value {
        Some(v) if !v.trim().is_empty() => Ok(v),
        _ => Input::new()
            .with_prompt(text)
            .interact_text()
            .map_err(|e| anyhow::anyhow!("prompt failed: {e}")),
    }
}

fn optional(text: &str, value: Option<String>, default: &str) -> Result<String, anyhow::Error> {
    match value {
        Some(v) => Ok(v),
        None => Input::new()
            .with_prompt(text)
            .default(default.to_string())
            .interact_text()
            .map_err(|e| anyhow::anyhow!("prompt failed: {e}")),
    }
}

pub fn prompt_optional_secret(name: &str, value: Option<String>) -> Result<String, anyhow::Error> {
    match value {
        Some(v) => Ok(v),
        None => Input::new()
            .with_prompt(name)
            .interact_text()
            .map_err(|e| anyhow::anyhow!("prompt failed: {e}")),
    }
}

pub fn provider_prompt(
    name: Option<String>,
    base_url: Option<String>,
    format: Option<String>,
    key: Option<String>,
    auth: Option<String>,
) -> Result<(String, String, String, String, String), anyhow::Error> {
    let name = required("Provider name", name)?;
    let base_url = optional("Base URL", base_url, "https://api.openai.com/v1")?;

    let formats = vec!["openai", "anthropic", "google", "responses"];
    let format = match format {
        Some(f) => f,
        None => {
            let idx = Select::new()
                .with_prompt("API format")
                .items(&formats)
                .default(0)
                .interact()
                .map_err(|e| anyhow::anyhow!("prompt failed: {e}"))?;
            formats[idx].to_string()
        }
    };

    let auth = match auth {
        Some(a) => a,
        None => match format.as_str() {
            "anthropic" => "x-api-key".to_string(),
            "google" => "x-goog-api-key".to_string(),
            _ => "bearer".to_string(),
        },
    };

    let key = required("API key", key)?;
    Ok((name, base_url, format, key, auth))
}

pub fn project_prompt(
    name: Option<String>,
    label_key: Option<String>,
    budget: Option<f64>,
    budget_period: Option<String>,
    budget_action: Option<String>,
) -> Result<(String, String, f64, String, String), anyhow::Error> {
    let name = required("Project name", name)?;
    let label_key = match label_key {
        Some(k) => k,
        None => {
            let bytes: Vec<u8> = (0..16).map(|_| rand::random()).collect();
            let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
            Input::new()
                .with_prompt("Label key")
                .default(format!("tg_{hex}"))
                .interact_text()
                .map_err(|e| anyhow::anyhow!("prompt failed: {e}"))?
        }
    };
    let budget = budget.unwrap_or(0.0);
    let budget_period = budget_period.unwrap_or_else(|| "daily".to_string());
    let budget_action = budget_action.unwrap_or_else(|| "warn".to_string());
    Ok((name, label_key, budget, budget_period, budget_action))
}

pub type LimitPromptResult = (
    String,
    String,
    f64,
    String,
    String,
    f64,
    String,
    Option<i64>,
    Option<String>,
    Option<String>,
    u8,
);

#[allow(clippy::too_many_arguments)]
pub fn limit_prompt(
    name: Option<String>,
    metric: Option<String>,
    cap: Option<f64>,
    action: Option<String>,
    period: Option<String>,
    warning_threshold: Option<f64>,
    scope: Option<String>,
    scope_id: Option<i64>,
    active_hours_start: Option<String>,
    active_hours_end: Option<String>,
    active_days: Option<u8>,
) -> Result<LimitPromptResult, anyhow::Error> {
    let name = required("Limit name", name)?;

    let metrics = vec!["money", "tokens", "requests", "time", "rpm", "tpm"];
    let metric = match metric {
        Some(m) => m,
        None => {
            let idx = Select::new()
                .with_prompt("Metric")
                .items(&metrics)
                .default(0)
                .interact()
                .map_err(|e| anyhow::anyhow!("prompt failed: {e}"))?;
            metrics[idx].to_string()
        }
    };

    let cap = match cap {
        Some(c) => c,
        None => Input::new()
            .with_prompt("Cap")
            .interact_text()
            .map_err(|e| anyhow::anyhow!("prompt failed: {e}"))?,
    };

    let action = action.unwrap_or_else(|| "warn".to_string());
    let period = period.unwrap_or_else(|| "daily".to_string());
    let warning_threshold = warning_threshold.unwrap_or(0.8);
    let scope = scope.unwrap_or_else(|| "global".to_string());
    let active_days = active_days.unwrap_or(0b1111111);
    Ok((
        name,
        metric,
        cap,
        action,
        period,
        warning_threshold,
        scope,
        scope_id,
        active_hours_start,
        active_hours_end,
        active_days,
    ))
}
