use tauri::State;

use crate::state::AppState;
use crate::traffic::TrafficEvent;

#[tauri::command]
pub async fn list_traffic(state: State<'_, AppState>) -> Result<Vec<TrafficEvent>, String> {
    Ok(state.traffic.list())
}

#[tauri::command]
pub async fn clear_traffic(state: State<'_, AppState>) -> Result<(), String> {
    state.traffic.clear();
    Ok(())
}
