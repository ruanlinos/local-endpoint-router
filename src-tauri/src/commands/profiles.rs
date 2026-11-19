use tauri::State;

use crate::config::settings::Settings;
use crate::profiles::manager::{self, ProfileDocument};
use crate::state::AppState;
use crate::storage::database::{self, Profile};

#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<Vec<Profile>, String> {
    Ok(database::list_profiles(&state.db).await?)
}

#[tauri::command]
pub async fn create_profile(state: State<'_, AppState>, name: String) -> Result<Profile, String> {
    Ok(database::insert_profile(&state.db, &name).await?)
}

#[tauri::command]
pub async fn rename_profile(state: State<'_, AppState>, id: String, name: String) -> Result<Profile, String> {
    Ok(database::rename_profile(&state.db, &id, &name).await?)
}

#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, id: String) -> Result<Settings, String> {
    let settings = database::delete_profile(&state.db, &id).await?;
    *state.settings.write().await = settings.clone();
    super::publish(&state).await?;
    Ok(settings)
}

#[tauri::command]
pub async fn set_active_profile(state: State<'_, AppState>, id: String) -> Result<Settings, String> {
    if !database::profile_exists(&state.db, &id).await? {
        return Err("profile not found".into());
    }
    let mut settings = state.settings.read().await.clone();
    settings.active_profile_id = id;
    let settings = crate::config::settings::validate_settings(&state.db, settings).await?;
    database::save_settings(&state.db, &settings).await?;
    *state.settings.write().await = settings.clone();
    super::publish(&state).await?;
    Ok(settings)
}

#[tauri::command]
pub async fn export_profile(state: State<'_, AppState>, id: String) -> Result<ProfileDocument, String> {
    Ok(manager::export_profile(&state.db, &id).await?)
}

#[tauri::command]
pub async fn import_profile(
    state: State<'_, AppState>,
    document: ProfileDocument,
) -> Result<Profile, String> {
    Ok(manager::import_profile(&state.db, document).await?)
}
