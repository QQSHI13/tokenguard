//! SQLite: schema, queries, config load.

use crate::config::{
    AuthScheme, BudgetPeriod, Config, Limit, LimitAction, LimitInput, LimitMetric, LimitPeriod,
    LimitScope, ModelMapping, Provider, ProviderFormat,
};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};

pub type DbPool = Pool<SqliteConnectionManager>;

struct Migration {
    id: i64,
    name: &'static str,
    apply: fn(&Connection) -> rusqlite::Result<()>,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        id: 1,
        name: "initial_schema",
        apply: migration_001_initial_schema,
    },
    Migration {
        id: 2,
        name: "logs_duration_ms",
        apply: migration_002_logs_duration_ms,
    },
    Migration {
        id: 3,
        name: "provider_costs_into_models",
        apply: migration_003_provider_costs_into_models,
    },
    Migration {
        id: 4,
        name: "provider_fallback",
        apply: migration_004_provider_fallback,
    },
    Migration {
        id: 5,
        name: "logs_status",
        apply: migration_005_logs_status,
    },
    Migration {
        id: 6,
        name: "logs_bodies",
        apply: migration_006_logs_bodies,
    },
    Migration {
        id: 7,
        name: "limit_schedules",
        apply: migration_007_limit_schedules,
    },
    Migration {
        id: 8,
        name: "project_budgets",
        apply: migration_008_project_budgets,
    },
    Migration {
        id: 9,
        name: "provider_extra_headers",
        apply: migration_009_provider_extra_headers,
    },
];

fn migration_001_initial_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS logs (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          ts TEXT NOT NULL,
          provider TEXT NOT NULL,
          model TEXT NOT NULL,
          prompt_tokens INTEGER NOT NULL,
          completion_tokens INTEGER NOT NULL,
          cost REAL NOT NULL,
          duration_ms INTEGER NOT NULL DEFAULT 0,
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
        CREATE TABLE IF NOT EXISTS projects (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL,
          label_key TEXT NOT NULL UNIQUE
        );
        CREATE TABLE IF NOT EXISTS limits (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          name TEXT NOT NULL,
          metric TEXT NOT NULL,
          period TEXT NOT NULL,
          period_value INTEGER NOT NULL DEFAULT 0,
          cap REAL NOT NULL,
          warning_threshold REAL NOT NULL DEFAULT 0.8,
          scope TEXT NOT NULL DEFAULT 'global',
          scope_id INTEGER,
          action TEXT NOT NULL DEFAULT 'warn',
          enabled INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_logs_ts ON logs(ts);
        CREATE INDEX IF NOT EXISTS idx_logs_provider ON logs(provider);
        CREATE INDEX IF NOT EXISTS idx_logs_project ON logs(project_tag);
        ",
    )
}

fn migration_002_logs_duration_ms(conn: &Connection) -> rusqlite::Result<()> {
    // Older DBs created before duration_ms existed. Ignore "duplicate column" errors.
    let _ = conn.execute(
        "ALTER TABLE logs ADD COLUMN duration_ms INTEGER NOT NULL DEFAULT 0",
        [],
    );
    Ok(())
}

fn migration_006_logs_bodies(conn: &Connection) -> rusqlite::Result<()> {
    let _ = conn.execute("ALTER TABLE logs ADD COLUMN request_body TEXT", []);
    let _ = conn.execute("ALTER TABLE logs ADD COLUMN response_body TEXT", []);
    Ok(())
}

fn migration_007_limit_schedules(conn: &Connection) -> rusqlite::Result<()> {
    let _ = conn.execute("ALTER TABLE limits ADD COLUMN active_hours_start TEXT", []);
    let _ = conn.execute("ALTER TABLE limits ADD COLUMN active_hours_end TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE limits ADD COLUMN active_days INTEGER NOT NULL DEFAULT 127",
        [],
    );
    Ok(())
}

fn migration_008_project_budgets(conn: &Connection) -> rusqlite::Result<()> {
    let _ = conn.execute("ALTER TABLE projects ADD COLUMN budget REAL NOT NULL DEFAULT 0", []);
    let _ = conn.execute(
        "ALTER TABLE projects ADD COLUMN budget_period TEXT NOT NULL DEFAULT 'daily'",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE projects ADD COLUMN budget_action TEXT NOT NULL DEFAULT 'warn'",
        [],
    );
    Ok(())
}

fn migration_009_provider_extra_headers(conn: &Connection) -> rusqlite::Result<()> {
    let _ = conn.execute("ALTER TABLE providers ADD COLUMN extra_headers TEXT", []);
    Ok(())
}

