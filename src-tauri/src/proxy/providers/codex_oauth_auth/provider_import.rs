//! Explicit the reference implementation account copies. No refresh, native-login writes or source mutation.
use super::*;

pub(crate) fn imported_identity(value: &serde_json::Value) -> Option<(String, String)> {
    let tokens = value.get("tokens")?;
    let token = tokens.get("id_token")?.as_str()?;
    let refresh = tokens.get("refresh_token")?.as_str()?;
    if refresh.trim().is_empty()
        || refresh.len() > 32768
        || refresh.chars().any(char::is_control)
        || token.len() > 32768
        || value["auth_mode"] == "apikey"
    {
        return None;
    }
    let user = crate::codex_config::extract_codex_id_token_user_identity(token)?;
    let claims = parse_jwt_claims(token)?;
    let workspace = claims
        .chatgpt_account_id
        .or_else(|| claims.openai_auth?.chatgpt_account_id)?;
    if workspace.is_empty()
        || workspace.len() > 200
        || workspace.chars().any(char::is_control)
        || tokens
            .get("account_id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| id != workspace)
    {
        return None;
    }
    Some((user, workspace))
}

impl CodexOAuthManager {
    pub(crate) async fn imported_account_id(&self, value: &serde_json::Value) -> Option<String> {
        let (user, workspace) = imported_identity(value)?;
        self.accounts
            .read()
            .await
            .iter()
            .find(|(_, account)| {
                account.chatgpt_account_id.as_deref() == Some(workspace.as_str())
                    && account
                        .id_token
                        .as_deref()
                        .and_then(crate::codex_config::extract_codex_id_token_user_identity)
                        .as_deref()
                        == Some(user.as_str())
            })
            .map(|(id, _)| id.clone())
    }

    pub(crate) async fn import_provider_accounts<T, F>(
        &self,
        values: Vec<serde_json::Value>,
        commit: F,
    ) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(Vec<String>) -> Result<T, String> + Send + 'static,
    {
        const FAILED: &str = "providerImport.failed";
        let _lifecycle = self.lifecycle_lock.write().await;
        let _persist = self.storage_lock.lock().await;
        let original = self.accounts.read().await.clone();
        let original_default = self.resolve_default_account_id().await;
        let mut accounts = original.clone();
        let mut ids = Vec::new();
        for value in values {
            let (user, workspace) = imported_identity(&value).ok_or("providerImport.invalid")?;
            let existing = accounts
                .iter()
                .find(|(_, account)| {
                    account.chatgpt_account_id.as_deref() == Some(workspace.as_str())
                        && account
                            .id_token
                            .as_deref()
                            .and_then(crate::codex_config::extract_codex_id_token_user_identity)
                            .as_deref()
                            == Some(user.as_str())
                })
                .map(|(id, _)| id.clone());
            if let Some(id) = existing {
                ids.push(id);
                continue;
            }
            let token = value["tokens"]["id_token"]
                .as_str()
                .ok_or("providerImport.invalid")?;
            let claims = parse_jwt_claims(token).ok_or("providerImport.invalid")?;
            let id = Uuid::new_v4().to_string();
            accounts.insert(
                id.clone(),
                CodexAccountData {
                    account_id: id.clone(),
                    chatgpt_account_id: Some(workspace),
                    email: claims.email,
                    refresh_token: value["tokens"]["refresh_token"]
                        .as_str()
                        .ok_or("providerImport.invalid")?
                        .into(),
                    id_token: Some(token.into()),
                    authenticated_at: chrono::Utc::now().timestamp(),
                    token_updated_at_ms: 0,
                },
            );
            ids.push(id);
        }
        let default = original_default.clone().or_else(|| ids.first().cloned());
        let next = CodexOAuthStore {
            version: 2,
            accounts: accounts.clone(),
            default_account_id: default.clone(),
        };
        let content = serde_json::to_string_pretty(&next).map_err(|_| FAILED)?;
        self.write_store_atomic(&content).map_err(|_| FAILED)?;
        let result = tokio::task::spawn_blocking(move || commit(ids))
            .await
            .map_err(|_| FAILED.to_string())
            .and_then(|r| r);
        if result.is_err() {
            let old = CodexOAuthStore {
                version: 2,
                accounts: original,
                default_account_id: original_default,
            };
            let rollback = serde_json::to_string_pretty(&old).map_err(|_| FAILED)?;
            self.write_store_atomic(&rollback).map_err(|_| FAILED)?;
            return result;
        }
        *self.accounts.write().await = accounts;
        *self.default_account_id.write().await = default;
        result
    }
}
