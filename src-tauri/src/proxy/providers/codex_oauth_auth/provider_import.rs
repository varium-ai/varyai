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

fn imported_access_token(value: &serde_json::Value) -> Option<CachedAccessToken> {
    let (user, workspace) = imported_identity(value)?;
    let token = value.pointer("/tokens/access_token")?.as_str()?;
    if token.len() > 32768
        || token.chars().any(char::is_control)
        || crate::codex_config::extract_codex_id_token_user_identity(token)? != user
    {
        return None;
    }
    let claims = parse_jwt_claims(token)?;
    if claims
        .chatgpt_account_id
        .or_else(|| claims.openai_auth.and_then(|a| a.chatgpt_account_id))?
        != workspace
    {
        return None;
    }
    let payload = token.split('.').nth(1)?;
    let claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    let expires_at_ms = claims["exp"].as_i64()?.checked_mul(1000)?;
    let now = chrono::Utc::now().timestamp_millis();
    let obtained_at_ms = value["last_refresh"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.timestamp_millis())
        .or_else(|| claims["iat"].as_i64()?.checked_mul(1000))?;
    if expires_at_ms.saturating_sub(now) < TOKEN_REFRESH_BUFFER_MS
        || obtained_at_ms <= 0
        || obtained_at_ms > now.saturating_add(300_000)
        || obtained_at_ms >= expires_at_ms
    {
        return None;
    }
    Some(CachedAccessToken {
        token: token.into(),
        expires_at_ms,
        obtained_at_ms,
    })
}

pub(super) fn retained_imported_access(account: &CodexAccountData) -> Option<CachedAccessToken> {
    let cached = account.imported_access.as_ref()?;
    let refreshed = chrono::DateTime::from_timestamp_millis(cached.obtained_at_ms)?.to_rfc3339();
    let value = serde_json::json!({"last_refresh":refreshed,"tokens":{
        "access_token":cached.token,"id_token":account.id_token,
        "refresh_token":account.refresh_token,"account_id":account.chatgpt_account_id}});
    let validated = imported_access_token(&value)?;
    Some(CachedAccessToken {
        expires_at_ms: validated.expires_at_ms.min(cached.expires_at_ms),
        obtained_at_ms: cached.obtained_at_ms,
        ..validated
    })
}

#[cfg(test)]
mod editor_identity_tests {
    use super::*;

