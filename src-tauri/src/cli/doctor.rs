//! `tokenguard doctor` — one-shot diagnostics for the gateway environment.
//!
//! Checks the pieces that most commonly block a first run: database,
//! keychain (provider keys), port, Tailscale sharing, and configuration.

use crate::secrets;
use crate::share::{self, ShareMode};
use crate::state::AppState;
use anyhow::Result;
use std::net::TcpListener;
use std::sync::Arc;

#[derive(PartialEq)]
enum Verdict {
    Ok,
    Warn,
    Fail,
}

impl Verdict {
    fn tag(&self) -> &'static str {
        match self {
            Verdict::Ok => "ok  ",
            Verdict::Warn => "warn",
            Verdict::Fail => "FAIL",
        }
    }
}

fn report(verdict: Verdict, what: &str, detail: &str) -> Verdict {
    println!("  [{}] {what}: {detail}", verdict.tag());
    verdict
}

/// Run all checks; returns the number of hard failures.
pub fn run(state: &Arc<AppState>) -> Result<usize> {
    let cfg = state.config.read().map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Token Guard doctor");

    let mut failures = 0usize;

    // Database
    let db_ok = state.db.get().is_ok();
    let v = report(
        if db_ok { Verdict::Ok } else { Verdict::Fail },
        "database",
        &state.db_path.display().to_string(),
    );
    if v == Verdict::Fail {
        failures += 1;
    }

    // Keychain — provider API keys live here. A locked keyring blocks adding
    // providers but does not break an already-configured gateway.
    let selftest = secrets::selftest();
    let keychain_ok = !selftest.contains("FAILED");
    let v = report(
        if keychain_ok {
            Verdict::Ok
        } else {
            Verdict::Fail
        },
        "keychain",
        if keychain_ok {
            "write/read/delete OK"
        } else {
            "unavailable — unlock your system keyring, or provider keys cannot be stored"
        },
    );
    if v == Verdict::Fail {
        failures += 1;
    }

    // Port
    let port = cfg.port;
    let in_use = TcpListener::bind(("127.0.0.1", port)).is_err();
    report(
        Verdict::Ok,
        &format!("port {port}"),
        if in_use {
            "in use (gateway running?)"
        } else {
            "free"
        },
    );

    // Proxy state
    let paused = state.paused.load(std::sync::atomic::Ordering::Relaxed);
    report(
        Verdict::Ok,
        "proxy",
        if paused { "paused" } else { "active" },
    );

    // Tailscale
    match share::detect_mode() {
        ShareMode::Direct(ip) => {
            report(Verdict::Ok, "tailscale", &format!("direct mode ({ip})"));
        }
        ShareMode::Serve(fqdn) => {
            let route = share::serve_route_active();
            report(
                if route { Verdict::Ok } else { Verdict::Warn },
                "tailscale",
                &format!(
                    "serve mode ({fqdn}) — route {}",
                    if route {
                        "active"
                    } else {
                        "not configured (run `tokenguard share on`)"
                    }
                ),
            );
        }
        ShareMode::Unavailable => {
            report(
                Verdict::Warn,
                "tailscale",
                "not available — team sharing disabled (install/sign in to enable)",
            );
        }
    }

    // Configuration summary
    report(Verdict::Ok, "providers", &cfg.providers.len().to_string());
    if cfg.providers.is_empty() {
        report(
            Verdict::Warn,
            "setup",
            "no providers configured — add one with `tokenguard provider add`",
        );
    }
    report(Verdict::Ok, "projects", &cfg.projects.len().to_string());
    report(Verdict::Ok, "limits", &cfg.limits.len().to_string());
    report(
        Verdict::Ok,
        "team sharing",
        if cfg.share_over_tailscale {
            "enabled"
        } else {
            "disabled"
        },
    );

    println!();
    if failures == 0 {
        println!("All critical checks passed.");
    } else {
        println!("{failures} check(s) failed — fix the FAIL items above.");
    }
    Ok(failures)
}

#[cfg(test)]
mod tests {
    use super::Verdict;

    #[test]
    fn verdict_tags() {
        assert_eq!(Verdict::Ok.tag(), "ok  ");
        assert_eq!(Verdict::Warn.tag(), "warn");
        assert_eq!(Verdict::Fail.tag(), "FAIL");
    }
}
