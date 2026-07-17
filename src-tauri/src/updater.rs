//! Update checks against GitHub Releases, with license gating and sha256
//! verification of the downloaded asset.

use crate::license;
use tauri::{AppHandle, Manager};
use tokio_stream::StreamExt;

/// GitHub repo that hosts the release binaries.
const GITHUB_REPO: &str = "QQSHI13/tokenguard";

#[derive(Debug, serde::Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub asset_url: String,
    /// Expected sha256 of the asset, as reported by the GitHub release API
    /// (`"digest": "sha256:<hex>"`). `None` if GitHub did not provide one.
    pub digest: Option<String>,
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

fn linux_package_family() -> &'static str {
    let content = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let id = content
        .lines()
        .find_map(|l| l.strip_prefix("ID="))
        .map(|s| s.trim_matches('"').to_lowercase())
        .unwrap_or_default();
    let id_like = content
        .lines()
        .find_map(|l| l.strip_prefix("ID_LIKE="))
        .map(|s| s.trim_matches('"').to_lowercase())
        .unwrap_or_default();
    if id.contains("fedora")
        || id_like.contains("fedora")
        || id.contains("rhel")
        || id_like.contains("rhel")
        || id.contains("suse")
        || id_like.contains("suse")
    {
        return "rpm";
    }
    if id.contains("debian")
        || id_like.contains("debian")
        || id.contains("ubuntu")
        || id_like.contains("ubuntu")
    {
        return "deb";
    }
    ""
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
        ))
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
    let key = match license::get_license_key() {
        Ok(Some(k)) => k,
        _ => return Ok(None),
    };
    let device = license::get_device_fingerprint()?;
    let validate_url = format!(
        "{}/api/license/validate?key={key}&device={device}",
        license::WORKER_BASE
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

    let asset = match os {
        "windows" => assets.iter().find(|a| {
            a["name"]
                .as_str()
                .map(|n| n.ends_with(".msi") || n.ends_with(".exe"))
                .unwrap_or(false)
        }),
        "macos" => assets.iter().find(|a| {
            a["name"]
                .as_str()
                .map(|n| n.ends_with(".dmg") || n.ends_with(".app.tar.gz"))
                .unwrap_or(false)
        }),
        "linux" => {
            let preferred = match linux_package_family() {
                "deb" => ".deb",
                "rpm" => ".rpm",
                _ => ".AppImage",
            };
            assets
                .iter()
                .find(|a| {
                    a["name"]
                        .as_str()
                        .map(|n| n.ends_with(preferred))
                        .unwrap_or(false)
                })
                .or_else(|| {
                    assets.iter().find(|a| {
                        a["name"]
                            .as_str()
                            .map(|n| n.ends_with(".AppImage"))
                            .unwrap_or(false)
                    })
                })
        }
        _ => None,
    }
    .ok_or("no suitable update asset found")?;

    let asset_url = asset["browser_download_url"]
        .as_str()
        .ok_or("missing browser_download_url")?
        .to_string();
    // GitHub reports e.g. "sha256:ab12..." since mid-2024; tolerate absence.
    let digest = asset["digest"]
        .as_str()
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(|h| h.to_string());

    Ok(Some(UpdateInfo {
        version: tag_name.to_string(),
        asset_url,
        digest,
        downloaded_path: None,
    }))
}

#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    asset_url: String,
    digest: Option<String>,
) -> Result<String, String> {
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
    drop(file);

    // Verify integrity against the digest reported by the GitHub API.
    if let Some(expected) = digest {
        use sha2::{Digest, Sha256};
        let bytes = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = hex::encode(hasher.finalize());
        if actual != expected.to_lowercase() {
            let _ = tokio::fs::remove_file(&path).await;
            return Err("update download failed integrity check".into());
        }
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
    let lower = path_str.to_lowercase();

    match os {
        "windows" => {
            if lower.ends_with(".msi") {
                // Silent MSI install with progress bar.
                std::process::Command::new("msiexec")
                    .args(["/i", &path_str, "/passive"])
                    .spawn()
                    .map_err(|e| format!("failed to start MSI installer: {e}"))?;
            } else if lower.ends_with(".exe") {
                // NSIS setup.exe: run directly.
                std::process::Command::new(&path_str)
                    .spawn()
                    .map_err(|e| format!("failed to start installer: {e}"))?;
            } else {
                return Err("unsupported Windows installer".into());
            }
        }
        "macos" => {
            // .dmg mounts in Finder; .app.tar.gz opens in Archive Utility.
            std::process::Command::new("open")
                .arg(&path_str)
                .spawn()
                .map_err(|e| format!("failed to open installer: {e}"))?;
        }
        "linux" => {
            if lower.ends_with(".appimage") {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
                }
                std::process::Command::new(&path_str)
                    .spawn()
                    .map_err(|e| format!("failed to run AppImage: {e}"))?;
            } else if lower.ends_with(".deb") || lower.ends_with(".rpm") {
                // Open with the default package installer (GNOME Software, Discover, etc.).
                std::process::Command::new("xdg-open")
                    .arg(&path_str)
                    .spawn()
                    .map_err(|e| format!("failed to open package installer: {e}"))?;
            } else {
                return Err("unsupported Linux installer".into());
            }
        }
        _ => return Err("unsupported OS".into()),
    }

    Ok(())
}
