//! Copilot model discovery and quota commands.
//! Authentication uses the shared managed-auth command interface.

use crate::proxy::providers::copilot_auth::{
    CopilotAuthManager, CopilotModel, CopilotUsageResponse,
};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Backend-owned Copilot authentication state.
pub struct CopilotAuthState(pub Arc<RwLock<CopilotAuthManager>>);

/// List models using the default account.
#[tauri::command]
pub async fn copilot_get_models(
    state: State<'_, CopilotAuthState>,
) -> Result<Vec<CopilotModel>, String> {
    let auth_manager = state.0.read().await;
    auth_manager.fetch_models().await.map_err(|e| e.to_string())
}

/// List models for a selected account.
#[tauri::command(rename_all = "camelCase")]
pub async fn copilot_get_models_for_account(
    account_id: String,
    state: State<'_, CopilotAuthState>,
) -> Result<Vec<CopilotModel>, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .fetch_models_for_account(&account_id)
        .await
        .map_err(|e| e.to_string())
}

/// Read usage for the default account.
#[tauri::command]
pub async fn copilot_get_usage(
    state: State<'_, CopilotAuthState>,
) -> Result<CopilotUsageResponse, String> {
    let auth_manager = state.0.read().await;
    auth_manager.fetch_usage().await.map_err(|e| e.to_string())
}

/// Read usage for a selected account.
#[tauri::command(rename_all = "camelCase")]
pub async fn copilot_get_usage_for_account(
    account_id: String,
    state: State<'_, CopilotAuthState>,
) -> Result<CopilotUsageResponse, String> {
    let auth_manager = state.0.read().await;
    auth_manager
        .fetch_usage_for_account(&account_id)
        .await
        .map_err(|e| e.to_string())
}
