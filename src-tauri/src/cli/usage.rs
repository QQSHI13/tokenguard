//! Usage reporting commands for the CLI.

use crate::db;
use crate::state::AppState;
use anyhow::{Context, Result};
use std::sync::Arc;

pub fn provider(state: &Arc<AppState>, name: String, days: u64) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let rows = db::provider_daily_usage(&conn, &name, days).context("provider daily usage")?;
    if rows.is_empty() {
        println!(
            "No usage data for provider '{}' in the last {} days.",
            name, days
        );
        return Ok(());
    }
    println!("{:<12} {:<10} {:<10} COST", "DATE", "REQUESTS", "TOKENS");
    for r in rows {
        println!(
            "{:<12} {:<10} {:<10} ${:.4}",
            r.day, r.requests, r.tokens, r.cost
        );
    }
    Ok(())
}

pub fn project(state: &Arc<AppState>, tag: String, days: u64) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let rows = db::project_daily_usage(&conn, Some(&tag), days).context("project daily usage")?;
    if rows.is_empty() {
        println!(
            "No usage data for project '{}' in the last {} days.",
            tag, days
        );
        return Ok(());
    }
    println!("{:<12} {:<10} {:<10} COST", "DATE", "REQUESTS", "TOKENS");
    for r in rows {
        println!(
            "{:<12} {:<10} {:<10} ${:.4}",
            r.day, r.requests, r.tokens, r.cost
        );
    }
    Ok(())
}

pub fn totals(state: &Arc<AppState>, days: u64) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let rows = db::project_totals(&conn, days).context("project totals")?;
    if rows.is_empty() {
        println!("No project usage in the last {} days.", days);
        return Ok(());
    }
    println!("{:<20} {:<10} {:<10} COST", "PROJECT", "REQUESTS", "TOKENS");
    for r in rows {
        println!(
            "{:<20} {:<10} {:<10} ${:.4}",
            r.project_tag.as_deref().unwrap_or("(untagged)"),
            r.requests,
            r.tokens,
            r.cost
        );
    }
    Ok(())
}

pub fn monthly(state: &Arc<AppState>, months: u32) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let rows = db::monthly_usage(&conn, months).context("monthly usage")?;
    if rows.is_empty() {
        println!("No monthly usage data.");
        return Ok(());
    }
    println!("{:<10} {:<10} {:<10} COST", "MONTH", "REQUESTS", "TOKENS");
    for r in rows {
        println!(
            "{:<10} {:<10} {:<10} ${:.4}",
            r.month, r.requests, r.tokens, r.cost
        );
    }
    Ok(())
}
