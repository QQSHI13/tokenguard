//! Token Guard — local LLM gateway.

mod commands;
mod config;
mod cost;
mod db;
mod limits;
mod notifications;
mod prices;
mod proxy;
mod secrets;
mod state;
mod webhook;
mod health;

use std::sync::Arc;
use tauri::{AppHandle, Manager, Wry};

/// Close every webview window, wait briefly for WebView2 teardown, then exit.
///
/// Calling `app.exit(0)` immediately can leave WebView2 mid-shutdown and print
/// `Failed to unregister class Chrome_WidgetWin_0. Error = 1412` on Windows.
/// Closing windows first and sleeping a little lets the renderer clean up.
pub async fn graceful_exit(app: &AppHandle<Wry>) {
    if let Some(state) = app.try_state::<Arc<crate::state::AppState>>() {
        state.shutdown();
    }
    for (_label, window) in app.webview_windows() {
        let _ = window.close();
    }
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt::try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::add_provider,
            commands::update_provider,
            commands::delete_provider,
            commands::set_provider_key,
            commands::get_settings,
            commands::set_budget,
            commands::set_port,
            commands::set_log_bodies,
            commands::set_webhook_url,
            commands::test_webhook,
            commands::pause_resume,
            commands::get_today_spend,
            commands::get_logs,
            commands::write_text_file,
            commands::get_models,
            commands::export_logs,
            commands::refresh_models,
            commands::list_projects,
            commands::add_project,
            commands::delete_project,
            commands::keyring_selftest,
            commands::list_limits,
            commands::add_limit,
            commands::update_limit,
            commands::delete_limit,
            commands::get_limit_status,
            commands::get_limit_presets,
            commands::get_license_key,
            commands::set_license_key,
            commands::delete_license_key,
            commands::get_registered_devices,
            commands::check_for_update,
            commands::download_update,
            commands::install_update,
            commands::get_device_fingerprint,
            commands::get_default_model_prices,
            commands::refresh_model_prices_from_url,
            commands::fill_provider_prices_from_database,
            commands::get_provider_usage,
            commands::get_project_usage,
            commands::get_monthly_usage,
            commands::check_provider_health,
            commands::get_provider_healths,
            commands::get_logs_filtered,
            commands::replay_request,
            commands::backup_database,
            commands::restore_database,
            commands::set_auto_export,
            commands::maybe_run_auto_export,
            commands::run_auto_export_now_cmd,
            commands::get_auto_start,
            commands::set_auto_start,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Local-first SQLite in the per-user app data dir.
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("tokenguard.db");
            let pool = db::build_pool(db_path.to_str().ok_or("invalid db path")?)?;
            let conn = pool
                .get()
                .map_err(|e| format!("failed to get DB connection: {e}"))?;
            let config = db::load_config(&conn)?;
            let port = config.port;

            let state = Arc::new(state::AppState::new(pool, db_path, config, handle)?);

            // Native tray (left-click toggles pause; menu items below).
            state::build_tray(app.handle())?;

            app.manage(state.clone());

            // Spawn the loopback proxy in the Tauri tokio runtime.
            let s = state.clone();
            let shutdown_rx = state.shutdown_rx();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = proxy::server::serve(s, port, shutdown_rx).await {
                    tracing::error!("proxy server error: {e}");
                }
            });

            // Run provider health checks periodically and cache the results.
            let s = state.clone();
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(5 * 60));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    interval.tick().await;
                    let providers = s.config.read().map(|cfg| cfg.providers.clone()).unwrap_or_default();
                    crate::health::refresh_all(&s.client, &providers, s.provider_health_cache()).await;
                }
            });

            state.refresh_tray();
            tracing::info!("Token Guard started — proxy on http://127.0.0.1:{port}");

            // Run scheduled usage export if due.
            if crate::commands::is_auto_export_due(&state).unwrap_or(false) {
                if let Err(e) = crate::commands::run_auto_export_now(&state) {
                    tracing::warn!("auto export failed: {e}");
                }
            }

            // Graceful Ctrl+C: close windows first so WebView2 can tear down
            // before the process exits (avoids the "Failed to unregister class" noise).
            let ah = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    tracing::info!("Ctrl+C received, exiting gracefully");
                    graceful_exit(&ah).await;
                }
            });

            Ok(())
        })
        .on_menu_event(state::handle_menu_event)
        .on_window_event(|window, event| {
            // Close → hide to tray (keeps the proxy running).
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Token Guard");
}
