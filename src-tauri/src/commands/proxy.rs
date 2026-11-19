use tauri::State;

use crate::proxy::{start_with_settings, ProxyStatus};
use crate::state::AppState;

#[tauri::command]
pub async fn proxy_status(state: State<'_, AppState>) -> Result<ProxyStatus, String> {
    let settings = state.settings.read().await.clone();
    Ok(crate::proxy::status_for(state.proxy.address().await, &settings))
}

#[tauri::command]
pub async fn start_proxy(state: State<'_, AppState>) -> Result<ProxyStatus, String> {
    let settings = state.settings.read().await.clone();
    Ok(start_with_settings(&state.proxy, &settings, state.engine.clone(), state.traffic.clone(), state.ca.clone()).await?)
}

#[tauri::command]
pub async fn stop_proxy(state: State<'_, AppState>) -> Result<ProxyStatus, String> {
    state.proxy.stop().await;
    if let Err(err) = state.system.restore() {
        tracing::error!(error = %err, "could not restore the system proxy");
    }
    proxy_status(state).await
}
