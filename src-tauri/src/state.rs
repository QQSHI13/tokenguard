//! Global application state shared between the Tauri shell and the proxy.

use crate::config::{
    Config, Limit, LimitAction, LimitMetric, LimitScope, Project, Provider, ProviderFormat,
};
use crate::db::{self, DbPool};
use crate::health::{HealthCache, ProviderHealth};
use crate::limits::LimitCounters;
use crate::notifications;
use crate::webhook;

/// Pure routing logic, extracted for unit testing.
pub fn route_in_list(
    providers: &[Provider],
    family: ProviderFormat,
    model: &str,
) -> Option<Provider> {
    if !model.is_empty() {
        if let Some(p) = providers
            .iter()
            .find(|p| p.format == family && p.models.iter().any(|m| m.local == model))
            .cloned()
        {
            return Some(p);
        }
    }
    providers
        .iter()
        .find(|p| p.format == family && p.is_default)
        .cloned()
        .or_else(|| providers.iter().find(|p| p.format == family).cloned())
}

/// Find the remote model name for a given local model name on a provider.
/// Returns the local name as fallback if no mapping exists.
pub fn remote_model_name(provider: &Provider, local_name: &str) -> String {
    provider
        .models
        .iter()
        .find(|m| m.local == local_name)
        .map(|m| m.remote.clone())
        .unwrap_or_else(|| local_name.to_string())
}

/// Find the cached-input cost for a given local model name on a provider.
pub fn cached_input_cost_per_1k(provider: &Provider, local_name: &str) -> Option<f64> {
    provider
        .models
        .iter()
        .find(|m| m.local == local_name)
        .and_then(|m| m.cached_input_cost_per_1k)
}

/// Find the input/output costs for a given local model name on a provider.
pub fn input_output_cost_per_1k(
    provider: &Provider,
    local_name: &str,
) -> (Option<f64>, Option<f64>) {
    provider
        .models
        .iter()
        .find(|m| m.local == local_name)
        .map(|m| (m.input_cost_per_1k, m.output_cost_per_1k))
        .unwrap_or((None, None))
}

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use chrono::{Datelike, Timelike};

/// Pure scope-matching logic, extracted for unit testing.
pub fn limit_scope_matches(
    limit: &Limit,
    provider_id: i64,
    project_tag: Option<&str>,
    projects: &[Project],
) -> bool {
    match limit.scope {
        LimitScope::Global => true,
        LimitScope::Provider => limit.scope_id == Some(provider_id),
        LimitScope::Project => {
            if let Some(pid) = limit.scope_id {
                projects
                    .iter()
                    .find(|p| p.id == pid)
                    .map(|p| project_tag == Some(p.name.as_str()))
                    .unwrap_or(false)
            } else {
                false
            }
        }
    }
}
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

const ICON_GREEN: &[u8] = include_bytes!("../icons/icon_green.png");
const ICON_YELLOW: &[u8] = include_bytes!("../icons/icon_yellow.png");
const ICON_ORANGE: &[u8] = include_bytes!("../icons/icon_orange.png");
const ICON_RED: &[u8] = include_bytes!("../icons/icon_red.png");

#[derive(Debug, Clone)]
pub struct LimitViolation {
    pub limit: Limit,
    pub used: f64,
}

const WARNING_COOLDOWN: Duration = Duration::from_secs(300);

pub struct AppState {
    pub db: DbPool,
    pub db_path: std::path::PathBuf,
    pub config: RwLock<Config>,
    pub paused: AtomicBool,
    pub client: reqwest::Client,
    pub app: AppHandle<Wry>,
    /// Per-limit cooldown so warning notifications don't spam every request.
    last_warning: Mutex<HashMap<i64, Instant>>,
    /// Per-limit cooldown so block/pause notifications don't spam.
    last_block_notify: Mutex<HashMap<i64, Instant>>,
    /// Signal the proxy server to stop accepting new connections on shutdown.
    shutdown_tx: watch::Sender<()>,
    /// In-flight request counters for atomic request-limit enforcement.
    limit_counters: LimitCounters,
    /// Monotonic request IDs for tracing.
    next_request_id: AtomicU64,
    /// Tracks left-clicks on the tray icon to distinguish single vs double clicks.
    tray_click: Mutex<TrayClickState>,
    /// Cached provider health check results.
    provider_health: Arc<Mutex<HealthCache>>,
}

