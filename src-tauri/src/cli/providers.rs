//! Provider management commands for the CLI.

use crate::config::{AuthScheme, ModelMapping, ProviderFormat, ProviderInput};
use crate::db;
use crate::secrets;
use crate::state::AppState;
use anyhow::{Context, Result};
use std::sync::Arc;

pub fn list(state: &Arc<AppState>) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let providers = db::list_providers(&conn).context("list providers")?;
    if providers.is_empty() {
        println!("No providers configured.");
        return Ok(());
    }
    println!(
        "{:<5} {:<20} {:<12} {:<40} KEY SET",
        "ID", "NAME", "FORMAT", "BASE URL"
    );
    for p in providers {
        let (set, err) = secrets::status(&p.name);
        let key_col = if set {
            "yes"
        } else if let Some(e) = err {
            &format!("error: {e}")
        } else {
            "no"
        };
        println!(
            "{:<5} {:<20} {:<12} {:<40} {}",
            p.id,
            p.name,
            p.format.as_db_str(),
            p.base_url,
            key_col
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn add(
    state: &Arc<AppState>,
    name: String,
    base_url: String,
    format: String,
    key: String,
    auth: String,
    is_default: bool,
    models: Vec<String>,
    fallback_provider_id: Option<i64>,
    extra_headers: Vec<String>,
) -> Result<()> {
    let format = parse_format(&format)?;
    let auth = parse_auth(&auth)?;
    let models = parse_models(&models)?;
    let extra_headers = parse_headers(&extra_headers)?;
    let input = ProviderInput {
        name: name.clone(),
        base_url,
        format,
        auth,
        api_key: key.clone(),
        models,
        is_default,
        clear_key: false,
        fallback_provider_id,
        extra_headers,
    };

    validate_provider_input(&input)?;

    let conn = state.db.get().context("get DB connection")?;
    let id = db::insert_provider(&conn, &input).context("insert provider")?;
    secrets::set(&name, &key).map_err(|e| anyhow::anyhow!("store provider key: {e}"))?;

    println!("Added provider '{}' with ID {}", name, id);
    Ok(())
}

pub fn delete(state: &Arc<AppState>, id: i64) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let providers = db::list_providers(&conn).context("list providers")?;
    let provider = providers
        .into_iter()
        .find(|p| p.id == id)
        .context("provider not found")?;

    db::delete_provider(&conn, id).context("delete provider")?;
    let _ = secrets::delete(&provider.name);

    println!("Deleted provider '{}' (ID {})", provider.name, id);
    Ok(())
}

pub fn set_key(state: &Arc<AppState>, name: String, key: String) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let providers = db::list_providers(&conn).context("list providers")?;
    if !providers.iter().any(|p| p.name == name) {
        anyhow::bail!("provider '{}' not found", name);
    }
    secrets::set(&name, &key).map_err(|e| anyhow::anyhow!("store provider key: {e}"))?;
    println!("Updated key for provider '{}'", name);
    Ok(())
}

pub fn delete_key(state: &Arc<AppState>, name: String) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let providers = db::list_providers(&conn).context("list providers")?;
    if !providers.iter().any(|p| p.name == name) {
        anyhow::bail!("provider '{}' not found", name);
    }
    secrets::delete(&name).map_err(|e| anyhow::anyhow!("delete provider key: {e}"))?;
    println!("Deleted key for provider '{}'", name);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    state: &Arc<AppState>,
    id: i64,
    name: Option<String>,
    base_url: Option<String>,
    format: Option<String>,
    key: Option<String>,
    auth: Option<String>,
    is_default: Option<bool>,
    models: Vec<String>,
    fallback_provider_id: Option<i64>,
    extra_headers: Vec<String>,
    clear_key: bool,
) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let providers = db::list_providers(&conn).context("list providers")?;
    let existing = providers
        .into_iter()
        .find(|p| p.id == id)
        .context("provider not found")?;

    let old_name = existing.name.clone();
    let new_name = name.unwrap_or_else(|| existing.name.clone());

    let mut input = ProviderInput {
        name: new_name.clone(),
        base_url: existing.base_url,
        format: existing.format,
        auth: existing.auth,
        api_key: key.clone().unwrap_or_default(),
        models: if models.is_empty() {
            existing.models
        } else {
            parse_models(&models)?
        },
        is_default: is_default.unwrap_or(existing.is_default),
        clear_key,
        fallback_provider_id: fallback_provider_id.or(existing.fallback_provider_id),
        extra_headers: if extra_headers.is_empty() {
            existing.extra_headers
        } else {
            parse_headers(&extra_headers)?
        },
    };

    if let Some(url) = base_url {
        input.base_url = url;
    }
    if let Some(fmt) = format {
        input.format = parse_format(&fmt)?;
    }
    if let Some(a) = auth {
        input.auth = parse_auth(&a)?;
    }

    validate_provider_input(&input)?;
    db::update_provider(&conn, id, &input).context("update provider")?;

    if clear_key {
        secrets::delete(&new_name).ok();
        if new_name != old_name {
            secrets::delete(&old_name).ok();
        }
    } else if let Some(k) = key {
        secrets::set(&new_name, &k).map_err(|e| anyhow::anyhow!("store provider key: {e}"))?;
        if new_name != old_name {
            secrets::delete(&old_name).ok();
        }
    } else if new_name != old_name {
        // move key to new name
        match secrets::get(&old_name) {
            Ok(k) => {
                secrets::set(&new_name, &k)
                    .map_err(|e| anyhow::anyhow!("move provider key: {e}"))?;
                secrets::delete(&old_name).ok();
            }
            Err(e) => {
                let lower = e.to_lowercase();
                if !lower.contains("noentry") && !lower.contains("no entry") {
                    anyhow::bail!("could not read API key from keychain: {e}");
                }
            }
        }
    }

    // Reload config so proxy sees the change immediately.
    let new_cfg = db::load_config(&conn).context("reload config")?;
    drop(conn);
    *state.config.write().map_err(|e| anyhow::anyhow!("{e}"))? = new_cfg;

    println!("Updated provider '{}' (ID {})", new_name, id);
    Ok(())
}

