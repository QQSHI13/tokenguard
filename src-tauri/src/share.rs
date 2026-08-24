//! Tailscale team sharing, two modes:
//!
//! - **Direct** (default): bind the gateway to the host's Tailscale IP
//!   (100.64.0.0/10) plus loopback. Used when a TUN interface exists.
//! - **Serve** (fallback): used when Tailscale runs in userspace networking
//!   mode (common in WSL) and no 100.x interface exists. The gateway stays on
//!   loopback and a `tailscale serve` route with the `/tg` path prefix exposes
//!   it to the tailnet. The prefix keeps the route independent of whatever
//!   else is served on the same host (e.g. OpenClaw's root-path route).

use crate::db;
use crate::proxy::server::tailscale_ipv4;
use crate::state::AppState;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;
use std::sync::Arc;

/// Path prefix used for the serve-mode route so it never collides with a
/// root-path route owned by another service (OpenClaw's gateway claims `/`).
pub const SERVE_PATH: &str = "/tg";

#[derive(Debug, Clone, PartialEq)]
pub enum ShareMode {
    /// Bind directly to the tailnet IP.
    Direct(std::net::IpAddr),
    /// Tailscale userspace networking: expose via `tailscale serve`.
    Serve(String),
    /// Tailscale is not available at all.
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct ShareInfo {
    pub mode: ShareMode,
    /// Base URL (without trailing `/v1`) teammates point their clients at.
    pub endpoint: String,
}

/// Current sharing state for display.
pub fn info(state: &Arc<AppState>) -> ShareInfo {
    let port = state.config.read().map(|c| c.port).unwrap_or(3742);
    let mode = detect_mode();
    let endpoint = endpoint(&mode, port);
    ShareInfo { mode, endpoint }
}

/// Endpoint for a given mode and port.
pub fn endpoint(mode: &ShareMode, port: u16) -> String {
    match mode {
        ShareMode::Direct(ip) => format!("http://{ip}:{port}"),
        ShareMode::Serve(fqdn) => format!("https://{fqdn}{SERVE_PATH}"),
        ShareMode::Unavailable => String::new(),
    }
}

/// Detect how this machine can reach the tailnet. Prefers a real tailnet
/// interface (TUN); falls back to `tailscale status --json` for userspace
/// networking (WSL), where the FQDN is resolved by Tailscale itself.
pub fn detect_mode() -> ShareMode {
    if let Some(ip) = tailscale_ipv4() {
        return ShareMode::Direct(ip);
    }
    match tailscale_self_dns_name() {
        Some(fqdn) => ShareMode::Serve(fqdn),
        None => ShareMode::Unavailable,
    }
}

#[derive(Deserialize)]
struct SelfNode {
    #[serde(rename = "DNSName")]
    dns_name: Option<String>,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Vec<String>,
}

#[derive(Deserialize)]
struct TailscaleStatus {
    #[serde(rename = "Self")]
    self_node: Option<SelfNode>,
}

/// Pull the self node's name out of `tailscale status --json` output, preferring
/// the MagicDNS name and falling back to the first tailnet IP.
fn parse_self_dns_name(stdout: &[u8]) -> Option<String> {
    let status: TailscaleStatus = serde_json::from_slice(stdout).ok()?;
    let node = status.self_node?;
    node.dns_name
        .map(|n| n.trim_end_matches('.').to_string())
        .filter(|n| !n.is_empty())
        .or_else(|| node.tailscale_ips.first().cloned())
}

/// `tailscale status --json` → the self node's DNS name (e.g.
/// `desktop-host.tailXXXX.ts.net.`). MagicDNS does not resolve inside WSL
/// userspace networking, so the name comes from the daemon, not the resolver.
fn tailscale_self_dns_name() -> Option<String> {
    let out = Command::new("tailscale")
        .arg("status")
        .arg("--json")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_self_dns_name(&out.stdout)
}

/// Enable sharing. Persists the setting, and in serve mode claims the
/// `tailscale serve` route for `/tg`. Returns the team endpoint.
pub fn enable(state: &Arc<AppState>) -> Result<String> {
    let port = state
        .config
        .read()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .port;
    match detect_mode() {
        ShareMode::Direct(ip) => {
            set_setting(state, true)?;
            Ok(format!("http://{ip}:{port}/v1"))
        }
        ShareMode::Serve(fqdn) => {
            ensure_serve_route(port)?;
            set_setting(state, true)?;
            Ok(format!("https://{fqdn}{SERVE_PATH}/v1"))
        }
        ShareMode::Unavailable => Err(anyhow::anyhow!(
            "no Tailscale interface (100.x.x.x) found and `tailscale` is not available — \
             install Tailscale and sign in first"
        )),
    }
}

/// Disable sharing. Removes a serve route we own, then clears the setting.
pub fn disable(state: &Arc<AppState>) -> Result<()> {
    set_setting(state, false)?;
    if matches!(detect_mode(), ShareMode::Serve(_)) {
        let _ = Command::new("tailscale")
            .args(["serve", "--https=443", "--set-path", SERVE_PATH, "off"])
            .status();
    }
    Ok(())
}

/// True if a serve route for our path prefix is currently configured.
pub fn serve_route_active() -> bool {
    let Ok(out) = Command::new("tailscale").args(["serve", "status"]).output() else {
        return false;
    };
    out.status.success() && String::from_utf8_lossy(&out.stdout).contains(SERVE_PATH)
}

/// Claim (or re-claim) the `tailscale serve` route for our path prefix, in
/// background mode so the route survives the CLI process. Polls `serve status`
/// to confirm the claim before returning.
fn ensure_serve_route(port: u16) -> Result<()> {
    if serve_route_active() {
        return Ok(());
    }
    let status = Command::new("tailscale")
        .args([
            "serve",
            "--yes",
            "--bg=true",
            "--set-path",
            SERVE_PATH,
            &port.to_string(),
        ])
        .status()
        .context("failed to run `tailscale serve` — is the tailscale CLI installed?")?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "`tailscale serve` failed — check `tailscale serve status`"
        ));
    }
    // The daemon applies the config asynchronously; give it a moment.
    for _ in 0..10 {
        if serve_route_active() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Err(anyhow::anyhow!(
        "`tailscale serve` accepted the config but the route is not visible yet — \
         check `tailscale serve status`"
    ))
}

