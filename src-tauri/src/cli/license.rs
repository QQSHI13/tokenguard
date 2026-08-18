//! License management commands for the CLI.

use crate::secrets;
use anyhow::{Context, Result};

const WORKER_URL: &str = "https://tokenguard-license.qingquanshi65.workers.dev";

pub fn show() -> Result<()> {
    match secrets::get("license") {
        Ok(k) => {
            let masked = if k.len() > 8 {
                format!("{}...{}", &k[..4], &k[k.len() - 4..])
            } else {
                k
            };
            println!("License: {masked}");
        }
        Err(e) => {
            let lower = e.to_lowercase();
            if lower.contains("noentry") || lower.contains("no entry") {
                println!("No license activated.");
            } else {
                anyhow::bail!("read license key: {e}");
            }
        }
    }
    Ok(())
}

pub fn activate(key: String) -> Result<()> {
    secrets::set("license", &key).map_err(|e| anyhow::anyhow!("activate license: {e}"))?;
    println!("License activated.");
    Ok(())
}

pub fn deactivate() -> Result<()> {
    secrets::delete("license").map_err(|e| anyhow::anyhow!("deactivate license: {e}"))?;
    println!("License deactivated.");
    Ok(())
}

pub fn fingerprint() -> Result<()> {
    println!("Device fingerprint: {}", device_fingerprint()?);
    Ok(())
}

pub async fn devices() -> Result<()> {
    let key = match secrets::get("license") {
        Ok(k) => k,
        Err(e) => {
            let lower = e.to_lowercase();
            if lower.contains("noentry") || lower.contains("no entry") {
                anyhow::bail!("no license activated");
            }
            anyhow::bail!("read license key: {e}");
        }
    };
    let device = device_fingerprint()?;
    let res = reqwest::Client::new()
        .get(format!(
            "{}/api/license/devices?device={}",
            WORKER_URL, device
        ))
        .header("X-License-Key", &key)
        .send()
        .await
        .context("request registered devices")?;

    if !res.status().is_success() {
        let body = res.text().await.unwrap_or_default();
        anyhow::bail!("license server error: {body}");
    }

    let json: serde_json::Value = res.json().await.context("parse devices response")?;
    let devices = json["devices"].as_array().cloned().unwrap_or_default();
    let max = json["maxDevices"].as_u64().unwrap_or(2);
    println!("Registered devices (max {}):", max);
    for (i, d) in devices.iter().enumerate() {
        let fp = d["fingerprint"].as_str().unwrap_or("unknown");
        let current = d["current"].as_bool().unwrap_or(false);
        let at = d["registeredAt"].as_str().unwrap_or("");
        let marker = if current { " (this device)" } else { "" };
        println!("  {}. {}{} {}", i + 1, fp, marker, at);
    }
    Ok(())
}

fn device_fingerprint() -> Result<String> {
    let hostname = whoami::hostname().map_err(|e| anyhow::anyhow!("hostname: {e}"))?;
    let username = whoami::username().map_err(|e| anyhow::anyhow!("username: {e}"))?;
    let id = format!("{hostname}-{username}");
    let hash = sha256_hex(&id);
    Ok(hash)
}

fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}
