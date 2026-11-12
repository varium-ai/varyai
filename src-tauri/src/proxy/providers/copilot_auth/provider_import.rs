//! Offline saved-login imports with transactional provider publication.
use super::*;
use sha2::{Digest, Sha256};
use std::future::Future;

#[derive(Clone, Serialize)]
pub(crate) struct ImportedAccount {
    login: String,
    host: String,
    token: String,
    plan: Option<String>,
}

impl ImportedAccount {
    pub(crate) fn parse(row: &serde_json::Value) -> Result<Self, String> {
        let auth = &row["auth"];
        let host = auth["host"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_GITHUB_DOMAIN);
        let normalized = normalize_github_domain(host).map_err(|_| "providerImport.invalid")?;
        if !host.eq_ignore_ascii_case(&normalized) {
            return Err("providerImport.invalid".into());
        }
        let host = normalized;
        if !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-.".contains(&byte))
            || host
                .split('.')
                .any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-'))
        {
            return Err("providerImport.invalid".into());
        }
        if host != DEFAULT_GITHUB_DOMAIN && !host.ends_with(".ghe.com") {
            return Err("providerImport.invalid".into());
        }
        let login = row["user"]
            .as_str()
            .unwrap_or("")
            .split('@')
            .next()
            .unwrap_or("");
        let token = auth["oauth_token"].as_str().unwrap_or("");
        if login.is_empty()
            || login.len() > 120
            || !login
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            || token.is_empty()
            || token.len() > 32768
            || token.chars().any(char::is_control)
        {
            return Err("providerImport.invalid".into());
        }
        Ok(Self {
            login: login.into(),
            host,
            token: token.into(),
            plan: row["plan"]
                .as_str()
                .and_then(|p| crate::services::subscription_plan::vendor_plan("Copilot", p)),
        })
    }
}

impl CopilotAuthManager {
    pub(crate) async fn imported_account_id(&self, incoming: &ImportedAccount) -> Option<String> {
        self.accounts
            .read()
            .await
            .iter()
            .find(|(_, a)| {
                a.github_domain == incoming.host
                    && a.user.login.eq_ignore_ascii_case(&incoming.login)
            })
            .map(|(id, _)| id.clone())
    }

