//! License key storage and device registration against the license worker.

use crate::secrets;

/// Base URL of the Cloudflare worker that issues/validates supporter codes.
pub const WORKER_BASE: &str = "https://tokenguard-license.qingquanshi65.workers.dev";

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
    let url = format!("{WORKER_BASE}/api/license/devices?key={key}&device={device}");
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

/// Stable per-device identity: sha256(hostname|username|os).
#[tauri::command]
pub fn get_device_fingerprint() -> Result<String, String> {
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())?;
    let username = whoami::username().map_err(|e| e.to_string())?;
    let os = std::env::consts::OS;
    let input = format!("{hostname}|{username}|{os}");

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    Ok(hex::encode(digest))
}
