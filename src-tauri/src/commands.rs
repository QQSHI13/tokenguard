//! Tauri command handlers exposed to the frontend.

use crate::config::{ProviderDto, ProviderInput};
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
        .map(|p| ProviderDto {
            api_key_set: secrets::has(&p.name),
            provider: p,
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
    Ok(ProviderDto {
        api_key_set: !input.api_key.is_empty(),
        provider,
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
pub fn set_provider_key(name: String, key: String) -> Result<(), String> {
    secrets::set(&name, &key)
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
pub fn get_logs(state: State<'_, Arc<AppState>>, limit: Option<u64>) -> Result<Vec<LogRow>, String> {
    let conn = state.inner().db.lock().map_err(|e| e.to_string())?;
    db::list_logs(&conn, limit.unwrap_or(100)).map_err(|e| e.to_string())
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