#[derive(Default)]
struct TrayClickState {
    count: u32,
    last_ms: u64,
    timer_running: bool,
}

impl AppState {
    pub fn new(
        pool: DbPool,
        db_path: std::path::PathBuf,
        config: Config,
        app: AppHandle<Wry>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(600))
            .pool_max_idle_per_host(8)
            .user_agent("TokenGuard/0.1.0")
            .build()
            .map_err(|e| format!("reqwest client build failed: {e}"))?;
        let (shutdown_tx, _shutdown_rx) = watch::channel(());
        Ok(Self {
            db: pool,
            db_path,
            config: RwLock::new(config),
            paused: AtomicBool::new(false),
            client,
            app,
            last_warning: Mutex::new(HashMap::new()),
            last_block_notify: Mutex::new(HashMap::new()),
            shutdown_tx,
            limit_counters: LimitCounters::new(),
            next_request_id: AtomicU64::new(1),
            tray_click: Mutex::new(TrayClickState::default()),
            provider_health: Arc::new(Mutex::new(HealthCache::default())),
        })
    }

    /// Signal the proxy server to shut down gracefully.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Subscribe to the shutdown signal for the proxy server.
    pub fn shutdown_rx(&self) -> watch::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Allocate the next request ID for tracing.
    pub fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Check whether a block/pause notification for this limit is still in the
    /// cooldown window. If not, record the current time and return true.
    pub fn should_notify_block(&self, limit_id: i64) -> bool {
        let now = Instant::now();
        self.last_block_notify
            .lock()
            .map(|mut map| {
                let notify = map
                    .get(&limit_id)
                    .map(|last| now.duration_since(*last) >= WARNING_COOLDOWN)
                    .unwrap_or(true);
                if notify {
                    map.insert(limit_id, now);
                }
                notify
            })
            .unwrap_or(true)
    }

    /// Route a request to a provider by (format family, model name).
    /// Falls back to the default provider for that family, then None.
    pub fn route_provider(&self, family: ProviderFormat, model: &str) -> Option<Provider> {
        let Ok(cfg) = self.config.read() else {
            return None;
        };
        route_in_list(&cfg.providers, family, model)
    }

    pub fn today_spend(&self) -> f64 {
        let Ok(conn) = self.db.get() else {
            tracing::error!("failed to get DB connection from pool");
            return 0.0;
        };
        db::today_spend(&conn).unwrap_or(0.0)
    }

    /// Check whether a project's spend in its budget period has exceeded its
    /// budget. Returns `(used, budget, action)` when the budget is configured
    /// and exceeded; otherwise None.
    pub fn check_project_budget(
        &self,
        project_tag: &str,
    ) -> Option<(f64, f64, LimitAction)> {
        let Ok(conn) = self.db.get() else {
            tracing::error!("failed to get DB connection from pool for project budget");
            return None;
        };
        let Ok(cfg) = self.config.read() else {
            return None;
        };
        let project = cfg.projects.iter().find(|p| p.name == project_tag)?;
        if project.budget <= 0.0 {
            return None;
        }
        let used = db::project_period_spend(&conn, project_tag, project.budget_period).unwrap_or(0.0);
        if used >= project.budget {
            Some((used, project.budget, project.budget_action))
        } else {
            None
        }
    }

    /// Map a client-supplied API key (the label_key the user set in their
    /// coding agent) to a project name. None if no project matches.
    pub fn project_for_key(&self, key: &str) -> Option<String> {
        let Ok(cfg) = self.config.read() else {
            return None;
        };
        cfg.projects
            .iter()
            .find(|p| p.label_key == key)
            .map(|p| p.name.clone())
    }

    /// Check enabled limits and return those currently exceeded.
    /// Also emits a desktop warning notification when a limit crosses its
    /// warning threshold (but hasn't hit the cap yet), throttled per limit.
    ///
    /// Request limits are enforced atomically via in-memory counters so a burst
    /// of concurrent requests cannot overshoot the cap between the check and
    /// the log insert.
    pub fn check_limits(
        &self,
        provider_id: i64,
        project_tag: Option<&str>,
        cost: f64,
        tokens: u64,
        duration_ms: u64,
    ) -> Vec<LimitViolation> {
        let Ok(conn) = self.db.get() else {
            tracing::error!("failed to get DB connection from pool for check_limits");
            return Vec::new();
        };
        let Ok(cfg) = self.config.read() else {
            return Vec::new();
        };

        let mut violations = Vec::new();
        let webhook_url = cfg.webhook_url.clone();
        let now = Instant::now();
        for limit in &cfg.limits {
            if !limit.enabled {
                continue;
            }
            if !limit_scope_matches(limit, provider_id, project_tag, &cfg.projects) {
                continue;
            }
            if !is_limit_active(limit) {
                continue;
            }

            let persisted = db::usage_for_limit(&conn, limit).unwrap_or(0.0);

            // For request-based limits, reserve one in the atomic counter first so
            // concurrent requests see each other. If the request is going to be
            // blocked/paused, the caller must call release_request_limit() later.
            // Rate-based metrics use a 60-second rolling window (see db::usage_for_limit).
            let (current, used) = if limit.metric == LimitMetric::Requests
                || limit.metric == LimitMetric::RequestsPerMinute
            {
                let reserved = self.limit_counters.increment(limit, 1.0);
                (1.0, persisted + reserved - 1.0)
            } else {
                let current = match limit.metric {
                    LimitMetric::Money => cost,
                    LimitMetric::Tokens | LimitMetric::TokensPerMinute => tokens as f64,
                    LimitMetric::TimeSec => duration_ms as f64 / 1000.0,
                    LimitMetric::Requests | LimitMetric::RequestsPerMinute => unreachable!(),
                };
                (current, persisted)
            };

            let total = used + current;
            if limit.cap > 0.0 && total >= limit.cap {
                if self.should_notify_block(limit.id) {
                    if let Some(ref url) = webhook_url {
                        webhook::send_limit_event(
                            &self.client,
                            url,
                            "limit_hit",
                            limit,
                            total,
                            limit.cap,
                        );
                    }
                }
                // Blocked/paused requests don't count; the caller will release.
                if limit.metric == LimitMetric::Requests
                    && (limit.action == LimitAction::Block || limit.action == LimitAction::Pause)
                {
                    self.limit_counters.increment(limit, -1.0);
                }
                violations.push(LimitViolation {
                    limit: limit.clone(),
                    used,
                });
                continue;
            }

            // Warning threshold notification (throttled).
            if limit.cap > 0.0
                && limit.warning_threshold > 0.0
                && used >= limit.warning_threshold * limit.cap
            {
                let should_notify = self
                    .last_warning
                    .lock()
                    .map(|map| {
                        map.get(&limit.id)
                            .map(|last| now.duration_since(*last) >= WARNING_COOLDOWN)
                            .unwrap_or(true)
                    })
                    .unwrap_or(true);
                if should_notify {
                    notifications::limit_warning(&self.app, &limit.name, used, limit.cap);
                    if let Some(ref url) = webhook_url {
                        webhook::send_limit_event(
                            &self.client,
                            url,
                            "limit_warning",
                            limit,
                            used,
                            limit.cap,
                        );
                    }
                    if let Ok(mut map) = self.last_warning.lock() {
                        map.insert(limit.id, now);
                    }
                }
            }
        }
        violations
    }

    /// Release a reserved request-limit unit when a request is blocked/paused
    /// before it reaches the upstream provider.
    pub fn release_request_limit(&self, limit: &Limit) {
        if limit.metric == LimitMetric::Requests || limit.metric == LimitMetric::RequestsPerMinute {
            self.limit_counters.increment(limit, -1.0);
        }
    }

    /// Compute the most critical active limit status for the tray.
    /// Returns (overall_ratio, critical_limit_name_or_none).
    pub fn limit_status(&self) -> (f64, Option<String>) {
        let Ok(conn) = self.db.get() else {
            return (0.0, None);
        };
        let Ok(cfg) = self.config.read() else {
            return (0.0, None);
        };

        let mut max_ratio = 0.0;
        let mut critical: Option<String> = None;
        for limit in &cfg.limits {
            if !limit.enabled || limit.cap <= 0.0 {
                continue;
            }
            let used = db::usage_for_limit(&conn, limit).unwrap_or(0.0);
            let ratio = used / limit.cap;
            if ratio > max_ratio {
                max_ratio = ratio;
                critical = Some(limit.name.clone());
            }
        }
        (max_ratio, critical)
    }

    /// Insert a request log from a background thread, then refresh the tray.
    #[allow(clippy::too_many_arguments)]
    pub async fn log_request(
        self: Arc<Self>,
        provider: Provider,
        model: String,
        prompt: u64,
        completion: u64,
        cost: f64,
        duration_ms: u64,
        project_tag: Option<String>,
        status: Option<u16>,
        request_body: Option<String>,
        response_body: Option<String>,
    ) {
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let Ok(conn) = this.db.get() else {
                tracing::error!("failed to get DB connection from pool for log_request");
                this.refresh_tray();
                return;
            };
            if let Err(e) = db::insert_log(
                &conn,
                &provider.name,
                &model,
                prompt,
                completion,
                cost,
                duration_ms,
                project_tag.as_deref(),
                status,
                request_body.as_deref(),
                response_body.as_deref(),
            ) {
                tracing::error!("insert_log failed: {e}");
            }
            this.refresh_tray();
        })
        .await
        .ok();
    }

    /// Rebuild the tray menu + tooltip + icon color from current state.
    pub fn refresh_tray(&self) {
        let spend = self.today_spend();
        let (ratio, critical) = self.limit_status();
        let paused = self.paused.load(Ordering::Relaxed);

        let icon_bytes = if paused {
            ICON_ORANGE
        } else if ratio >= 1.0 {
            ICON_RED
        } else if ratio >= 0.8 {
            ICON_YELLOW
        } else {
            ICON_GREEN
        };

        let tooltip = format!(
            "Token Guard — ${spend:.2} today{paused}{critical}",
            paused = if paused { " (paused)" } else { "" },
            critical = critical
                .map(|c| format!(" — limit: {c}"))
                .unwrap_or_default(),
        );

        if let Some(tray) = self.app.tray_by_id("main") {
            let _ = tray.set_tooltip(Some(&tooltip));
            if let Ok(img) = tauri::image::Image::from_bytes(icon_bytes) {
                let _ = tray.set_icon(Some(img));
            }
            if let Ok(menu) = build_tray_menu(&self.app, spend, paused) {
                let _ = tray.set_menu(Some(menu));
            }
        }
    }

    pub fn all_provider_health(&self) -> std::collections::HashMap<i64, ProviderHealth> {
        self.provider_health.lock().ok().map(|c| c.all()).unwrap_or_default()
    }

    pub fn provider_health_cache(&self) -> Arc<Mutex<HealthCache>> {
        self.provider_health.clone()
    }

    pub fn toggle_pause(&self) -> bool {
        let new = !self.paused.load(Ordering::Relaxed);
        self.paused.store(new, Ordering::Relaxed);
        if new {
            notifications::proxy_paused(&self.app);
        } else {
            notifications::proxy_resumed(&self.app);
        }
        self.refresh_tray();
        new
    }
}

