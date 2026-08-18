//! Model listing command for the CLI.

use crate::state::AppState;
use anyhow::Result;
use std::sync::Arc;

pub fn list(state: &Arc<AppState>) -> Result<()> {
    let cfg = state.config.read().map_err(|e| anyhow::anyhow!("{e}"))?;
    if cfg.providers.is_empty() {
        println!("No providers configured.");
        return Ok(());
    }
    for p in &cfg.providers {
        println!("{} ({})", p.name, p.format.as_db_str());
        if p.models.is_empty() {
            println!("  (no models)");
        } else {
            for m in &p.models {
                println!(
                    "  {} -> {}  in={:?} out={:?} cached={:?}",
                    m.local,
                    m.remote,
                    m.pricing.input_per_1k,
                    m.pricing.output_per_1k,
                    m.pricing.cached_input_per_1k
                );
            }
        }
    }
    Ok(())
}
