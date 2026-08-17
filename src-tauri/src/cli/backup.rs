//! Backup and restore commands for the CLI.

use crate::backend::restore_marker_path;
use crate::state::AppState;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;

pub fn create(state: &Arc<AppState>, output: PathBuf) -> Result<()> {
    let source = &state.db_path;
    std::fs::copy(source, &output).context("copy database")?;
    println!("Database backed up to {}", output.display());
    Ok(())
}

pub fn schedule_restore(state: &Arc<AppState>, source: PathBuf) -> Result<()> {
    if !source.exists() {
        anyhow::bail!("backup file does not exist: {}", source.display());
    }
    let marker = restore_marker_path(&state.db_path);
    std::fs::write(&marker, source.to_string_lossy().as_bytes()).context("write restore marker")?;
    println!(
        "Restore scheduled from {}. Run `tokenguard start` to apply.",
        source.display()
    );
    Ok(())
}
