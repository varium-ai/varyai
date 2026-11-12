//! Offline copies of visible subscription accounts; source files are never mutated.
use super::*;
use std::{future::Future, path::Path, sync::Weak};

const INVALID: &str = "providerImport.invalid";
const IMPORT_FAILED: &str = "providerImport.failed";

#[derive(Clone, Serialize)]
pub(crate) struct ImportedAccount {
    pub account: Account,
    credentials: Option<Value>,
    profile: Option<Value>,
}

fn text(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap_or("").trim().into()
}

fn token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32768 && !value.chars().any(char::is_control)
}

pub(crate) fn parse(
    provider: SubscriptionProvider,
    row: &Value,
) -> Result<ImportedAccount, String> {
    supported(provider)?;
    if provider == SubscriptionProvider::Cursor {
        return Err(INVALID.into());
    }
    let auth = &row["auth"];
    let mut account = Account {
        id: String::new(),
        provider,
        login: text(row, "user"),
        project: text(row, "project"),
        claude_home: None,
        reauth_required: false,
        generation: String::new(),
        plan: row["plan"].as_str().and_then(|plan| {
            if google(provider) {
                super::super::subscription_plan::google_plan(&json!({"paidTier":{"name":plan}}))
            } else {
                super::super::subscription_plan::vendor_plan(
                    if provider == SubscriptionProvider::Claude {
                        "Claude"
                    } else {
                        "ChatGPT"
                    },
                    plan,
                )
            }
        }),
        access: String::new(),
        refresh: String::new(),
        expires: 0,
        client_id: String::new(),
        subject: String::new(),
        id_token: String::new(),
    };
    let mut credentials = None;
    let mut profile = None;
    if google(provider) {
        account.client_id = if provider == SubscriptionProvider::Antigravity {
            ANTIGRAVITY_OAUTH_CLIENT_ID.into()
        } else {
            super::super::subscription::GEMINI_OAUTH_CLIENT_ID.into()
        };
        account.access = text(auth, "access_token");
        account.refresh = text(auth, "refresh_token");
        account.expires = auth["expiry_date"].as_i64().unwrap_or(0) / 1000;
        if account.project.is_empty() {
            account.project = text(auth, "project");
        }
        if !token(&account.refresh) {
            return Err(INVALID.into());
        }
    } else if provider == SubscriptionProvider::ChatgptApi {
        account.client_id = text(auth, "clientId");
        account.subject = text(auth, "sub");
        account.access = text(auth, "access");
        account.refresh = text(auth, "refresh");
        account.id_token = text(auth, "idToken");
        account.expires = auth["expires"]
            .as_str()
            .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
            .map_or(0, |date| date.timestamp());
        if account.login.is_empty() {
            account.login = text(auth, "email");
        }
        if !token(&account.refresh) || !token(&account.client_id) || !token(&account.subject) {
            return Err(INVALID.into());
        }
    } else if provider == SubscriptionProvider::Claude {
        let oauth = &auth["claudeAiOauth"];
        let refresh = text(oauth, "refreshToken");
        if !token(&text(oauth, "accessToken")) || (!refresh.is_empty() && !token(&refresh)) {
            return Err(INVALID.into());
        }
        account.subject = text(&row["profile"], "accountUuid");
        account.plan = super::super::subscription_plan::claude_plan(auth).or(account.plan);
        credentials = Some(auth.clone());
        profile = Some(json!({"hasCompletedOnboarding":true,"oauthAccount":row["profile"]}));
    }
    if !crate::services::routing_upstreams::valid_identity(&account.login)
        || account.project.len() > 1024
        || account.project.chars().any(char::is_control)
        || (!account.access.is_empty() && !token(&account.access))
        || (!account.refresh.is_empty() && !token(&account.refresh))
        || (!account.id_token.is_empty() && !token(&account.id_token))
    {
        return Err(INVALID.into());
    }
    account.id = format!(
        "imported-{:x}",
        Sha256::digest(format!(
            "{}:{}:{}",
            serde_json::to_string(&provider).map_err(|_| INVALID)?,
            account.client_id,
            if account.subject.is_empty() {
                account.login.to_ascii_lowercase()
            } else {
                account.subject.clone()
            }
        ))
    );
    Ok(ImportedAccount {
        account,
        credentials,
        profile,
    })
}

fn matches(existing: &Account, incoming: &Account) -> bool {
    existing.provider == incoming.provider
        && (existing.provider == SubscriptionProvider::Claude
            || existing.client_id == incoming.client_id)
        && if !existing.subject.is_empty() && !incoming.subject.is_empty() {
            existing.subject == incoming.subject
        } else {
            existing.login.eq_ignore_ascii_case(&incoming.login)
        }
}

async fn read_at(path: PathBuf) -> Result<Vec<Account>, String> {
    tokio::task::spawn_blocking(move || match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| IMPORT_FAILED.into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(_) => Err(IMPORT_FAILED.into()),
    })
    .await
    .map_err(|_| IMPORT_FAILED)?
}

