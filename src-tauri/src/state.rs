//! Global application state shared between the Tauri shell and the proxy.

use crate::config::{Config, Limit, LimitMetric, LimitScope, Project, Provider, ProviderFormat};
use crate::db::{self, DbPool};
use crate::notifications;

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
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

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
use tauri::{AppHandle, Manager, Wry};

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
    pub config: RwLock<Config>,
    pub paused: AtomicBool,
    pub client: reqwest::Client,
    pub app: AppHandle<Wry>,
    /// Per-limit cooldown so warning notifications don't spam every request.
    last_warning: Mutex<HashMap<i64, Instant>>,
}

impl AppState {
    pub fn new(
        pool: DbPool,
        config: Config,
        app: AppHandle<Wry>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| format!("reqwest client build failed: {e}"))?;
        Ok(Self {
            db: pool,
            config: RwLock::new(config),
            paused: AtomicBool::new(false),
            client,
            app,
            last_warning: Mutex::new(HashMap::new()),
        })
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
        let now = Instant::now();
        for limit in &cfg.limits {
            if !limit.enabled {
                continue;
            }
            if !limit_scope_matches(limit, provider_id, project_tag, &cfg.projects) {
                continue;
            }

            let used = db::usage_for_limit(&conn, limit).unwrap_or(0.0);
            // Add the current in-flight request's contribution.
            let current = match limit.metric {
                LimitMetric::Money => cost,
                LimitMetric::Tokens => tokens as f64,
                LimitMetric::Requests => 1.0,
                LimitMetric::TimeSec => duration_ms as f64 / 1000.0,
            };
            let total = used + current;
            if limit.cap > 0.0 && total >= limit.cap {
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
                    if let Ok(mut map) = self.last_warning.lock() {
                        map.insert(limit.id, now);
                    }
                }
            }
        }
        violations
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
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(state) = tray.app_handle().try_state::<Arc<AppState>>() {
                    state.inner().toggle_pause();
                }
            }
        })
        .build(app)?;
    Ok(())
}

/// Menu item click handler (registered in lib.rs).
pub fn handle_menu_event(app: &AppHandle<Wry>, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        "open" | "settings" => {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
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
                })
                .collect(),
            input_cost_per_1k: None,
            output_cost_per_1k: None,
            is_default,
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
