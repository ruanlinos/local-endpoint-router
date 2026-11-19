use tauri::State;

use crate::state::AppState;
use crate::tls::CaStatus;

#[tauri::command]
pub async fn ca_status(state: State<'_, AppState>) -> Result<CaStatus, String> {
    Ok(state.ca.status())
}

#[tauri::command]
pub async fn trust_ca(state: State<'_, AppState>) -> Result<CaStatus, String> {
    state.ca.trust()?;
    Ok(state.ca.status())
}
