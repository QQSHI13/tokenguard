//! Tauri command handlers exposed to the frontend.

use crate::config::{
    Limit, LimitAction, LimitInput, LimitMetric, LimitPeriod, LimitScope, Project, ProjectInput,
    ProviderDto, ProviderInput,
};
use crate::db::{self, LogRow};
use crate::health::{self, ProviderHealth};
use crate::prices;
use crate::secrets;
use crate::state::AppState;
use rusqlite::params;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::net::IpAddr;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tokio_stream::StreamExt;

#[derive(Debug, serde::Serialize)]
pub struct SettingsDto {
    pub port: u16,
    pub budget: f64,
    pub paused: bool,
    pub proxy_url: String,
    pub provider_count: usize,
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
    Ok(providers
        .into_iter()
        .map(|p| {
            let (api_key_set, key_error) = secrets::status(&p.name);
            let key_created_at = db::get_setting(&conn, &provider_key_setting_name(&p.name));
            ProviderDto {
                provider: p,
                api_key_set,
                key_error,
                key_created_at,
            }
        })
        .collect())
}

fn provider_key_setting_name(provider_name: &str) -> String {
    format!("provider_key_created_at:{provider_name}")
}

fn record_provider_key_time(
    conn: &rusqlite::Connection,
    provider_name: &str,
) -> Result<(), String> {
    db::set_setting(
        conn,
        &provider_key_setting_name(provider_name),
        &chrono::Utc::now().to_rfc3339(),
    )
    .map_err(|e| e.to_string())
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
        {
            let conn = state.inner().db.get().map_err(|e| e.to_string())?;
            record_provider_key_time(&conn, &input.name)?;
            drop(conn);
        }
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
    state.audit("provider_added", &format!("name={}", input.name));
    let (api_key_set, key_error) = secrets::status(&input.name);
    let key_created_at = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        let ts = db::get_setting(&conn, &provider_key_setting_name(&input.name));
        drop(conn);
        ts
    };
    Ok(ProviderDto {
        provider,
        api_key_set,
        key_error,
        key_created_at,
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
    if let Some(ref name) = name {
        let _ = secrets::delete(name);
        {
            let conn = state.inner().db.get().map_err(|e| e.to_string())?;
            let _ = conn.execute(
                "DELETE FROM settings WHERE key = ?1",
                rusqlite::params![provider_key_setting_name(name)],
            );
            drop(conn);
        }
    }
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::delete_provider(&conn, id).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    if let Some(name) = name {
        state.audit("provider_deleted", &format!("name={name}"));
    }
    Ok(())
}

#[tauri::command]
pub fn keyring_selftest() -> String {
    crate::secrets::selftest()
}

#[tauri::command]
pub fn set_provider_key(
    state: State<'_, Arc<AppState>>,
    name: String,
    key: String,
) -> Result<(), String> {
    secrets::set(&name, &key)?;
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    record_provider_key_time(&conn, &name)?;
    Ok(())
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
        if input.clear_key {
            let _ = conn.execute(
                "DELETE FROM settings WHERE key = ?1",
                rusqlite::params![provider_key_setting_name(&input.name)],
            );
            if old_name != input.name {
                let _ = conn.execute(
                    "DELETE FROM settings WHERE key = ?1",
                    rusqlite::params![provider_key_setting_name(&old_name)],
                );
            }
        } else if !input.api_key.is_empty() {
            record_provider_key_time(&conn, &input.name)?;
        } else if old_name != input.name {
            // Copy the original creation timestamp to the new provider name.
            if let Some(ts) = db::get_setting(&conn, &provider_key_setting_name(&old_name)) {
                db::set_setting(&conn, &provider_key_setting_name(&input.name), &ts)
                    .map_err(|e| e.to_string())?;
            }
            let _ = conn.execute(
                "DELETE FROM settings WHERE key = ?1",
                rusqlite::params![provider_key_setting_name(&old_name)],
            );
        }
        drop(conn);
    }
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::update_provider(&conn, id, &input).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    state.audit(
        "provider_updated",
        &format!("id={id} old_name={old_name} new_name={}", input.name),
    );
    let p = {
        let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
        cfg.providers
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or("provider not found after update")?
    };
    let (api_key_set, key_error) = secrets::status(&input.name);
    let key_created_at = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        let ts = db::get_setting(&conn, &provider_key_setting_name(&input.name));
        drop(conn);
        ts
    };
    Ok(ProviderDto {
        provider: p,
        api_key_set,
        key_error,
        key_created_at,
    })
}

