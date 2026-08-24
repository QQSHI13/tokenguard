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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_escape_quotes_only_when_needed() {
        // Exported CSVs are opened in spreadsheets; a stray comma in a provider
        // name shifts every later column, silently corrupting the whole row.
        assert_eq!(csv_escape("gpt-4o"), "gpt-4o");
        assert_eq!(csv_escape("Acme, Inc"), "\"Acme, Inc\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("line1\nline2"), "\"line1\nline2\"");
        // Quotes inside an already-quoted field must be doubled, not escaped
        // with a backslash (which Excel reads literally).
        assert_eq!(csv_escape("a,\"b\""), "\"a,\"\"b\"\"\"");
        assert_eq!(csv_escape(""), "");
    }

    #[test]
    fn restore_marker_sits_next_to_the_database() {
        // It must be a sibling suffix, not a path inside the db file's name, or
        // the marker written by the GUI is not the one startup looks for.
        let db = PathBuf::from("/data/tokenguard.db");
        assert_eq!(
            restore_marker_path(&db),
            PathBuf::from("/data/tokenguard.db.restore-pending")
        );
    }

    #[test]
    fn a_non_sqlite_source_is_rejected() {
        // The marker records a user-chosen path. Copying an arbitrary file over
        // the live database would leave the app unable to open its own storage.
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("notes.txt");
        std::fs::write(&junk, b"this is not a database").unwrap();
        assert!(!validate_sqlite_source(&junk));
        assert!(!validate_sqlite_source(&dir.path().join("missing.db")));
    }

    #[test]
    fn a_real_sqlite_file_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("backup.db");
        let conn = rusqlite::Connection::open(&src).unwrap();
        conn.execute_batch("CREATE TABLE t (a INTEGER)").unwrap();
        drop(conn);
        assert!(validate_sqlite_source(&src));
    }

    #[test]
    fn pending_restore_replaces_the_database_and_clears_the_marker() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("tokenguard.db");
        let src = dir.path().join("backup.db");

        // Live database with one marker row, plus stale WAL/SHM siblings.
        let live = rusqlite::Connection::open(&db).unwrap();
        live.execute_batch("CREATE TABLE origin (name TEXT); INSERT INTO origin VALUES ('live')")
            .unwrap();
        drop(live);
        std::fs::write(db.with_extension("db-wal"), b"stale").unwrap();

        let backup = rusqlite::Connection::open(&src).unwrap();
        backup
            .execute_batch("CREATE TABLE origin (name TEXT); INSERT INTO origin VALUES ('backup')")
            .unwrap();
        drop(backup);

        std::fs::write(restore_marker_path(&db), src.to_str().unwrap()).unwrap();
        apply_pending_restore(&db);

        // The marker must be consumed, or the restore repeats on every boot and
        // silently discards everything logged since.
        assert!(!restore_marker_path(&db).exists(), "marker not cleared");
        let conn = rusqlite::Connection::open(&db).unwrap();
        let origin: String = conn
            .query_row("SELECT name FROM origin", [], |r| r.get(0))
            .unwrap();
        assert_eq!(origin, "backup");
    }

    #[test]
    fn a_bad_marker_clears_itself_and_leaves_the_database_alone() {
        // Booting into a loop of failed restores, or refusing to boot at all,
        // would be worse than ignoring an unusable marker.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("tokenguard.db");
        let live = rusqlite::Connection::open(&db).unwrap();
        live.execute_batch("CREATE TABLE origin (name TEXT); INSERT INTO origin VALUES ('live')")
            .unwrap();
        drop(live);

        let junk = dir.path().join("notes.txt");
        std::fs::write(&junk, b"not a database").unwrap();

        for recorded in [
            dir.path().join("does-not-exist.db").to_str().unwrap(),
            junk.to_str().unwrap(),
            "",
        ] {
            std::fs::write(restore_marker_path(&db), recorded).unwrap();
            apply_pending_restore(&db);
            assert!(
                !restore_marker_path(&db).exists(),
                "marker survived for {recorded:?}"
            );
            let conn = rusqlite::Connection::open(&db).unwrap();
            let origin: String = conn
                .query_row("SELECT name FROM origin", [], |r| r.get(0))
                .unwrap();
            assert_eq!(origin, "live", "database was clobbered by {recorded:?}");
        }
    }

    #[test]
    fn no_marker_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("tokenguard.db");
        // Must not create the database, the marker, or panic.
        apply_pending_restore(&db);
        assert!(!db.exists());
        assert!(!restore_marker_path(&db).exists());
    }

    #[test]
    fn init_backend_creates_a_usable_data_dir_and_state() {
        // The CLI passes a path that may not exist yet; failing to create it
        // would abort startup with a bare "unable to open database file".
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("nested/does/not/exist");
        let init = init_backend(data_dir.clone(), None).expect("init_backend");
        assert!(data_dir.join("tokenguard.db").exists());
        assert!(init.port > 0, "a zero port would bind randomly");
        // Config defaults come from the fresh database, not from the GUI.
        let cfg = init.state.config.read().unwrap();
        assert_eq!(cfg.port, init.port);
        assert_eq!(cfg.expose_to_lan, init.expose_to_lan);
    }

    #[test]
    fn init_backend_is_idempotent_across_restarts() {
        // Reopening the same data dir must not reset settings — this is every
        // ordinary app restart.
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_path_buf();
        let first = init_backend(data_dir.clone(), None).expect("first init");
        {
            let conn = first.state.db.get().unwrap();
            db::set_setting(&conn, "port", "18123").unwrap();
        }
        drop(first);

        let second = init_backend(data_dir, None).expect("second init");
        assert_eq!(second.port, 18123, "settings did not survive a restart");
    }

    #[test]
    fn auto_export_is_not_due_when_disabled_or_unconfigured() {
        // `auto_export_days == 0` means off; a due-check that returned true would
        // write a CSV into whatever stale folder was last configured.
        let dir = tempfile::tempdir().unwrap();
        let init = init_backend(dir.path().to_path_buf(), None).expect("init_backend");
        {
            let mut cfg = init.state.config.write().unwrap();
            cfg.auto_export_days = 0;
            cfg.auto_export_folder = Some(dir.path().to_string_lossy().into_owned());
        }
        assert!(!is_auto_export_due(&init.state).unwrap());

        // Enabled but with no folder, and with a folder that no longer exists
        // (removed USB drive, deleted directory).
        {
            let mut cfg = init.state.config.write().unwrap();
            cfg.auto_export_days = 7;
            cfg.auto_export_folder = None;
        }
        assert!(!is_auto_export_due(&init.state).unwrap());
        {
            let mut cfg = init.state.config.write().unwrap();
            cfg.auto_export_folder = Some(
                dir.path()
                    .join("removed-drive")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        assert!(!is_auto_export_due(&init.state).unwrap());
    }

    #[test]
    fn auto_export_is_due_before_the_first_run_and_not_again_until_the_interval() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("exports");
        std::fs::create_dir_all(&out).unwrap();
        let init = init_backend(dir.path().to_path_buf(), None).expect("init_backend");
        {
            let mut cfg = init.state.config.write().unwrap();
            cfg.auto_export_days = 7;
            cfg.auto_export_folder = Some(out.to_string_lossy().into_owned());
        }

        // Never exported: due.
        assert!(is_auto_export_due(&init.state).unwrap());

        let path = run_auto_export_now(&init.state).expect("export");
        let csv = std::fs::read_to_string(&path).unwrap();
        assert!(
            csv.starts_with("timestamp,provider,model,"),
            "header: {csv}"
        );

        // Just exported: not due again until the interval elapses.
        assert!(!is_auto_export_due(&init.state).unwrap());

        // Backdate the stamp past the interval.
        {
            let conn = init.state.db.get().unwrap();
            let old = (chrono::Utc::now() - chrono::Duration::days(8)).to_rfc3339();
            db::set_setting(&conn, "last_auto_export_at", &old).unwrap();
        }
        assert!(is_auto_export_due(&init.state).unwrap());
    }

    #[test]
    fn auto_export_without_a_folder_errors_instead_of_writing_somewhere() {
        let dir = tempfile::tempdir().unwrap();
        let init = init_backend(dir.path().to_path_buf(), None).expect("init_backend");
        {
            let mut cfg = init.state.config.write().unwrap();
            cfg.auto_export_folder = None;
        }
        let err = run_auto_export_now(&init.state).expect_err("no folder");
        assert!(err.contains("folder"), "unhelpful message: {err:?}");

        {
            let mut cfg = init.state.config.write().unwrap();
            cfg.auto_export_folder = Some(String::new());
        }
        assert!(run_auto_export_now(&init.state).is_err());
    }
}
