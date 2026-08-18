//! Log and audit export commands for the CLI.

use crate::db;
use crate::state::AppState;
use anyhow::{Context, Result};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

pub fn export_logs(
    state: &Arc<AppState>,
    output: PathBuf,
    limit: u64,
    days: Option<u64>,
) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let rows = db::list_logs(&conn, limit, days).context("list logs")?;

    let mut file = std::fs::File::create(&output).context("create output file")?;
    writeln!(
        file,
        "timestamp,provider,model,prompt_tokens,completion_tokens,cost,duration_ms,status,project"
    )?;
    for r in rows.iter().rev() {
        writeln!(
            file,
            "{},{},{},{},{},{:.6},{},{},{}",
            r.ts,
            csv_escape(&r.provider),
            csv_escape(&r.model),
            r.prompt_tokens,
            r.completion_tokens,
            r.cost,
            r.duration_ms,
            r.status.map(|s| s.to_string()).unwrap_or_default(),
            r.project_tag.as_deref().unwrap_or("")
        )?;
    }
    println!("Exported {} log rows to {}", rows.len(), output.display());
    Ok(())
}

pub fn export_audit(state: &Arc<AppState>, output: PathBuf, days: u32) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let rows = db::list_audit_events(&conn, days, 100_000).context("list audit events")?;

    let mut file = std::fs::File::create(&output).context("create output file")?;
    writeln!(file, "timestamp,event_type,details")?;
    for r in &rows {
        writeln!(
            file,
            "{},{},{}",
            r.ts,
            csv_escape(&r.event_type),
            csv_escape(&r.details)
        )?;
    }
    println!(
        "Exported {} audit events to {}",
        rows.len(),
        output.display()
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn query(
    state: &Arc<AppState>,
    provider: Option<String>,
    model: Option<String>,
    project: Option<String>,
    start: Option<String>,
    end: Option<String>,
    page: u64,
    page_size: u64,
) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let filter = db::LogFilter {
        provider,
        model,
        project,
        start,
        end,
        page,
        page_size,
    };
    let rows = db::list_logs_filtered(&conn, &filter).context("query logs")?;
    let total = db::count_logs_filtered(&conn, &filter).context("count logs")?;

    if rows.is_empty() {
        println!("No log rows match the filter (total: {}).", total);
        return Ok(());
    }

    println!(
        "Page {} of {} ({} rows)",
        filter.page,
        total.div_ceil(filter.page_size),
        total
    );
    println!(
        "{:<24} {:<16} {:<24} {:>8} {:>11} {:>10} {:>8} PROJECT",
        "TIMESTAMP", "PROVIDER", "MODEL", "PROMPT", "COMPLETION", "COST", "STATUS"
    );
    for r in &rows {
        println!(
            "{:<24} {:<16} {:<24} {:>8} {:>11} {:>10.6} {:>8} {}",
            r.ts,
            truncate(&r.provider, 16),
            truncate(&r.model, 24),
            r.prompt_tokens,
            r.completion_tokens,
            r.cost,
            r.status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".to_string()),
            r.project_tag.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max - 1).collect::<String>() + "…"
    }
}
