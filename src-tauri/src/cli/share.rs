//! Team sharing over Tailscale: let every device on your tailnet use the
//! gateway. One member hosts Token Guard, everyone points their clients at
//! the host's Tailscale IP.

use crate::db;
use crate::proxy::server::tailscale_ipv4;
use crate::state::AppState;
use anyhow::{Context, Result};
use clap::Subcommand;
use std::sync::Arc;

#[derive(Subcommand, Debug)]
pub enum ShareCommands {
    /// Enable sharing over Tailscale (binds to the tailnet IP + loopback).
    On,
    /// Disable sharing over Tailscale (loopback only).
    Off,
    /// Show sharing state, the tailnet IP, and the share URL.
    Status,
}

pub fn run(state: &Arc<AppState>, command: ShareCommands) -> Result<()> {
    match command {
        ShareCommands::On => enable(state),
        ShareCommands::Off => disable(state),
        ShareCommands::Status => status(state),
    }
}

fn enable(state: &Arc<AppState>) -> Result<()> {
    let ip = tailscale_ipv4().context(
        "no Tailscale interface (100.x.x.x) found — install Tailscale and sign in first",
    )?;
    let port = state
        .config
        .read()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .port;
    set_setting(state, true)?;
    println!("Tailscale sharing enabled");
    print_share_info(&ip.to_string(), port);
    Ok(())
}

fn disable(state: &Arc<AppState>) -> Result<()> {
    set_setting(state, false)?;
    println!("Tailscale sharing disabled (loopback only)");
    Ok(())
}

fn status(state: &Arc<AppState>) -> Result<()> {
    let cfg = state.config.read().map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("share_over_tailscale: {}", cfg.share_over_tailscale);
    println!("expose_to_lan: {}", cfg.expose_to_lan);
    match tailscale_ipv4() {
        Some(ip) => print_share_info(&ip.to_string(), cfg.port),
        None => println!("tailscale: not detected — install Tailscale and sign in to share"),
    }
    Ok(())
}

fn print_share_info(ip: &str, port: u16) {
    println!();
    println!("Team members can now use Token Guard on this tailnet:");
    println!("  OPENAI_BASE_URL=http://{ip}:{port}/v1");
    println!("  OPENAI_API_KEY=<a project label key from this machine>");
    println!();
    println!("Every teammate's key must be a project label key (tg_...) created");
    println!("on this machine. Requests are tagged to that project and count");
    println!("against its budget and limits. The gateway binds ONLY to the");
    println!("tailnet IP and loopback — it is not exposed to the LAN or internet.");
}

fn set_setting(state: &Arc<AppState>, enabled: bool) -> Result<()> {
    let value = if enabled { "1" } else { "0" };
    let conn = state.db.get().context("get DB connection")?;
    db::set_setting(&conn, "share_over_tailscale", value).context("save setting")?;
    let mut cfg = state.config.write().map_err(|e| anyhow::anyhow!("{e}"))?;
    cfg.share_over_tailscale = enabled;
    Ok(())
}
