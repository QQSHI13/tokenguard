//! Health check command for the CLI.

use crate::db;
use crate::health;
use crate::state::AppState;
use anyhow::{Context, Result};
use std::sync::Arc;

pub async fn check(state: &Arc<AppState>, name: Option<String>) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let providers = db::list_providers(&conn).context("list providers")?;
    if providers.is_empty() {
        println!("No providers configured.");
        return Ok(());
    }

    let to_check: Vec<_> = match name {
        Some(n) => providers.into_iter().filter(|p| p.name == n).collect(),
        None => providers,
    };

    for p in to_check {
        let result = health::check_provider(&state.client, &p).await;
        let status = if result.ok { "OK" } else { "FAIL" };
        println!(
            "{:<20} {}  {} ms  {}",
            p.name,
            status,
            result.latency_ms,
            result.error.as_deref().unwrap_or("")
        );
    }
    Ok(())
}
