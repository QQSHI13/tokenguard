//! Project management commands for the CLI.

use crate::config::ProjectInput;
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
    println!("{:<5} {:<20} {:<20} {:<10} PERIOD", "ID", "NAME", "LABEL KEY", "BUDGET");
    for p in projects {
        println!(
            "{:<5} {:<20} {:<20} {:<10.2} {:?}",
            p.id, p.name, p.label_key, p.budget, p.budget_period
        );
    }
    Ok(())
}

pub fn add(state: &Arc<AppState>, name: String, label_key: String) -> Result<()> {
    let input = ProjectInput {
        name,
        label_key,
        budget: 0.0,
        budget_period: crate::config::BudgetPeriod::Daily,
        budget_action: crate::config::LimitAction::Warn,
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