fn preferred_lan_ip() -> Option<String> {
    let interfaces = local_ip_address::list_afinet_netifas().ok()?;
    let mut candidates: Vec<(String, IpAddr)> = interfaces
        .into_iter()
        .filter_map(|(name, ip)| {
            if ip.is_loopback() || !matches!(ip, IpAddr::V4(_)) || is_likely_vpn_interface(&name) {
                return None;
            }
            Some((name, ip))
        })
        .collect();

    // Prefer RFC1918 private addresses when multiple non-VPN interfaces exist.
    candidates.sort_by_key(|(_, ip)| !is_private_lan(*ip));

    if let Some((name, ip)) = candidates.first() {
        tracing::info!("preferred LAN IP selected: {ip} on interface {name}");
        return Some(ip.to_string());
    }

    None
}

fn is_likely_vpn_interface(name: &str) -> bool {
    let name = name.to_lowercase();
    [
        // Generic tunnels
        "tun", "tap", "wg", "vpn", "ppp", "ipsec", "utun",
        // Common VPN protocols / products
        "wireguard", "openvpn", "sslvpn", "pptp", "l2tp", "sstp", "ikev2",
        // Commercial VPNs
        "nordlynx", "nordvpn", "expressvpn", "proton", "surfshark", "mullvad",
        "windscribe", "cyberghost", "pia", "private internet access",
        "hotspot shield", "tunnelbear", "perimeter 81", "hide.me", "torguard",
        // Mesh/remote access
        "tailscale", "zerotier", "headscale", "nebula", "netbird",
        // Enterprise VPN clients
        "anyconnect", "cisco anyconnect", "fortissl", "forticlient", "globalprotect",
        "palo alto", "sonicwall", "netextender", "checkpoint", "f5 vpn", "big-ip",
        "juniper", "pulse secure", "citrix", "sophos", "openconnect",
        // Windows adapter names
        "tap-windows", "tap-win32", "wintun", "ndis", "virtual adapter",
    ]
    .iter()
    .any(|p| name.contains(p))
}

fn is_private_lan(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(v4) if v4.is_private())
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
        proxy_url: if cfg.expose_to_lan {
            let host = cfg
                .lan_ip
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(preferred_lan_ip)
                .or_else(|| {
                    local_ip_address::local_ip()
                        .ok()
                        .map(|ip| ip.to_string())
                })
                .or_else(|| {
                    hostname::get()
                        .ok()
                        .and_then(|h| h.into_string().ok())
                })
                .unwrap_or_else(|| "localhost".to_string());
            format!("http://{host}:{}", cfg.port)
        } else {
            format!("http://localhost:{}", cfg.port)
        },
        provider_count: cfg.providers.len(),
        log_bodies: cfg.log_bodies,
        auto_export_days: cfg.auto_export_days,
        auto_export_folder: cfg.auto_export_folder.clone(),
        webhook_url: cfg.webhook_url.clone(),
        auto_start: cfg.auto_start,
        key_rotation_days: state
            .inner()
            .config
            .read()
            .map_err(|e| e.to_string())?
            .key_rotation_days,
        log_retention_days: state
            .inner()
            .config
            .read()
            .map_err(|e| e.to_string())?
            .log_retention_days,
        expose_to_lan: state
            .inner()
            .config
            .read()
            .map_err(|e| e.to_string())?
            .expose_to_lan,
        lan_ip: state
            .inner()
            .config
            .read()
            .map_err(|e| e.to_string())?
            .lan_ip
            .clone(),
    })
}

