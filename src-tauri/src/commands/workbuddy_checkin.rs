use crate::services::workbuddy_checkin::{AccountList, LoginStart, LoginStatus, Manager, Status};
use std::sync::Arc;

#[tauri::command]
pub(crate) async fn get_workbuddy_checkin_status(
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<Status, String> {
    manager.status().await
}

#[tauri::command]
pub(crate) async fn check_in_workbuddy(
    manager: tauri::State<'_, Arc<Manager>>,
    account_id: Option<String>,
) -> Result<Status, String> {
    match account_id {
        Some(id) => manager.check_in_for(&id).await,
        None => manager.check_in(true).await,
    }
}

#[tauri::command]
pub(crate) async fn set_workbuddy_auto_checkin(
    manager: tauri::State<'_, Arc<Manager>>,
    enabled: bool,
    account_id: Option<String>,
) -> Result<Status, String> {
    match account_id {
        Some(id) => manager.set_auto_for(&id, enabled).await,
        None => manager.set_auto_enabled(enabled).await,
    }
}

#[tauri::command]
pub(crate) async fn get_workbuddy_checkin_accounts(
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<AccountList, String> {
    manager.accounts().await
}

#[tauri::command]
pub(crate) async fn start_workbuddy_checkin_login(
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<LoginStart, String> {
    manager.start_login().await
}

#[tauri::command]
pub(crate) async fn poll_workbuddy_checkin_login(
    manager: tauri::State<'_, Arc<Manager>>,
    login_id: String,
) -> Result<LoginStatus, String> {
    manager.poll_login(&login_id).await
}

#[tauri::command]
pub(crate) async fn cancel_workbuddy_checkin_login(
    manager: tauri::State<'_, Arc<Manager>>,
    login_id: String,
) -> Result<(), String> {
    manager.cancel_login(&login_id).await;
    Ok(())
}

#[tauri::command]
pub(crate) async fn remove_workbuddy_checkin_account(
    manager: tauri::State<'_, Arc<Manager>>,
    account_id: String,
) -> Result<(), String> {
    manager.remove_account(&account_id).await
}
