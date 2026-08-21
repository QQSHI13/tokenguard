//! Team sharing over Tailscale: let every device on your tailnet use the
//! gateway. One member hosts Token Guard, everyone points their clients at
//! the host's Tailscale address. See `crate::share` for the two modes
//! (direct tailnet bind, or `tailscale serve` fallback for userspace/WSL).

use crate::share::{self, ShareMode};
use crate::state::AppState;
use anyhow::Result;
use clap::Subcommand;
use std::sync::Arc;

#[derive(Subcommand, Debug)]
pub enum ShareCommands {
    /// Enable sharing over Tailscale (direct bind, or `tailscale serve` fallback).
    On,
    /// Disable sharing over Tailscale (loopback only).
    Off,
    /// Show sharing state, mode, and the team endpoint.
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
    let endpoint = share::enable(state)?;
    println!("Tailscale sharing enabled");
    print_share_info(state, &endpoint);
    Ok(())
}

fn disable(state: &Arc<AppState>) -> Result<()> {
    share::disable(state)?;
    println!("Tailscale sharing disabled (loopback only)");
    Ok(())
}

fn status(state: &Arc<AppState>) -> Result<()> {
    let cfg = state.config.read().map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("share_over_tailscale: {}", cfg.share_over_tailscale);
    println!("expose_to_lan: {}", cfg.expose_to_lan);
    let info = share::info(state);
    match info.mode {
        ShareMode::Direct(ip) => println!("mode: direct (tailnet IP {ip})"),
        ShareMode::Serve(fqdn) => {
            println!("mode: tailscale serve ({fqdn})");
            println!(
                "serve route: {}",
                if share::serve_route_active() {
                    "active"
                } else {
                    "not configured"
                }
            );
        }
        ShareMode::Unavailable => {
            println!("mode: unavailable — install Tailscale and sign in to share")
        }
    }
    if !info.endpoint.is_empty() {
        println!("endpoint: {}", info.endpoint);
    }
    Ok(())
}

fn print_share_info(state: &Arc<AppState>, endpoint: &str) {
    let info = share::info(state);
    println!();
    println!("Team members can now use Token Guard on this tailnet:");
    println!("  OPENAI_BASE_URL={endpoint}");
    println!("  OPENAI_API_KEY=<a project label key from this machine>");
    println!();
    println!("Every teammate's key must be a project label key (tg_...) created");
    println!("on this machine. Requests are tagged to that project and count");
    println!("against its budget and limits.");
    match info.mode {
        ShareMode::Direct(_) => {
            println!("The gateway binds ONLY to the tailnet IP and loopback — it is not");
            println!("exposed to the LAN or internet.");
        }
        ShareMode::Serve(_) => {
            println!("Exposure goes through `tailscale serve` at the /tg path — the");
            println!("gateway itself stays on loopback. Only tailnet devices can reach it.");
        }
        ShareMode::Unavailable => {}
    }
}
