//! Token Guard — local LLM gateway.

mod commands;
mod config;
mod cost;
mod db;
mod proxy;
mod secrets;
mod state;

use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt::try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::add_provider,
            commands::delete_provider,
            commands::set_provider_key,
            commands::get_settings,
            commands::set_budget,
            commands::set_port,
            commands::pause_resume,
            commands::get_today_spend,
            commands::get_logs,
            commands::get_models,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Local-first SQLite in the per-user app data dir.
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("tokenguard.db");
            let conn = db::connect(db_path.to_str().ok_or("invalid db path")?)?;
            let config = db::load_config(&conn)?;
            let port = config.port;

            let state = Arc::new(state::AppState::new(conn, config, handle)?);

            // Native tray (left-click toggles pause; menu items below).
            state::build_tray(app.handle())?;

            app.manage(state.clone());

            // Spawn the loopback proxy in the Tauri tokio runtime.
            let s = state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = proxy::server::serve(s, port).await {
                    tracing::error!("proxy server error: {e}");
                }
            });

            state.refresh_tray();
            tracing::info!("Token Guard started — proxy on http://127.0.0.1:{port}");
            Ok(())
        })
        .on_menu_event(|app, event| state::handle_menu_event(app, event))
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
