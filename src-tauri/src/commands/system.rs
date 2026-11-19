use tauri::State;

use crate::proxy::start_with_settings;
use crate::state::AppState;
use crate::system::SystemProxyStatus;

#[tauri::command]
pub async fn system_proxy_status(state: State<'_, AppState>) -> Result<SystemProxyStatus, String> {
    let address = browser_address(&state).await;
    Ok(state.system.status(&address))
}

#[tauri::command]
pub async fn enable_system_proxy(state: State<'_, AppState>) -> Result<SystemProxyStatus, String> {
    let settings = state.settings.read().await.clone();
    let running = start_with_settings(
        &state.proxy,
        &settings,
        state.engine.clone(),
        state.traffic.clone(),
        state.ca.clone(),
    )
    .await?;
    let (host, port) = browser_endpoint(running.address.as_deref(), &settings.bind_host, settings.port);
    state.system.enable(&host, port)?;
    Ok(state.system.status(&format!("{host}:{port}")))
}

#[tauri::command]
pub async fn disable_system_proxy(state: State<'_, AppState>) -> Result<SystemProxyStatus, String> {
    state.system.restore()?;
    let address = browser_address(&state).await;
    Ok(state.system.status(&address))
}

async fn browser_address(state: &AppState) -> String {
    let settings = state.settings.read().await.clone();
    let current = state.proxy.address().await;
    let (host, port) = browser_endpoint(current.as_ref().map(|addr| addr.to_string()).as_deref(), &settings.bind_host, settings.port);
    format!("{host}:{port}")
}

fn browser_endpoint(running: Option<&str>, bind_host: &str, configured_port: u16) -> (String, u16) {
    if let Some((host, port_text)) = running.and_then(split_host_port) {
        let port = port_text.parse().unwrap_or(configured_port);
        let host = if host == "0.0.0.0" || host == "::" || host.is_empty() {
            "127.0.0.1"
        } else {
            host
        };
        return (host.to_string(), port);
    }
    let host = if bind_host == "0.0.0.0" || bind_host == "::" { "127.0.0.1" } else { bind_host };
    (host.to_string(), configured_port)
}

fn split_host_port(value: &str) -> Option<(&str, &str)> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('[') {
        return rest.split_once("]:");
    }
    value.rsplit_once(':')
}
