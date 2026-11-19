use tauri::State;

use crate::routing::rules::{validate_rule_input, Rule, RuleInput};
use crate::state::AppState;
use crate::storage::database;

use super::publish;

#[tauri::command]
pub async fn list_rules(state: State<'_, AppState>) -> Result<Vec<Rule>, String> {
    let profile_id = state.settings.read().await.active_profile_id.clone();
    Ok(database::list_rules(&state.db, &profile_id).await?)
}

#[tauri::command]
pub async fn create_rule(state: State<'_, AppState>, rule: RuleInput) -> Result<Rule, String> {
    let profile_id = state.settings.read().await.active_profile_id.clone();
    let normalized = validate_rule_input(&rule)?;
    let created = database::insert_rule(&state.db, &profile_id, &normalized).await?;
    publish(&state).await?;
    Ok(created)
}

#[tauri::command]
pub async fn update_rule(state: State<'_, AppState>, id: String, rule: RuleInput) -> Result<Rule, String> {
    let normalized = validate_rule_input(&rule)?;
    let updated = database::update_rule(&state.db, &id, &normalized).await?;
    publish(&state).await?;
    Ok(updated)
}

#[tauri::command]
pub async fn delete_rule(state: State<'_, AppState>, id: String) -> Result<(), String> {
    database::delete_rule(&state.db, &id).await?;
    publish(&state).await?;
    Ok(())
}

#[tauri::command]
pub async fn set_rule_enabled(state: State<'_, AppState>, id: String, enabled: bool) -> Result<Rule, String> {
    let updated = database::set_rule_enabled(&state.db, &id, enabled).await?;
    publish(&state).await?;
    Ok(updated)
}
