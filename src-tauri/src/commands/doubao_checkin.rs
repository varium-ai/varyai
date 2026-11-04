use crate::services::doubao_checkin::{Manager, Snapshot};
use std::sync::Arc;

#[tauri::command]
pub(crate) async fn get_doubao_checkin_accounts(
    manager: tauri::State<'_, Arc<Manager>>,
) -> Result<Snapshot, String> {
    manager.status().await
}

#[tauri::command]
pub(crate) async fn check_in_doubao(
    manager: tauri::State<'_, Arc<Manager>>,
    account_id: String,
) -> Result<Snapshot, String> {
    manager.check_in(&account_id).await
}

#[tauri::command]
pub(crate) async fn set_doubao_auto_checkin(
    manager: tauri::State<'_, Arc<Manager>>,
    account_id: String,
    enabled: bool,
) -> Result<Snapshot, String> {
    manager.set_auto(&account_id, enabled).await
}
