//! Resolve immutable execution snapshots off the forwarding path.
use super::{catalog, eligible_provider, RoutingGroup};
use crate::{
    app_config::AppType,
    database::Database,
    proxy::{
        intent::{Session, Target},
        providers::codex_oauth_auth::{CodexOAuthManager, CODEX_OAUTH_ORIGINATOR},
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) async fn prepare(
    db: Arc<Database>,
    manager: Arc<CodexOAuthManager>,
    group: RoutingGroup,
) -> Result<Session, String> {
    group.validate()?;
    let (providers, _) = catalog::prepare_providers(db, manager.clone(), false).await?;
    prepare_with_providers(manager, group, providers).await
}

pub(crate) async fn temporary_catalog(session: &Session) -> Result<tempfile::TempPath, String> {
    let efforts = std::collections::HashMap::from([(
        session.group.id.clone(),
        super::effort::shared(session.menu_targets().map(|target| &target.effort)),
    )]);
    let images = std::collections::HashMap::from([(
        session.group.id.clone(),
        session
            .menu_targets()
            .any(|target| target.effort.images != Some(false)),
    )]);
    let catalog =
        super::client::catalog(std::slice::from_ref(&session.group), &efforts, "", &images)?;
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut file = tempfile::Builder::new()
            .prefix("vary-routing-session-")
            .suffix(".json")
            .tempfile()
            .map_err(|_| "intentRouting.failed")?;
        let bytes = serde_json::to_vec(&catalog).map_err(|_| "intentRouting.failed")?;
        file.write_all(&bytes).map_err(|_| "intentRouting.failed")?;
        file.flush().map_err(|_| "intentRouting.failed")?;
        Ok(file.into_temp_path())
    })
    .await
    .map_err(|_| "intentRouting.failed")?
}