#[tauri::command]
pub fn backup_database(state: State<'_, Arc<AppState>>, target_path: String) -> Result<(), String> {
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

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AutoExportInput {
    pub days: u32,
    pub folder: String,
}

#[tauri::command]
pub fn set_auto_export(
    state: State<'_, Arc<AppState>>,
    input: AutoExportInput,
) -> Result<(), String> {
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "auto_export_days", &input.days.to_string())
            .map_err(|e| e.to_string())?;
        db::set_setting(&conn, "auto_export_folder", &input.folder).map_err(|e| e.to_string())?;
        drop(conn);
    }
    let mut cfg = state.inner().config.write().map_err(|e| e.to_string())?;
    cfg.auto_export_days = input.days;
    cfg.auto_export_folder = Some(input.folder);
    Ok(())
}

fn last_auto_export_at(
    conn: &rusqlite::Connection,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    match db::get_setting(conn, "last_auto_export_at") {
        Some(v) => chrono::DateTime::parse_from_rfc3339(&v)
            .map(|dt| Some(dt.with_timezone(&chrono::Utc)))
            .map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

pub fn is_auto_export_due(state: &Arc<AppState>) -> Result<bool, String> {
    let cfg = state.config.read().map_err(|e| e.to_string())?;
    if cfg.auto_export_days == 0 {
        return Ok(false);
    }
    let folder = match &cfg.auto_export_folder {
        Some(f) if !f.is_empty() => f,
        _ => return Ok(false),
    };
    let folder_path = std::path::PathBuf::from(folder);
    if !folder_path.exists() {
        return Ok(false);
    }
    let conn = state.db.get().map_err(|e| e.to_string())?;
    let last = last_auto_export_at(&conn)?;
    let due = match last {
        Some(t) => {
            let interval = chrono::Duration::days(cfg.auto_export_days as i64);
            chrono::Utc::now() - t >= interval
        }
        None => true,
    };
    Ok(due)
}

pub fn run_auto_export_now(state: &Arc<AppState>) -> Result<String, String> {
    let cfg = state.config.read().map_err(|e| e.to_string())?;
    let folder = cfg
        .auto_export_folder
        .as_ref()
        .ok_or("auto export folder not set")?;
    if folder.is_empty() {
        return Err("auto export folder not set".into());
    }
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;

    let rows = {
        let conn = state.db.get().map_err(|e| e.to_string())?;
        db::list_logs(&conn, 100_000, None).map_err(|e| e.to_string())?
    };
    let filename = format!(
        "tokenguard-usage-{}.csv",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let path = std::path::PathBuf::from(folder).join(&filename);

    let mut w =
        String::from("timestamp,provider,model,prompt_tokens,completion_tokens,cost,project\n");
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
    std::fs::write(&path, w).map_err(|e| e.to_string())?;

    let conn = state.db.get().map_err(|e| e.to_string())?;
    db::set_setting(
        &conn,
        "last_auto_export_at",
        &chrono::Utc::now().to_rfc3339(),
    )
    .map_err(|e| e.to_string())?;

    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn maybe_run_auto_export(state: State<'_, Arc<AppState>>) -> Result<Option<String>, String> {
    if !is_auto_export_due(&state)? {
        return Ok(None);
    }
    run_auto_export_now(&state).map(Some)
}

#[tauri::command]
pub fn run_auto_export_now_cmd(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    run_auto_export_now(&state)
}

#[tauri::command]
pub fn set_webhook_url(state: State<'_, Arc<AppState>>, url: Option<String>) -> Result<(), String> {
    let value = url.unwrap_or_default();
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "webhook_url", &value).map_err(|e| e.to_string())?;
        drop(conn);
    }
    state
        .inner()
        .config
        .write()
        .map_err(|e| e.to_string())?
        .webhook_url = Some(value).filter(|s| !s.is_empty());
    Ok(())
}

#[tauri::command]
pub async fn test_webhook(state: State<'_, Arc<AppState>>, url: String) -> Result<(), String> {
    crate::webhook::send_test(&state.inner().client, &url).await
}

#[tauri::command]
pub fn get_auto_start(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cleanup_logs_now(state: State<'_, Arc<AppState>>) -> Result<usize, String> {
    let days = state
        .inner()
        .config
        .read()
        .map_err(|e| e.to_string())?
        .log_retention_days;
    if days == 0 {
        return Ok(0);
    }
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    db::cleanup_old_logs(&conn, days).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_key_rotation_days(state: State<'_, Arc<AppState>>, days: u32) -> Result<u32, String> {
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "key_rotation_days", &days.to_string())
            .map_err(|e| e.to_string())?;
        drop(conn);
    }
    state
        .inner()
        .config
        .write()
        .map_err(|e| e.to_string())?
        .key_rotation_days = days;
    Ok(days)
}

#[tauri::command]
pub fn set_auto_start(
    state: State<'_, Arc<AppState>>,
    app: AppHandle,
    enabled: bool,
) -> Result<bool, String> {
    if enabled {
        app.autolaunch().enable().map_err(|e| e.to_string())?;
    } else {
        app.autolaunch().disable().map_err(|e| e.to_string())?;
    }
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "auto_start", if enabled { "1" } else { "0" })
            .map_err(|e| e.to_string())?;
        drop(conn);
    }
    state
        .inner()
        .config
        .write()
        .map_err(|e| e.to_string())?
        .auto_start = enabled;
    Ok(enabled)
}

