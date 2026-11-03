//! Resolve immutable execution snapshots off the forwarding path.
use super::{catalog, eligible_provider, RoutingGroup};
use crate::{
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

#[cfg(test)]
pub(crate) async fn temporary_catalog(session: &Session) -> Result<tempfile::TempPath, String> {
    temporary_catalog_with_settings(session, &Default::default()).await
}

pub(crate) async fn temporary_catalog_with_settings(
    session: &Session,
    settings: &super::context::Settings,
) -> Result<tempfile::TempPath, String> {
    let efforts =
        std::collections::HashMap::from([(session.group.id.clone(), session.offered_levels())]);
    let images = std::collections::HashMap::from([(
        session.group.id.clone(),
        session
            .menu_targets()
            .any(|target| target.effort.images != Some(false)),
    )]);
    let mut catalog =
        super::client::catalog(std::slice::from_ref(&session.group), &efforts, "", &images)?;
    super::context::Metadata::for_session(session).publish(
        settings,
        super::context::Metadata::session_typed_window(session),
        !session.targets.is_empty() && session.targets.iter().all(|t| t.official),
        &mut catalog["models"][0],
    );
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
    prepare_inner(manager, group, providers, true).await
}

pub(super) async fn prepare_templates(
    manager: Arc<CodexOAuthManager>,
    group: RoutingGroup,
    providers: Vec<crate::provider::Provider>,
) -> Result<Session, String> {
    prepare_inner(manager, group, providers, false).await
}

async fn prepare_inner(
    manager: Arc<CodexOAuthManager>,
    group: RoutingGroup,
    providers: Vec<crate::provider::Provider>,
    enforce_account_models: bool,
) -> Result<Session, String> {
    let group = super::membership::resolve(group, &providers)?;
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
        if !providers.contains_key(&member.provider_id) && !group.includes_in_menu(member) {
            continue;
        }
        let source = providers
            .get(&member.provider_id)
            .ok_or("intentRouting.providerUnavailable")?;
        if !crate::services::codex_provider::serves(source, &member.model_id)
            || !crate::services::routing_upstreams::serves(source, &member.model_id)
        {
            if group.includes_in_menu(member) {
                return Err("intentRouting.providerUnavailable".into());
            }
            continue;
        }
        let before = targets.len();
        for provider in crate::services::routing_upstreams::variants(source) {
            if enforce_account_models
                && !crate::services::codex_provider::account_serves(&provider, &member.model_id)
            {
                continue;
            }
            let prepared = async {
                let provider = &provider;
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
                let (url, mut headers, workspace) = if let Some(credentials) = &credentials {
                    let mut headers = http::HeaderMap::new();
                    // Shared menus freeze identity, not access tokens. Forwarding refreshes
                    // that same identity; a temporary auth outage must not erase its models.
                    if enforce_account_models {
                        let token =
                            tokio::time::timeout(Duration::from_secs(30), credentials.token())
                                .await
                                .map_err(|_| "intentRouting.providerUnavailable")?
                                .ok_or("intentRouting.providerUnavailable")?;
                        headers.insert(
                            http::header::AUTHORIZATION,
                            http::HeaderValue::from_str(&format!("Bearer {token}"))
                                .map_err(|_| "intentRouting.providerUnavailable")?,
                        );
                    }
                    (
                        "https://chatgpt.com/backend-api/codex/responses".to_string(),
                        headers,
                        Some(credentials.workspace.clone()),
                    )
                } else {
                    let (url, headers) = static_transport(provider, &member.model_id)?;
                    (url, headers, None)
                };
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
                    fixed_effort: member.effort.clone(),
                    provider_id: member.provider_id.clone(),
                    model: member.model_id.clone(),
                    upstream: if official {
                        None
                    } else {
                        Some(normalized_static_provider(provider)?)
                    },
                    effort: super::effort::Capability::for_provider(provider, &member.model_id),
                    url: reqwest::Url::parse(&url)
                        .map_err(|_| "intentRouting.providerUnavailable")?,
                    headers,
                    official,
                    credentials,
                    scheduling: Arc::new(crate::proxy::intent::scheduling::Scheduling::new(
                        official,
                    )),
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
                Err(_)
                    if source
                        .settings_config
                        .get(crate::services::codex_provider::POLICY)
                        .is_some() => {}
                Err(error) => return Err(error),
            }
        }
        if targets.len() == before && group.includes_in_menu(member) {
            return Err("intentRouting.providerUnavailable".into());
        }
    }
    let client = tokio::task::spawn_blocking(crate::proxy::http_client::get_without_redirects)
        .await
        .map_err(|_| "intentRouting.failed")?
        .map_err(|_| "intentRouting.failed")?;
    #[cfg(not(test))]
    for (index, target) in targets.iter().enumerate() {
        if enforce_account_models
            && target.official
            && !targets[..index].iter().any(|t| t.same_account(target))
        {
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

fn normalized_static_provider(
    provider: &crate::provider::Provider,
) -> Result<crate::provider::Provider, String> {
    let mut provider = provider.clone();
    super::context::Metadata::strip_from_settings(&mut provider.settings_config);
    let format = provider
        .meta
        .as_ref()
        .and_then(|m| m.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("api_format")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            provider
                .settings_config
                .get("apiFormat")
                .and_then(serde_json::Value::as_str)
        })
        .ok_or("intentRouting.providerUnavailable")?
        .to_owned();
    provider
        .meta
        .get_or_insert_with(Default::default)
        .api_format = Some(format);
    Ok(provider)
}

pub(super) fn retarget(target: &mut Target, model: &str) -> Result<(), String> {
    target.model = model.into();
    if let Some(provider) = &target.upstream {
        let (url, _) = static_transport(provider, model)?;
        target.url = reqwest::Url::parse(&url).map_err(|_| "intentRouting.providerUnavailable")?;
    }
    Ok(())
}

fn static_transport(
    provider: &crate::provider::Provider,
    model: &str,
) -> Result<(String, http::HeaderMap), String> {
    use crate::proxy::providers::{ClaudeAdapter, ProviderAdapter};
    if !eligible_provider(provider, false) {
        return Err("intentRouting.providerUnavailable".into());
    }
    let adapter = ClaudeAdapter::new();
    let (legacy_base, token) =
        provider.resolve_usage_credentials(&crate::app_config::AppType::Codex);
    let base = adapter.extract_base_url(provider).unwrap_or(legacy_base);
    let format = provider
        .meta
        .as_ref()
        .and_then(|m| m.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("api_format")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            provider
                .settings_config
                .get("apiFormat")
                .and_then(serde_json::Value::as_str)
        })
        .ok_or("intentRouting.providerUnavailable")?;
    let url = if provider.meta.as_ref().and_then(|m| m.is_full_url) == Some(true) {
        base.clone()
    } else {
        match format {
            "anthropic" => adapter.build_url(&base, "/v1/messages"),
            "openai_chat" => format!("{}/chat/completions", base.trim_end_matches('/')),
            "openai_responses" => format!("{}/responses", base.trim_end_matches('/')),
            "gemini_native" => {
                let model = crate::proxy::gemini_url::normalize_gemini_model_id(model);
                let endpoint = "/v1beta/models/routing-placeholder:generateContent";
                let mut url = reqwest::Url::parse(
                    &crate::proxy::gemini_url::build_gemini_native_url(&base, endpoint),
                )
                .map_err(|_| "intentRouting.providerUnavailable")?;
                url.path_segments_mut()
                    .map_err(|_| "intentRouting.providerUnavailable")?
                    .pop()
                    .push(&format!("{model}:generateContent"));
                url.to_string()
            }
            _ => return Err("intentRouting.providerUnavailable".into()),
        }
    };
    let mut headers = http::HeaderMap::new();
    let (name, value) = match format {
        "gemini_native" => ("x-goog-api-key", token.clone()),
        "anthropic"
            if provider.settings_config["authType"] == "api-key"
                || provider.claude_uses_api_key_field()
                || (provider
                    .settings_config
                    .pointer("/env/ANTHROPIC_AUTH_TOKEN")
                    .and_then(serde_json::Value::as_str)
                    .is_none_or(|key| key.trim().is_empty())
                    && provider
                        .settings_config
                        .pointer("/env/ANTHROPIC_API_KEY")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|key| !key.trim().is_empty())) =>
        {
            ("x-api-key", token.clone())
        }
        _ => ("authorization", format!("Bearer {token}")),
    };
    headers.insert(
        http::HeaderName::from_static(name),
        http::HeaderValue::from_str(&value).map_err(|_| "intentRouting.providerUnavailable")?,
    );
    if format == "anthropic" {
        if provider.settings_config["authType"] == "both" {
            headers.insert(
                "x-api-key",
                http::HeaderValue::from_str(&token)
                    .map_err(|_| "intentRouting.providerUnavailable")?,
            );
        }
        headers.insert(
            "anthropic-version",
            http::HeaderValue::from_static("2023-06-01"),
        );
    }
    if let Some(overrides) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.local_proxy_request_overrides.as_ref())
    {
        for (name, value) in &overrides.headers {
            let name = http::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| "intentRouting.providerUnavailable")?;
            if crate::proxy::is_protected_local_proxy_override_header(&name) {
                continue;
            }
            headers.insert(
                name,
                http::HeaderValue::from_str(value)
                    .map_err(|_| "intentRouting.providerUnavailable")?,
            );
        }
    }
    Ok((url, headers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn imported_transport_preserves_private_headers_and_dual_auth(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let record = serde_json::from_value(json!({
            "schemaVersion":1,"id":"fixture","name":"Fixture","revision":1,
            "source":{"kind":"api","baseUrl":"https://relay.example","apiKey":"synthetic-import-key","apiFormat":"anthropic","authType":"both","mode":"vendor","models":[],"catalog":[],"groupsOnly":false},
            "transport":{"headers":{"x-workspace":"synthetic-workspace"}}
        }))?;
        let provider = crate::services::routing_upstreams::resolve_provider(&record)?;
        let (url, headers) = static_transport(&provider, "exact-model")?;
        let (_, token) = provider.resolve_usage_credentials(&crate::app_config::AppType::Codex);
        assert_eq!(url, "https://relay.example/v1/messages");
        assert!(headers["x-api-key"].as_bytes() == token.as_bytes());
        assert!(headers["authorization"].as_bytes() == format!("Bearer {token}").as_bytes());
        assert!(headers["x-workspace"] == "synthetic-workspace");
        Ok(())
    }

    #[tokio::test]
    async fn codex_editor_pools_keep_account_order_and_do_not_expand_a_disabled_model(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::codex_provider::{self, ModelMode, Settings};
        let dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        manager
            .add_test_account_with_user_identity("one", "synthetic-access-one", "user-one")
            .await?;
        manager
            .add_test_account_with_user_identity("two", "synthetic-access-two", "user-two")
            .await?;
        let mut provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({"modelCatalog":{"models":[{"model":"model-a"}]}}),
            None,
        );
        provider.category = Some("official".into());
        let settings = Settings {
            accounts: vec!["two".into(), "one".into()],
            mode: ModelMode::Picked,
            models: vec![json!({"model":"model-a"})],
            groups_only: false,
        };
        codex_provider::attach(&mut provider, &settings)?;
        let group: RoutingGroup = serde_json::from_value(
            json!({"id":"fixture","name":"Synthetic","revision":0,"enabled":true,"members":[{"id":"a","providerId":"fixture","modelId":"model-a"}],"defaultMemberId":"a","fallbackMemberIds":[],"intentRecognitionEnabled":false,"classificationBudgetMs":2000,"rules":[]}),
        )?;
        let session =
            prepare_with_providers(manager.clone(), group.clone(), vec![provider.clone()]).await?;
        assert_eq!(session.targets.len(), 2);
        let second = session.targets[1]
            .credentials
            .as_ref()
            .ok_or("credentials")?;
        let mut expected = provider.clone();
        codex_provider::bind(&mut expected, "one");
        let expected = catalog::freeze_execution(&expected, manager.clone())
            .await
            .ok_or("expected")?;
        assert!(second.same_identity(&expected));
        let mut hidden = group;
        hidden.members[0].model_id = "model-b".into();
        assert!(prepare_with_providers(manager, hidden, vec![provider])
            .await
            .is_err());
        Ok(())
    }

    #[test]
    fn public_model_facts_do_not_change_frozen_transport_identity() {
        let provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({"apiFormat":"openai_chat","base_url":"https://synthetic.invalid/v1"}),
            None,
        );
        let mut enriched = provider.clone();
        enriched.settings_config["routingModelFacts"] = json!({
            "fixture-model":{"outputPrice":12,"maker":"synthetic","released":"2026-10-01","images":true}
        });
        enriched.settings_config["routingModelCapabilities"] =
            json!({"fixture-model":{"reasoningLevels":["low","high"]}});
        enriched.settings_config["routingPublicCapabilities"] =
            json!({"fixture-model":{"reasoning":true}});
        let first = normalized_static_provider(&provider).expect("plain snapshot");
        let next = normalized_static_provider(&enriched).expect("enriched snapshot");
        assert_eq!(first.settings_config, next.settings_config);
        let mut targets = synthetic_session().targets;
        targets[0].upstream = Some(first);
        let mut refreshed = targets[0].clone();
        refreshed.upstream = Some(next);
        assert!(targets[0].same_destination(&refreshed));
        refreshed
            .upstream
            .as_mut()
            .expect("snapshot")
            .settings_config["base_url"] = json!("https://changed.synthetic.invalid/v1");
        assert!(!targets[0].same_destination(&refreshed));
    }
    #[test]
    fn display_name_overrides_do_not_change_transport_but_reasoning_overrides_do() {
        let mut provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({"apiFormat":"openai_chat","modelCatalog":{"models":[{"model":"exact-model","displayName":"Original","reasoningLevels":["high"]}]}}),
            None,
        );
        let first = normalized_static_provider(&provider).expect("snapshot");
        provider.settings_config["modelCatalog"]["models"][0]["displayName"] = json!("Changed");
        provider.settings_config["modelCatalog"]["models"][0]["modelPreferences"] =
            json!({"name":"Custom"});
        let renamed = normalized_static_provider(&provider).expect("renamed");
        assert_eq!(first.settings_config, renamed.settings_config);
        provider.settings_config["modelCatalog"]["models"][0]["modelPreferences"]["sameAs"] =
            json!("canonical-model");
        let equivalent = normalized_static_provider(&provider).expect("equivalent");
        assert_eq!(first.settings_config, equivalent.settings_config);
        provider.settings_config["modelCatalog"]["models"][0]["modelPreferences"]
            ["reasoningLevels"] = json!([]);
        let changed = normalized_static_provider(&provider).expect("changed");
        assert_ne!(first.settings_config, changed.settings_config);
    }

    fn synthetic_session() -> Session {
        let group = super::super::tests::fixture();
        let targets = group.members.iter().map(|member| {
            let mut headers = http::HeaderMap::new();
            headers.insert(http::header::AUTHORIZATION, http::HeaderValue::from_static("Bearer synthetic-private-key"));
            Target { fixed_effort: None,
                provider_id: member.provider_id.clone(),
                model: member.model_id.clone(),
                upstream: None,
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
    async fn static_targets_use_explicit_protocol_endpoints_and_source_credentials() {
        use crate::services::routing_upstreams::{
            resolve_provider, RoutingUpstream, UpstreamSource,
        };
        let dir = tempfile::tempdir().expect("dir");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        for (format, suffix, auth_header) in [
            ("openai_responses", "/v1/responses", "authorization"),
            ("openai_chat", "/v1/chat/completions", "authorization"),
            ("anthropic", "/v1/messages", "x-api-key"),
            (
                "gemini_native",
                "/v1beta/models/exact-model:generateContent",
                "x-goog-api-key",
            ),
        ] {
            let record = RoutingUpstream {
                transport: None,
                schema_version: 1,
                preset_id: None,
                region: None,
                codex_settings: None,
                id: "fixture".into(),
                name: "Synthetic".into(),
                revision: 1,
                has_api_key: false,
                enabled: true,
                source: UpstreamSource::Api {
                    catalog: vec![],
                    mode: crate::services::codex_provider::ModelMode::Picked,
                    groups_only: false,
                    base_url: "https://synthetic.example/v1".into(),
                    api_key: Some("synthetic-fixture-key".into()),
                    api_format: format.into(),
                    models: vec![json!({"model":"exact-model"})],
                    auth_type: Some("api-key".into()),
                },
            };
            let provider = resolve_provider(&record).expect("provider");
            let mut group = super::super::tests::fixture();
            group.members.truncate(1);
            group.members[0].model_id = "exact-model".into();
            group.fallback_member_ids.clear();
            group.rules.clear();
            group.intent_recognition_enabled = false;
            let session = prepare_with_providers(manager.clone(), group, vec![provider])
                .await
                .expect("prepare");
            let target = &session.targets[0];
            assert_eq!(
                target.url.as_str(),
                format!("https://synthetic.example{suffix}")
            );
            assert!(target.headers.contains_key(auth_header));
            assert_eq!(target.model, "exact-model");
            assert!(target.upstream.is_some());
            assert!(target.credentials.is_none());
            if format == "anthropic" {
                assert_eq!(target.headers["anthropic-version"], "2023-06-01");
                assert!(!target.headers.contains_key("authorization"));
            }
        }
    }

    #[test]
    fn static_full_urls_and_gemini_exact_model_segments_are_not_reinterpreted() {
        use crate::services::routing_upstreams::{
            resolve_provider, RoutingUpstream, UpstreamSource,
        };
        let record = RoutingUpstream {
            transport: None,
            schema_version: 1,
            preset_id: None,
            region: None,
            codex_settings: None,
            id: "fixture".into(),
            name: "Synthetic".into(),
            revision: 1,
            has_api_key: false,
            enabled: true,
            source: UpstreamSource::Api {
                catalog: vec![],
                mode: crate::services::codex_provider::ModelMode::Picked,
                groups_only: false,
                base_url: "https://synthetic.example/v1".into(),
                api_key: Some("synthetic-key".into()),
                api_format: "gemini_native".into(),
                models: vec![],
                auth_type: None,
            },
        };
        let mut provider = resolve_provider(&record).expect("provider");
        let (url, _) = static_transport(&provider, "model/with?query#fragment").expect("transport");
        let url = reqwest::Url::parse(&url).expect("url");
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
        assert!(url
            .path()
            .ends_with("model%2Fwith%3Fquery%23fragment:generateContent"));
        provider.meta.as_mut().expect("meta").is_full_url = Some(true);
        provider.settings_config["env"]["ANTHROPIC_BASE_URL"] =
            serde_json::json!("https://synthetic.example/custom/endpoint");
        assert_eq!(
            static_transport(&provider, "exact-model")
                .expect("full URL")
                .0,
            "https://synthetic.example/custom/endpoint"
        );
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
                "modelCatalog":{"models":[{"model":"strong-model","reasoningLevels":["high","max"],"input_modalities":["text","image"],"contextWindow":1000000,"autoCompactTokenLimit":200000}]}
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
        assert_eq!(model["models"][0]["context_window"], 1000000);
        assert_eq!(model["models"][0]["auto_compact_token_limit"], 200000);
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
        db.register_routing_fixture("codex", &provider.id)
            .expect("explicit import");
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
