//! Log and audit export commands for the CLI.

use crate::db;
use crate::state::AppState;
use anyhow::{Context, Result};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

pub fn export_logs(state: &Arc<AppState>, output: PathBuf, limit: u64) -> Result<()> {
    let conn = state.db.get().context("get DB connection")?;
    let rows = db::list_logs(&conn, limit, None).context("list logs")?;

    let mut file = std::fs::File::create(&output).context("create output file")?;
    writeln!(
        file,
        "timestamp,provider,model,prompt_tokens,completion_tokens,cost,project"
    )?;
    for r in rows.iter().rev() {
        writeln!(
            file,
            "{},{},{},{},{},{:.6},{}",
            r.ts,
            csv_escape(&r.provider),
            csv_escape(&r.model),
            r.prompt_tokens,
            r.completion_tokens,
            r.cost,
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

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
