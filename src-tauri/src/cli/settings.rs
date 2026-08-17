//! Settings management commands for the CLI.

use crate::db;
use crate::state::AppState;
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
    Ok(())
}

pub fn set_port(state: &Arc<AppState>, port: u16) -> Result<()> {
    set_and_update(state, "port", &port.to_string(), |cfg| cfg.port = port)?;
    println!("Set port to {}", port);
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
