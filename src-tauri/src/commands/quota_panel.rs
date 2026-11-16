use crate::{
    services::quota_panel::{network::Outcome, Controls, Point, Policy},
    store::AppState,
};
use tauri::State;

#[tauri::command]
pub(crate) async fn quota_panel_history(
    state: State<'_, AppState>,
    key: String,
) -> Result<Vec<Point>, String> {
    state.quota_panel.history(&key).await
}
#[tauri::command]
pub(crate) async fn quota_panel_note(
    state: State<'_, AppState>,
    key: String,
    point: Point,
) -> Result<Vec<Point>, String> {
    state.quota_panel.note(&key, point).await
}
#[tauri::command]
pub(crate) async fn quota_panel_codex(
    state: State<'_, AppState>,
    account: String,
) -> Result<Controls, String> {
    state.quota_panel.controls(&account).await
}
#[tauri::command]
pub(crate) async fn quota_panel_policy(
    state: State<'_, AppState>,
    account: String,
    identity: String,
    policy: Policy,
) -> Result<Policy, String> {
    state.quota_panel.policy(&account, &identity, policy).await
}
#[tauri::command]
pub(crate) async fn quota_panel_reset(
    state: State<'_, AppState>,
    account: String,
    identity: String,
    credit: String,
    confirmed: bool,
) -> Result<Outcome, String> {
    if !confirmed {
        return Err("quotaPanel.confirmRequired".into());
    }
    state
        .quota_panel
        .consume(&account, &identity, &credit, false, false)
        .await
}
