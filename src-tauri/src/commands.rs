//! Tauri command handlers exposed to the frontend.

use crate::config::{Project, ProjectInput, ProviderDto, ProviderInput};
use crate::db::{self, LogRow};
use crate::secrets;
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

#[derive(Debug, serde::Serialize)]
pub struct SettingsDto {
    pub port: u16,
    pub budget: f64,
    pub paused: bool,
    pub proxy_url: String,
    pub provider_count: usize,
    pub accurate_streaming: bool,
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

#[tauri::command]
pub fn list_providers(state: State<'_, Arc<AppState>>) -> Result<Vec<ProviderDto>, String> {
    let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
    let providers =
        db::list_providers(&conn).map_err(|e| e.to_string())?;
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

#[tauri::command]
pub fn add_provider(
    state: State<'_, Arc<AppState>>,
    input: ProviderInput,
) -> Result<ProviderDto, String> {
    if !input.api_key.is_empty() {
        secrets::set(&input.name, &input.api_key)?;
    }
    let provider = {
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
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
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
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
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
        let providers = db::list_providers(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        providers.iter().find(|p| p.id == id).map(|p| p.name.clone())
    };
    if let Some(name) = name {
        let _ = secrets::delete(&name);
    }
    {
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
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
    // current name, to handle rename + key move
    let old_name = {
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
        let providers = db::list_providers(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        providers
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .ok_or("provider not found")?
    };
    // key handling: new key wins; otherwise move existing key on rename
    if !input.api_key.is_empty() {
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
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
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
    let cfg = state
        .inner()
        .config
        .read()
        .map_err(|e| e.to_string())?;
    Ok(SettingsDto {
        port: cfg.port,
        budget: cfg.budget,
        paused: state.inner().paused.load(std::sync::atomic::Ordering::Relaxed),
        proxy_url: format!("http://localhost:{}", cfg.port),
        provider_count: cfg.providers.len(),
        accurate_streaming: cfg.accurate_streaming,
    })
}

#[tauri::command]
pub fn set_budget(state: State<'_, Arc<AppState>>, budget: f64) -> Result<(), String> {
    {
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
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
    let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
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
    let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
    db::list_logs(&conn, limit.unwrap_or(5000), days).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_models(state: State<'_, Arc<AppState>>) -> Result<Vec<ModelInfo>, String> {
    let cfg = state
        .inner()
        .config
        .read()
        .map_err(|e| e.to_string())?;
    let mut out: Vec<ModelInfo> = Vec::new();
    for p in &cfg.providers {
        for m in &p.models {
            out.push(ModelInfo {
                id: m.clone(),
                provider: p.name.clone(),
            });
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn set_accurate_streaming(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
) -> Result<(), String> {
    {
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "accurate_streaming", if enabled { "true" } else { "false" })
            .map_err(|e| e.to_string())?;
        drop(conn);
    }
    state
        .inner()
        .config
        .write()
        .map_err(|e| e.to_string())?
        .accurate_streaming = enabled;
    Ok(())
}

/// Export logs as CSV or JSON text (frontend triggers a Blob download).
#[tauri::command]
pub fn export_logs(
    state: State<'_, Arc<AppState>>,
    format: String,
    days: Option<u64>,
) -> Result<String, String> {
    let rows = {
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
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
) -> Result<Vec<String>, String> {
    let provider = {
        let cfg = state
            .inner()
            .config
            .read()
            .map_err(|e| e.to_string())?;
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
        crate::config::AuthScheme::XApiKey => {
            req.header("x-api-key", &api_key).header("anthropic-version", "2023-06-01")
        }
        crate::config::AuthScheme::ApiKey => req.header("api-key", &api_key),
    };
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("provider returned {status}: {}", &body[..body.len().min(200)]));
    }
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let mut models: Vec<String> = v
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    {
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
        db::update_provider_models(&conn, id, &models).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state
            .inner()
            .config
            .write()
            .map_err(|e| e.to_string())? = new_cfg;
    }
    Ok(models)
}

// ---- projects ----

#[tauri::command]
pub fn list_projects(state: State<'_, Arc<AppState>>) -> Result<Vec<Project>, String> {
    let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
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
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
        db::insert_project(&conn, &input.name, &input.label_key).map_err(|e| e.to_string())?
    };
    // reload config
    {
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
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
        let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
        db::delete_project(&conn, id).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    Ok(())
}
