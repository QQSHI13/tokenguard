//! Tauri command handlers exposed to the frontend.

use crate::config::{
    Limit, LimitAction, LimitInput, LimitMetric, LimitPeriod, LimitScope, Project, ProjectInput,
    ProviderDto, ProviderInput,
};
use crate::db::{self, LogRow};
use crate::prices;
use crate::secrets;
use crate::state::AppState;
use futures::StreamExt;
use rusqlite::params;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, serde::Serialize)]
pub struct SettingsDto {
    pub port: u16,
    pub budget: f64,
    pub paused: bool,
    pub proxy_url: String,
    pub provider_count: usize,
    pub log_bodies: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct SpendDto {
    pub today: f64,
    pub budget: f64,
}

#[derive(Debug, serde::Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
}

#[derive(Debug, serde::Serialize)]
pub struct LimitStatusDto {
    pub limit: Limit,
    pub used: f64,
    pub ratio: f64,
}

#[derive(Debug, serde::Serialize)]
pub struct LimitPreset {
    pub name: String,
    pub input: LimitInput,
}

#[tauri::command]
pub fn list_providers(state: State<'_, Arc<AppState>>) -> Result<Vec<ProviderDto>, String> {
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    let providers = db::list_providers(&conn).map_err(|e| e.to_string())?;
    drop(conn);
    Ok(providers
        .into_iter()
        .map(|p| {
            let (api_key_set, key_error) = secrets::status(&p.name);
            ProviderDto {
                provider: p,
                api_key_set,
                key_error,
            }
        })
        .collect())
}

fn validate_provider_input(input: &ProviderInput) -> Result<(), String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("provider name is required".into());
    }
    let base = input.base_url.trim();
    if base.is_empty() {
        return Err("provider base URL is required".into());
    }
    if reqwest::Url::parse(base).is_err() {
        return Err("provider base URL is not valid".into());
    }
    Ok(())
}

#[tauri::command]
pub fn add_provider(
    state: State<'_, Arc<AppState>>,
    input: ProviderInput,
) -> Result<ProviderDto, String> {
    validate_provider_input(&input)?;
    if !input.api_key.is_empty() {
        secrets::set(&input.name, &input.api_key)?;
    }
    let provider = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        let id = db::insert_provider(&conn, &input).map_err(|e| e.to_string())?;
        let providers = db::list_providers(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        providers
            .into_iter()
            .find(|p| p.id == id)
            .ok_or("provider not found after insert")?
    };
    // reload config into memory
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    let (api_key_set, key_error) = secrets::status(&input.name);
    Ok(ProviderDto {
        provider,
        api_key_set,
        key_error,
    })
}

#[tauri::command]
pub fn delete_provider(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    // also wipe keychain entry (need provider name first)
    let name = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        let providers = db::list_providers(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        providers
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
    };
    if let Some(name) = name {
        let _ = secrets::delete(&name);
    }
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::delete_provider(&conn, id).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    Ok(())
}

#[tauri::command]
pub fn keyring_selftest() -> String {
    crate::secrets::selftest()
}

#[tauri::command]
pub fn set_provider_key(name: String, key: String) -> Result<(), String> {
    secrets::set(&name, &key)
}

#[tauri::command]
pub fn update_provider(
    state: State<'_, Arc<AppState>>,
    id: i64,
    input: ProviderInput,
) -> Result<ProviderDto, String> {
    validate_provider_input(&input)?;
    // current name, to handle rename + key move
    let old_name = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        let providers = db::list_providers(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        providers
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .ok_or("provider not found")?
    };
    // key handling: explicit clear wins; else new key; else move on rename
    if input.clear_key {
        let _ = secrets::delete(&input.name);
        if old_name != input.name {
            let _ = secrets::delete(&old_name);
        }
    } else if !input.api_key.is_empty() {
        secrets::set(&input.name, &input.api_key)?;
        if old_name != input.name {
            let _ = secrets::delete(&old_name);
        }
    } else if old_name != input.name {
        if let Ok(k) = secrets::get(&old_name) {
            secrets::set(&input.name, &k)?;
            let _ = secrets::delete(&old_name);
        }
    }
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::update_provider(&conn, id, &input).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    let p = {
        let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
        cfg.providers
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or("provider not found after update")?
    };
    let (api_key_set, key_error) = secrets::status(&input.name);
    Ok(ProviderDto {
        provider: p,
        api_key_set,
        key_error,
    })
}