    fn fixture_access(user: &str, workspace: &str, expires: i64) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::json!({"sub":user,"exp":expires,"iat":chrono::Utc::now().timestamp()-60,
                "https://api.openai.com/auth":{"chatgpt_account_id":workspace}})
            .to_string(),
        );
        format!("{header}.{payload}.")
    }

    fn fixture_auth(user: &str, workspace: &str) -> serde_json::Value {
        let id_token = fixture_access(user, workspace, chrono::Utc::now().timestamp() - 10);
        serde_json::json!({"auth_mode":"chatgpt","last_refresh":"2026-10-01T00:00:00Z",
            "tokens":{"id_token":id_token,"refresh_token":"synthetic-refresh",
            "account_id":workspace,"access_token":fixture_access(user,workspace,chrono::Utc::now().timestamp()+3600)}})
    }

    #[tokio::test]
    async fn imported_access_survives_restart_without_refreshing_or_exposing_it(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = CodexOAuthManager::new(dir.path().to_owned());
        let auth = fixture_auth("user-a", "workspace-a");
        let ids = manager
            .import_provider_accounts(vec![auth.clone()], Ok)
            .await?;
        let id = ids.first().ok_or("missing account")?;
        assert!(manager.access_tokens.read().await.contains_key(id));
        assert!(manager.accounts.read().await[id].token_updated_at_ms > 0);
        let reloaded = CodexOAuthManager::new(dir.path().to_owned());
        assert!(reloaded.access_tokens.read().await.contains_key(id));
        let actual = reloaded.get_valid_token_for_account(id).await?;
        assert!(Some(actual.as_str()) == auth["tokens"]["access_token"].as_str());
        let public = serde_json::to_string(&reloaded.list_accounts().await)?;
        assert!(!public.contains("access_token"));
        assert!(!public.contains(auth["tokens"]["refresh_token"].as_str().ok_or("refresh")?));
        Ok(())
    }

    #[test]
    fn imported_access_requires_exact_user_workspace_and_a_known_future_expiry() {
        let auth = fixture_auth("user-a", "workspace-a");
        assert!(imported_access_token(&auth).is_some());
        for token in [
            fixture_access(
                "user-b",
                "workspace-a",
                chrono::Utc::now().timestamp() + 3600,
            ),
            fixture_access(
                "user-a",
                "workspace-b",
                chrono::Utc::now().timestamp() + 3600,
            ),
            fixture_access("user-a", "workspace-a", chrono::Utc::now().timestamp() - 1),
            "unknown-expiry".into(),
        ] {
            let mut invalid = auth.clone();
            invalid["tokens"]["access_token"] = token.into();
            assert!(imported_access_token(&invalid).is_none());
        }
    }

    #[tokio::test]
    async fn failed_import_never_seeds_access_cache_or_persistent_credentials(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = CodexOAuthManager::new(dir.path().to_owned());
        let result = manager
            .import_provider_accounts(vec![fixture_auth("user-a", "workspace-a")], |_| {
                Err::<(), _>("synthetic failure".into())
            })
            .await;
        assert!(result.is_err());
        assert!(manager.access_tokens.read().await.is_empty());
        assert!(CodexOAuthManager::new(dir.path().to_owned())
            .access_tokens
            .read()
            .await
            .is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn legacy_import_recovery_only_reuses_the_exact_same_credential_generation(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = CodexOAuthManager::new(dir.path().to_owned());
        let auth = fixture_auth("user-a", "workspace-a");
        let mut old = auth.clone();
        old["tokens"]
            .as_object_mut()
            .ok_or("tokens")?
            .remove("access_token");
        old.as_object_mut().ok_or("auth")?.remove("last_refresh");
        let ids = manager.import_provider_accounts(vec![old], Ok).await?;
        let id = ids.first().ok_or("account")?;
        for mut wrong in [
            fixture_auth("user-b", "workspace-a"),
            fixture_auth("user-a", "workspace-b"),
            auth.clone(),
        ] {
            wrong["tokens"]["refresh_token"] = "different-generation".into();
            manager.restore_legacy_imported_access(id, &wrong).await;
            assert!(manager.access_tokens.read().await.is_empty());
        }
        manager.restore_legacy_imported_access(id, &auth).await;
        assert!(manager.access_tokens.read().await.contains_key(id));
        let actual = manager.get_valid_token_for_account(id).await?;
        assert!(Some(actual.as_str()) == auth["tokens"]["access_token"].as_str());
        let reloaded = CodexOAuthManager::new(dir.path().to_owned());
        assert!(reloaded.access_tokens.read().await.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn explicit_reimport_can_repair_missing_access_without_replacing_a_newer_generation(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = CodexOAuthManager::new(dir.path().to_owned());
        let auth = fixture_auth("user-a", "workspace-a");
        let mut old = auth.clone();
        old["tokens"]
            .as_object_mut()
            .ok_or("tokens")?
            .remove("access_token");
        let ids = manager.import_provider_accounts(vec![old], Ok).await?;
        let id = ids.first().ok_or("account")?;
        manager
            .import_provider_accounts(vec![auth.clone()], Ok)
            .await?;
        assert!(manager.access_tokens.read().await.contains_key(id));
        let before = manager.access_tokens.read().await[id].token.clone();
        let mut outdated = auth.clone();
        outdated["tokens"]["refresh_token"] = "different-refresh".into();
        manager.import_provider_accounts(vec![outdated], Ok).await?;
        assert!(manager.access_tokens.read().await[id].token == before);
        assert!(
            Some(manager.accounts.read().await[id].refresh_token.as_str())
                == auth["tokens"]["refresh_token"].as_str()
        );
        Ok(())
    }

    #[tokio::test]
    async fn native_editor_matching_requires_both_user_and_workspace_without_refresh(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = CodexOAuthManager::new(dir.path().to_owned());
        let token = crate::codex_config::test_codex_id_token("user-a");
        manager
            .add_test_account_with_workspace_and_access_token(
                "managed-a",
                "workspace-a",
                "synthetic-access",
                Some(&token),
            )
            .await?;
        assert_eq!(
            manager
                .account_for_editor_identity("sub:user-a", "workspace-a")
                .await
                .as_deref(),
            Some("managed-a")
        );
        assert!(manager
            .account_for_editor_identity("sub:user-b", "workspace-a")
            .await
            .is_none());
        assert!(manager
            .account_for_editor_identity("sub:user-a", "workspace-b")
            .await
            .is_none());
        assert!(manager
            .account_for_editor_identity("", "workspace-a")
            .await
            .is_none());
        let before = manager.continuation_identity("managed-a").await;
        let _ = manager
            .account_for_editor_identity("sub:user-a", "workspace-a")
            .await;
        assert_eq!(manager.continuation_identity("managed-a").await, before);
        Ok(())
    }
}

impl CodexOAuthManager {
    async fn restore_legacy_imported_access(&self, id: &str, value: &serde_json::Value) {
        let Some(cached) = imported_access_token(value) else {
            return;
        };
        let Some((user, workspace)) = imported_identity(value) else {
            return;
        };
        let refresh_lock = self.get_refresh_lock(id).await;
        let _refresh = refresh_lock.lock().await;
        let accounts = self.accounts.read().await;
        let Some(account) = accounts.get(id) else {
            return;
        };
        if account.token_updated_at_ms > 0
            || account.chatgpt_account_id.as_deref() != Some(workspace.as_str())
            || value
                .pointer("/tokens/refresh_token")
                .and_then(serde_json::Value::as_str)
                != Some(account.refresh_token.as_str())
            || account
                .id_token
                .as_deref()
                .and_then(crate::codex_config::extract_codex_id_token_user_identity)
                .as_deref()
                != Some(user.as_str())
        {
            return;
        }
        // Same-generation recovery is memory-only and never claims native-login ownership.
        self.access_tokens
            .write()
            .await
            .entry(id.into())
            .or_insert(cached);
    }

    pub(super) async fn reuse_legacy_imported_access(&self, id: &str) {
        if self.storage_path.parent() != Some(crate::config::get_device_config_dir().as_path()) {
            return;
        }
        {
            let accounts = self.accounts.read().await;
            if !accounts.get(id).is_some_and(|a| a.token_updated_at_ms <= 0)
                || self.access_tokens.read().await.contains_key(id)
            {
                return;
            }
        }
        let value = tokio::task::spawn_blocking(|| {
            use std::io::Read;
            let config = crate::codex_config::get_codex_config_path();
            if config.exists() {
                let mut text = String::new();
                fs::File::open(config)
                    .ok()?
                    .take(1024 * 1024 + 1)
                    .read_to_string(&mut text)
                    .ok()?;
                if text.len() > 1024 * 1024
                    || crate::codex_config::codex_config_auth_store_mode(&text)
                        != crate::codex_config::CodexAuthStoreMode::File
                {
                    return None;
                }
            }
            let file = fs::File::open(crate::codex_config::get_codex_auth_path()).ok()?;
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
            if bytes.len() > 1024 * 1024 {
                return None;
            }
            serde_json::from_slice::<serde_json::Value>(&bytes).ok()
        })
        .await
        .ok()
        .flatten();
        if let Some(value) = value {
            self.restore_legacy_imported_access(id, &value).await;
        }
    }

    /// Read identity metadata only; never renew credentials to reconcile editor rows.
    pub(crate) async fn account_for_editor_identity(
        &self,
        user: &str,
        workspace: &str,
    ) -> Option<String> {
        if user.is_empty() || workspace.is_empty() {
            return None;
        }
        self.accounts
            .read()
            .await
            .iter()
            .find(|(_, account)| {
                account.chatgpt_account_id.as_deref() == Some(workspace)
                    && account
                        .id_token
                        .as_deref()
                        .and_then(crate::codex_config::extract_codex_id_token_user_identity)
                        .as_deref()
                        == Some(user)
            })
            .map(|(id, _)| id.clone())
    }

    pub(crate) async fn imported_account_id(&self, value: &serde_json::Value) -> Option<String> {
        let (user, workspace) = imported_identity(value)?;
        self.account_for_editor_identity(&user, &workspace).await
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
                if let Some(account) = accounts.get_mut(&id) {
                    if account.refresh_token
                        == value["tokens"]["refresh_token"]
                            .as_str()
                            .unwrap_or_default()
                        && retained_imported_access(account).is_none()
                    {
                        if let Some(cached) = imported_access_token(&value) {
                            account.token_updated_at_ms =
                                account.token_updated_at_ms.max(cached.obtained_at_ms);
                            account.imported_access = Some(cached);
                        }
                    }
                }
                ids.push(id);
                continue;
            }
            let token = value["tokens"]["id_token"]
                .as_str()
                .ok_or("providerImport.invalid")?;
            let claims = parse_jwt_claims(token).ok_or("providerImport.invalid")?;
            let id = Uuid::new_v4().to_string();
            let imported_access = imported_access_token(&value);
            let token_updated_at_ms = value["last_refresh"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.timestamp_millis())
                .filter(|at| *at > 0 && *at <= chrono::Utc::now().timestamp_millis())
                .or_else(|| imported_access.as_ref().map(|c| c.obtained_at_ms))
                .unwrap_or(0);
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
                    token_updated_at_ms,
                    reauth_required: false,
                    imported_access,
                    browser_session: None,
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
        let ids_for_cache = ids.clone();
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
        {
            let mut current = self.accounts.write().await;
            let mut cached = self.access_tokens.write().await;
            for id in &ids_for_cache {
                if let Some(access) = accounts.get(id).and_then(retained_imported_access) {
                    if !cached
                        .get(id)
                        .is_some_and(|token| !token.is_expiring_soon())
                    {
                        cached.insert(id.clone(), access);
                    }
                }
            }
            *current = accounts;
        }
        *self.default_account_id.write().await = default;
        result
    }
}