/// Parse a "HH:MM" string into minutes since midnight.
fn parse_minutes(s: &str) -> Option<u32> {
    let mut parts = s.split(':');
    let h: u32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    if h >= 24 || m >= 60 {
        return None;
    }
    Some(h * 60 + m)
}

/// Return true if the current UTC time falls inside the limit's optional
/// schedule. A missing schedule means always active.
pub fn is_limit_active(limit: &Limit) -> bool {
    let now = chrono::Utc::now();

    if limit.active_days != 0b1111111 {
        // Bit 0 = Monday .. bit 6 = Sunday.
        let weekday = now.weekday().num_days_from_monday() as u8;
        if limit.active_days & (1 << weekday) == 0 {
            return false;
        }
    }

    if let (Some(start), Some(end)) = (&limit.active_hours_start, &limit.active_hours_end) {
        if let (Some(start_min), Some(end_min)) = (parse_minutes(start), parse_minutes(end)) {
            let cur = now.hour() as u32 * 60 + now.minute() as u32;
            if start_min <= end_min {
                return cur >= start_min && cur <= end_min;
            } else {
                return cur >= start_min || cur <= end_min;
            }
        }
    }

    true
}

fn build_tray_menu(
    app: &AppHandle<Wry>,
    spend: f64,
    paused: bool,
) -> Result<Menu<Wry>, tauri::Error> {
    let spend_item = MenuItem::with_id(
        app,
        "spend",
        format!("Today's spend: ${spend:.2}"),
        false,
        None::<&str>,
    )?;
    let open_item = MenuItem::with_id(app, "open", "Open Dashboard", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let pause_item = MenuItem::with_id(
        app,
        "pause",
        if paused {
            "Resume proxy"
        } else {
            "Pause proxy"
        },
        true,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let items: [&dyn tauri::menu::IsMenuItem<Wry>; 8] = [
        &spend_item,
        &sep1,
        &open_item,
        &settings_item,
        &sep2,
        &pause_item,
        &sep3,
        &quit_item,
    ];
    Menu::with_items(app, &items)
}

/// Build the tray icon at startup. Left-click toggles pause.
pub fn build_tray(app: &AppHandle<Wry>) -> Result<(), tauri::Error> {
    let menu = build_tray_menu(app, 0.0, false)?;
    let icon = tauri::image::Image::from_bytes(ICON_GREEN)?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Token Guard")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                handle_tray_left_click(app);
            }
        })
        .build(app)?;
    Ok(())
}