fn migration_005_logs_status(conn: &Connection) -> rusqlite::Result<()> {
    let _ = conn.execute(
        "ALTER TABLE logs ADD COLUMN status INTEGER",
        [],
    );
    Ok(())
}

fn migration_004_provider_fallback(conn: &Connection) -> rusqlite::Result<()> {
    let _ = conn.execute(
        "ALTER TABLE providers ADD COLUMN fallback_provider_id INTEGER",
        [],
    );
    Ok(())
}

fn migration_003_provider_costs_into_models(conn: &Connection) -> rusqlite::Result<()> {
    // Move per-provider input/output costs into each model mapping, then drop
    // the provider-level columns.
    let mut stmt = conn.prepare("SELECT id, input_cost, output_cost, models FROM providers")?;
    let rows: Vec<(i64, Option<f64>, Option<f64>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<f64>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    for (id, input_cost, output_cost, models_json) in rows {
        let mut models: Vec<crate::config::ModelMapping> =
            serde_json::from_str(&models_json).unwrap_or_default();
        for m in &mut models {
            if m.input_cost_per_1k.is_none() {
                m.input_cost_per_1k = input_cost;
            }
            if m.output_cost_per_1k.is_none() {
                m.output_cost_per_1k = output_cost;
            }
        }
        let new_json = serde_json::to_string(&models).unwrap_or_default();
        conn.execute(
            "UPDATE providers SET models = ?1 WHERE id = ?2",
            params![new_json, id],
        )?;
    }

    // SQLite supports dropping columns only in newer versions; ignore failures.
    let _ = conn.execute("ALTER TABLE providers DROP COLUMN input_cost", []);
    let _ = conn.execute("ALTER TABLE providers DROP COLUMN output_cost", []);
    Ok(())
}

/// Apply pending migrations in order, tracked by the `migrations` table.
pub fn run_migrations(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS migrations (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;
    let max_applied: i64 = conn
        .query_row("SELECT COALESCE(MAX(id), 0) FROM migrations", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);
    for mig in MIGRATIONS {
        if mig.id > max_applied {
            let tx = conn.transaction()?;
            (mig.apply)(&tx)?;
            tx.execute(
                "INSERT INTO migrations (id, name) VALUES (?1, ?2)",
                params![mig.id, mig.name],
            )?;
            tx.commit()?;
        }
    }
    Ok(())
}

fn setup_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(())
}

#[cfg(test)]
pub fn connect(path: &str) -> rusqlite::Result<Connection> {
    let mut conn = Connection::open(path)?;
    setup_connection(&conn)?;
    run_migrations(&mut conn)?;
    Ok(conn)
}

/// Build an r2d2 pool for the SQLite database at `path`.
///
/// The manager opens a new connection per pool request; `WAL` mode plus
/// `PRAGMA foreign_keys` are applied via the connection customizer.
/// Migrations run once on the first connection before the pool is returned.
pub fn build_pool(path: &str) -> Result<DbPool, Box<dyn std::error::Error>> {
    let manager = SqliteConnectionManager::file(path);
    let pool = Pool::builder()
        .max_size(8)
        .connection_customizer(Box::new(SqliteCustomizer))
        .build(manager)?;
    // Eagerly verify schema and run pending migrations on one connection.
    {
        let mut conn = pool.get()?;
        run_migrations(&mut conn)?;
    }
    Ok(pool)
}

#[derive(Debug)]
struct SqliteCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for SqliteCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        setup_connection(conn)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn insert_log(
    conn: &Connection,
    provider: &str,
    model: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    cost: f64,
    duration_ms: u64,
    project_tag: Option<&str>,
    status: Option<u16>,
    request_body: Option<&str>,
    response_body: Option<&str>,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO logs (ts, provider, model, prompt_tokens, completion_tokens, cost, duration_ms, project_tag, status, request_body, response_body) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
        params![now, provider, model, prompt_tokens, completion_tokens, cost, duration_ms, project_tag, status, request_body, response_body],
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
    pub duration_ms: u64,
    pub project_tag: Option<String>,
    pub status: Option<u16>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
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
        duration_ms: row.get(7)?,
        project_tag: row.get(8)?,
        status: row.get(9)?,
        request_body: row.get(10)?,
        response_body: row.get(11)?,
    })
}

#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub page: u64,
    pub page_size: u64,
}

pub fn list_logs(
    conn: &Connection,
    limit: u64,
    _days: Option<u64>,
) -> rusqlite::Result<Vec<LogRow>> {
    list_logs_filtered(conn, &LogFilter { page_size: limit, ..LogFilter::default() })
}

