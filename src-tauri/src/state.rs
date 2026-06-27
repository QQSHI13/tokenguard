//! Global application state shared between the Tauri shell and the proxy.

use crate::config::{Config, Provider, ProviderFormat};
use crate::db;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const ICON_GREEN: &[u8] = include_bytes!("../icons/icon_green.png");
const ICON_YELLOW: &[u8] = include_bytes!("../icons/icon_yellow.png");
const ICON_RED: &[u8] = include_bytes!("../icons/icon_red.png");

pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
    pub config: RwLock<Config>,
    pub paused: AtomicBool,
    pub client: reqwest::Client,
    pub app: AppHandle<Wry>,
}

impl AppState {
    pub fn new(
        conn: rusqlite::Connection,
        config: Config,
        app: AppHandle<Wry>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| format!("reqwest client build failed: {e}"))?;
        Ok(Self {
            db: Mutex::new(conn),
            config: RwLock::new(config),
            paused: AtomicBool::new(false),
            client,
            app,
        })
    }

    /// Route a request to a provider by (format family, model name).
    /// Falls back to the default provider for that family, then None.
    pub fn route_provider(
        &self,
        family: ProviderFormat,
        model: &str,
    ) -> Option<Provider> {
        let cfg = self.config.read().unwrap();
        if !model.is_empty() {
            if let Some(p) = cfg
                .providers
                .iter()
                .find(|p| p.format == family && p.models.iter().any(|m| m == model))
                .cloned()
            {
                return Some(p);
            }
        }
        cfg.providers
            .iter()
            .find(|p| p.format == family && p.is_default)
            .cloned()
            .or_else(|| cfg.providers.iter().find(|p| p.format == family).cloned())
    }

    pub fn today_spend(&self) -> f64 {
        let Ok(conn) = self.db.lock() else {
            return 0.0;
        };
        db::today_spend(&conn).unwrap_or(0.0)
    }

    /// Map a client-supplied API key (the label_key the user set in their
    /// coding agent) to a project name. None if no project matches.
    pub fn project_for_key(&self, key: &str) -> Option<String> {
        self.config
            .read()
            .unwrap()
            .projects
            .iter()
            .find(|p| p.label_key == key)
            .map(|p| p.name.clone())
    }

    /// Insert a request log from a background thread, then refresh the tray.
    pub async fn log_request(
        self: Arc<Self>,
        provider: Provider,
        model: String,
        prompt: u64,
        completion: u64,
        cost: f64,
        project_tag: Option<String>,
    ) {
        tokio::task::spawn_blocking(move || {
            if let Ok(conn) = self.db.lock() {
                let _ = db::insert_log(
                    &conn,
                    &provider.name,
                    &model,
                    prompt,
                    completion,
                    cost,
                    project_tag.as_deref(),
                );
            }
            self.refresh_tray();
        })
        .await
        .ok();
    }

    /// Rebuild the tray menu + tooltip + icon color from current state.
    pub fn refresh_tray(&self) {
        let spend = self.today_spend();
        let budget = self.config.read().unwrap().budget;
        let paused = self.paused.load(Ordering::Relaxed);

        let icon_bytes = if budget > 0.0 {
            let ratio = spend / budget;
            if ratio >= 1.0 {
                ICON_RED
            } else if ratio >= 0.8 {
                ICON_YELLOW
            } else {
                ICON_GREEN
            }
        } else {
            ICON_GREEN
        };

        let tooltip = format!(
            "Token Guard — ${spend:.2} today{paused} / ${budget:.2} budget",
            paused = if paused { " (paused)" } else { "" },
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
        &format!("Today's spend: ${spend:.2}"),
        false,
        None::<&str>,
    )?;
    let open_item = MenuItem::with_id(app, "open", "Open Dashboard", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let pause_item = MenuItem::with_id(
        app,
        "pause",
        if paused { "Resume proxy" } else { "Pause proxy" },
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
            app.exit(0);
        }
        _ => {}
    }
}