#[tauri::command]
pub fn set_log_retention_days(state: State<'_, Arc<AppState>>, days: u32) -> Result<u32, String> {
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "log_retention_days", &days.to_string())
            .map_err(|e| e.to_string())?;
        drop(conn);
    }
    state
        .inner()
        .config
        .write()
        .map_err(|e| e.to_string())?
        .log_retention_days = days;
    Ok(days)
}

#[tauri::command]
pub fn set_log_bodies(state: State<'_, Arc<AppState>>, enabled: bool) -> Result<(), String> {
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "log_bodies", if enabled { "1" } else { "0" })
            .map_err(|e| e.to_string())?;
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
pub fn set_expose_to_lan(state: State<'_, Arc<AppState>>, enabled: bool) -> Result<(), String> {
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "expose_to_lan", if enabled { "1" } else { "0" })
            .map_err(|e| e.to_string())?;
        drop(conn);
    }
    state
        .inner()
        .config
        .write()
        .map_err(|e| e.to_string())?
        .expose_to_lan = enabled;
    Ok(())
}

#[tauri::command]
pub fn set_lan_ip(state: State<'_, Arc<AppState>>, ip: Option<String>) -> Result<(), String> {
    let value = ip.as_deref().unwrap_or("").trim();
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::set_setting(&conn, "lan_ip", value).map_err(|e| e.to_string())?;
        drop(conn);
    }
    state
        .inner()
        .config
        .write()
        .map_err(|e| e.to_string())?
        .lan_ip = if value.is_empty() { None } else { Some(value.to_string()) };
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
pub async fn replay_request(
    state: State<'_, Arc<AppState>>,
    log_id: i64,
) -> Result<String, String> {
    let log = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::get_log(&conn, log_id)
            .map_err(|e| e.to_string())?
            .ok_or("log not found")?
    };
    let request_body = log.request_body.ok_or("request body was not logged")?;
    if request_body.is_empty() {
        return Err("request body was not logged".into());
    }

    let provider = {
        let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
        cfg.providers
            .iter()
            .find(|p| p.name == log.provider)
            .cloned()
            .ok_or("provider no longer exists")?
    };
    let api_key = crate::secrets::get(&provider.name)?;

    // Infer the original endpoint from the provider format and logged body.
    let path = match provider.format {
        crate::config::ProviderFormat::Anthropic => "/v1/messages".to_string(),
        crate::config::ProviderFormat::OpenAI => {
            let body_json: serde_json::Value =
                serde_json::from_str(&request_body).unwrap_or(serde_json::Value::Null);
            if body_json.get("messages").is_some() {
                "/v1/chat/completions".to_string()
            } else if body_json.get("prompt").is_some() {
                "/v1/completions".to_string()
            } else {
                "/v1/chat/completions".to_string()
            }
        }
        crate::config::ProviderFormat::Google => {
            return Err("replay is not supported for Google Gemini native API".into());
        }
    };

    let base = provider.base_url.trim_end_matches('/');
    let url = format!("{base}{path}");
    let mut req = state.inner().client.post(&url);
    req = match provider.auth {
        crate::config::AuthScheme::Bearer => req.bearer_auth(&api_key),
        crate::config::AuthScheme::XApiKey => req
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01"),
        crate::config::AuthScheme::ApiKey => req.header("api-key", &api_key),
        crate::config::AuthScheme::XGoogApiKey => req.header("x-goog-api-key", &api_key),
    };
    for (k, v) in &provider.extra_headers {
        req = req.header(k, v);
    }

    let resp = req
        .body(request_body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok(format!("HTTP {status}\n{body}"))
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

/// Export audit events as CSV or JSON text.
#[tauri::command]
pub fn export_audit_logs(
    state: State<'_, Arc<AppState>>,
    format: String,
    days: u32,
) -> Result<String, String> {
    let rows = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::list_audit_events(&conn, days, 100_000).map_err(|e| e.to_string())?
    };
    match format.as_str() {
        "json" => Ok(serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?),
        _ => {
            let mut w = String::from("timestamp,event_type,details\n");
            for r in rows.iter().rev() {
                w.push_str(&format!(
                    "{},{},{}\n",
                    r.ts,
                    csv_escape(&r.event_type),
                    csv_escape(&r.details),
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
    let url = match provider.format {
        crate::config::ProviderFormat::Google => format!("{base}/v1beta/models"),
        _ => format!("{base}/v1/models"),
    };
    let mut req = state.inner().client.get(&url);
    req = match provider.auth {
        crate::config::AuthScheme::Bearer => req.bearer_auth(&api_key),
        crate::config::AuthScheme::XApiKey => req
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01"),
        crate::config::AuthScheme::ApiKey => req.header("api-key", &api_key),
        crate::config::AuthScheme::XGoogApiKey => req.header("x-goog-api-key", &api_key),
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

    // Google returns { "models": [{ "name": "models/gemini-..." }] };
    // OpenAI/Anthropic return { "data": [{ "id": "..." }] }.
    let mut names: Vec<String> = match provider.format {
        crate::config::ProviderFormat::Google => v
            .get("models")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        m.get("name")
                            .and_then(|x| x.as_str())
                            .map(|s| s.strip_prefix("models/").unwrap_or(s).to_string())
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => v
            .get("data")
            .and_then(|d| d.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
    };
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
        db::insert_project(&conn, &input).map_err(|e| e.to_string())?
    };
    // reload config
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    state.audit("project_added", &format!("name={}", input.name));
    Ok(Project {
        id,
        name: input.name,
        label_key: input.label_key,
        budget: input.budget,
        budget_period: input.budget_period,
        budget_action: input.budget_action,
    })
}

#[tauri::command]
pub fn delete_project(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    let name = {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        let projects = db::list_projects(&conn).map_err(|e| e.to_string())?;
        let name = projects.iter().find(|p| p.id == id).map(|p| p.name.clone());
        drop(conn);
        name
    };
    {
        let conn = state.inner().db.get().map_err(|e| e.to_string())?;
        db::delete_project(&conn, id).map_err(|e| e.to_string())?;
        let new_cfg = db::load_config(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        *state.inner().config.write().map_err(|e| e.to_string())? = new_cfg;
    }
    if let Some(name) = name {
        state.audit("project_deleted", &format!("name={name}"));
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
                active_hours_start: None,
                active_hours_end: None,
                active_days: 0b1111111,
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
                active_hours_start: None,
                active_hours_end: None,
                active_days: 0b1111111,
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
                active_hours_start: None,
                active_hours_end: None,
                active_days: 0b1111111,
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
                active_hours_start: None,
                active_hours_end: None,
                active_days: 0b1111111,
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
                active_hours_start: None,
                active_hours_end: None,
                active_days: 0b1111111,
            },
        },
    ]
}

// ---- provider health ----

#[tauri::command]
pub async fn check_provider_health(
    state: State<'_, Arc<AppState>>,
    id: i64,
) -> Result<ProviderHealth, String> {
    let provider = {
        let cfg = state.inner().config.read().map_err(|e| e.to_string())?;
        cfg.providers
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or("provider not found")?
    };
    let health = health::check_provider(&state.inner().client, &provider).await;
    state
        .inner()
        .provider_health_cache()
        .lock()
        .map(|mut c| c.insert(id, health.clone()))
        .map_err(|e| e.to_string())?;
    Ok(health)
}

#[tauri::command]
pub fn get_provider_healths(
    state: State<'_, Arc<AppState>>,
) -> Result<std::collections::HashMap<i64, ProviderHealth>, String> {
    Ok(state.inner().all_provider_health())
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

#[derive(Debug, serde::Serialize)]
pub struct RegisteredDevice {
    pub fingerprint: String,
    pub registered_at: String,
    pub current: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct DeviceListDto {
    pub devices: Vec<RegisteredDevice>,
    pub max_devices: usize,
}

#[tauri::command]
pub async fn get_registered_devices(key: String, device: String) -> Result<DeviceListDto, String> {
    let url = format!(
        "https://tokenguard-license.qingquanshi65.workers.dev/api/license/devices?key={key}&device={device}"
    );
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("failed to contact license server: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("license server returned {}", resp.status()));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let max_devices = json["maxDevices"].as_u64().unwrap_or(2) as usize;
    let devices = json["devices"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    Some(RegisteredDevice {
                        fingerprint: d["fingerprint"].as_str()?.to_string(),
                        registered_at: d["registeredAt"].as_str()?.to_string(),
                        current: d["current"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(DeviceListDto {
        devices,
        max_devices,
    })
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
    let tag = if project_tag.is_empty() {
        None
    } else {
        Some(project_tag.as_str())
    };
    db::project_daily_usage(&conn, tag, days).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_monthly_usage(
    state: State<'_, Arc<AppState>>,
    months: Option<u32>,
) -> Result<Vec<db::MonthlyUsage>, String> {
    let conn = state.inner().db.get().map_err(|e| e.to_string())?;
    db::monthly_usage(&conn, months.unwrap_or(12)).map_err(|e| e.to_string())
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

#[tauri::command]
pub fn install_update(path: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(path);
    if !path.exists() {
        return Err("installer not found".into());
    }

    let os = std::env::consts::OS;
    let path_str = path.to_string_lossy().to_string();

    match os {
        "windows" => {
            // Launch the MSI installer. For a silent install use:
            //   msiexec /i "path" /qn
            // We use /passive so the user sees progress but doesn't need to interact.
            std::process::Command::new("msiexec")
                .args(["/i", &path_str, "/passive"])
                .spawn()
                .map_err(|e| format!("failed to start installer: {e}"))?;
        }
        "macos" => {
            // Open the DMG; user drags the app to Applications.
            std::process::Command::new("open")
                .arg(&path_str)
                .spawn()
                .map_err(|e| format!("failed to open DMG: {e}"))?;
        }
        "linux" => {
            // Make AppImage executable and run it.
            #[cfg(unix)]
            {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
            }
            std::process::Command::new(&path_str)
                .spawn()
                .map_err(|e| format!("failed to run AppImage: {e}"))?;
        }
        _ => return Err("unsupported OS".into()),
    }

    Ok(())
}