/// List logs with optional filters and pagination. Returns the requested page
/// ordered by id descending.
pub fn list_logs_filtered(
    conn: &Connection,
    filter: &LogFilter,
) -> rusqlite::Result<Vec<LogRow>> {
    let mut sql = String::from(
        "SELECT id, ts, provider, model, prompt_tokens, completion_tokens, cost, duration_ms, project_tag, status, request_body, response_body \
         FROM logs WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(p) = &filter.provider {
        sql.push_str(" AND provider = ?");
        params.push(Box::new(p.clone()));
    }
    if let Some(m) = &filter.model {
        sql.push_str(" AND model = ?");
        params.push(Box::new(m.clone()));
    }
    if let Some(proj) = &filter.project {
        sql.push_str(" AND project_tag = ?");
        params.push(Box::new(proj.clone()));
    }
    if let Some(start) = &filter.start {
        sql.push_str(" AND ts >= ?");
        params.push(Box::new(start.clone()));
    }
    if let Some(end) = &filter.end {
        sql.push_str(" AND ts < ?");
        params.push(Box::new(end.clone()));
    }

    sql.push_str(" ORDER BY id DESC LIMIT ? OFFSET ?");
    let page_size = filter.page_size.max(1);
    let offset = filter.page.saturating_sub(1) * page_size;
    params.push(Box::new(page_size));
    params.push(Box::new(offset));

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), row_to_log)?;
    rows.collect::<rusqlite::Result<Vec<LogRow>>>()
}

/// Fetch a single log row by id.
pub fn get_log(conn: &Connection, id: i64) -> rusqlite::Result<Option<LogRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, ts, provider, model, prompt_tokens, completion_tokens, cost, duration_ms, project_tag, status, request_body, response_body \
         FROM logs WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], row_to_log)?;
    rows.next().transpose()
}

/// Count logs matching the same filters (without pagination).
pub fn count_logs_filtered(conn: &Connection, filter: &LogFilter) -> rusqlite::Result<u64> {
    let mut sql = String::from("SELECT COUNT(*) FROM logs WHERE 1=1");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(p) = &filter.provider {
        sql.push_str(" AND provider = ?");
        params.push(Box::new(p.clone()));
    }
    if let Some(m) = &filter.model {
        sql.push_str(" AND model = ?");
        params.push(Box::new(m.clone()));
    }
    if let Some(proj) = &filter.project {
        sql.push_str(" AND project_tag = ?");
        params.push(Box::new(proj.clone()));
    }
    if let Some(start) = &filter.start {
        sql.push_str(" AND ts >= ?");
        params.push(Box::new(start.clone()));
    }
    if let Some(end) = &filter.end {
        sql.push_str(" AND ts < ?");
        params.push(Box::new(end.clone()));
    }

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.query_row(&sql, param_refs.as_slice(), |row| row.get(0))
}

pub fn list_projects(conn: &Connection) -> rusqlite::Result<Vec<crate::config::Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, label_key, budget, budget_period, budget_action FROM projects ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        let period_str: String = row.get(4).unwrap_or_else(|_| "daily".to_string());
        let action_str: String = row.get(5).unwrap_or_else(|_| "warn".to_string());
        Ok(crate::config::Project {
            id: row.get(0)?,
            name: row.get(1)?,
            label_key: row.get(2)?,
            budget: row.get(3).unwrap_or(0.0),
            budget_period: BudgetPeriod::from_db_str(&period_str),
            budget_action: LimitAction::from_db_str(&action_str),
        })
    })?;
    rows.collect()
}

