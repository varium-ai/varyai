use crate::services::workbuddy_checkin::{Manager, Status};
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
) -> Result<Status, String> {
    manager.check_in(true).await
}

#[tauri::command]
pub(crate) async fn set_workbuddy_auto_checkin(
    manager: tauri::State<'_, Arc<Manager>>,
    enabled: bool,
) -> Result<Status, String> {
    manager.set_auto_enabled(enabled).await
}