async fn write_at(path: PathBuf, accounts: Vec<Account>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let bytes = serde_json::to_vec(&accounts).map_err(|_| IMPORT_FAILED)?;
        crate::config::atomic_write_private(&path, &bytes).map_err(|_| IMPORT_FAILED.into())
    })
    .await
    .map_err(|_| IMPORT_FAILED)?
}

pub(crate) async fn existing_id(
    path: &Path,
    incoming: &ImportedAccount,
) -> Result<Option<String>, String> {
    Ok(read_at(path.to_owned())
        .await?
        .iter()
        .find(|a| matches(a, &incoming.account))
        .map(|a| a.id.clone()))
}

pub(crate) async fn import_accounts<T, F, Fut>(
    path: PathBuf,
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
    let coordinator = if path == store_path() {
        None
    } else {
        Some(store_coordinator(&path).await)
    };
    let _guard = match &coordinator {
        Some(coordinator) => coordinator.lock().await,
        None => ACCOUNTS.lock().await,
    };
    let original = read_at(path.clone()).await?;
    let existed = path.try_exists().map_err(|_| IMPORT_FAILED)?;
    let mut accounts = original.clone();
    let mut ids = vec![];
    let mut directories = vec![];
    for value in values {
        if let Some(old) = accounts.iter().find(|a| matches(a, &value.account)) {
            ids.push(old.id.clone());
            continue;
        }
        let mut account = value.account;
        account.generation = random();
        if let Some(credentials) = value.credentials {
            let home = uuid::Uuid::new_v4().to_string();
            let dir = path
                .parent()
                .ok_or(IMPORT_FAILED)?
                .join("claude")
                .join(&home);
            directories.push(dir.clone());
            account.claude_home = Some(home);
            let result = tokio::task::spawn_blocking(move || {
                crate::config::atomic_write_private(
                    &dir.join(".credentials.json"),
                    credentials.to_string().as_bytes(),
                )
                .and_then(|_| {
                    crate::config::atomic_write_private(
                        &dir.join(".claude.json"),
                        value
                            .profile
                            .unwrap_or_else(|| json!({"hasCompletedOnboarding":true}))
                            .to_string()
                            .as_bytes(),
                    )
                })
            })
            .await
            .map_err(|_| IMPORT_FAILED)
            .and_then(|result| result.map_err(|_| IMPORT_FAILED));
            if result.is_err() {
                cleanup(directories).await;
                return Err(IMPORT_FAILED.into());
            }
        }
        ids.push(account.id.clone());
        accounts.push(account);
    }
    if write_at(path.clone(), accounts).await.is_err() {
        cleanup(directories).await;
        return Err(IMPORT_FAILED.into());
    }
    match commit(ids).await {
        Ok(value) => Ok(value),
        Err(error) => {
            let restored = if existed {
                write_at(path, original).await
            } else {
                tokio::task::spawn_blocking(move || std::fs::remove_file(path))
                    .await
                    .map_err(|_| IMPORT_FAILED)?
                    .map_err(|_| IMPORT_FAILED.into())
            };
            cleanup(directories).await;
            restored?;
            Err(error)
        }
    }
}

async fn cleanup(directories: Vec<PathBuf>) {
    let _ = tokio::task::spawn_blocking(move || {
        for path in directories {
            let _ = std::fs::remove_dir_all(path);
        }
    })
    .await;
}

async fn store_coordinator(path: &Path) -> Arc<Mutex<()>> {
    type Coordinators = Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>;
    static COORDINATORS: OnceLock<Coordinators> = OnceLock::new();
    let mut coordinators = COORDINATORS.get_or_init(Default::default).lock().await;
    coordinators.retain(|_, coordinator| coordinator.strong_count() != 0);
    if let Some(coordinator) = coordinators.get(path).and_then(Weak::upgrade) {
        return coordinator;
    }
    let coordinator = Arc::new(Mutex::new(()));
    coordinators.insert(path.to_owned(), Arc::downgrade(&coordinator));
    coordinator
}

#[cfg(test)]
mod tests {
    use super::*;

    fn google_row() -> Value {
        json!({"user":"person@example.test","plan":"Google AI Pro","project":"chosen-project",
            "auth":{"access_token":"synthetic-access","refresh_token":"synthetic-refresh",
                "expiry_date":4102444800000_i64,"project":"auth-project"}})
    }