/// Show the main window and tell the UI which tab to activate.
fn show_tab(app: &AppHandle<Wry>, tab: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
    let _ = app.emit("set_tab", tab);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Single left-click pauses/resumes; two quick left-clicks open the dashboard.
fn handle_tray_left_click(app: &AppHandle<Wry>) {
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return;
    };
    let t = now_ms();
    let needs_timer = {
        let mut tray = state.tray_click.lock().unwrap();
        tray.count += 1;
        tray.last_ms = t;
        let needs = !tray.timer_running;
        tray.timer_running = needs;
        needs
    };

    if !needs_timer {
        return;
    }

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let (stable, count) = {
                let state = app2.state::<Arc<AppState>>();
                let tray = state.tray_click.lock().unwrap();
                let elapsed = now_ms().saturating_sub(tray.last_ms);
                (elapsed > 250, tray.count)
            };
            if stable {
                let state = app2.state::<Arc<AppState>>();
                let mut tray = state.tray_click.lock().unwrap();
                tray.timer_running = false;
                tray.count = 0;
                drop(tray);
                if count == 1 {
                    state.toggle_pause();
                } else {
                    show_tab(&app2, "dashboard");
                }
                break;
            }
        }
    });
}

/// Menu item click handler (registered in lib.rs).
pub fn handle_menu_event(app: &AppHandle<Wry>, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "open" => show_tab(app, "dashboard"),
        "settings" => show_tab(app, "settings"),
        "pause" => {
            if let Some(state) = app.try_state::<Arc<AppState>>() {
                state.inner().toggle_pause();
            }
        }
        "quit" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                crate::graceful_exit(&app).await;
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthScheme, ProviderFormat};

    fn provider(name: &str, family: ProviderFormat, models: &[&str], is_default: bool) -> Provider {
        Provider {
            id: 0,
            name: name.to_string(),
            base_url: "https://example.com".to_string(),
            format: family,
            auth: AuthScheme::Bearer,
            models: models
                .iter()
                .map(|s| crate::config::ModelMapping {
                    local: s.to_string(),
                    remote: s.to_string(),
                    input_cost_per_1k: None,
                    output_cost_per_1k: None,
                    cached_input_cost_per_1k: None,
                })
                .collect(),
            is_default,
            fallback_provider_id: None,
            extra_headers: Vec::new(),
        }
    }

    #[test]
    fn route_by_model_match() {
        let providers = vec![
            provider("OpenAI", ProviderFormat::OpenAI, &["gpt-4o"], true),
            provider("Anthropic", ProviderFormat::Anthropic, &["claude-3"], true),
        ];
        let chosen = route_in_list(&providers, ProviderFormat::OpenAI, "gpt-4o");
        assert_eq!(chosen.unwrap().name, "OpenAI");
    }

    #[test]
    fn route_falls_back_to_default() {
        let providers = vec![
            provider("OpenAI", ProviderFormat::OpenAI, &["gpt-4o"], true),
            provider("Backup", ProviderFormat::OpenAI, &["gpt-3.5"], false),
        ];
        let chosen = route_in_list(&providers, ProviderFormat::OpenAI, "unknown-model");
        assert_eq!(chosen.unwrap().name, "OpenAI");
    }

    #[test]
    fn route_falls_back_to_any_same_format() {
        let providers = vec![provider(
            "OpenAI",
            ProviderFormat::OpenAI,
            &["gpt-4o"],
            false,
        )];
        let chosen = route_in_list(&providers, ProviderFormat::OpenAI, "other");
        assert_eq!(chosen.unwrap().name, "OpenAI");
    }

    #[test]
    fn route_no_match_for_wrong_format() {
        let providers = vec![provider(
            "OpenAI",
            ProviderFormat::OpenAI,
            &["gpt-4o"],
            true,
        )];
        assert!(route_in_list(&providers, ProviderFormat::Anthropic, "claude-3").is_none());
    }

    fn mk_limit(scope: LimitScope, scope_id: Option<i64>) -> Limit {
        Limit {
            id: 1,
            name: "test".into(),
            metric: LimitMetric::Requests,
            period: crate::config::LimitPeriod::Daily,
            cap: 10.0,
            warning_threshold: 0.8,
            scope,
            scope_id,
            action: crate::config::LimitAction::Warn,
            enabled: true,
            active_hours_start: None,
            active_hours_end: None,
            active_days: 0b1111111,
        }
    }

    #[test]
    fn limit_global_scope_matches_anything() {
        let limit = mk_limit(LimitScope::Global, None);
        assert!(limit_scope_matches(&limit, 99, Some("any"), &[]));
    }

    #[test]
    fn limit_provider_scope_matches_id() {
        let limit = mk_limit(LimitScope::Provider, Some(7));
        assert!(limit_scope_matches(&limit, 7, None, &[]));
        assert!(!limit_scope_matches(&limit, 8, None, &[]));
    }

    #[test]
    fn limit_project_scope_matches_tag() {
        let projects = vec![Project {
            id: 3,
            name: "cursor-app".into(),
            label_key: "tg_abc".into(),
            budget: 0.0,
            budget_period: crate::config::BudgetPeriod::Daily,
            budget_action: crate::config::LimitAction::Warn,
        }];
        let limit = mk_limit(LimitScope::Project, Some(3));
        assert!(limit_scope_matches(
            &limit,
            1,
            Some("cursor-app"),
            &projects
        ));
        assert!(!limit_scope_matches(&limit, 1, Some("other"), &projects));
        assert!(!limit_scope_matches(&limit, 1, None, &projects));
    }
}
