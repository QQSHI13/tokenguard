//! Headless backend initialization shared by the GUI and CLI entrypoints.

use crate::commands::{apply_pending_restore, is_auto_export_due, run_auto_export_now};
use crate::db;
use crate::proxy::server;
use crate::state::AppState;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Wry};

pub struct BackendInit {
    pub state: Arc<AppState<Wry>>,
    pub port: u16,
    pub expose_to_lan: bool,
}

/// Initialize the backend: data dir, SQLite pool, config, and state.
/// `app_handle` is None for the headless CLI; the GUI passes its Tauri handle.
pub fn init_backend(data_dir: PathBuf, app_handle: Option<AppHandle<Wry>>) -> Result<BackendInit> {
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
        AppState::new(pool, db_path, config, app_handle)
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
    })
}

/// Run the proxy server until the shutdown signal fires.
pub async fn serve_proxy(state: Arc<AppState<Wry>>, port: u16, expose_to_lan: bool) -> Result<()> {
    let shutdown_rx = state.shutdown_rx();
    server::serve(state, port, expose_to_lan, shutdown_rx)
        .await
        .map_err(|e| anyhow::anyhow!("proxy server error: {e}"))
}
