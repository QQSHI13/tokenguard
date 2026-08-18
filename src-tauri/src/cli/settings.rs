//! Settings management commands for the CLI.

use crate::backend;
use crate::db;
use crate::state::AppState;
use crate::webhook;
use anyhow::{Context, Result};
use std::sync::Arc;

pub fn show(state: &Arc<AppState>) -> Result<()> {
    let cfg = state.config.read().map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Settings");
    println!("  port: {}", cfg.port);
    println!("  expose_to_lan: {}", cfg.expose_to_lan);
    println!("  budget: {:.2}", cfg.budget);
    println!("  log_retention_days: {}", cfg.log_retention_days);
    println!(
        "  webhook_url: {}",
        cfg.webhook_url.as_deref().unwrap_or("(none)")
    );
    println!("  auto_export_days: {}", cfg.auto_export_days);
    println!(
        "  auto_export_folder: {}",
        cfg.auto_export_folder.as_deref().unwrap_or("(none)")
    );
    println!(
        "  auto_update_interval_minutes: {}",
        cfg.auto_update_interval_minutes
    );
    println!("  beta_channel: {}", cfg.beta_channel);
    println!(
        "  proxy_paused: {}",
        state.paused.load(std::sync::atomic::Ordering::Relaxed)
    );
    Ok(())
}

pub fn set_port(state: &Arc<AppState>, port: u16) -> Result<()> {
    set_and_update(state, "port", &port.to_string(), |cfg| cfg.port = port)?;
    println!("Set port to {} (applied on next start)", port);
    Ok(())
}

pub fn set_expose_to_lan(state: &Arc<AppState>, expose: bool) -> Result<()> {
    let value = if expose { "1" } else { "0" };
    set_and_update(state, "expose_to_lan", value, |cfg| {
        cfg.expose_to_lan = expose
    })?;
    println!("Set expose_to_lan to {}", expose);
    Ok(())
}

pub fn set_budget(state: &Arc<AppState>, budget: f64) -> Result<()> {
    set_and_update(state, "budget", &budget.to_string(), |cfg| {
        cfg.budget = budget
    })?;
    println!("Set budget to {:.2}", budget);
    Ok(())
}

pub fn set_log_retention(state: &Arc<AppState>, days: u32) -> Result<()> {
    set_and_update(state, "log_retention_days", &days.to_string(), |cfg| {
        cfg.log_retention_days = days
    })?;
    println!("Set log_retention_days to {}", days);
    Ok(())
}

pub fn set_webhook(state: &Arc<AppState>, url: String) -> Result<()> {
    set_and_update(state, "webhook_url", &url, |cfg| {
        cfg.webhook_url = Some(url.clone())
    })?;
    println!("Set webhook_url");
    Ok(())
}

pub async fn test_webhook(state: &Arc<AppState>) -> Result<()> {
    let url = {
        let cfg = state.config.read().map_err(|e| anyhow::anyhow!("{e}"))?;
        cfg.webhook_url
            .clone()
            .filter(|s| !s.is_empty())
            .context("webhook_url is not set")?
    };
    webhook::send_test(&state.client, &url)
        .await
        .map_err(|e| anyhow::anyhow!("webhook test failed: {e}"))?;
    println!("Webhook test sent successfully");
    Ok(())
}

pub fn set_auto_export(state: &Arc<AppState>, days: u32, folder: String) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    db::set_setting(&conn, "auto_export_days", &days.to_string()).context("save setting")?;
    db::set_setting(&conn, "auto_export_folder", &folder).context("save setting")?;

    let mut cfg = state.config.write().map_err(|e| anyhow::anyhow!("{e}"))?;
    cfg.auto_export_days = days;
    cfg.auto_export_folder = Some(folder);
    println!("Set auto export to every {} days", days);
    Ok(())
}

pub fn run_auto_export_now(state: &Arc<AppState>) -> Result<()> {
    let path = backend::run_auto_export_now(state).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Exported usage to {}", path);
    Ok(())
}

pub fn cleanup_logs(state: &Arc<AppState>) -> Result<()> {
    let days = state
        .config
        .read()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .log_retention_days;
    if days == 0 {
        println!("Log retention is disabled (0 days); nothing cleaned");
        return Ok(());
    }
    let conn = state.db.get().context("get DB connection")?;
    let n = db::cleanup_old_logs(&conn, days).context("cleanup logs")?;
    let m = db::cleanup_old_audit_events(&conn, days).context("cleanup audit events")?;
    println!(
        "Cleaned {} log rows and {} audit events older than {} days",
        n, m, days
    );
    Ok(())
}

pub fn set_auto_update_interval(state: &Arc<AppState>, minutes: u32) -> Result<()> {
    set_and_update(
        state,
        "auto_update_interval_minutes",
        &minutes.to_string(),
        |cfg| cfg.auto_update_interval_minutes = minutes,
    )?;
    println!("Set auto_update_interval_minutes to {}", minutes);
    Ok(())
}

pub fn set_beta_channel(state: &Arc<AppState>, enabled: bool) -> Result<()> {
    let value = if enabled { "1" } else { "0" };
    set_and_update(state, "beta_channel", value, |cfg| {
        cfg.beta_channel = enabled
    })?;
    println!("Set beta_channel to {}", enabled);
    Ok(())
}

pub fn complete_onboarding(state: &Arc<AppState>) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    db::set_setting(&conn, "onboarding_completed", "1").context("save setting")?;
    println!("Onboarding marked as completed");
    Ok(())
}

fn set_and_update<F>(state: &Arc<AppState>, key: &str, value: &str, update: F) -> Result<()>
where
    F: FnOnce(&mut crate::config::Config),
{
    let conn = state.db.get().context("get DB connection")?;
    db::set_setting(&conn, key, value).context("save setting")?;
    let mut cfg = state.config.write().map_err(|e| anyhow::anyhow!("{e}"))?;
    update(&mut cfg);
    Ok(())
}
