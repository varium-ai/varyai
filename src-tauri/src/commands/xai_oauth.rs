//! xAI OAuth state and xAI-specific commands.

use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;
use crate::services::grok_subscription;
use crate::services::model_fetch::FetchedModel;
use crate::services::subscription::{CredentialStatus, SubscriptionQuota};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

pub struct XaiOAuthState(pub Arc<RwLock<XaiOAuthManager>>);

/// Read the selected managed Grok account through the CLI billing contract.
/// A rejected access token is renewed once; transient failures retain cached usage.
pub(crate) async fn query_xai_oauth_quota_for(
    state: &XaiOAuthState,
    account_id: Option<String>,
) -> Result<SubscriptionQuota, String> {
    let manager = state.0.read().await;

    // 解析最终使用的账号 ID：显式 > 默认账号 > 无账号 (not_found)
    let resolved = match account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => Some(id.to_string()),
        None => manager.default_account_id().await,
    };
    let Some(id) = resolved else {
        return Ok(SubscriptionQuota::not_found("xai_oauth"));
    };

    // 获取（必要时自动刷新）access_token
    let token = match manager.get_valid_token_for_account(&id).await {
        Ok(t) => t,
        Err(
            crate::proxy::providers::xai_oauth_auth::XaiOAuthError::RefreshTokenInvalid
            | crate::proxy::providers::xai_oauth_auth::XaiOAuthError::ReauthRequired(_),
        ) => {
            return Ok(SubscriptionQuota::error(
                "xai_oauth",
                CredentialStatus::Expired,
                "Authorization unavailable".into(),
            ));
        }
        Err(crate::proxy::providers::xai_oauth_auth::XaiOAuthError::AccountNotFound(_)) => {
            return Ok(SubscriptionQuota::not_found("xai_oauth"))
        }
        Err(_) => return Err("nativeSubscription.failed".into()),
    };

    let quota = grok_subscription::query(&token).await?;
    if matches!(quota.credential_status, CredentialStatus::Expired) {
        let renewed = manager
            .renew_rejected_token(&id, &token)
            .await
            .map_err(|_| "nativeSubscription.failed")?;
        return grok_subscription::query(&renewed).await;
    }
    Ok(quota)
}

/// Read managed Grok subscription usage.
#[tauri::command(rename_all = "camelCase")]
pub async fn get_xai_oauth_quota(
    account_id: Option<String>,
    state: State<'_, XaiOAuthState>,
) -> Result<SubscriptionQuota, String> {
    query_xai_oauth_quota_for(&state, account_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_xai_oauth_models(
    account_id: Option<String>,
    state: State<'_, XaiOAuthState>,
) -> Result<Vec<FetchedModel>, String> {
    let manager = state.0.read().await;
    let resolved = match account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => Some(id.to_string()),
        None => manager.default_account_id().await,
    };
    let account_id = resolved.ok_or_else(|| "No usable xAI account available".to_string())?;
    let mut token = manager
        .get_valid_token_for_account(&account_id)
        .await
        .map_err(|_| "nativeSubscription.failed")?;

    let client = crate::proxy::http_client::get();
    let mut response = grok_subscription::list_models(&client, grok_subscription::BASE, &token)
        .await
        .map_err(|_| "nativeSubscription.failed")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        token = manager
            .renew_rejected_token(&account_id, &token)
            .await
            .map_err(|_| "nativeSubscription.failed")?;
        response = grok_subscription::list_models(&client, grok_subscription::BASE, &token)
            .await
            .map_err(|_| "nativeSubscription.failed")?;
    }
    let status = response.status();
    if !status.is_success() {
        return Err(format!("xAI models request failed: HTTP {status}"));
    }
    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "nativeSubscription.failed")?;
    let mut models = grok_subscription::models(&payload);
    if models.is_empty() {
        return Err("nativeSubscription.failed".into());
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}
