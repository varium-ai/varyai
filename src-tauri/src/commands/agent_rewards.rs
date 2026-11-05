use crate::services::agent_rewards::{LoginStart, LoginStatus, Manager, Status};
use std::sync::Arc;

#[tauri::command]
pub(crate) async fn connect_qoder_native_rewards(
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<Status, String> {
    manager.connect_qoder_native().await
}

#[tauri::command]
pub(crate) async fn get_agent_rewards(
    app: String,
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<Status, String> {
    manager.status(&app).await
}
#[tauri::command]
pub(crate) async fn claim_agent_rewards(
    app: String,
    account_id: String,
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<Status, String> {
    manager.claim(&app, &account_id).await
}
#[tauri::command]
pub(crate) async fn set_agent_rewards_auto(
    app: String,
    account_id: String,
    enabled: bool,
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<Status, String> {
    manager.set_auto(&app, &account_id, enabled).await
}
#[tauri::command]
pub(crate) async fn start_agent_rewards_login(
    app: String,
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<LoginStart, String> {
    manager.start_login(&app).await
}
#[tauri::command]
pub(crate) async fn poll_agent_rewards_login(
    app: String,
    login_id: String,
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<LoginStatus, String> {
    manager.poll_login(&app, &login_id).await
}

#[tauri::command]
pub(crate) async fn cancel_agent_rewards_login(
    app: String,
    login_id: String,
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<(), String> {
    manager.cancel_login(&app, &login_id).await
}

#[tauri::command]
pub(crate) async fn forget_agent_rewards_account(
    app: String,
    account_id: String,
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<(), String> {
    manager.forget(&app, &account_id).await
}
