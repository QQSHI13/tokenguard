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

/// Write a helper shell script to the temp dir and spawn it detached.
/// The script outlives the app process so it can swap binaries after exit.
#[cfg(unix)]
fn spawn_helper_script(script: &str, args: &[&str]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let script_path =
        std::env::temp_dir().join(format!("tokenguard-update-{}.sh", std::process::id()));
    std::fs::write(&script_path, script).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    std::process::Command::new("sh")
        .arg(&script_path)
        .args(args)
        .spawn()
        .map_err(|e| format!("failed to start update helper: {e}"))?;
    Ok(())
}

/// Install the downloaded update in place and remove the downloaded file.
///
/// Every platform works the same way: a detached helper waits for this process
/// to exit, swaps/updates the installed binary, relaunches it, and deletes the
/// downloaded installer. Falls back to opening the installer for manual
/// installation when an in-place update isn't possible (e.g. no write access,
/// no pkexec); in that case the file is kept so the user can finish manually.
#[tauri::command]
pub async fn install_update(app: AppHandle, path: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(path);
    if !path.exists() {
        return Err("installer not found".into());
    }

    let os = std::env::consts::OS;
    let path_str = path.to_string_lossy().to_string();
    let lower = path_str.to_lowercase();
    let pid = std::process::id().to_string();

    match os {
        "windows" => {
            // Batch runs the installer, then deletes it (and itself).
            let bat = std::env::temp_dir().join(format!("tokenguard-update-{pid}.bat"));
            let content = if lower.ends_with(".msi") {
                format!(
                    "@echo off\r\nmsiexec /i \"{path_str}\" /passive\r\ndel \"{path_str}\"\r\ndel \"%~f0\"\r\n"
                )
            } else if lower.ends_with(".exe") {
                format!("@echo off\r\n\"{path_str}\"\r\ndel \"{path_str}\"\r\ndel \"%~f0\"\r\n")
            } else {
                return Err("unsupported Windows installer".into());
            };
            std::fs::write(&bat, content).map_err(|e| e.to_string())?;
            std::process::Command::new("cmd")
                .args(["/c", "start", "", "/min", &bat.to_string_lossy()])
                .spawn()
                .map_err(|e| format!("failed to start installer: {e}"))?;
        }
        "macos" => {
            // $1 = downloaded file, $2 = app PID to wait for.
            let script = r#"#!/bin/sh
FILE="$1"
PID="$2"
while kill -0 "$PID" 2>/dev/null; do sleep 0.2; done

install_app() {
  SRC="$1"
  if [ -w "/Applications" ]; then
    rm -rf "/Applications/Token Guard.app"
    cp -R "$SRC" "/Applications/Token Guard.app" && \
      xattr -dr com.apple.quarantine "/Applications/Token Guard.app" 2>/dev/null
    return $?
  fi
  return 1
}

OK=1
case "$FILE" in
  *.dmg)
    MNT=$(hdiutil attach -nobrowse -readonly "$FILE" 2>/dev/null | sed -n 's|.*\(/Volumes/.*\)|\1|p' | head -1)
    if [ -n "$MNT" ] && [ -d "$MNT/Token Guard.app" ]; then
      install_app "$MNT/Token Guard.app" && OK=0
      hdiutil detach -quiet "$MNT" 2>/dev/null
    fi
    ;;
  *.tar.gz)
    TMP=$(mktemp -d)
    tar -xzf "$FILE" -C "$TMP" 2>/dev/null
    if [ -d "$TMP/Token Guard.app" ]; then
      install_app "$TMP/Token Guard.app" && OK=0
    fi
    rm -rf "$TMP"
    ;;
esac

if [ "$OK" = "0" ]; then
  rm -f "$FILE"
  open "/Applications/Token Guard.app"
else
  # No write access to /Applications: fall back to manual install, keep the file.
  open "$FILE"
fi
rm -f "$0"
"#;
            spawn_helper_script(script, &[&path_str, &pid])?;
        }
        "linux" => {
            if lower.ends_with(".appimage") {
                let target = std::env::var("APPIMAGE").unwrap_or_default();
                if target.is_empty() {
                    // Not running from an AppImage (dev build): just launch the new one.
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ =
                            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
                    }
                    std::process::Command::new(&path_str)
                        .spawn()
                        .map_err(|e| format!("failed to run AppImage: {e}"))?;
                    return Ok(());
                }
                // $1 = new AppImage, $2 = path of the running AppImage, $3 = app PID.
                let script = r#"#!/bin/sh
NEW="$1"
TARGET="$2"
PID="$3"
while kill -0 "$PID" 2>/dev/null; do sleep 0.2; done
if mv "$NEW" "$TARGET"; then
  chmod +x "$TARGET"
  "$TARGET" &
else
  chmod +x "$NEW"
  "$NEW" &
fi
rm -f "$0"
"#;
                spawn_helper_script(script, &[&path_str, &target, &pid])?;
            } else if lower.ends_with(".deb") || lower.ends_with(".rpm") {
                let exec_path = std::env::current_exe()
                    .map(|p| p.to_string_lossy().into_owned())
                    .map_err(|e| e.to_string())?;
                // $1 = package file, $2 = app PID, $3 = installed binary to relaunch.
                let script = r#"#!/bin/sh
FILE="$1"
PID="$2"
EXEC="$3"
DONE=1
if command -v pkexec >/dev/null 2>&1; then
  case "$FILE" in
    *.deb)
      if command -v apt-get >/dev/null 2>&1; then
        pkexec apt-get install -y "$FILE" && DONE=0
      fi
      ;;
    *.rpm)
      if command -v dnf >/dev/null 2>&1; then
        pkexec dnf install -y "$FILE" && DONE=0
      elif command -v rpm >/dev/null 2>&1; then
        pkexec rpm -U "$FILE" && DONE=0
      fi
      ;;
  esac
fi

if [ "$DONE" = "0" ]; then
  rm -f "$FILE"
  while kill -0 "$PID" 2>/dev/null; do sleep 0.2; done
  "$EXEC" &
else
  # No pkexec / unsupported: open the graphical package installer, keep the file.
  xdg-open "$FILE" &
fi
rm -f "$0"
"#;
                spawn_helper_script(script, &[&path_str, &pid, &exec_path])?;
            } else {
                return Err("unsupported Linux installer".into());
            }
        }
        _ => return Err("unsupported OS".into()),
    }

    // Exit so the helper can replace binaries without file locks.
    crate::graceful_exit(&app).await;
    Ok(())
}