    #[tokio::test]
    async fn independent_account_stores_do_not_share_a_publication_lock() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| IMPORT_FAILED)?;
        let account = parse(SubscriptionProvider::Antigravity, &google_row())?;
        let first = dir.path().join("first/accounts.json");
        let second = dir.path().join("second/accounts.json");
        let (ready, entered) = tokio::sync::oneshot::channel();
        let (release, waiting) = tokio::sync::oneshot::channel();
        let first_account = account.clone();
        let task = tokio::spawn(async move {
            import_accounts(first, vec![first_account], |_| async move {
                let _ = ready.send(());
                waiting.await.map_err(|_| IMPORT_FAILED)?;
                Ok(())
            })
            .await
        });
        entered.await.map_err(|_| IMPORT_FAILED)?;
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            import_accounts(second, vec![account], |_| async { Ok(()) }),
        )
        .await;
        let _ = release.send(());
        task.await.map_err(|_| IMPORT_FAILED)??;
        result.map_err(|_| "independent account store blocked")??;
        Ok(())
    }

    #[tokio::test]
    async fn imported_google_credentials_keep_project_expiry_and_newer_local_sessions(
    ) -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| IMPORT_FAILED)?;
        let path = dir.path().join("accounts.json");
        for provider in [
            SubscriptionProvider::Antigravity,
            SubscriptionProvider::GeminiCli,
        ] {
            let incoming = parse(provider, &google_row())?;
            assert_eq!(incoming.account.project, "chosen-project");
            assert_eq!(incoming.account.expires, 4102444800);
            let ids = import_accounts(path.clone(), vec![incoming.clone()], |ids| async {
                Ok(ids)
            })
            .await?;
            assert_eq!(existing_id(&path, &incoming).await?, Some(ids[0].clone()));
            let mut stored = read_at(path.clone()).await?;
            let saved = stored.iter_mut().find(|a| a.id == ids[0]).ok_or(INVALID)?;
            assert!(saved.token() == "synthetic-access");
            saved.access = "synthetic-newer-access".into();
            saved.refresh = "synthetic-newer-refresh".into();
            write_at(path.clone(), stored).await?;
            let again =
                import_accounts(path.clone(), vec![incoming], |ids| async { Ok(ids) }).await?;
            assert_eq!(again, ids);
            let stored = read_at(path.clone()).await?;
            assert!(stored
                .iter()
                .find(|a| a.id == ids[0])
                .is_some_and(|a| a.access == "synthetic-newer-access"
                    && a.refresh == "synthetic-newer-refresh"));
        }
        assert_eq!(read_at(path.clone()).await?.len(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path)
                    .map_err(|_| IMPORT_FAILED)?
                    .permissions()
                    .mode()
                    & 0o077,
                0
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn rejected_provider_batch_removes_new_accounts_and_claude_credential_files(
    ) -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| IMPORT_FAILED)?;
        let path = dir.path().join("accounts.json");
        let claude = parse(
            SubscriptionProvider::Claude,
            &json!({"user":"claude@example.test",
            "auth":{"claudeAiOauth":{"accessToken":"synthetic-claude-access","refreshToken":"synthetic-claude-refresh"}},
            "profile":{"emailAddress":"claude@example.test","accountUuid":"fixture-uuid"}}),
        )?;
        let result = import_accounts(
            path.clone(),
            vec![
                parse(SubscriptionProvider::Antigravity, &google_row())?,
                claude,
            ],
            |_| async { Err::<(), _>("providerImport.conflict".into()) },
        )
        .await;
        assert_eq!(result, Err("providerImport.conflict".into()));
        assert!(!path.exists());
        assert!(std::fs::read_dir(dir.path().join("claude"))
            .map_err(|_| IMPORT_FAILED)?
            .next()
            .is_none());
        Ok(())
    }

    #[test]
    fn claude_imports_match_existing_cli_accounts_independently_of_unused_client_metadata(
    ) -> Result<(), String> {
        let incoming = parse(
            SubscriptionProvider::Claude,
            &json!({"user":"claude@example.test",
            "auth":{"claudeAiOauth":{"accessToken":"synthetic-claude-access"}}}),
        )?;
        let mut existing = incoming.account.clone();
        assert!(incoming.account.access.is_empty() && incoming.account.refresh.is_empty());
        existing.client_id = "legacy-unused-client".into();
        assert!(matches(&existing, &incoming.account));
        existing.login = "other@example.test".into();
        assert!(!matches(&existing, &incoming.account));
        Ok(())
    }

    #[test]
    fn imports_validate_tokens_and_bind_reauthentication_to_the_same_google_identity(
    ) -> Result<(), String> {
        for provider in [
            SubscriptionProvider::Antigravity,
            SubscriptionProvider::GeminiCli,
        ] {
            let mut row = google_row();
            row["auth"]["refresh_token"] = json!("bad\ncredential");
            assert!(parse(provider, &row).is_err());
            let imported = parse(provider, &google_row())?.account;
            let mut fresh = imported.clone();
            fresh.subject = "fixture-google-id".into();
            assert!(reauth_matches(&imported, &fresh));
            fresh.login = "other@example.test".into();
            assert!(!reauth_matches(&imported, &fresh));
        }
        let row = json!({"user":"openai@example.test","auth":{"clientId":"fixture-client","sub":"fixture-user",
            "access":"synthetic-direct-access","refresh":"synthetic-direct-refresh","expires":"2100-01-01T00:00:00Z"}});
        let imported = parse(SubscriptionProvider::ChatgptApi, &row)?;
        assert_eq!(imported.account.client_id, "fixture-client");
        assert_eq!(imported.account.expires, 4102444800);
        Ok(())
    }
}
