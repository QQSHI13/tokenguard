//! Headless backend initialization shared by the GUI and CLI entrypoints.

use crate::db;
use crate::notifier::Notifier;
use crate::proxy::server;
use crate::state::AppState;
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct BackendInit {
    pub state: Arc<AppState>,
    pub port: u16,
    pub expose_to_lan: bool,
    pub share_over_tailscale: bool,
}

/// Initialize the backend: data dir, SQLite pool, config, and state.
/// `notifier` is None for the headless CLI; the GUI passes a Tauri-backed
/// notifier so tray icons and desktop notifications work.
pub fn init_backend(
    data_dir: PathBuf,
    notifier: Option<Box<dyn Notifier + Send + Sync>>,
) -> Result<BackendInit> {
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| anyhow::anyhow!("create data directory: {e}"))?;
    let db_path = data_dir.join("tokenguard.db");
    apply_pending_restore(&db_path);

    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid db path"))?;
    let pool = db::build_pool(db_path_str).map_err(|e| anyhow::anyhow!("open SQLite pool: {e}"))?;
    let conn = pool
        .get()
        .map_err(|e| anyhow::anyhow!("get initial DB connection from pool: {e}"))?;
    let config =
        db::load_config(&conn).map_err(|e| anyhow::anyhow!("load config from database: {e}"))?;
    let port = config.port;
    let expose_to_lan = config.expose_to_lan;
    let share_over_tailscale = config.share_over_tailscale;

    if config.log_retention_days > 0 {
        match db::cleanup_old_logs(&conn, config.log_retention_days) {
            Ok(deleted) if deleted > 0 => tracing::info!("deleted {deleted} old log rows"),
            Ok(_) => {}
            Err(e) => tracing::warn!("log cleanup failed: {e}"),
        }
        match db::cleanup_old_audit_events(&conn, config.log_retention_days) {
            Ok(deleted) if deleted > 0 => tracing::info!("deleted {deleted} old audit events"),
            Ok(_) => {}
            Err(e) => tracing::warn!("audit cleanup failed: {e}"),
        }
    }

    let state = Arc::new(
        AppState::new(pool, db_path, config, notifier)
            .map_err(|e| anyhow::anyhow!("initialize app state: {e}"))?,
    );

    if is_auto_export_due(&state).unwrap_or(false) {
        if let Err(e) = run_auto_export_now(&state) {
            tracing::warn!("auto export failed: {e}");
        }
    }

    Ok(BackendInit {
        state,
        port,
        expose_to_lan,
        share_over_tailscale,
    })
}

/// Run the proxy server until the shutdown signal fires.
pub async fn serve_proxy(
    state: Arc<AppState>,
    port: u16,
    expose_to_lan: bool,
    share_over_tailscale: bool,
) -> Result<()> {
    let shutdown_rx = state.shutdown_rx();
    server::serve(
        state,
        port,
        expose_to_lan,
        share_over_tailscale,
        shutdown_rx,
    )
    .await
    .map_err(|e| anyhow::anyhow!("proxy server error: {e}"))
}

// ---------------------------------------------------------------------------
// Database restore scheduling (used during startup before the pool is open).
// ---------------------------------------------------------------------------

/// Path of the marker file that schedules a database restore on next boot.
pub fn restore_marker_path(db_path: &Path) -> PathBuf {
    let mut s = db_path.as_os_str().to_os_string();
    s.push(".restore-pending");
    PathBuf::from(s)
}

/// Read-only check that `source` is a SQLite database with a schema.
fn validate_sqlite_source(source: &Path) -> bool {
    let Ok(test) =
        rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return false;
    };
    test.query_row("PRAGMA schema_version", [], |row| row.get::<_, bool>(0))
        .unwrap_or(false)
}

/// Apply a restore scheduled by the GUI: while the database is still closed,
/// copy the recorded source over it and drop stale WAL/SHM files. Any problem
/// just removes the marker and lets the app boot normally.
pub fn apply_pending_restore(db_path: &Path) {
    let marker = restore_marker_path(db_path);
    let Ok(recorded) = std::fs::read_to_string(&marker) else {
        return;
    };
    let source = PathBuf::from(recorded.trim());
    if source.exists() && validate_sqlite_source(&source) && std::fs::copy(&source, db_path).is_ok()
    {
        for suffix in ["-wal", "-shm"] {
            let mut s = db_path.as_os_str().to_os_string();
            s.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(s));
        }
        tracing::info!("database restored from {}", source.display());
    } else {
        tracing::warn!("pending database restore skipped (bad marker or source)");
    }
    let _ = std::fs::remove_file(&marker);
}

// ---------------------------------------------------------------------------
// Scheduled usage export.
// ---------------------------------------------------------------------------

fn last_auto_export_at(
    conn: &rusqlite::Connection,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    match db::get_setting(conn, "last_auto_export_at") {
        Some(v) => chrono::DateTime::parse_from_rfc3339(&v)
            .map(|dt| Some(dt.with_timezone(&chrono::Utc)))
            .map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

pub fn is_auto_export_due(state: &Arc<AppState>) -> Result<bool, String> {
    let cfg = state.config.read().map_err(|e| e.to_string())?;
    if cfg.auto_export_days == 0 {
        return Ok(false);
    }
    let folder = match &cfg.auto_export_folder {
        Some(f) if !f.is_empty() => f,
        _ => return Ok(false),
    };
    let folder_path = PathBuf::from(folder);
    if !folder_path.exists() {
        return Ok(false);
    }
    let conn = state.db.get().map_err(|e| e.to_string())?;
    let last = last_auto_export_at(&conn)?;
    let due = match last {
        Some(t) => {
            let interval = chrono::Duration::days(cfg.auto_export_days as i64);
            chrono::Utc::now() - t >= interval
        }
        None => true,
    };
    Ok(due)
}

pub fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn run_auto_export_now(state: &Arc<AppState>) -> Result<String, String> {
    let cfg = state.config.read().map_err(|e| e.to_string())?;
    let folder = cfg
        .auto_export_folder
        .as_ref()
        .ok_or("auto export folder not set")?;
    if folder.is_empty() {
        return Err("auto export folder not set".into());
    }
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;

    let rows = {
        let conn = state.db.get().map_err(|e| e.to_string())?;
        db::list_logs(&conn, 100_000, None).map_err(|e| e.to_string())?
    };
    let filename = format!(
        "tokenguard-usage-{}.csv",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let path = PathBuf::from(folder).join(&filename);

    let mut w =
        String::from("timestamp,provider,model,prompt_tokens,completion_tokens,cost,project\n");
    for r in rows.iter().rev() {
        w.push_str(&format!(
            "{},{},{},{},{},{:.6},{}\n",
            r.ts,
            csv_escape(&r.provider),
            csv_escape(&r.model),
            r.prompt_tokens,
            r.completion_tokens,
            r.cost,
            r.project_tag.as_deref().unwrap_or(""),
        ));
    }
    std::fs::write(&path, w).map_err(|e| e.to_string())?;

    let conn = state.db.get().map_err(|e| e.to_string())?;
    db::set_setting(
        &conn,
        "last_auto_export_at",
        &chrono::Utc::now().to_rfc3339(),
    )
    .map_err(|e| e.to_string())?;

    Ok(path.to_string_lossy().to_string())
}
