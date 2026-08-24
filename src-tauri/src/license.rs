//! License key storage and device registration against the license worker.

use crate::secrets;

/// Base URL of the Cloudflare worker that issues/validates supporter codes.
pub const WORKER_BASE: &str = "https://tokenguard-license.qingquanshi65.workers.dev";

#[tauri::command]
pub fn get_license_key() -> Result<Option<String>, String> {
    secrets::get_optional("license")
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

/// Default device allowance when the worker does not say.
///
/// Chosen to match the worker's own default: guessing higher would let a user
/// register a third device and then be rejected server-side with no explanation.
const DEFAULT_MAX_DEVICES: usize = 2;

/// Parse the worker's `/api/license/devices` response.
///
/// Split out from the request so the shape can be tested without the network.
/// Entries missing a fingerprint or timestamp are dropped rather than defaulted:
/// a device row with an empty fingerprint would render as an un-revocable entry
/// in the UI's device list.
fn parse_device_list(json: &serde_json::Value) -> DeviceListDto {
    let max_devices = json["maxDevices"]
        .as_u64()
        .unwrap_or(DEFAULT_MAX_DEVICES as u64) as usize;
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
    DeviceListDto {
        devices,
        max_devices,
    }
}

#[tauri::command]
pub async fn get_registered_devices(key: String, device: String) -> Result<DeviceListDto, String> {
    // Key travels in a header, not the query string (which lands in CDN/proxy logs).
    let url = format!("{WORKER_BASE}/api/license/devices?device={device}");
    let resp = reqwest::Client::new()
        .get(&url)
        .header("X-License-Key", &key)
        .send()
        .await
        .map_err(|e| format!("failed to contact license server: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("license server returned {}", resp.status()));
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parse_device_list(&json))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_base_is_https() {
        // The license key travels in a header on every validate call; plain HTTP
        // would put it on the wire in cleartext.
        assert!(
            WORKER_BASE.starts_with("https://"),
            "license worker must be HTTPS: {WORKER_BASE}"
        );
        assert!(
            !WORKER_BASE.ends_with('/'),
            "trailing slash would produce '//api/...' paths"
        );
    }

    #[test]
    fn device_list_parses_the_worker_shape() {
        let json = serde_json::json!({
            "maxDevices": 3,
            "devices": [
                {"fingerprint": "aa11", "registeredAt": "2026-01-01T00:00:00Z", "current": true},
                {"fingerprint": "bb22", "registeredAt": "2026-02-01T00:00:00Z"},
            ],
        });
        let dto = parse_device_list(&json);
        assert_eq!(dto.max_devices, 3);
        assert_eq!(dto.devices.len(), 2);
        assert_eq!(dto.devices[0].fingerprint, "aa11");
        assert!(dto.devices[0].current);
        // `current` is absent on the second entry: default to false, so the UI
        // does not label a foreign device as "this device".
        assert!(!dto.devices[1].current);
    }

    #[test]
    fn missing_max_devices_falls_back_to_the_worker_default() {
        let dto = parse_device_list(&serde_json::json!({"devices": []}));
        assert_eq!(dto.max_devices, DEFAULT_MAX_DEVICES);
    }

    #[test]
    fn malformed_device_entries_are_dropped_not_defaulted() {
        // A row with no fingerprint cannot be revoked from the UI; showing it as
        // an empty-string device would be worse than omitting it.
        let json = serde_json::json!({
            "devices": [
                {"registeredAt": "2026-01-01T00:00:00Z"},
                {"fingerprint": "aa11"},
                {"fingerprint": "bb22", "registeredAt": "2026-01-01T00:00:00Z"},
                "not-an-object",
            ],
        });
        let dto = parse_device_list(&json);
        assert_eq!(dto.devices.len(), 1);
        assert_eq!(dto.devices[0].fingerprint, "bb22");
    }

    #[test]
    fn absent_or_wrongly_typed_device_array_yields_an_empty_list() {
        // A worker error body ({"error": ...}) must not panic the settings pane.
        for json in [
            serde_json::json!({}),
            serde_json::json!({"error": "invalid key"}),
            serde_json::json!({"devices": "none"}),
        ] {
            assert!(parse_device_list(&json).devices.is_empty(), "{json}");
        }
    }

    #[test]
    fn device_fingerprint_is_a_stable_sha256_hex() {
        let a = get_device_fingerprint().expect("fingerprint");
        let b = get_device_fingerprint().expect("fingerprint");
        // Stability is the whole point: a fingerprint that changes between calls
        // would burn a device slot on every app start.
        assert_eq!(a, b);
        assert_eq!(a.len(), 64, "sha256 hex is 64 chars: {a}");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // Never leak the raw hostname/username the digest is built from.
        let username = whoami::username().unwrap_or_default();
        if !username.is_empty() {
            assert!(!a.contains(&username));
        }
    }
}
