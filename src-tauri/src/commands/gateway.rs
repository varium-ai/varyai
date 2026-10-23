use crate::{
    proxy::gateway::{CreatedKey, GatewayKey, KeyPolicy},
    store::AppState,
};
use tauri::State;

#[tauri::command]
pub async fn list_gateway_keys(state: State<'_, AppState>) -> Result<Vec<GatewayKey>, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.list_gateway_keys()
            .map_err(|_| "gatewayKeys.failed".to_string())
    })
    .await
    .map_err(|_| "gatewayKeys.failed".to_string())?
}
#[tauri::command]
pub async fn create_gateway_key(
    state: State<'_, AppState>,
    policy: KeyPolicy,
) -> Result<CreatedKey, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.create_gateway_key(policy)
            .map_err(|_| "gatewayKeys.failed".to_string())
    })
    .await
    .map_err(|_| "gatewayKeys.failed".to_string())?
}
#[tauri::command]
pub async fn update_gateway_key(
    state: State<'_, AppState>,
    id: String,
    policy: KeyPolicy,
) -> Result<(), String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.update_gateway_key(&id, policy)
            .map_err(|_| "gatewayKeys.failed".to_string())
    })
    .await
    .map_err(|_| "gatewayKeys.failed".to_string())?
}
#[tauri::command]
pub async fn revoke_gateway_key(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.revoke_gateway_key(&id)
            .map_err(|_| "gatewayKeys.failed".to_string())
    })
    .await
    .map_err(|_| "gatewayKeys.failed".to_string())?
}