pub(crate) async fn prepare_with_providers(
    manager: Arc<CodexOAuthManager>,
    group: RoutingGroup,
    providers: Vec<crate::provider::Provider>,
) -> Result<Session, String> {
    group.validate()?;
    let client_version = crate::codex_client::resolve_version().await;
    let providers: std::collections::HashMap<_, _> =
        providers.into_iter().map(|p| (p.id.clone(), p)).collect();
    let classifier = if group.uses_classifier() {
        let target = group
            .classifier_target
            .as_ref()
            .ok_or("intentRouting.providerUnavailable")?;
        let provider = providers
            .get(&target.provider_id)
            .filter(|p| eligible_provider(p, true))
            .ok_or("intentRouting.providerUnavailable")?;
        Some(provider.clone())
    } else {
        None
    };
    let mut targets: Vec<Target> = Vec::new();
    for member in &group.members {
        let prepared = async {
            let provider = providers
                .get(&member.provider_id)
                .ok_or("intentRouting.providerUnavailable")?;
            let official =
                catalog::official_source(provider) && !eligible_provider(provider, false);
            let credentials = if official {
                Some(Arc::new(
                    tokio::time::timeout(
                        Duration::from_secs(30),
                        catalog::freeze_execution(provider, manager.clone()),
                    )
                    .await
                    .map_err(|_| "intentRouting.providerUnavailable")?
                    .ok_or("intentRouting.providerUnavailable")?,
                ))
            } else {
                None
            };
            let (url, token, workspace) = if let Some(credentials) = &credentials {
                let token = tokio::time::timeout(Duration::from_secs(30), credentials.token())
                    .await
                    .map_err(|_| "intentRouting.providerUnavailable")?
                    .ok_or("intentRouting.providerUnavailable")?;
                (
                    "https://chatgpt.com/backend-api/codex/responses".to_string(),
                    token,
                    Some(credentials.workspace.clone()),
                )
            } else {
                if !eligible_provider(provider, false) {
                    return Err("intentRouting.providerUnavailable".into());
                }
                let (base, token) = provider.resolve_usage_credentials(&AppType::Codex);
                (
                    format!("{}/responses", base.trim_end_matches('/')),
                    token,
                    None,
                )
            };
            let mut headers = http::HeaderMap::new();
            headers.insert(
                http::header::AUTHORIZATION,
                http::HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| "intentRouting.providerUnavailable")?,
            );
            if let Some(workspace) = workspace {
                headers.insert(
                    "chatgpt-account-id",
                    http::HeaderValue::from_str(&workspace)
                        .map_err(|_| "intentRouting.providerUnavailable")?,
                );
                headers.insert(
                    "originator",
                    http::HeaderValue::from_static(CODEX_OAUTH_ORIGINATOR),
                );
                headers.insert(
                    "version",
                    http::HeaderValue::from_str(&client_version)
                        .map_err(|_| "intentRouting.providerUnavailable")?,
                );
                headers.insert(
                    http::header::USER_AGENT,
                    http::HeaderValue::from_str(&format!("codex_cli_rs/{client_version}"))
                        .map_err(|_| "intentRouting.providerUnavailable")?,
                );
                headers.insert(
                    "openai-beta",
                    http::HeaderValue::from_static("responses=experimental"),
                );
            }
            Ok::<Target, String>(Target {
                provider_id: member.provider_id.clone(),
                model: member.model_id.clone(),
                effort: provider
                    .settings_config
                    .pointer("/modelCatalog/models")
                    .and_then(|v| v.as_array())
                    .and_then(|entries| {
                        entries.iter().find(|entry| {
                            entry
                                .get("model")
                                .and_then(|v| v.as_str())
                                .is_some_and(|id| id.trim() == member.model_id)
                        })
                    })
                    .map(super::effort::Capability::parse)
                    .unwrap_or_default(),
                url: reqwest::Url::parse(&url).map_err(|_| "intentRouting.providerUnavailable")?,
                headers,
                official,
                credentials,
                scheduling: Arc::new(crate::proxy::intent::scheduling::Scheduling::new(official)),
            })
        }
        .await;
        match prepared {
            Ok(mut target) => {
                if let Some(previous) = targets.iter().find(|t| t.same_account(&target)) {
                    target.scheduling = Arc::new(previous.scheduling.for_model());
                }
                targets.push(target);
            }
            Err(_) if !group.includes_in_menu(member) => {}
            Err(error) => return Err(error),
        }
    }
    let client = tokio::task::spawn_blocking(crate::proxy::http_client::get_without_redirects)
        .await
        .map_err(|_| "intentRouting.failed")?
        .map_err(|_| "intentRouting.failed")?;
    for (index, target) in targets.iter().enumerate() {
        if target.official && !targets[..index].iter().any(|t| t.same_account(target)) {
            target.scheduling.prime(&client, &target.headers).await;
        }
    }
    Ok(Session {
        group,
        targets,
        classifier,
        client,
        created: Instant::now(),
        bindings: Arc::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn synthetic_session() -> Session {
        let group = super::super::tests::fixture();
        let targets = group.members.iter().map(|member| {
            let mut headers = http::HeaderMap::new();
            headers.insert(http::header::AUTHORIZATION, http::HeaderValue::from_static("Bearer synthetic-private-key"));
            Target {
                provider_id: member.provider_id.clone(),
                model: member.model_id.clone(),
                effort: super::super::effort::Capability::parse(&json!({
                    "reasoningLevels":["low","medium","high"],"input_modalities":["text","image"]
                })),
                url: "https://synthetic.example/v1/responses".parse().expect("url"),
                headers,
                official: false,
                credentials: None,
                scheduling: Arc::default(),
            }
        }).collect();
        Session {
            group,
            targets,
            classifier: None,
            client: reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("client"),
            created: Instant::now(),
            bindings: Arc::default(),
        }
    }
    #[tokio::test]
    async fn manual_temporary_catalog_and_preparation_use_the_pick_not_unavailable_members() {
        let dir = tempfile::tempdir().expect("dir");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut group = super::super::tests::fixture();
        group.routing = super::super::RoutingStrategy::Manual;
        group.manual_member_id = Some("strong".into());
        group.intent_recognition_enabled = true;
        group.members[0].provider_id = "missing".into();
        let provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Fixture".into(),
            json!({
                "auth":{"OPENAI_API_KEY":"synthetic"},"apiFormat":"openai_responses",
                "config":"model_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.example/v1'\nwire_api='responses'",
                "modelCatalog":{"models":[{"model":"strong-model","reasoningLevels":["high","max"],"input_modalities":["text","image"]}]}
            }),
            None,
        );
        let session = prepare_with_providers(manager, group, vec![provider])
            .await
            .expect("manual");
        assert_eq!(session.targets.len(), 1);
        assert_eq!(session.targets[0].model, "strong-model");
        let file = temporary_catalog(&session).await.expect("catalog");
        let model: serde_json::Value =
            serde_json::from_slice(&std::fs::read(file).expect("read")).expect("json");
        assert_eq!(
            model["models"][0]["input_modalities"],
            json!(["text", "image"])
        );
        assert_eq!(
            model["models"][0]["supported_reasoning_levels"]
                .as_array()
                .expect("levels")
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn temporary_catalog_contains_only_selected_group_and_no_credentials() {
        let file = temporary_catalog(&synthetic_session())
            .await
            .expect("catalog");
        let text = std::fs::read_to_string(&file).expect("read catalog");
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["models"].as_array().expect("models").len(), 1);
        let model = &value["models"][0];
        assert_eq!(model["slug"], "vary-auto/development");
        assert!(model["display_name"]
            .as_str()
            .expect("label")
            .ends_with(" · routing group"));
        assert_eq!(model["visibility"], "list");
        assert_eq!(model["supported_in_api"], true);
        assert_eq!(model["input_modalities"], json!(["text", "image"]));
        assert_eq!(
            model["supported_reasoning_levels"]
                .as_array()
                .expect("levels")
                .len(),
            3
        );
        assert!(!text.contains("synthetic-private-key"));
        assert!(!text.contains("synthetic.example"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&file).expect("file").permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn temporary_catalog_is_removed_on_revoke_expiry_and_runtime_drop() {
        use crate::proxy::intent::{Runtime, TEMPORARY_SESSION_TTL};
        let runtime = Arc::new(Runtime::default());
        let session = synthetic_session();
        let file = temporary_catalog(&session).await.expect("catalog");
        let path = file.to_path_buf();
        let token = runtime.install(session, Some(file)).expect("session");
        assert!(path.exists());
        runtime.revoke(&token);
        assert!(!path.exists());

        let mut session = synthetic_session();
        let file = temporary_catalog(&session).await.expect("catalog");
        let path = file.to_path_buf();
        session.created = Instant::now() - TEMPORARY_SESSION_TTL + Duration::from_millis(100);
        runtime
            .install(session, Some(file))
            .expect("expiring session");
        tokio::time::timeout(Duration::from_secs(2), async {
            while path.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("expiry cleans catalog");

        let session = synthetic_session();
        let file = temporary_catalog(&session).await.expect("catalog");
        let path = file.to_path_buf();
        runtime.install(session, Some(file)).expect("session");
        drop(runtime);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn temporary_catalog_is_removed_if_session_capacity_is_exhausted() {
        let runtime = Arc::new(crate::proxy::intent::Runtime::default());
        for _ in 0..16 {
            runtime.install(synthetic_session(), None).expect("session");
        }
        let session = synthetic_session();
        let file = temporary_catalog(&session).await.expect("catalog");
        let path = file.to_path_buf();
        assert!(runtime.install(session, Some(file)).is_err());
        assert!(!path.exists());
    }
    #[tokio::test]
    async fn official_account_snapshot_never_follows_default_account_changes() {
        let dir = tempfile::tempdir().expect("temporary account store");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        manager
            .add_test_account_with_workspace_and_access_token(
                "account-a",
                "workspace-a",
                "synthetic-token-a",
                Some(&crate::codex_config::test_codex_id_token("user-a")),
            )
            .await
            .expect("account a");
        manager
            .add_test_account_with_workspace_and_access_token(
                "account-b",
                "workspace-b",
                "synthetic-token-b",
                Some(&crate::codex_config::test_codex_id_token("user-b")),
            )
            .await
            .expect("account b");
        let db = Arc::new(Database::memory().expect("database"));
        let mut provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Official".into(),
            json!({}),
            None,
        );
        provider.category = Some("official".into());
        provider.meta = Some(serde_json::from_value(json!({"authBinding":{"source":"managed_account","authProvider":"codex_oauth","accountId":"account-a"}})).expect("binding"));
        db.save_provider("codex", &provider).expect("provider");
        let session = prepare(db, manager.clone(), super::super::tests::fixture())
            .await
            .expect("snapshot");
        manager
            .set_default_account("account-b")
            .await
            .expect("change default");
        assert_eq!(
            session.targets[0].headers["authorization"],
            "Bearer synthetic-token-a"
        );
        assert_eq!(
            session.targets[0].headers["chatgpt-account-id"],
            "workspace-a"
        );
        assert_eq!(
            session.targets[0].url.as_str(),
            "https://chatgpt.com/backend-api/codex/responses"
        );
        assert_eq!(session.targets[0].model, "balanced-model");
        assert_eq!(session.targets[1].model, "strong-model");
        let version = session.targets[0].headers["version"]
            .to_str()
            .expect("version");
        assert_eq!(
            session.targets[0].headers[http::header::USER_AGENT],
            format!("codex_cli_rs/{version}")
        );
        assert!(
            version
                .split('.')
                .map(|p| p.parse::<u32>().expect("numeric version"))
                .collect::<Vec<_>>()
                .as_slice()
                >= [0, 164, 0].as_slice()
        );
    }
}
