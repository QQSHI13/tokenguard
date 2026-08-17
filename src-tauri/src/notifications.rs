//! Desktop notifications for limit warnings, errors, and state changes.

use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::NotificationExt;

fn show<R: Runtime>(app: &AppHandle<R>, title: String, body: String) {
    let app_for_notif = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        let _ = app_for_notif
            .notification()
            .builder()
            .title(&title)
            .body(&body)
            .show();
    });
}

pub fn notify<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    show(app, title.to_string(), body.to_string());
}

pub fn limit_warning<R: Runtime>(app: &AppHandle<R>, name: &str, used: f64, cap: f64) {
    notify(
        app,
        "Token Guard — Limit warning",
        &format!("{name}: {used:.2} / {cap:.2}"),
    );
}

pub fn limit_blocked<R: Runtime>(app: &AppHandle<R>, name: &str, used: f64, cap: f64) {
    notify(
        app,
        "Token Guard — Request blocked",
        &format!("{name} exceeded ({used:.2} / {cap:.2}). Request returned 429."),
    );
}

pub fn limit_paused<R: Runtime>(app: &AppHandle<R>, name: &str, used: f64, cap: f64) {
    notify(
        app,
        "Token Guard — Proxy paused",
        &format!("{name} exceeded ({used:.2} / {cap:.2}). Proxy is paused."),
    );
}

pub fn proxy_paused<R: Runtime>(app: &AppHandle<R>) {
    notify(app, "Token Guard", "Proxy paused — requests are blocked.");
}

pub fn proxy_resumed<R: Runtime>(app: &AppHandle<R>) {
    notify(app, "Token Guard", "Proxy resumed — requests are flowing.");
}