pub fn insert_project(
    conn: &Connection,
    input: &crate::config::ProjectInput,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO projects (name, label_key, budget, budget_period, budget_action) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            input.name,
            input.label_key,
            input.budget,
            input.budget_period.as_db_str(),
            input.budget_action.as_db_str(),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_project(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn update_provider_models(
    conn: &Connection,
    id: i64,
    models: &[ModelMapping],
) -> rusqlite::Result<()> {
    let s = serde_json::to_string(models).unwrap_or_default();
    conn.execute(
        "UPDATE providers SET models = ?1 WHERE id = ?2",
        params![s, id],
    )?;
    Ok(())
}

pub fn update_provider(
    conn: &Connection,
    id: i64,
    p: &crate::config::ProviderInput,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE providers SET name = ?1, base_url = ?2, format = ?3, auth = ?4, \
         models = ?5, is_default = ?6, fallback_provider_id = ?7, extra_headers = ?8 WHERE id = ?9",
        params![
            p.name,
            p.base_url,
            p.format.as_db_str(),
            p.auth.as_db_str(),
            serde_json::to_string(&p.models).unwrap_or_default(),
            p.is_default as i64,
            p.fallback_provider_id,
            serde_json::to_string(&p.extra_headers).unwrap_or_default(),
            id,
        ],
    )?;
    if p.is_default {
        conn.execute(
            "UPDATE providers SET is_default = 0 WHERE id != ?1 AND format = ?2",
            params![id, p.format.as_db_str()],
        )?;
    }
    Ok(())
}

pub fn today_spend(conn: &Connection) -> rusqlite::Result<f64> {
    // All timestamps are stored in UTC (chrono::Utc::now().to_rfc3339()).
    // SQLite's 'now' is UTC, so 'start of day' is UTC midnight.
    conn.query_row(
        "SELECT COALESCE(SUM(cost), 0.0) FROM logs WHERE ts >= datetime('now','start of day')",
        [],
        |row| row.get(0),
    )
}

/// Sum cost for a project over the given budget period. `project_tag` is the
/// project name (stored as `project_tag` in logs).
pub fn project_period_spend(
    conn: &Connection,
    project_tag: &str,
    period: BudgetPeriod,
) -> rusqlite::Result<f64> {
    let cutoff = chrono::Utc::now() - chrono::TimeDelta::seconds(period.seconds() as i64);
    conn.query_row(
        "SELECT COALESCE(SUM(cost), 0.0) FROM logs WHERE project_tag = ?1 AND ts >= ?2",
        params![project_tag, cutoff.to_rfc3339()],
        |row| row.get(0),
    )
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DailyUsage {
    pub day: String,
    pub cost: f64,
    pub tokens: u64,
    pub requests: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MonthlyUsage {
    pub month: String,
    pub cost: f64,
    pub tokens: u64,
    pub requests: u64,
}

/// Aggregate usage per day for a given provider (by name) over the last `days`.
/// `days = 0` means all history.
pub fn provider_daily_usage(
    conn: &Connection,
    provider_name: &str,
    days: u64,
) -> rusqlite::Result<Vec<DailyUsage>> {
    let mut sql = String::from(
        "SELECT date(ts), COALESCE(SUM(cost),0.0), COALESCE(SUM(prompt_tokens+completion_tokens),0), COUNT(*) \
         FROM logs WHERE provider = ?1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    params.push(Box::new(provider_name.to_string()));
    if days > 0 {
        sql.push_str(" AND ts >= datetime('now', ?2)");
        params.push(Box::new(format!("-{days} days")));
    }
    sql.push_str(" GROUP BY date(ts) ORDER BY day");
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(DailyUsage {
            day: row.get(0)?,
            cost: row.get(1)?,
            tokens: row.get(2)?,
            requests: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// Aggregate usage per day for a given project tag over the last `days`.
/// `days = 0` means all history. `project_tag = None` aggregates untagged requests.
pub fn project_daily_usage(
    conn: &Connection,
    project_tag: Option<&str>,
    days: u64,
) -> rusqlite::Result<Vec<DailyUsage>> {
    let mut sql = String::from(
        "SELECT date(ts), COALESCE(SUM(cost),0.0), COALESCE(SUM(prompt_tokens+completion_tokens),0), COUNT(*) \
         FROM logs WHERE project_tag IS ?1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    params.push(Box::new(project_tag));
    if days > 0 {
        sql.push_str(" AND ts >= datetime('now', ?2)");
        params.push(Box::new(format!("-{days} days")));
    }
    sql.push_str(" GROUP BY date(ts) ORDER BY day");
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(DailyUsage {
            day: row.get(0)?,
            cost: row.get(1)?,
            tokens: row.get(2)?,
            requests: row.get(3)?,
        })
    })?;
    rows.collect()
}

/// Aggregate usage per month for the last `months` months (including the current month).
pub fn monthly_usage(conn: &Connection, months: u32) -> rusqlite::Result<Vec<MonthlyUsage>> {
    let sql = "SELECT strftime('%Y-%m', ts), COALESCE(SUM(cost),0.0), COALESCE(SUM(prompt_tokens+completion_tokens),0), COUNT(*) \
               FROM logs WHERE ts >= datetime('now', 'start of month', ?1) \
               GROUP BY strftime('%Y-%m', ts) ORDER BY month";
    let offset = format!("-{} months", months.saturating_sub(1));
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([&offset], |row| {
        Ok(MonthlyUsage {
            month: row.get(0)?,
            cost: row.get(1)?,
            tokens: row.get(2)?,
            requests: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn list_providers(conn: &Connection) -> rusqlite::Result<Vec<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, base_url, format, auth, models, is_default, fallback_provider_id, extra_headers FROM providers ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| {
        let format_str: String = row.get(3)?;
        let auth_str: String = row.get(4)?;
        let models_str: String = row.get(5)?;
        let is_default: i64 = row.get(6)?;
        let fallback_provider_id: Option<i64> = row.get(7)?;
        let extra_headers_str: Option<String> = row.get(8)?;
        let models = parse_models(&models_str);
        let extra_headers = extra_headers_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Ok(Provider {
            id: row.get(0)?,
            name: row.get(1)?,
            base_url: row.get(2)?,
            format: ProviderFormat::from_db_str(&format_str),
            auth: AuthScheme::from_db_str(&auth_str),
            models,
            is_default: is_default != 0,
            fallback_provider_id,
            extra_headers,
        })
    })?;
    rows.collect()
}

/// Parse the models JSON column, migrating legacy arrays of strings into
/// ModelMapping entries where local == remote.
fn parse_models(s: &str) -> Vec<ModelMapping> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    // Try the new format first.
    if let Ok(mappings) = serde_json::from_str::<Vec<ModelMapping>>(s) {
        return mappings;
    }
    // Fall back to legacy array of strings.
    if let Ok(names) = serde_json::from_str::<Vec<String>>(s) {
        return names
            .into_iter()
            .map(|name| ModelMapping {
                local: name.clone(),
                remote: name,
                input_cost_per_1k: None,
                output_cost_per_1k: None,
                cached_input_cost_per_1k: None,
            })
            .collect();
    }
    Vec::new()
}

pub fn insert_provider(
    conn: &Connection,
    p: &crate::config::ProviderInput,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO providers (name, base_url, format, auth, models, is_default, fallback_provider_id, extra_headers) \
         VALUES (?,?,?,?,?,?,?,?)",
        params![
            p.name,
            p.base_url,
            p.format.as_db_str(),
            p.auth.as_db_str(),
            serde_json::to_string(&p.models).unwrap_or_default(),
            p.is_default as i64,
            p.fallback_provider_id,
            serde_json::to_string(&p.extra_headers).unwrap_or_default(),
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

// ---- limits ----

fn row_to_limit(row: &rusqlite::Row) -> rusqlite::Result<Limit> {
    let metric_str: String = row.get(2)?;
    let period_str: String = row.get(3)?;
    let period_value: i64 = row.get(4)?;
    let scope_str: String = row.get(7)?;
    let action_str: String = row.get(9)?;
    let enabled: i64 = row.get(10)?;
    let active_days: i64 = row.get(13).unwrap_or(127);

    let period = if period_str == "custom_sec" {
        LimitPeriod::CustomSec(period_value as u64)
    } else {
        LimitPeriod::from_db_str(&period_str)
    };

    Ok(Limit {
        id: row.get(0)?,
        name: row.get(1)?,
        metric: LimitMetric::from_db_str(&metric_str),
        period,
        cap: row.get(5)?,
        warning_threshold: row.get(6)?,
        scope: LimitScope::from_db_str(&scope_str),
        scope_id: row.get(8)?,
        action: LimitAction::from_db_str(&action_str),
        enabled: enabled != 0,
        active_hours_start: row.get(11)?,
        active_hours_end: row.get(12)?,
        active_days: active_days as u8,
    })
}

pub fn list_limits(conn: &Connection) -> rusqlite::Result<Vec<Limit>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, metric, period, period_value, cap, warning_threshold, scope, scope_id, action, enabled, \
         active_hours_start, active_hours_end, active_days FROM limits ORDER BY id",
    )?;
    let rows = stmt.query_map([], row_to_limit)?;
    rows.collect()
}

pub fn insert_limit(conn: &Connection, l: &LimitInput) -> rusqlite::Result<i64> {
    let (period_str, period_value) = match l.period {
        LimitPeriod::CustomSec(s) => ("custom_sec", s as i64),
        p => (p.as_db_str(), 0),
    };
    conn.execute(
        "INSERT INTO limits (name, metric, period, period_value, cap, warning_threshold, scope, scope_id, action, enabled, \
         active_hours_start, active_hours_end, active_days) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
        params![
            l.name,
            l.metric.as_db_str(),
            period_str,
            period_value,
            l.cap,
            l.warning_threshold,
            l.scope.as_db_str(),
            l.scope_id,
            l.action.as_db_str(),
            l.enabled as i64,
            l.active_hours_start,
            l.active_hours_end,
            l.active_days as i64,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_limit(conn: &Connection, id: i64, l: &LimitInput) -> rusqlite::Result<()> {
    let (period_str, period_value) = match l.period {
        LimitPeriod::CustomSec(s) => ("custom_sec", s as i64),
        p => (p.as_db_str(), 0),
    };
    conn.execute(
        "UPDATE limits SET name = ?1, metric = ?2, period = ?3, period_value = ?4, cap = ?5, \
         warning_threshold = ?6, scope = ?7, scope_id = ?8, action = ?9, enabled = ?10, \
         active_hours_start = ?11, active_hours_end = ?12, active_days = ?13 WHERE id = ?14",
        params![
            l.name,
            l.metric.as_db_str(),
            period_str,
            period_value,
            l.cap,
            l.warning_threshold,
            l.scope.as_db_str(),
            l.scope_id,
            l.action.as_db_str(),
            l.enabled as i64,
            l.active_hours_start,
            l.active_hours_end,
            l.active_days as i64,
            id,
        ],
    )?;
    Ok(())
}

pub fn delete_limit(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM limits WHERE id = ?1", params![id])?;
    Ok(())
}

/// Sum the metric for a limit over its rolling period.
/// For `Once` limits, sums over all history.
/// Rate-based metrics (RPM/TPM) always use a 60-second rolling window.
pub fn usage_for_limit(conn: &Connection, limit: &Limit) -> rusqlite::Result<f64> {
    let mut sql = String::from("SELECT COALESCE(SUM(");
    let column = match limit.metric {
        LimitMetric::Money => "cost",
        LimitMetric::Tokens | LimitMetric::TokensPerMinute => "prompt_tokens + completion_tokens",
        LimitMetric::Requests | LimitMetric::RequestsPerMinute => "1",
        LimitMetric::TimeSec => "duration_ms / 1000.0",
    };
    sql.push_str(column);
    sql.push_str("), 0.0) FROM logs WHERE 1=1");

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    let window_seconds = if limit.metric.is_rate() {
        Some(60)
    } else {
        limit.period.seconds()
    };

    if let Some(seconds) = window_seconds {
        // Compute the cutoff timestamp in Rust to keep the query parameterised.
        let cutoff = chrono::Utc::now() - chrono::TimeDelta::seconds(seconds as i64);
        sql.push_str(" AND ts >= ?");
        params.push(Box::new(cutoff.to_rfc3339()));
    }

    match limit.scope {
        LimitScope::Provider => {
            if let Some(id) = limit.scope_id {
                let name: Option<String> = conn
                    .query_row(
                        "SELECT name FROM providers WHERE id = ?1",
                        params![id],
                        |r| r.get(0),
                    )
                    .ok();
                if let Some(name) = name {
                    sql.push_str(" AND provider = ?");
                    params.push(Box::new(name));
                }
            }
        }
        LimitScope::Project => {
            if let Some(id) = limit.scope_id {
                let name: Option<String> = conn
                    .query_row(
                        "SELECT name FROM projects WHERE id = ?1",
                        params![id],
                        |r| r.get(0),
                    )
                    .ok();
                if let Some(name) = name {
                    sql.push_str(" AND project_tag = ?");
                    params.push(Box::new(name));
                }
            }
        }
        LimitScope::Global => {}
    }

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.query_row(&sql, param_refs.as_slice(), |row| row.get(0))
}

/// Migrate the legacy `settings.budget` value into a global daily money limit.
/// Does nothing if a limit already exists or if budget is 0.
pub fn migrate_legacy_budget(conn: &Connection) -> rusqlite::Result<()> {
    let existing: i64 = conn.query_row("SELECT COUNT(*) FROM limits", [], |r| r.get(0))?;
    if existing > 0 {
        return Ok(());
    }
    let budget = get_setting(conn, "budget")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    if budget <= 0.0 {
        return Ok(());
    }
    let limit = LimitInput {
        name: "Daily budget".to_string(),
        metric: LimitMetric::Money,
        period: LimitPeriod::Daily,
        cap: budget,
        warning_threshold: 0.8,
        scope: LimitScope::Global,
        scope_id: None,
        action: LimitAction::Warn,
        enabled: true,
        active_hours_start: None,
        active_hours_end: None,
        active_days: 0b1111111,
    };
    insert_limit(conn, &limit)?;
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
    let projects = list_projects(conn)?;
    migrate_legacy_budget(conn)?;
    let limits = list_limits(conn)?;
    let port = get_setting(conn, "port")
        .and_then(|v| v.parse().ok())
        .unwrap_or(3742);
    let budget = get_setting(conn, "budget")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let log_bodies = get_setting(conn, "log_bodies")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let auto_export_days = get_setting(conn, "auto_export_days")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let auto_export_folder = get_setting(conn, "auto_export_folder");
    let webhook_url = get_setting(conn, "webhook_url").filter(|s| !s.is_empty());
    let auto_start = get_setting(conn, "auto_start")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    Ok(Config {
        providers,
        projects,
        limits,
        port,
        budget,
        log_bodies,
        auto_export_days,
        auto_export_folder,
        webhook_url,
        auto_start,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderInput;

    fn temp_db() -> (Connection, tempfile::TempPath) {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.into_temp_path();
        let conn = connect(path.to_str().unwrap()).unwrap();
        (conn, path)
    }

    fn sample_provider(name: &str) -> ProviderInput {
        ProviderInput {
            name: name.to_string(),
            base_url: "https://example.com".to_string(),
            format: ProviderFormat::OpenAI,
            auth: AuthScheme::Bearer,
            api_key: "sk-test".to_string(),
            models: vec![ModelMapping {
                local: "gpt-4o".to_string(),
                remote: "gpt-4o".to_string(),
                input_cost_per_1k: Some(1.0),
                output_cost_per_1k: Some(2.0),
                cached_input_cost_per_1k: None,
            }],
            is_default: true,
            clear_key: false,
            fallback_provider_id: None,
            extra_headers: Vec::new(),
        }
    }

    #[test]
    fn provider_round_trip() {
        let (conn, _path) = temp_db();
        let input = sample_provider("TestProvider");
        let id = insert_provider(&conn, &input).unwrap();

        let providers = list_providers(&conn).unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, id);
        assert_eq!(providers[0].name, "TestProvider");
        assert_eq!(providers[0].models.len(), 1);
        assert_eq!(providers[0].models[0].local, "gpt-4o");
        assert_eq!(providers[0].models[0].remote, "gpt-4o");
        assert!(providers[0].is_default);
    }

    #[test]
    fn provider_default_uniqueness_per_format() {
        let (conn, _path) = temp_db();
        let mut a = sample_provider("A");
        a.format = ProviderFormat::OpenAI;
        a.is_default = true;

        let mut b = sample_provider("B");
        b.format = ProviderFormat::OpenAI;
        b.is_default = true;

        insert_provider(&conn, &a).unwrap();
        insert_provider(&conn, &b).unwrap();

        let providers = list_providers(&conn).unwrap();
        let defaults: Vec<_> = providers.iter().filter(|p| p.is_default).collect();
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0].name, "B");
    }

    #[test]
    fn project_round_trip() {
        let (conn, _path) = temp_db();
        let input = crate::config::ProjectInput {
            name: "cursor-app".into(),
            label_key: "tg_abc123".into(),
            budget: 10.0,
            budget_period: crate::config::BudgetPeriod::Daily,
            budget_action: crate::config::LimitAction::Warn,
        };
        let id = insert_project(&conn, &input).unwrap();
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, id);
        assert_eq!(projects[0].name, "cursor-app");
        assert_eq!(projects[0].budget, 10.0);

        delete_project(&conn, id).unwrap();
        assert!(list_projects(&conn).unwrap().is_empty());
    }

    #[test]
    fn log_insert_and_list() {
        let (conn, _path) = temp_db();
        insert_log(
            &conn,
            "OpenAI",
            "gpt-4o",
            10,
            5,
            0.003,
            1200,
            Some("cursor-app"),
            Some(200),
            None,
            None,
        )
        .unwrap();
        insert_log(&conn, "OpenAI", "gpt-4o-mini", 100, 50, 0.001, 800, None, Some(200), None, None).unwrap();

        let logs = list_logs(&conn, 10, None).unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].model, "gpt-4o-mini"); // DESC order
        assert_eq!(logs[1].project_tag.as_deref(), Some("cursor-app"));
    }

    #[test]
    fn today_spend_uses_utc() {
        let (conn, _path) = temp_db();
        let now = chrono::Utc::now();
        let ts = now.to_rfc3339();
        conn.execute(
            "INSERT INTO logs (ts, provider, model, prompt_tokens, completion_tokens, cost, duration_ms, project_tag) VALUES (?1, 'p', 'm', 1, 1, 1.23, 500, NULL)",
            params![ts],
        )
        .unwrap();

        let spend = today_spend(&conn).unwrap();
        assert!((spend - 1.23).abs() < 0.001);
    }

    #[test]
    fn settings_round_trip() {
        let (conn, _path) = temp_db();
        set_setting(&conn, "budget", "12.50").unwrap();
        assert_eq!(get_setting(&conn, "budget").as_deref(), Some("12.50"));
    }

    #[test]
    fn load_config_defaults() {
        let (conn, _path) = temp_db();
        let cfg = load_config(&conn).unwrap();
        assert_eq!(cfg.port, 3742);
        assert_eq!(cfg.budget, 0.0);
    }

    #[test]
    fn limit_crud_and_usage() {
        let (conn, _path) = temp_db();
        let limit = LimitInput {
            name: "Daily tokens".into(),
            metric: LimitMetric::Tokens,
            period: LimitPeriod::Daily,
            cap: 100.0,
            warning_threshold: 0.8,
            scope: LimitScope::Global,
            scope_id: None,
            action: LimitAction::Warn,
            enabled: true,
            active_hours_start: None,
            active_hours_end: None,
            active_days: 0b1111111,
        };
        let id = insert_limit(&conn, &limit).unwrap();

        let limits = list_limits(&conn).unwrap();
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].id, id);
        assert_eq!(limits[0].cap, 100.0);

        insert_log(&conn, "OpenAI", "gpt-4o", 30, 20, 0.001, 100, None, Some(200), None, None).unwrap();
        let used = usage_for_limit(&conn, &limits[0]).unwrap();
        assert_eq!(used, 50.0);

        let mut updated = limit.clone();
        updated.cap = 200.0;
        update_limit(&conn, id, &updated).unwrap();
        let limits = list_limits(&conn).unwrap();
        assert_eq!(limits[0].cap, 200.0);

        delete_limit(&conn, id).unwrap();
        assert!(list_limits(&conn).unwrap().is_empty());
    }

    #[test]
    fn limit_provider_scope() {
        let (conn, _path) = temp_db();
        let provider = sample_provider("ScopedProvider");
        let provider_id = insert_provider(&conn, &provider).unwrap();

        let limit = LimitInput {
            name: "Provider tokens".into(),
            metric: LimitMetric::Tokens,
            period: LimitPeriod::Daily,
            cap: 1000.0,
            warning_threshold: 0.8,
            scope: LimitScope::Provider,
            scope_id: Some(provider_id),
            action: LimitAction::Warn,
            enabled: true,
            active_hours_start: None,
            active_hours_end: None,
            active_days: 0b1111111,
        };
        let limit_id = insert_limit(&conn, &limit).unwrap();
        let limits = list_limits(&conn).unwrap();
        let found = limits.iter().find(|l| l.id == limit_id).unwrap();

        insert_log(&conn, "ScopedProvider", "m", 10, 10, 0.0, 100, None, Some(200), None, None).unwrap();
        insert_log(&conn, "Other", "m", 100, 100, 0.0, 100, None, Some(200), None, None).unwrap();

        let used = usage_for_limit(&conn, found).unwrap();
        assert_eq!(used, 20.0);
    }

    #[test]
    fn limit_provider_scope_with_quotes_is_safe() {
        let (conn, _path) = temp_db();
        let provider = sample_provider("O'Reilly \"AI\"");
        let provider_id = insert_provider(&conn, &provider).unwrap();

        let limit = LimitInput {
            name: "Quoted provider tokens".into(),
            metric: LimitMetric::Tokens,
            period: LimitPeriod::Daily,
            cap: 1000.0,
            warning_threshold: 0.8,
            scope: LimitScope::Provider,
            scope_id: Some(provider_id),
            action: LimitAction::Warn,
            enabled: true,
            active_hours_start: None,
            active_hours_end: None,
            active_days: 0b1111111,
        };
        let limit_id = insert_limit(&conn, &limit).unwrap();
        let limits = list_limits(&conn).unwrap();
        let found = limits.iter().find(|l| l.id == limit_id).unwrap();

        insert_log(&conn, "O'Reilly \"AI\"", "m", 5, 5, 0.0, 100, None, Some(200), None, None).unwrap();
        insert_log(&conn, "Other", "m", 100, 100, 0.0, 100, None, Some(200), None, None).unwrap();

        let used = usage_for_limit(&conn, found).unwrap();
        assert_eq!(used, 10.0);
    }

    #[test]
    fn limit_custom_period() {
        let (conn, _path) = temp_db();
        let limit = LimitInput {
            name: "5 hours".into(),
            metric: LimitMetric::TimeSec,
            period: LimitPeriod::CustomSec(5 * 3600),
            cap: 5.0 * 3600.0,
            warning_threshold: 0.8,
            scope: LimitScope::Global,
            scope_id: None,
            action: LimitAction::Warn,
            enabled: true,
            active_hours_start: None,
            active_hours_end: None,
            active_days: 0b1111111,
        };
        let id = insert_limit(&conn, &limit).unwrap();
        let limits = list_limits(&conn).unwrap();
        assert_eq!(limits[0].id, id);
        assert_eq!(limits[0].period.seconds(), Some(5 * 3600));
    }

    #[test]
    fn legacy_budget_migration() {
        let (conn, _path) = temp_db();
        set_setting(&conn, "budget", "25.00").unwrap();
        load_config(&conn).unwrap();
        let limits = list_limits(&conn).unwrap();
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].metric, LimitMetric::Money);
        assert_eq!(limits[0].cap, 25.0);
        assert_eq!(limits[0].period, LimitPeriod::Daily);
    }
}
