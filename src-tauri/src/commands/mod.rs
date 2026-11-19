pub mod ca;
pub mod health;
pub mod profiles;
pub mod proxy;
pub mod rules;
pub mod settings;
pub mod system;
pub mod traffic;

use crate::state::AppState;

pub async fn publish(state: &AppState) -> Result<(), String> {
    let settings = state.settings.read().await.clone();
    crate::routing::engine::publish(&state.db, &state.engine, &settings)
        .await
        .map_err(String::from)
}
