//! Project management commands for the CLI.

use crate::config::{BudgetPeriod, LimitAction, ProjectInput};
use crate::db;
use crate::state::AppState;
use anyhow::{Context, Result};
use std::sync::Arc;

pub fn list(state: &Arc<AppState>) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let projects = db::list_projects(&conn).context("list projects")?;
    if projects.is_empty() {
        println!("No projects configured.");
        return Ok(());
    }
    println!(
        "{:<5} {:<20} {:<24} {:<10} PERIOD ACTION",
        "ID", "NAME", "LABEL KEY", "BUDGET"
    );
    for p in projects {
        println!(
            "{:<5} {:<20} {:<24} {:<10.2} {:?} {:?}",
            p.id, p.name, p.label_key, p.budget, p.budget_period, p.budget_action
        );
    }
    Ok(())
}

pub fn add(
    state: &Arc<AppState>,
    name: String,
    label_key: String,
    budget: f64,
    budget_period: String,
    budget_action: String,
) -> Result<()> {
    let input = ProjectInput {
        name,
        label_key,
        budget,
        budget_period: parse_budget_period(&budget_period)?,
        budget_action: parse_budget_action(&budget_action)?,
    };
    let conn = state.db.get().context("get DB connection")?;
    let id = db::insert_project(&conn, &input).context("insert project")?;
    println!("Added project '{}' with ID {}", input.name, id);
    Ok(())
}

pub fn delete(state: &Arc<AppState>, id: i64) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let projects = db::list_projects(&conn).context("list projects")?;
    let project = projects
        .into_iter()
        .find(|p| p.id == id)
        .context("project not found")?;
    db::delete_project(&conn, id).context("delete project")?;
    println!("Deleted project '{}' (ID {})", project.name, id);
    Ok(())
}

fn parse_budget_period(s: &str) -> Result<BudgetPeriod> {
    match s.to_lowercase().as_str() {
        "daily" => Ok(BudgetPeriod::Daily),
        "weekly" => Ok(BudgetPeriod::Weekly),
        "monthly" => Ok(BudgetPeriod::Monthly),
        _ => anyhow::bail!(
            "unknown budget period '{}'; use daily, weekly, or monthly",
            s
        ),
    }
}

fn parse_budget_action(s: &str) -> Result<LimitAction> {
    match s.to_lowercase().as_str() {
        "warn" => Ok(LimitAction::Warn),
        "block" => Ok(LimitAction::Block),
        "pause" => Ok(LimitAction::Pause),
        _ => anyhow::bail!("unknown budget action '{}'; use warn, block, or pause", s),
    }
}
