//! SQLite: schema, queries, config load.

use crate::config::{AuthScheme, Config, Provider, ProviderFormat};
use rusqlite::{params, Connection};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  prompt_tokens INTEGER NOT NULL,
  completion_tokens INTEGER NOT NULL,
  cost REAL NOT NULL,
  project_tag TEXT
);
CREATE TABLE IF NOT EXISTS providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  base_url TEXT NOT NULL,
  format TEXT NOT NULL,
  auth TEXT NOT NULL,
  models TEXT NOT NULL DEFAULT '[]',
  input_cost REAL,
  output_cost REAL,
  is_default INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_logs_ts ON logs(ts);
";

pub fn connect(path: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

pub fn insert_log(
    conn: &Connection,
    provider: &str,
    model: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    cost: f64,
    project_tag: Option<&str>,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO logs (ts, provider, model, prompt_tokens, completion_tokens, cost, project_tag) VALUES (?,?,?,?,?,?,?)",
        params![now, provider, model, prompt_tokens, completion_tokens, cost, project_tag],
    )?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogRow {
    pub id: i64,
    pub ts: String,
    pub provider: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cost: f64,
    pub project_tag: Option<String>,
}

fn row_to_log(row: &rusqlite::Row) -> rusqlite::Result<LogRow> {
    Ok(LogRow {
        id: row.get(0)?,
        ts: row.get(1)?,
        provider: row.get(2)?,
        model: row.get(3)?,
        prompt_tokens: row.get(4)?,
        completion_tokens: row.get(5)?,
        cost: row.get(6)?,
        project_tag: row.get(7)?,
    })
}

pub fn list_logs(conn: &Connection, limit: u64, days: Option<u64>) -> rusqlite::Result<Vec<LogRow>> {
    if let Some(d) = days {
        let mut stmt = conn.prepare(
            "SELECT id, ts, provider, model, prompt_tokens, completion_tokens, cost, project_tag \
             FROM logs WHERE ts >= datetime('now', ?2) ORDER BY id DESC LIMIT ?1",
        )?;
        let modifier = format!("-{d} days");
        stmt.query_map(params![limit, modifier], row_to_log)?.collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, ts, provider, model, prompt_tokens, completion_tokens, cost, project_tag \
             FROM logs ORDER BY id DESC LIMIT ?1",
        )?;
        stmt.query_map(params![limit], row_to_log)?.collect()
    }
}

pub fn update_provider_models(
    conn: &Connection,
    id: i64,
    models: &[String],
) -> rusqlite::Result<()> {
    let s = serde_json::to_string(models).unwrap_or_default();
    conn.execute(
        "UPDATE providers SET models = ?1 WHERE id = ?2",
        params![s, id],
    )?;
    Ok(())
}

pub fn today_spend(conn: &Connection) -> rusqlite::Result<f64> {
    // date('now','start of day') is UTC midnight — matches Utc::now() logging.
    conn.query_row(
        "SELECT COALESCE(SUM(cost), 0.0) FROM logs WHERE ts >= datetime('now','start of day')",
        [],
        |row| row.get(0),
    )
}

pub fn list_providers(conn: &Connection) -> rusqlite::Result<Vec<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, base_url, format, auth, models, input_cost, output_cost, is_default FROM providers ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        let format_str: String = row.get(3)?;
        let auth_str: String = row.get(4)?;
        let models_str: String = row.get(5)?;
        let is_default: i64 = row.get(8)?;
        Ok(Provider {
            id: row.get(0)?,
            name: row.get(1)?,
            base_url: row.get(2)?,
            format: ProviderFormat::from_db_str(&format_str),
            auth: AuthScheme::from_db_str(&auth_str),
            models: serde_json::from_str(&models_str).unwrap_or_default(),
            input_cost_per_1k: row.get(6)?,
            output_cost_per_1k: row.get(7)?,
            is_default: is_default != 0,
        })
    })?;
    rows.collect()
}

pub fn insert_provider(conn: &Connection, p: &crate::config::ProviderInput) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO providers (name, base_url, format, auth, models, input_cost, output_cost, is_default) \
         VALUES (?,?,?,?,?,?,?,?)",
        params![
            p.name,
            p.base_url,
            p.format.as_db_str(),
            p.auth.as_db_str(),
            serde_json::to_string(&p.models).unwrap_or_default(),
            p.input_cost_per_1k,
            p.output_cost_per_1k,
            p.is_default as i64,
        ],
    )?;
    if p.is_default {
        // only one default per format family
        let id = conn.last_insert_rowid();
        conn.execute(
            "UPDATE providers SET is_default = 0 WHERE id != ?1 AND format = ?2",
            params![id, p.format.as_db_str()],
        )?;
    }
    Ok(conn.last_insert_rowid())
}

pub fn delete_provider(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .ok()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn load_config(conn: &Connection) -> rusqlite::Result<Config> {
    let providers = list_providers(conn)?;
    let port = get_setting(conn, "port")
        .and_then(|v| v.parse().ok())
        .unwrap_or(3742);
    let budget = get_setting(conn, "budget")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let accurate_streaming = get_setting(conn, "accurate_streaming")
        .map(|v| v == "true" || v.is_empty())
        .unwrap_or(true);
    Ok(Config {
        providers,
        port,
        budget,
        accurate_streaming,
    })
}
