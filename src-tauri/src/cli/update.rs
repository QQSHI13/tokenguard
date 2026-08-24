//! Update check and download commands for the CLI.

use anyhow::{Context, Result};
use std::path::PathBuf;

const REPO: &str = "QQSHI13/tokenguard";

pub async fn check(beta: bool) -> Result<()> {
    let release = latest_release(beta).await?;
    let latest_tag = release["tag_name"].as_str().unwrap_or("unknown");
    let current = env!("CARGO_PKG_VERSION");
    println!("Current version: v{current}");
    println!("Latest release:  {latest_tag}");
    // Compare versions, not tag strings: a string match calls a *newer* local
    // build "up to date" only by luck, and reports every prerelease mismatch as
    // an available update even when it is older than what is installed.
    let newer = match (
        crate::version::parse_release_version(latest_tag),
        crate::version::parse_release_version(current),
    ) {
        (Some(latest), Some(current)) => latest > current,
        // Unparseable tag: fall back to a string comparison rather than staying
        // silent about a release we cannot rank.
        _ => latest_tag != format!("v{current}"),
    };
    if newer {
        println!("Update available: {latest_tag}");
        println!("  https://github.com/{REPO}/releases/{latest_tag}");
    } else {
        println!("You are up to date.");
    }
    Ok(())
}

pub async fn download(output: PathBuf, beta: bool) -> Result<()> {
    let release = latest_release(beta).await?;
    let tag = release["tag_name"].as_str().context("release tag_name")?;
    let asset = release["assets"]
        .as_array()
        .context("assets array")?
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .map(|n| n == asset_name())
                .unwrap_or(false)
        })
        .context(format!(
            "no asset named '{}' found in {}",
            asset_name(),
            tag
        ))?;
    let url = asset["browser_download_url"]
        .as_str()
        .context("asset download url")?;
    let size = asset["size"].as_u64().unwrap_or(0);

    println!("Downloading {} ({})...", asset_name(), human_size(size));
    let bytes = reqwest::get(url)
        .await
        .context("download request")?
        .bytes()
        .await
        .context("download body")?;

    std::fs::write(&output, &bytes).context("write downloaded file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&output)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&output, perms)?;
    }
    println!("Saved {} ({} bytes)", output.display(), bytes.len());
    Ok(())
}

async fn latest_release(include_prerelease: bool) -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    if include_prerelease {
        let releases = client
            .get(format!("https://api.github.com/repos/{REPO}/releases"))
            .header("User-Agent", "tokenguard-cli")
            .send()
            .await
            .context("list releases request")?
            .json::<serde_json::Value>()
            .await
            .context("parse releases list")?;
        let arr = releases.as_array().context("releases array")?;
        arr.first().cloned().context("no releases found")
    } else {
        client
            .get(format!(
                "https://api.github.com/repos/{REPO}/releases/latest"
            ))
            .header("User-Agent", "tokenguard-cli")
            .send()
            .await
            .context("latest release request")?
            .json::<serde_json::Value>()
            .await
            .context("parse latest release")
    }
}

fn asset_name() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!("tokenguard-{}-{}", os, arch)
}

fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}