#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> Result<SettingsDto, String> {
    let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
    Ok(SettingsDto {
        port: cfg.port,
        budget: cfg.budget,
        paused: state
            .inner()
            .paused
            .load(std::sync::atomic::Ordering::Relaxed),
        proxy_url: format!("http://localhost:{}", cfg.port),
        provider_count: cfg.providers.len(),
        log_bodies: cfg.log_bodies,
    })
}

#[tauri::command]
pub fn backup_database(
    state: State<'_, Arc<AppState>>,
    target_path: String,
) -> Result<(), String> {
    let target = std::path::PathBuf::from(target_path);
    if target.exists() {
        return Err("target file already exists".into());
    }
    std::fs::create_dir_all(target.parent().ok_or("invalid target path")?)
        .map_err(|e| e.to_string())?;

    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    let mut dst = rusqlite::Connection::open(&target).map_err(|e| e.to_string())?;
    let backup = rusqlite::backup::Backup::new(&conn, &mut dst).map_err(|e| e.to_string())?;
    backup
        .run_to_completion(5, std::time::Duration::from_millis(100), None)
        .map_err(|e| format!("backup failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn restore_database(
    state: State<'_, Arc<AppState>>,
    source_path: String,
) -> Result<(), String> {
    let source = std::path::PathBuf::from(source_path);
    if !source.exists() {
        return Err("source file does not exist".into());
    }
    // Validate that the source is a SQLite database.
    {
        let test = rusqlite::Connection::open(&source).map_err(|e| e.to_string())?;
        let ok: bool = test
            .query_row("PRAGMA schema_version", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if !ok {
            return Err("source is not a valid SQLite database".into());
        }
    }

    let target = state.inner().db_path.clone();
    // Copy over the current database. The app will restart so the pool re-opens
    // the restored file.
    std::fs::copy(&source, &target).map_err(|e| e.to_string())?;
    state.inner().app.restart();
}

#[tauri::command]
pub fn set_log_bodies(state: State<'_, Arc<AppState>>, enabled: bool) -> Result<(), String> {
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "log_bodies", if enabled { "1" } else { "0" }).map_err(|e| e.to_string())?;
        drop(conn);
    }
    state
        .inner()
        .config
        .write()
        .map_err(|e| e.to_string())?
        .log_bodies = enabled;
    Ok(())
}

#[tauri::command]
pub fn set_budget(state: State<'_, Arc<AppState>>, budget: f64) -> Result<(), String> {
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "budget", &budget.to_string()).map_err(|e| e.to_string())?;
        drop(conn);
    }
    state
        .inner()
        .config
        .write()
        .map_err(|e| e.to_string())?
        .budget = budget;
    state.inner().refresh_tray();
    Ok(())
}

#[tauri::command]
pub fn set_port(state: State<'_, Arc<AppState>>, port: u16) -> Result<(), String> {
    // Persist; applied on next app launch (listener is bound at startup).
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    db::set_setting(&conn, "port", &port.to_string()).map_err(|e| e.to_string())?;
    state
        .inner()
        .config
        .write()
        .map_err(|e| e.to_string())?
        .port = port;
    Ok(())
}

#[tauri::command]
pub fn pause_resume(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    Ok(state.inner().toggle_pause())
}

#[tauri::command]
pub fn get_today_spend(state: State<'_, Arc<AppState>>) -> Result<SpendDto, String> {
    Ok(SpendDto {
        today: state.inner().today_spend(),
        budget: state
            .inner()
            .config
            .read()
            .map_err(|e| e.to_string())?
            .budget,
    })
}

