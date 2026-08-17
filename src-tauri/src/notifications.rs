#![cfg(feature = "gui")]

//! Desktop notifications for limit warnings, errors, and state changes.

use crate::notifier::Notifier;
use tauri::{AppHandle, Wry};
use tauri_plugin_notification::NotificationExt;

pub struct TauriNotifier {
    app: AppHandle<Wry>,
}

impl TauriNotifier {
    pub fn new(app: AppHandle<Wry>) -> Self {
        Self { app }
    }
    pub fn handle(&self) -> AppHandle<Wry> {
        self.app.clone()
    }
}

impl Notifier for TauriNotifier {
    fn limit_warning(&self, name: &str, used: f64, cap: f64) {
        show(
            &self.app,
            "Token Guard — Limit warning",
            &format!("{name}: {used:.2} / {cap:.2}"),
        );
    }
    fn limit_blocked(&self, name: &str, used: f64, cap: f64) {
        show(
            &self.app,
            "Token Guard — Request blocked",
            &format!("{name} exceeded ({used:.2} / {cap:.2}). Request returned 429."),
        );
    }
    fn limit_paused(&self, name: &str, used: f64, cap: f64) {
        show(
            &self.app,
            "Token Guard — Proxy paused",
            &format!("{name} exceeded ({used:.2} / {cap:.2}). Proxy is paused."),
        );
    }
    fn proxy_paused(&self) {
        show(
            &self.app,
            "Token Guard",
            "Proxy paused — requests are blocked.",
        );
    }
    fn proxy_resumed(&self) {
        show(
            &self.app,
            "Token Guard",
            "Proxy resumed — requests are flowing.",
        );
    }
}

fn show(app: &AppHandle<Wry>, title: &str, body: &str) {
    let app_for_notif = app.clone();
    let title = title.to_string();
    let body = body.to_string();
    let _ = app.clone().run_on_main_thread(move || {
        let _ = app_for_notif
            .notification()
            .builder()
            .title(&title)
            .body(&body)
            .show();
    });
}
