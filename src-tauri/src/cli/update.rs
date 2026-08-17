//! Update check command for the CLI.

use anyhow::Result;

const REPO: &str = "QQSHI13/tokenguard";

pub async fn check() -> Result<()> {
    let latest = reqwest::get(format!(
        "https://api.github.com/repos/{REPO}/releases/latest"
    ))
    .await?
    .json::<serde_json::Value>()
    .await?;
    let latest_tag = latest["tag_name"].as_str().unwrap_or("unknown");
    let current = env!("CARGO_PKG_VERSION");
    println!("Current version: v{current}");
    println!("Latest stable:   {latest_tag}");
    if latest_tag == format!("v{current}") {
        println!("You are up to date.");
    } else {
        println!("Update available: {latest_tag}");
        println!("  https://github.com/{REPO}/releases/{latest_tag}");
    }
    Ok(())
}