    pub(crate) async fn import_provider_accounts<T, F, Fut>(
        &self,
        values: Vec<ImportedAccount>,
        commit: F,
    ) -> Result<T, String>
    where
        F: FnOnce(Vec<String>) -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        if values.is_empty() {
            return commit(vec![]).await;
        }
        const FAILED: &str = "providerImport.failed";
        if self.pending_migration.read().await.is_some() {
            return Err(FAILED.into());
        }
        let mut current = self.accounts.write().await;
        let mut default = self.default_account_id.write().await;
        let original = current.clone();
        let original_default = default.clone();
        let existed = self.storage_path.try_exists().map_err(|_| FAILED)?;
        let mut next = original.clone();
        let mut ids = vec![];
        for incoming in values {
            if let Some((id, _)) = next.iter().find(|(_, a)| {
                a.github_domain == incoming.host
                    && a.user.login.eq_ignore_ascii_case(&incoming.login)
            }) {
                ids.push(id.clone());
                continue;
            }
            let id = format!(
                "imported-{:x}",
                Sha256::digest(format!(
                    "{}:{}",
                    incoming.host,
                    incoming.login.to_ascii_lowercase()
                ))
            );
            // The store key is authoritative until GitHub supplies its numeric identity.
            next.insert(
                id.clone(),
                GitHubAccountData {
                    github_token: incoming.token,
                    reauth_required: false,
                    plan: incoming.plan,
                    user: GitHubUser {
                        login: incoming.login,
                        id: 0,
                        avatar_url: None,
                    },
                    authenticated_at: chrono::Utc::now().timestamp(),
                    github_domain: incoming.host,
                },
            );
            ids.push(id);
        }
        let next_default = original_default
            .clone()
            .or_else(|| Self::fallback_default_account_id(&next));
        let store = |accounts: HashMap<String, GitHubAccountData>,
                     default_account_id: Option<String>|
         -> Result<Vec<u8>, String> {
            serde_json::to_vec(&CopilotAuthStore {
                version: 3,
                accounts,
                default_account_id,
                github_token: None,
                authenticated_at: None,
            })
            .map_err(|_| FAILED.into())
        };
        let path = self.storage_path.clone();
        let bytes = store(next.clone(), next_default.clone())?;
        tokio::task::spawn_blocking(move || crate::config::atomic_write_private(&path, &bytes))
            .await
            .map_err(|_| FAILED)?
            .map_err(|_| FAILED)?;
        match commit(ids).await {
            Ok(value) => {
                *current = next;
                *default = next_default;
                Ok(value)
            }
            Err(error) => {
                let bytes = store(original, original_default)?;
                let path = self.storage_path.clone();
                tokio::task::spawn_blocking(move || {
                    if existed {
                        crate::config::atomic_write_private(&path, &bytes).map_err(|_| FAILED)
                    } else {
                        std::fs::remove_file(path).map_err(|_| FAILED)
                    }
                })
                .await
                .map_err(|_| FAILED)?
                .map_err(|_| FAILED)?;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn copied_copilot_login_keeps_binding_when_numeric_identity_arrives() -> Result<(), String>
    {
        let dir = tempfile::tempdir().map_err(|_| "fixture")?;
        let manager = CopilotAuthManager::new(dir.path().to_owned());
        let imported = ImportedAccount::parse(
            &json!({"user":"fixture-login","auth":{"oauth_token":"synthetic-github-token"}}),
        )?;
        let ids = manager
            .import_provider_accounts(vec![imported.clone()], |ids| async { Ok(ids) })
            .await?;
        assert!(ids[0].starts_with("imported-"));
        assert_eq!(manager.list_accounts().await[0].id, ids[0]);
        let returned = manager
            .add_account_internal(
                "synthetic-newer-token".into(),
                GitHubUser {
                    login: "fixture-login".into(),
                    id: 42,
                    avatar_url: None,
                },
                DEFAULT_GITHUB_DOMAIN.into(),
            )
            .await
            .map_err(|_| "sign-in")?;
        assert_eq!(returned.id, ids[0]);
        let signed_in_again = manager
            .add_account_internal(
                "synthetic-latest-token".into(),
                GitHubUser {
                    login: "Fixture-Login".into(),
                    id: 42,
                    avatar_url: None,
                },
                DEFAULT_GITHUB_DOMAIN.into(),
            )
            .await
            .map_err(|_| "second sign-in")?;
        assert_eq!(signed_in_again.id, ids[0]);
        manager
            .import_provider_accounts(vec![imported], |actual| async move {
                assert_eq!(actual, ids);
                Ok(())
            })
            .await?;
        assert!(manager
            .accounts
            .read()
            .await
            .values()
            .all(|a| a.github_token == "synthetic-latest-token"));
        let reloaded = CopilotAuthManager::new(dir.path().to_owned());
        assert_eq!(reloaded.list_accounts().await.len(), 1);
        assert_eq!(reloaded.list_accounts().await[0].id, returned.id);
        Ok(())
    }

    #[tokio::test]
    async fn rejected_batch_does_not_publish_copilot_credentials_or_default_account(
    ) -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| "fixture")?;
        let manager = CopilotAuthManager::new(dir.path().to_owned());
        let imported = ImportedAccount::parse(
            &json!({"user":"fixture-login","auth":{"oauth_token":"synthetic-github-token","host":"fixture.ghe.com"}}),
        )?;
        let result = manager
            .import_provider_accounts(vec![imported], |_| async {
                Err::<(), _>("providerImport.conflict".into())
            })
            .await;
        assert_eq!(result, Err("providerImport.conflict".into()));
        assert!(manager.list_accounts().await.is_empty());
        assert!(!dir.path().join("copilot_auth.json").exists());
        assert!(manager.default_account_id.read().await.is_none());
        Ok(())
    }

    #[test]
    fn copied_copilot_tokens_never_target_arbitrary_hosts() {
        for host in [
            "evil.example",
            "https://github.com/path",
            "github.com@evil.example",
            "github.com\n",
            "fixture\n.ghe.com",
            "fixture..ghe.com",
        ] {
            assert!(ImportedAccount::parse(
                &json!({"user":"fixture","auth":{"oauth_token":"synthetic-token","host":host}})
            )
            .is_err());
        }
    }
}