pub async fn refresh_models(state: &Arc<AppState>, id: i64) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let providers = db::list_providers(&conn).context("list providers")?;
    let provider = providers
        .into_iter()
        .find(|p| p.id == id)
        .context("provider not found")?;

    let api_key =
        secrets::get(&provider.name).map_err(|e| anyhow::anyhow!("read provider key: {e}"))?;

    let models = fetch_models(
        &provider.base_url,
        &provider.format,
        &api_key,
        &provider.auth,
    )
    .await
    .context("fetch models")?;
    db::update_provider_models(&conn, id, &models).context("update provider models")?;

    let new_cfg = db::load_config(&conn).context("reload config")?;
    drop(conn);
    *state.config.write().map_err(|e| anyhow::anyhow!("{e}"))? = new_cfg;

    println!(
        "Updated models for provider '{}' ({} models)",
        provider.name,
        models.len()
    );
    Ok(())
}

async fn fetch_models(
    base_url: &str,
    format: &ProviderFormat,
    api_key: &str,
    auth: &crate::config::AuthScheme,
) -> Result<Vec<crate::config::ModelMapping>> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let mut req = client.get(&url).timeout(std::time::Duration::from_secs(15));
    req = match auth {
        crate::config::AuthScheme::Bearer => req.bearer_auth(api_key),
        crate::config::AuthScheme::XApiKey => req
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        crate::config::AuthScheme::ApiKey => req.header("api-key", api_key),
        crate::config::AuthScheme::XGoogApiKey => req.header("x-goog-api-key", api_key),
    };

    let resp = req.send().await.context("fetch models request")?;
    if !resp.status().is_success() {
        anyhow::bail!("models endpoint returned {}", resp.status());
    }

    let json: serde_json::Value = resp.json().await.context("parse models response")?;

    // Google returns { "models": [{ "name": "models/gemini-..." }] };
    // OpenAI/Anthropic/Responses return { "data": [{ "id": "..." }] }.
    let mut models = Vec::new();
    if *format == ProviderFormat::Google {
        if let Some(arr) = json.get("models").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                    let short = name.rsplit('/').next().unwrap_or(name);
                    models.push(crate::config::ModelMapping {
                        local: short.to_string(),
                        remote: name.to_string(),
                        input_cost_per_1k: None,
                        output_cost_per_1k: None,
                        cached_input_cost_per_1k: None,
                    });
                }
            }
        }
    }
    if models.is_empty() {
        if let Some(arr) = json.get("data").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    models.push(crate::config::ModelMapping {
                        local: id.to_string(),
                        remote: id.to_string(),
                        input_cost_per_1k: None,
                        output_cost_per_1k: None,
                        cached_input_cost_per_1k: None,
                    });
                }
            }
        }
    }

    Ok(models)
}

fn parse_format(s: &str) -> Result<ProviderFormat> {
    match s.to_lowercase().as_str() {
        "openai" => Ok(ProviderFormat::OpenAI),
        "anthropic" => Ok(ProviderFormat::Anthropic),
        "google" | "gemini" => Ok(ProviderFormat::Google),
        "responses" => Ok(ProviderFormat::Responses),
        _ => anyhow::bail!(
            "unknown format '{}'; use openai, anthropic, google, or responses",
            s
        ),
    }
}

fn parse_auth(s: &str) -> Result<AuthScheme> {
    match s.to_lowercase().replace('-', "_").as_str() {
        "bearer" => Ok(AuthScheme::Bearer),
        "x_api_key" | "xapikey" | "x-api-key" => Ok(AuthScheme::XApiKey),
        "api_key" | "apikey" | "api-key" => Ok(AuthScheme::ApiKey),
        "x_goog_api_key" | "xgoogapikey" | "x-goog-api-key" => Ok(AuthScheme::XGoogApiKey),
        _ => anyhow::bail!(
            "unknown auth scheme '{}'; use bearer, x-api-key, api-key, or x-goog-api-key",
            s
        ),
    }
}

/// Parse model specs of the form local=remote:cost_in:cost_out:cost_cached.
fn parse_models(specs: &[String]) -> Result<Vec<ModelMapping>> {
    let mut out = Vec::new();
    for spec in specs {
        let (local, rest) = spec.split_once('=').unwrap_or((spec, spec));
        let parts: Vec<&str> = rest.split(':').collect();
        let remote = parts.first().copied().unwrap_or(local);
        let parse_cost = |i: usize| parts.get(i).and_then(|s| s.parse().ok());
        out.push(ModelMapping {
            local: local.to_string(),
            remote: remote.to_string(),
            input_cost_per_1k: parse_cost(1),
            output_cost_per_1k: parse_cost(2),
            cached_input_cost_per_1k: parse_cost(3),
        });
    }
    Ok(out)
}

fn parse_headers(specs: &[String]) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for spec in specs {
        let (k, v) = spec
            .split_once('=')
            .context(format!("extra header must be KEY=VALUE: {spec}"))?;
        out.push((k.to_string(), v.to_string()));
    }
    Ok(out)
}

fn validate_provider_input(input: &ProviderInput) -> Result<()> {
    if input.name.is_empty() {
        anyhow::bail!("provider name cannot be empty");
    }
    if input.base_url.is_empty() {
        anyhow::bail!("base URL cannot be empty");
    }
    if input.api_key.is_empty() {
        anyhow::bail!("API key cannot be empty");
    }
    Ok(())
}
