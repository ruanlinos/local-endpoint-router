use std::time::Duration;

use tauri::State;

use crate::health::HealthReport;
use crate::proxy::upstream::Upstream;
use crate::state::AppState;

#[tauri::command]
pub async fn list_health(state: State<'_, AppState>) -> Result<Vec<HealthReport>, String> {
    Ok(state.health.list())
}

#[tauri::command]
pub async fn check_health(state: State<'_, AppState>) -> Result<Vec<HealthReport>, String> {
    let upstream = Upstream::new(Duration::from_secs(2))?;
    state.health.check(&state.engine, &upstream).await;
    Ok(state.health.list())
}
