use crate::provider::UsageScript;
use crate::services::pi_state::{PiCurrentState, PiStateService};
use crate::services::ProviderService;
use crate::session_manager::providers::pi::PiSessionDiscovery;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub(crate) fn get_pi_current_state(state: State<'_, AppState>) -> Result<PiCurrentState, String> {
    PiStateService::current(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn update_pi_provider_usage_script(
    state: State<'_, AppState>,
    id: String,
    #[allow(non_snake_case)] usageScript: UsageScript,
) -> Result<bool, String> {
    ProviderService::update_pi_usage_script(state.inner(), &id, usageScript)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn get_pi_session_discovery() -> PiSessionDiscovery {
    crate::session_manager::providers::pi::session_discovery()
}

#[tauri::command]
pub(crate) async fn get_pi_routing_status(
    state: State<'_, AppState>,
) -> Result<crate::services::pi_routing::Status, String> {
    state.proxy_service.pi_routing_status().await
}
#[tauri::command]
pub(crate) async fn preview_pi_routing(
    state: State<'_, AppState>,
    provider_ids: Vec<String>,
) -> Result<crate::services::pi_routing::Status, String> {
    state.proxy_service.preview_pi_routing(&provider_ids).await
}
#[tauri::command]
pub(crate) async fn configure_pi_routing(
    state: State<'_, AppState>,
    provider_ids: Vec<String>,
) -> Result<(), String> {
    state
        .proxy_service
        .configure_pi_routing(&provider_ids)
        .await
}
#[tauri::command]
pub(crate) async fn disable_pi_routing(state: State<'_, AppState>) -> Result<(), String> {
    state.proxy_service.disable_pi_routing().await
}