#[tauri::command]
pub fn get_logs(
    state: State<'_, Arc<AppState>>,
    limit: Option<u64>,
    days: Option<u64>,
) -> Result<Vec<LogRow>, String> {
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    db::list_logs(&conn, limit.unwrap_or(5000), days).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LogFilterInput {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogListResult {
    pub rows: Vec<LogRow>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

#[tauri::command]
pub fn get_logs_filtered(
    state: State<'_, Arc<AppState>>,
    filter: LogFilterInput,
) -> Result<LogListResult, String> {
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    let f = db::LogFilter {
        provider: filter.provider,
        model: filter.model,
        project: filter.project,
        start: filter.start,
        end: filter.end,
        page: filter.page.unwrap_or(1),
        page_size: filter.page_size.unwrap_or(50).max(1),
    };
    let rows = db::list_logs_filtered(&conn, &f).map_err(|e| e.to_string())?;
    let total = db::count_logs_filtered(&conn, &f).map_err(|e| e.to_string())?;
    Ok(LogListResult {
        rows,
        total,
        page: f.page,
        page_size: f.page_size,
    })
}

#[tauri::command]
pub fn write_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_models(state: State<'_, Arc<AppState>>) -> Result<Vec<ModelInfo>, String> {
    let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
    let mut out: Vec<ModelInfo> = Vec::new();
    for p in &cfg.providers {
        for m in &p.models {
            out.push(ModelInfo {
                id: m.local.clone(),
                provider: p.name.clone(),
            });
        }
    }
    Ok(out)
}

/// Export logs as CSV or JSON text (frontend triggers a Blob download).
#[tauri::command]
pub fn export_logs(
    state: State<'_, Arc<AppState>>,
    format: String,
    days: Option<u64>,
) -> Result<String, String> {
    let rows = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::list_logs(&conn, 100_000, days).map_err(|e| e.to_string())?
    };
    match format.as_str() {
        "json" => Ok(serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?),
        _ => {
            // CSV
            let mut w = String::from(
                "timestamp,provider,model,prompt_tokens,completion_tokens,cost,project\n",
            );
            for r in rows.iter().rev() {
                w.push_str(&format!(
                    "{},{},{},{},{},{:.6},{}\n",
                    r.ts,
                    csv_escape(&r.provider),
                    csv_escape(&r.model),
                    r.prompt_tokens,
                    r.completion_tokens,
                    r.cost,
                    r.project_tag.as_deref().unwrap_or(""),
                ));
            }
            Ok(w)
        }
    }
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Fetch the provider's /v1/models, persist the discovered model list, reload
/// config. Returns the new model list.
#[tauri::command]
pub async fn refresh_models(
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<Vec<crate::config::ModelMapping>, String> {
    let provider = {
        let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
        cfg.providers
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or("provider not found")?
    };
    let api_key = crate::secrets::get(&provider.name)?;
    let base = provider.base_url.trim_end_matches('/');
    let url = format!("{base}/v1/models");
    let mut req = state.inner().client.get(&url);
    req = match provider.auth {
        crate::config::AuthScheme::Bearer => req.bearer_auth(&api_key),
        crate::config::AuthScheme::XApiKey => req
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01"),
        crate::config::AuthScheme::ApiKey => req.header("api-key", &api_key),
    };
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "provider returned {status}: {}",
            &body[..body.len().min(200)]
        ));
    }
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let models: Vec<crate::config::ModelMapping> = names
        .into_iter()
        .map(|name| crate::config::ModelMapping {
            local: name.clone(),
            remote: name,
            input_cost_per_1k: None,
            output_cost_per_1k: None,
            cached_input_cost_per_1k: None,
        })
        .collect();
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::update_provider_models(&conn, id, &models).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    Ok(models)
}

// ---- projects ----

#[tauri::command]
pub fn list_projects(state: State<'_, Arc<AppState>>) -> Result<Vec<Project>, String> {
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    db::list_projects(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_project(
    state: State<'_, Arc<AppState>>,
    input: ProjectInput,
) -> Result<Project, String> {
    if input.name.trim().is_empty() || input.label_key.trim().is_empty() {
        return Err("name and label_key are required".into());
    }
    let id = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::insert_project(&conn, &input.name, &input.label_key).map_err(|e| e.to_string())?
    };
    // reload config
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    Ok(Project {
        id,
        name: input.name,
        label_key: input.label_key,
    })
}

#[tauri::command]
pub fn delete_project(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::delete_project(&conn, id).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    Ok(())
}

// ---- limits ----

#[tauri::command]
pub fn list_limits(state: State<'_, Arc<AppState>>) -> Result<Vec<Limit>, String> {
    let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
    Ok(cfg.limits.clone())
}

#[tauri::command]
pub fn add_limit(state: State<'_, Arc<AppState>>, input: LimitInput) -> Result<Limit, String> {
    let id = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::insert_limit(&conn, &input).map_err(|e| e.to_string())?
    };
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    state.inner().refresh_tray();
    let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
    cfg.limits
        .iter()
        .find(|l| l.id == id)
        .cloned()
        .ok_or("limit not found after insert".into())
}

#[tauri::command]
pub fn update_limit(
    state: State<'_, Arc<AppState>>,
    id: i64,
    input: LimitInput,
) -> Result<Limit, String> {
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::update_limit(&conn, id, &input).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    state.inner().refresh_tray();
    let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
    cfg.limits
        .iter()
        .find(|l| l.id == id)
        .cloned()
        .ok_or("limit not found after update".into())
}

#[tauri::command]
pub fn delete_limit(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::delete_limit(&conn, id).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    state.inner().refresh_tray();
    Ok(())
}

#[tauri::command]
pub fn get_limit_status(state: State<'_, Arc<AppState>>) -> Result<Vec<LimitStatusDto>, String> {
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for limit in &cfg.limits {
        if !limit.enabled {
            continue;
        }
        let used = db::usage_for_limit(&conn, limit).unwrap_or(0.0);
        let ratio = if limit.cap > 0.0 {
            used / limit.cap
        } else {
            0.0
        };
        out.push(LimitStatusDto {
            limit: limit.clone(),
            used,
            ratio,
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn get_limit_presets() -> Vec<LimitPreset> {
    vec![
        LimitPreset {
            name: "$50 / day".into(),
            input: LimitInput {
                name: "Daily money cap".into(),
                metric: LimitMetric::Money,
                period: LimitPeriod::Daily,
                cap: 50.0,
                warning_threshold: 0.8,
                scope: LimitScope::Global,
                scope_id: None,
                action: LimitAction::Warn,
                enabled: true,
            },
        },
        LimitPreset {
            name: "1 M tokens / month".into(),
            input: LimitInput {
                name: "Monthly token cap".into(),
                metric: LimitMetric::Tokens,
                period: LimitPeriod::Monthly,
                cap: 1_000_000.0,
                warning_threshold: 0.8,
                scope: LimitScope::Global,
                scope_id: None,
                action: LimitAction::Warn,
                enabled: true,
            },
        },
        LimitPreset {
            name: "1 000 requests / day".into(),
            input: LimitInput {
                name: "Daily request cap".into(),
                metric: LimitMetric::Requests,
                period: LimitPeriod::Daily,
                cap: 1000.0,
                warning_threshold: 0.9,
                scope: LimitScope::Global,
                scope_id: None,
                action: LimitAction::Block,
                enabled: true,
            },
        },
        LimitPreset {
            name: "5 hours / day".into(),
            input: LimitInput {
                name: "Daily time cap".into(),
                metric: LimitMetric::TimeSec,
                period: LimitPeriod::Daily,
                cap: 5.0 * 3600.0,
                warning_threshold: 0.8,
                scope: LimitScope::Global,
                scope_id: None,
                action: LimitAction::Warn,
                enabled: true,
            },
        },
        LimitPreset {
            name: "5 hours / week".into(),
            input: LimitInput {
                name: "Weekly time cap".into(),
                metric: LimitMetric::TimeSec,
                period: LimitPeriod::Weekly,
                cap: 5.0 * 3600.0,
                warning_threshold: 0.8,
                scope: LimitScope::Global,
                scope_id: None,
                action: LimitAction::Warn,
                enabled: true,
            },
        },
    ]
}

// ---- license key ----

#[tauri::command]
pub fn get_license_key() -> Result<Option<String>, String> {
    match secrets::get("license") {
        Ok(k) => Ok(Some(k)),
        Err(e) => {
            let lower = e.to_lowercase();
            if lower.contains("noentry") || lower.contains("no entry") {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

#[tauri::command]
pub fn set_license_key(key: String) -> Result<(), String> {
    secrets::set("license", &key)
}

#[tauri::command]
pub fn delete_license_key() -> Result<(), String> {
    secrets::delete("license")
}

// ---- update (GitHub Releases) ----

#[derive(Debug, serde::Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub asset_url: String,
    pub downloaded_path: Option<String>,
}

fn parse_version_triple(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim_start_matches('v');
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/repos/QQSHI13/tokenguard/releases/latest")
        .header("User-Agent", "tokenguard")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API returned {}", resp.status()));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let tag_name = json["tag_name"]
        .as_str()
        .ok_or("missing tag_name in release")?;
    let latest_ver = parse_version_triple(tag_name)
        .ok_or_else(|| format!("invalid release version: {tag_name}"))?;

    let current = app.package_info().version.clone();
    let current_ver = (current.major, current.minor, current.patch);
    if latest_ver <= current_ver {
        return Ok(None);
    }

    // License check: only proceed for valid, device-registered license keys.
    let key = match get_license_key() {
        Ok(Some(k)) => k,
        _ => return Ok(None),
    };
    let device = get_device_fingerprint()?;
    let validate_url = format!(
        "https://tokenguard-license.qingquanshi65.workers.dev/api/license/validate?key={key}&device={device}"
    );
    let valid_resp = reqwest::get(&validate_url)
        .await
        .map_err(|e| e.to_string())?;
    let valid_json: serde_json::Value = valid_resp.json().await.map_err(|e| e.to_string())?;
    if valid_json.get("valid").and_then(|v| v.as_bool()) != Some(true) {
        return Ok(None);
    }

    let assets = json["assets"]
        .as_array()
        .ok_or("missing assets in release")?;
    let os = std::env::consts::OS;
    let asset_url = assets
        .iter()
        .find_map(|a| {
            let name = a["name"].as_str()?;
            let url = a["browser_download_url"].as_str()?;
            match os {
                "windows" if name.contains(".msi") || name.contains(".exe") => {
                    Some(url.to_string())
                }
                "macos" if name.contains(".dmg") => Some(url.to_string()),
                "linux" if name.contains(".AppImage") => Some(url.to_string()),
                _ => None,
            }
        })
        .ok_or("no suitable update asset found")?;

    Ok(Some(UpdateInfo {
        version: tag_name.to_string(),
        asset_url,
        downloaded_path: None,
    }))
}

#[tauri::command]
pub fn get_device_fingerprint() -> Result<String, String> {
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())?;
    let username = whoami::username();
    let os = std::env::consts::OS;
    let input = format!("{hostname}|{username}|{os}");

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    Ok(hex::encode(digest))
}

// ---- model price database ----

#[derive(Debug, serde::Serialize)]
pub struct DefaultPriceMap {
    pub prices: std::collections::HashMap<String, prices::ModelPrice>,
}

#[tauri::command]
pub fn get_default_model_prices() -> DefaultPriceMap {
    DefaultPriceMap {
        prices: prices::get_default_prices(),
    }
}

#[tauri::command]
pub async fn refresh_model_prices_from_url(url: String) -> Result<usize, String> {
    prices::refresh_prices_from_url(&url).await
}

#[tauri::command]
pub fn fill_provider_prices_from_database(
    input: ProviderInput,
) -> Result<Vec<crate::config::ModelMapping>, String> {
    let mut out = input.models;
    for m in &mut out {
        if let Some(p) = prices::lookup_default_price(&m.local) {
            if m.input_cost_per_1k.is_none() {
                m.input_cost_per_1k = Some(p.input_per_1k);
            }
            if m.output_cost_per_1k.is_none() {
                m.output_cost_per_1k = Some(p.output_per_1k);
            }
            if m.cached_input_cost_per_1k.is_none() {
                m.cached_input_cost_per_1k = p.cached_input_per_1k;
            }
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn get_provider_usage(
    state: State<'_, Arc<AppState>>,
    provider_id: i64,
    days: u64,
) -> Result<Vec<db::DailyUsage>, String> {
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    let name: String = conn
        .query_row(
            "SELECT name FROM providers WHERE id = ?1",
            params![provider_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    db::provider_daily_usage(&conn, &name, days).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_project_usage(
    state: State<'_, Arc<AppState>>,
    project_tag: String,
    days: u64,
) -> Result<Vec<db::DailyUsage>, String> {
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    let tag = if project_tag.is_empty() { None } else { Some(project_tag.as_str()) };
    db::project_daily_usage(&conn, tag, days).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_update(app: AppHandle, asset_url: String) -> Result<String, String> {
    let download_dir = app.path().download_dir().map_err(|e| e.to_string())?;
    let filename = reqwest::Url::parse(&asset_url)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|mut s| s.next_back().map(String::from))
        })
        .unwrap_or_else(|| "tokenguard-update".to_string());
    let path = download_dir.join(&filename);

    let client = reqwest::Client::new();
    let resp = client
        .get(&asset_url)
        .header("User-Agent", "tokenguard")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("download failed with {}", resp.status()));
    }

    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| e.to_string())?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| e.to_string())?;
    }

    let path_str = path.to_string_lossy().to_string();
    Ok(path_str)
}
