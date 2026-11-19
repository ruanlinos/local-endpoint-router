use tauri::State;

use crate::config::settings::{validate_settings, Settings};
use crate::state::AppState;

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn update_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    let settings = validate_settings(&state.db, settings).await?;
    crate::storage::database::save_settings(&state.db, &settings).await?;
    *state.settings.write().await = settings.clone();
    state.traffic.configure(
        settings.redact_sensitive,
        crate::traffic::capture_limit(settings.capture_body, settings.body_limit_bytes),
    );
    super::publish(&state).await?;
    Ok(settings)
}
