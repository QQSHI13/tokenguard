//! Limit group management commands for the CLI.

use crate::config::{
    LimitAction, LimitGroupInput, LimitMetric, LimitPeriod, LimitScope,
};
use crate::db;
use crate::state::AppState;
use anyhow::{Context, Result};
use std::sync::Arc;

pub fn list(state: &Arc<AppState>) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let groups = db::list_limit_groups(&conn).context("list limit groups")?;
    if groups.is_empty() {
        println!("No limit groups configured.");
        return Ok(());
    }
    println!(
        "{:<5} {:<20} {:<12} {:<12} {:<10} {:<8} ENABLED MEMBERS",
        "ID", "NAME", "METRIC", "PERIOD", "CAP", "ACTION"
    );
    for g in groups {
        println!(
            "{:<5} {:<20} {:<12} {:<12} {:<10.2} {:<8} {}    {}",
            g.id,
            g.name,
            g.metric.as_db_str(),
            period_str(&g.period),
            g.cap,
            g.action.as_db_str(),
            g.enabled,
            g.member_limit_ids
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    Ok(())
}

pub fn status(state: &Arc<AppState>) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let groups = db::list_limit_groups(&conn).context("list limit groups")?;
    if groups.is_empty() {
        println!("No limit groups configured.");
        return Ok(());
    }
    println!(
        "{:<5} {:<20} {:<12} {:<14} {:<14} {:<6}",
        "ID", "NAME", "METRIC", "USED", "CAP", "RATIO"
    );
    for g in groups {
        if !g.enabled {
            println!(
                "{:<5} {:<20} {:<12} (disabled)",
                g.id,
                g.name,
                g.metric.as_db_str()
            );
            continue;
        }
        let used = db::usage_for_limit_group(&conn, &g).unwrap_or(0.0);
        let ratio = if g.cap > 0.0 { used / g.cap } else { 0.0 };
        println!(
            "{:<5} {:<20} {:<12} {:<14.2} {:<14.2} {:<6.1}%",
            g.id,
            g.name,
            g.metric.as_db_str(),
            used,
            g.cap,
            ratio * 100.0
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn add(
    state: &Arc<AppState>,
    name: String,
    metric: String,
    cap: f64,
    action: String,
    period: String,
    warning_threshold: f64,
    scope: String,
    scope_id: Option<i64>,
    model_pattern: Option<String>,
    active_hours_start: Option<String>,
    active_hours_end: Option<String>,
    active_days: u8,
    members: Vec<i64>,
    enabled: bool,
) -> Result<()> {
    let input = LimitGroupInput {
        name,
        metric: parse_metric(&metric)?,
        period: parse_period(&period)?,
        cap,
        warning_threshold,
        scope: parse_scope(&scope)?,
        scope_id,
        action: parse_action(&action)?,
        enabled,
        active_days,
        active_hours_start,
        active_hours_end,
        model_pattern,
        member_limit_ids: members,
    };
    validate_group_input(&input)?;
    let conn = state.db.get().context("get DB connection")?;
    let id = db::insert_limit_group(&conn, &input).context("insert limit group")?;

    let new_cfg = db::load_config(&conn).context("reload config")?;
    drop(conn);
    *state.config.write().map_err(|e| anyhow::anyhow!("{e}"))? = new_cfg;

    println!("Added limit group with ID {}", id);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    state: &Arc<AppState>,
    id: i64,
    name: Option<String>,
    metric: Option<String>,
    cap: Option<f64>,
    action: Option<String>,
    period: Option<String>,
    warning_threshold: Option<f64>,
    enabled: Option<bool>,
    scope: Option<String>,
    scope_id: Option<i64>,
    model_pattern: Option<String>,
    active_hours_start: Option<String>,
    active_hours_end: Option<String>,
    active_days: Option<u8>,
    members: Option<Vec<i64>>,
) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let groups = db::list_limit_groups(&conn).context("list limit groups")?;
    let mut existing = groups
        .into_iter()
        .find(|g| g.id == id)
        .context("limit group not found")?;

    if let Some(n) = name {
        existing.name = n;
    }
    if let Some(m) = metric {
        existing.metric = parse_metric(&m)?;
    }
    if let Some(c) = cap {
        existing.cap = c;
    }
    if let Some(a) = action {
        existing.action = parse_action(&a)?;
    }
    if let Some(p) = period {
        existing.period = parse_period(&p)?;
    }
    if let Some(w) = warning_threshold {
        existing.warning_threshold = w;
    }
    if let Some(e) = enabled {
        existing.enabled = e;
    }
    if let Some(s) = scope {
        existing.scope = parse_scope(&s)?;
    }
    if scope_id.is_some() {
        existing.scope_id = scope_id;
    }
    if model_pattern.is_some() {
        existing.model_pattern = model_pattern;
    }
    if let Some(s) = active_hours_start {
        existing.active_hours_start = Some(s);
    }
    if let Some(s) = active_hours_end {
        existing.active_hours_end = Some(s);
    }
    if let Some(d) = active_days {
        existing.active_days = d;
    }
    if let Some(m) = members {
        existing.member_limit_ids = m;
    }

    let input = LimitGroupInput {
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
        model_pattern: existing.model_pattern,
        member_limit_ids: existing.member_limit_ids,
    };
    validate_group_input(&input)?;
    db::update_limit_group(&conn, id, &input).context("update limit group")?;

    let new_cfg = db::load_config(&conn).context("reload config")?;
    drop(conn);
    *state.config.write().map_err(|e| anyhow::anyhow!("{e}"))? = new_cfg;

    println!("Updated limit group ID {}", id);
    Ok(())
}

pub fn delete(state: &Arc<AppState>, id: i64) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    db::delete_limit_group(&conn, id).context("delete limit group")?;

    let new_cfg = db::load_config(&conn).context("reload config")?;
    drop(conn);
    *state.config.write().map_err(|e| anyhow::anyhow!("{e}"))? = new_cfg;

    println!("Deleted limit group ID {}", id);
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

fn parse_period(s: &str) -> Result<LimitPeriod> {
    match s.to_lowercase().as_str() {
        "once" => Ok(LimitPeriod::Once),
        "hourly" => Ok(LimitPeriod::Hourly),
        "daily" => Ok(LimitPeriod::Daily),
        "weekly" => Ok(LimitPeriod::Weekly),
        "monthly" => Ok(LimitPeriod::Monthly),
        "calendar_week" | "calendar-week" | "calendarweek" => Ok(LimitPeriod::CalendarWeek),
        "calendar_month" | "calendar-month" | "calendarmonth" => Ok(LimitPeriod::CalendarMonth),
        _ => {
            if let Some(stripped) = s.to_lowercase().strip_prefix("custom_sec:") {
                let secs: u64 = stripped.parse().context("custom period seconds")?;
                Ok(LimitPeriod::CustomSec(secs))
            } else {
                anyhow::bail!(
                    "unknown period '{}'; use once, hourly, daily, weekly, monthly, calendar_week, calendar_month, or custom_sec:<seconds>",
                    s
                )
            }
        }
    }
}

fn parse_scope(s: &str) -> Result<LimitScope> {
    match s.to_lowercase().as_str() {
        "global" => Ok(LimitScope::Global),
        "provider" => Ok(LimitScope::Provider),
        "project" => Ok(LimitScope::Project),
        "model" => Ok(LimitScope::Model),
        _ => anyhow::bail!("unknown scope '{}'; use global, provider, project, or model", s),
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

fn validate_group_input(input: &LimitGroupInput) -> Result<()> {
    if input.name.is_empty() {
        anyhow::bail!("group name cannot be empty");
    }
    if input.cap < 0.0 {
        anyhow::bail!("cap cannot be negative");
    }
    if let LimitScope::Provider | LimitScope::Project = input.scope {
        if input.scope_id.is_none() {
            anyhow::bail!("scope_id is required for provider/project scope");
        }
    }
    if input.scope == LimitScope::Model && input.model_pattern.is_none() {
        anyhow::bail!("model_pattern is required for model scope");
    }
    Ok(())
}

fn period_str(p: &LimitPeriod) -> String {
    match p {
        LimitPeriod::CustomSec(s) => format!("custom_sec:{s}"),
        _ => p.as_db_str().to_string(),
    }
}
