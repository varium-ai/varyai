use crate::{
    proxy::gateway::{CreatedKey, GatewayKey, KeyPolicy},
    store::AppState,
};
use tauri::State;

#[tauri::command]
pub async fn get_gateway_catalog(
    state: State<'_, AppState>,
) -> Result<crate::proxy::gateway::GatewayCatalog, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::proxy::gateway::catalog(&db).map_err(|_| "gatewayKeys.failed".to_string())
    })
    .await
    .map_err(|_| "gatewayKeys.failed".to_string())?
}

#[tauri::command]
pub async fn bind_gateway_agent(
    state: State<'_, AppState>,
    app: String,
    id: Option<String>,
) -> Result<(), String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.bind_gateway_agent(&app, id.as_deref())
            .map_err(|_| "gatewayKeys.failed".to_string())
    })
    .await
    .map_err(|_| "gatewayKeys.failed".to_string())?
}

#[tauri::command]
pub async fn list_gateway_usage(
    state: State<'_, AppState>,
    id: Option<String>,
) -> Result<Vec<crate::proxy::gateway::GatewayUsage>, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.list_gateway_usage(id.as_deref())
            .map_err(|_| "gatewayKeys.failed".to_string())
    })
    .await
    .map_err(|_| "gatewayKeys.failed".to_string())?
}

#[tauri::command]
pub async fn list_gateway_keys(state: State<'_, AppState>) -> Result<Vec<GatewayKey>, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.list_saved_gateway_keys(&crate::config::get_app_config_dir().join("gateway-secrets"))
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
        db.create_saved_gateway_key(
            policy,
            &crate::config::get_app_config_dir().join("gateway-secrets"),
        )
        .map_err(|error| match error {
            crate::AppError::Message(key) if key == "gatewayKeys.nameExists" => key,
            _ => "gatewayKeys.failed".to_string(),
        })
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
            .map_err(|error| match error {
                crate::AppError::Message(key) if key == "gatewayKeys.nameExists" => key,
                _ => "gatewayKeys.failed".to_string(),
            })
    })
    .await
    .map_err(|_| "gatewayKeys.failed".to_string())?
}

#[tauri::command]
pub async fn reveal_gateway_key(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<String>, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.reveal_gateway_key(
            &id,
            &crate::config::get_app_config_dir().join("gateway-secrets"),
        )
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

#[tauri::command]
pub async fn delete_gateway_key(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        db.delete_saved_gateway_key(
            &id,
            &crate::config::get_app_config_dir().join("gateway-secrets"),
        )
        .map_err(|_| "gatewayKeys.failed".to_string())
    })
    .await
    .map_err(|_| "gatewayKeys.failed".to_string())?
}
