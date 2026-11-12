use crate::services::{native_subscription as accounts, routing_upstreams::SubscriptionProvider};

#[tauri::command]
pub(crate) async fn native_subscription_accounts(
    provider: SubscriptionProvider,
) -> Result<Vec<accounts::AccountSummary>, String> {
    accounts::list(provider).await
}
#[tauri::command]
pub(crate) async fn native_subscription_start(
    provider: SubscriptionProvider,
    target_account_id: Option<String>,
    proxy: Option<String>,
) -> Result<accounts::Flow, String> {
    accounts::start(provider, target_account_id, proxy).await
}
#[tauri::command]
pub(crate) async fn native_subscription_poll(id: String) -> Result<accounts::Flow, String> {
    accounts::poll(&id).await
}
#[tauri::command]
pub(crate) async fn native_subscription_cancel(id: String) {
    accounts::cancel(&id).await
}
#[tauri::command]
pub(crate) async fn native_subscription_callback(id: String, url: String) -> Result<(), String> {
    accounts::submit_callback(&id, &url).await
}
#[tauri::command]
pub(crate) async fn native_subscription_import(
    provider: SubscriptionProvider,
    path: Option<String>,
) -> Result<accounts::AccountSummary, String> {
    accounts::import(provider, path).await
}
#[tauri::command]
pub(crate) async fn native_subscription_remove(
    provider: SubscriptionProvider,
    id: String,
) -> Result<(), String> {
    accounts::remove(provider, &id).await
}
#[tauri::command]
pub(crate) async fn native_subscription_project(
    provider: SubscriptionProvider,
    id: String,
    project: String,
) -> Result<(), String> {
    accounts::project(provider, &id, project).await
}
#[tauri::command]
pub(crate) async fn native_subscription_models(
    provider: SubscriptionProvider,
    id: String,
) -> Result<Vec<serde_json::Value>, String> {
    accounts::models(provider, &id).await
}

#[tauri::command]
pub(crate) async fn native_subscription_quota(
    provider: SubscriptionProvider,
    id: String,
) -> Result<crate::services::subscription::SubscriptionQuota, String> {
    let mut quota = accounts::quota(provider, &id).await?;
    quota.error = None;
    quota.credential_message = None;
    Ok(quota)
}
