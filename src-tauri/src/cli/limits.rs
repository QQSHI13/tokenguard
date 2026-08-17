//! Limit management commands for the CLI.

use crate::config::{LimitAction, LimitInput, LimitMetric, LimitPeriod, LimitScope};
use crate::db;
use crate::state::AppState;
use anyhow::{Context, Result};
use std::sync::Arc;

pub fn list(state: &Arc<AppState>) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let limits = db::list_limits(&conn).context("list limits")?;
    if limits.is_empty() {
        println!("No limits configured.");
        return Ok(());
    }
    println!(
        "{:<5} {:<20} {:<12} {:<12} {:<10} {:<8} ENABLED",
        "ID", "NAME", "METRIC", "PERIOD", "CAP", "ACTION"
    );
    for l in limits {
        println!(
            "{:<5} {:<20} {:<12} {:<12} {:<10.2} {:<8} {}",
            l.id,
            l.name,
            l.metric.as_db_str(),
            l.period.as_db_str(),
            l.cap,
            l.action.as_db_str(),
            l.enabled
        );
    }
    Ok(())
}

pub fn add(
    state: &Arc<AppState>,
    name: String,
    metric: String,
    cap: f64,
    action: String,
) -> Result<()> {
    let input = LimitInput {
        name,
        metric: parse_metric(&metric)?,
        period: LimitPeriod::Daily,
        cap,
        warning_threshold: 0.8,
        scope: LimitScope::Global,
        scope_id: None,
        action: parse_action(&action)?,
        enabled: true,
        active_days: 0b1111111,
        active_hours_start: None,
        active_hours_end: None,
    };
    let conn = state.db.get().context("get DB connection")?;
    let id = db::insert_limit(&conn, &input).context("insert limit")?;
    println!("Added limit with ID {}", id);
    Ok(())
}

pub fn delete(state: &Arc<AppState>, id: i64) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    db::delete_limit(&conn, id).context("delete limit")?;
    println!("Deleted limit ID {}", id);
    Ok(())
}

pub fn update(
    state: &Arc<AppState>,
    id: i64,
    cap: Option<f64>,
    action: Option<String>,
    enabled: Option<bool>,
) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let limits = db::list_limits(&conn).context("list limits")?;
    let mut existing = limits
        .into_iter()
        .find(|l| l.id == id)
        .context("limit not found")?;

    if let Some(c) = cap {
        existing.cap = c;
    }
    if let Some(a) = action {
        existing.action = parse_action(&a)?;
    }
    if let Some(e) = enabled {
        existing.enabled = e;
    }

    let input = LimitInput {
        name: existing.name,
        metric: existing.metric,
        period: existing.period,
        cap: existing.cap,
        warning_threshold: existing.warning_threshold,
        scope: existing.scope,
        scope_id: existing.scope_id,
        action: existing.action,
        enabled: existing.enabled,
        active_days: existing.active_days,
        active_hours_start: existing.active_hours_start,
        active_hours_end: existing.active_hours_end,
    };
    db::update_limit(&conn, id, &input).context("update limit")?;
    println!("Updated limit ID {}", id);
    Ok(())
}

fn parse_metric(s: &str) -> Result<LimitMetric> {
    match s.to_lowercase().as_str() {
        "money" => Ok(LimitMetric::Money),
        "tokens" => Ok(LimitMetric::Tokens),
        "requests" => Ok(LimitMetric::Requests),
        "timesec" | "time" => Ok(LimitMetric::TimeSec),
        "requestsperminute" | "rpm" => Ok(LimitMetric::RequestsPerMinute),
        "tokensperminute" | "tpm" => Ok(LimitMetric::TokensPerMinute),
        _ => anyhow::bail!(
            "unknown metric '{}'; use money, tokens, requests, time, rpm, tpm",
            s
        ),
    }
}

fn parse_action(s: &str) -> Result<LimitAction> {
    match s.to_lowercase().as_str() {
        "warn" => Ok(LimitAction::Warn),
        "block" => Ok(LimitAction::Block),
        "pause" => Ok(LimitAction::Pause),
        _ => anyhow::bail!("unknown action '{}'; use warn, block, or pause", s),
    }
}