fn set_setting(state: &Arc<AppState>, enabled: bool) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    db::set_setting(
        &conn,
        "share_over_tailscale",
        if enabled { "1" } else { "0" },
    )
    .context("save setting")?;
    let mut cfg = state.config.write().map_err(|e| anyhow::anyhow!("{e}"))?;
    cfg.share_over_tailscale = enabled;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(dns: Option<&str>, ip: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "Self": {
                "DNSName": dns,
                "TailscaleIPs": ip.map(|v| vec![v]),
            }
        })
    }

    /// Exercises the same parser the production path uses, fed the bytes
    /// `tailscale status --json` would have written to stdout.
    fn parse(v: serde_json::Value) -> Option<String> {
        parse_self_dns_name(v.to_string().as_bytes())
    }

    #[test]
    fn parses_dns_name_and_strips_trailing_dot() {
        let v = node(
            Some("desktop-host.tailaf9e5c.ts.net."),
            Some("100.100.181.31"),
        );
        assert_eq!(parse(v), Some("desktop-host.tailaf9e5c.ts.net".to_string()));
    }

    #[test]
    fn falls_back_to_tailscale_ip_without_dns_name() {
        let v = node(None, Some("100.64.81.26"));
        assert_eq!(parse(v), Some("100.64.81.26".to_string()));
    }

    #[test]
    fn handles_missing_self_node() {
        assert_eq!(parse(serde_json::json!({})), None);
    }

    #[test]
    fn handles_non_json_output() {
        // e.g. the CLI printing a human-readable error on stdout.
        assert_eq!(parse_self_dns_name(b"tailscale: not logged in"), None);
    }

    #[test]
    fn falls_back_to_ip_when_dns_name_is_blank() {
        let v = node(Some(""), Some("100.64.81.26"));
        assert_eq!(parse(v), Some("100.64.81.26".to_string()));
    }

    #[test]
    fn serve_endpoint_uses_path_prefix() {
        assert_eq!(
            endpoint(&ShareMode::Serve("h.tail.ts.net".into()), 3742),
            "https://h.tail.ts.net/tg"
        );
        assert_eq!(
            endpoint(&ShareMode::Direct("100.100.181.31".parse().unwrap()), 3742),
            "http://100.100.181.31:3742"
        );
        assert_eq!(endpoint(&ShareMode::Unavailable, 3742), "");
    }
}
