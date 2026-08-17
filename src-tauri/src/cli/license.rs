//! License management commands for the CLI.

use crate::secrets;
use anyhow::Result;

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
