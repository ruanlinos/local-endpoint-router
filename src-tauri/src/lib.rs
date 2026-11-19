mod commands;
mod config;
mod error;
mod health;
mod logging;
mod profiles;
mod proxy;
mod routing;
mod state;
mod storage;
mod system;
mod tls;
mod traffic;
mod util;

use tauri::Manager;

use crate::state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("router.db");
            let state = tauri::async_runtime::block_on(AppState::open(&db_path))
                .map_err(|err| -> Box<dyn std::error::Error> { err.to_string().into() })?;
            let autostart = state
                .settings
                .try_read()
                .map(|settings| settings.autostart_proxy)
                .unwrap_or(false);
            state.traffic.attach(app.handle().clone());
            state.health.attach(app.handle().clone());
            let health = state.health.clone();
            let engine = state.engine.clone();
            tauri::async_runtime::spawn(async move {
                health.run(engine).await;
            });
            if let Err(err) = state.system.restore() {
                tracing::error!(error = %err, "could not restore the system proxy from a previous session");
            }
            let backup = state.system.backup_path().to_path_buf();
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let _ = crate::system::restore_file(&backup);
                previous(info);
            }));
            app.manage(state);
            if autostart {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    let settings = state.settings.read().await.clone();
                    if let Err(err) = crate::proxy::start_with_settings(
                        &state.proxy,
                        &settings,
                        state.engine.clone(),
                        state.traffic.clone(),
                        state.ca.clone(),
                    )
                    .await
                    {
                        tracing::error!(error = %err, "autostart failed");
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::profiles::list_profiles,
            commands::profiles::create_profile,
            commands::profiles::rename_profile,
            commands::profiles::delete_profile,
            commands::profiles::set_active_profile,
            commands::profiles::export_profile,
            commands::profiles::import_profile,
            commands::rules::list_rules,
            commands::rules::create_rule,
            commands::rules::update_rule,
            commands::rules::delete_rule,
            commands::rules::set_rule_enabled,
            commands::proxy::proxy_status,
            commands::proxy::start_proxy,
            commands::proxy::stop_proxy,
            commands::traffic::list_traffic,
            commands::traffic::clear_traffic,
            commands::system::system_proxy_status,
            commands::system::enable_system_proxy,
            commands::system::disable_system_proxy,
            commands::ca::ca_status,
            commands::ca::trust_ca,
            commands::health::list_health,
            commands::health::check_health,
        ])
        .build(tauri::generate_context!())
        .expect("failed to start Local Endpoint Router")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    tauri::async_runtime::block_on(async {
                        state.proxy.stop().await;
                        if let Err(err) = state.system.restore() {
                            tracing::error!(error = %err, "could not restore the system proxy on quit");
                        }
                    });
                }
            }
        });
}
