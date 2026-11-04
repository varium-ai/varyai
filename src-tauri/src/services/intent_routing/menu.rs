//! One immutable model menu contains direct provider models and routing groups.
use super::{eligible_provider, execution, Member, RoutingGroup};
use crate::{
    provider::Provider,
    proxy::{
        intent::{Menu, MenuModel, Session, Target},
        providers::codex_oauth_auth::CodexOAuthManager,
    },
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

pub(crate) const MAX_MENU_ENTRIES: usize = 256;

fn direct_id(provider: &str, model: &str) -> String {
    let mut hash = Sha256::new();
    for value in [provider, model] {
        hash.update(value.len().to_be_bytes());
        hash.update(value.as_bytes());
    }
    format!("model-{:x}", hash.finalize())
}
fn direct_group(provider: &Provider, model: &str) -> RoutingGroup {
    RoutingGroup {
        id: direct_id(&provider.id, model),
        name: "Direct model".into(),
        revision: 0,
        client: "codex".into(),
        enabled: false,
        members: vec![Member {
            effort: None,
            id: "direct".into(),
            provider_id: provider.id.clone(),
            model_id: model.into(),
            weight: 1,
            free: None,
        }],
        default_member_id: "direct".into(),
        fallback_member_ids: vec![],
        intent_recognition_enabled: false,
        classifier_target: None,
        classification_budget_ms: 2000,
        rules: vec![],
        auto_generated: false,
        routing: Default::default(),
        model_alias: None,
        manual_member_id: None,
        context_window: None,
        affinity: String::new(),
        first_token: 0,
        patience: -1,
        off: vec![],
        matches: vec![],
        matched: vec![],
        unlists: false,
        sink: false,
        levels: vec![],
    }
}
fn configured_model(provider: &Provider) -> Option<String> {
    let config = provider
        .settings_config
        .get("config")?
        .as_str()?
        .parse::<toml::Value>()
        .ok()?;
    config.get("model")?.as_str().map(str::to_owned)
}
fn declared_capability(provider: &Provider, model: &str) -> super::effort::Capability {
    super::effort::Capability::for_provider(provider, model)
}

pub(crate) async fn prepare(
    manager: Arc<CodexOAuthManager>,
    mut providers: Vec<Provider>,
    groups: Vec<RoutingGroup>,
    current_provider: Option<String>,
) -> Result<Menu, String> {
    providers.sort_by(|a, b| {
        (Some(&a.id) != current_provider.as_ref(), &a.id)
            .cmp(&(Some(&b.id) != current_provider.as_ref(), &b.id))
    });
    let mut omitted_groups = 0;
    let groups = groups
        .into_iter()
        .filter_map(
            |group| match super::membership::resolve(group, &providers) {
                Ok(group) => Some(group),
                Err(_) => {
                    omitted_groups += 1;
                    None
                }
            },
        )
        .collect::<Vec<_>>();
    let group_order = groups
        .iter()
        .enumerate()
        .map(|(index, group)| (group.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut menu = Menu {
        token: format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ),
        groups: HashMap::new(),
        models: HashMap::new(),
        default_model: None,
        omitted_entries: omitted_groups,
        context_settings: Default::default(),
    };
    let mut targets: HashMap<(String, String), Vec<Target>> = HashMap::new();
    let required: HashSet<_> = groups
        .iter()
        .flat_map(|g| &g.members)
        .map(|m| (m.provider_id.clone(), m.model_id.clone()))
        .collect();
    let mut http_client = None;
    let mut accounts: Vec<Target> = Vec::new();
    // Leave capacity for every saved/discovered routing group, even with large catalogs.
    let direct_limit = MAX_MENU_ENTRIES.saturating_sub(groups.len());
    for provider in &providers {
        let current = Some(&provider.id) == current_provider.as_ref();
        let default = configured_model(provider);
        let mut models = BTreeSet::new();
        if let Some(model) = &default {
            models.insert(model.clone());
        }
        if let Some(entries) = provider
            .settings_config
            .pointer("/modelCatalog/models")
            .and_then(|v| v.as_array())
        {
            models.extend(
                entries
                    .iter()
                    .filter_map(|e| e["model"].as_str())
                    .map(|s| s.trim().to_owned()),
            );
        }
        let policy = provider
            .settings_config
            .get(crate::services::codex_provider::POLICY)
            .map(|_| crate::services::codex_provider::Settings::from_provider(provider));
        if policy.as_ref().is_some_and(|p| {
            p.mode == crate::services::codex_provider::ModelMode::Usage || p.accounts.is_empty()
        }) {
            continue;
        }
        if provider.settings_config[crate::services::routing_upstreams::MODEL_POLICY]["mode"]
            == "usage"
        {
            continue;
        }
        let direct_models = if policy.as_ref().is_some_and(|p| p.groups_only)
            || provider.settings_config[crate::services::routing_upstreams::MODEL_POLICY]
                ["groupsOnly"]
                == true
        {
            BTreeSet::new()
        } else {
            models.clone()
        };
        for group in &groups {
            models.extend(
                group
                    .members
                    .iter()
                    .filter(|m| m.provider_id == provider.id)
                    .map(|m| m.model_id.clone()),
            );
        }
        let mut models = models
            .into_iter()
            .filter(|model| {
                crate::services::codex_provider::serves(provider, model)
                    && crate::services::routing_upstreams::serves(provider, model)
            })
            .filter(|model| direct_group(provider, model).validate().is_ok())
            .collect::<Vec<_>>();
        models.sort_by_key(|model| (Some(model) != default.as_ref(), model.clone()));
        let Some(first) = models.first() else {
            continue;
        };
        let prepared = execution::prepare_templates(
            manager.clone(),
            direct_group(provider, first),
            vec![provider.clone()],
        )
        .await;
        let Ok(prepared) = prepared else {
            menu.omitted_entries += models
                .iter()
                .filter(|model| direct_models.contains(*model))
                .count();
            continue;
        };
        if prepared.targets.is_empty() {
            continue;
        }
        let mut templates = prepared.targets.clone();
        for template in &mut templates {
            if let Some(previous) = accounts.iter().find(|t| t.same_account(template)) {
                template.scheduling = Arc::new(previous.scheduling.for_model());
            }
            accounts.push(template.clone());
        }
        http_client.get_or_insert_with(|| prepared.client.clone());
        for model in models {
            if !crate::services::codex_provider::serves(provider, &model) {
                continue;
            }
            let key = (provider.id.clone(), model.clone());
            let direct = direct_models.contains(&model);
            let publish = direct && menu.groups.len() < direct_limit;
            if !publish && !required.contains(&key) {
                menu.omitted_entries += usize::from(direct);
                continue;
            }
            let pool = templates
                .iter()
                .filter(|template| {
                    template
                        .credentials
                        .as_ref()
                        .is_none_or(|c| c.serves_model(&model))
                })
                .filter_map(|template| {
                    let mut target = template.clone();
                    execution::retarget(&mut target, &model).ok()?;
                    target.effort = declared_capability(provider, &model);
                    target.scheduling = Arc::new(template.scheduling.for_model());
                    Some(target)
                })
                .collect::<Vec<_>>();
            if pool.is_empty() {
                continue;
            }
            if required.contains(&key) {
                targets.insert(key, pool.clone());
            }
            if !publish {
                menu.omitted_entries += usize::from(direct);
                continue;
            }
            let group = direct_group(provider, &model);
            let alias = format!("vary-provider/{}", group.id);
            let display = provider
                .settings_config
                .pointer("/modelCatalog/models")
                .and_then(serde_json::Value::as_array)
                .and_then(|entries| {
                    entries
                        .iter()
                        .find(|entry| entry["model"].as_str() == Some(&model))
                })
                .and_then(super::model_preferences::name)
                .unwrap_or(&model);
            let label = format!("{} · {}", display, provider.name);
            if menu.default_model.is_none() || (current && Some(&model) == default.as_ref()) {
                menu.default_model = Some(alias.clone());
            }
            menu.models.insert(
                group.id.clone(),
                MenuModel {
                    alias: alias.clone(),
                    label,
                    description: "VaryAI direct provider model",
                    order: group_order.len() + menu.models.len(),
                },
            );
            menu.groups.insert(
                alias,
                Arc::new(Session {
                    group,
                    targets: pool,
                    classifier: None,
                    client: prepared.client.clone(),
                    created: Instant::now(),
                    bindings: Arc::default(),
                }),
            );
        }
    }
    for group in groups {
        let available = group
            .members
            .iter()
            .map(|m| {
                targets
                    .get(&(m.provider_id.clone(), m.model_id.clone()))
                    .cloned()
                    .map(|mut pool| {
                        for target in &mut pool {
                            target.fixed_effort = m.effort.clone();
                        }
                        pool
                    })
            })
            .collect::<Vec<_>>();
        let ready = group
            .members
            .iter()
            .zip(&available)
            .all(|(member, target)| !group.includes_in_menu(member) || target.is_some());
        let classifier = if group.uses_classifier() {
            group
                .classifier_target
                .as_ref()
                .and_then(|t| {
                    providers
                        .iter()
                        .find(|p| p.id == t.provider_id && eligible_provider(p, true))
                })
                .cloned()
        } else {
            None
        };
        if group.validate().is_err()
            || !group
                .members
                .iter()
                .any(|member| group.includes_in_menu(member))
            || !ready
            || (group.uses_classifier() && classifier.is_none())
            || menu.models.contains_key(&group.id)
            || menu.groups.len() >= MAX_MENU_ENTRIES
        {
            menu.omitted_entries += 1;
            continue;
        }
        let Some(client) = http_client.clone() else {
            menu.omitted_entries += 1;
            continue;
        };
        let plan = available.into_iter().flatten().flatten().collect();
        let alias = group.alias();
        if menu.groups.contains_key(&alias) {
            menu.omitted_entries += 1;
            continue;
        }
        menu.models.insert(
            group.id.clone(),
            MenuModel {
                alias: alias.clone(),
                label: format!("{} · routing group", group.name),
                description: "VaryAI routing group",
                order: group_order.get(&group.id).copied().unwrap_or(usize::MAX),
            },
        );
        menu.groups.insert(
            alias,
            Arc::new(Session {
                group,
                targets: plan,
                classifier,
                client,
                created: Instant::now(),
                bindings: Arc::default(),
            }),
        );
    }
    let hidden = menu
        .groups
        .values()
        .filter(|session| session.group.enabled && session.group.unlists)
        .flat_map(|session| {
            session
                .group
                .members
                .iter()
                .map(|member| direct_id(&member.provider_id, &member.model_id))
        })
        .collect::<HashSet<_>>();
    menu.models.retain(|id, model| {
        model.description != "VaryAI direct provider model" || !hidden.contains(id)
    });
    if menu
        .default_model
        .as_ref()
        .is_some_and(|alias| !menu.models.values().any(|model| &model.alias == alias))
    {
        menu.default_model = menu
            .models
            .values()
            .map(|model| &model.alias)
            .min()
            .cloned();
    }
    if menu.groups.is_empty() {
        return Err("intentRouting.providerUnavailable".into());
    }
    Ok(menu)
}

pub(crate) async fn refresh_active(service: &crate::services::ProxyService) {
    if service.refresh_global_routing().await.is_err() {
        log::warn!("Global routing catalog refresh failed; stale targets were revoked");
    }
    service.sync_shared_agent_catalogs().await;
    if service.intent_runtime.menu_active()
        && service.connect_codex_menu(false, false).await.is_err()
    {
        // Persistence succeeded; the service revoked stale targets under its switch lock.
        log::warn!("Codex model menu refresh failed; reconnect is required");
    }
}

pub(crate) async fn prepare_with_stand_ins(
    manager: Arc<CodexOAuthManager>,
    providers: Vec<Provider>,
    groups: Vec<RoutingGroup>,
    current: Option<String>,
    stand_ins: Vec<RoutingGroup>,
) -> Result<Menu, String> {
    let mut menu = prepare(manager, providers, groups, current).await?;
    // Stand-ins remain executable for old sessions, never offered as routing groups.
    // Reuse direct destinations so compatibility aliases do not consume picker slots.
    for group in stand_ins.into_iter().take(MAX_MENU_ENTRIES) {
        let alias = group.alias();
        if menu.groups.contains_key(&alias) || group.members.len() != 1 || group.validate().is_err()
        {
            continue;
        }
        let member = &group.members[0];
        let direct = format!(
            "vary-provider/{}",
            direct_id(&member.provider_id, &member.model_id)
        );
        if let Some(source) = menu.groups.get(&direct) {
            let mut session = source.as_ref().clone();
            session.group = group;
            session.bindings = Arc::default();
            menu.groups.insert(alias, Arc::new(session));
        }
    }
    Ok(menu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn provider(id: &str) -> Provider {
        Provider::with_id(
            id.into(),
            id.into(),
            json!({"auth":{"OPENAI_API_KEY":"synthetic"},"apiFormat":"openai_responses","config":"model='shared-model'\nmodel_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.example/v1'\nwire_api='responses'", "modelCatalog":{"models":[{"model":"shared-model","reasoningLevels":["low","medium","high"],"input_modalities":["text","image"]}]}}),
            None,
        )
    }
    #[tokio::test]
    #[serial_test::serial]
    async fn shared_catalog_keeps_account_models_and_routes_without_access_token_refresh(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::{
            agent_connections,
            codex_provider::{ModelMode, Settings},
            routing_upstreams::{RoutingUpstream, UpstreamSource},
        };
        let dir = tempfile::tempdir()?;
        struct Home(Option<std::ffi::OsString>);
        impl Drop for Home {
            fn drop(&mut self) {
                match &self.0 {
                    Some(home) => std::env::set_var("CC_SWITCH_TEST_HOME", home),
                    None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
                }
                let _ = crate::settings::reload_settings();
            }
        }
        let _home = Home(std::env::var_os("CC_SWITCH_TEST_HOME"));
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        crate::settings::reload_settings()?;
        struct Security(crate::proxy::security::SecurityRuntimeConfig);
        impl Drop for Security {
            fn drop(&mut self) {
                crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
            }
        }
        let _security = Security((*crate::proxy::security::get_security_runtime()).clone());
        crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        manager
            .add_test_account_with_user_identity("account", "synthetic-access", "user")
            .await?;
        let db = Arc::new(crate::database::Database::memory()?);
        db.set_intent_routing_auto_create(true)?;
        for (name, source, codex_settings) in [
            (
                "Subscription",
                UpstreamSource::Codex,
                Some(Settings {
                    accounts: vec!["account".into()],
                    mode: ModelMode::Picked,
                    models: (0..8).map(|index| json!({"model":format!("shared-{index}"),"displayName":format!("Shared {index}"),"contextWindow":500000})).collect(),
                    groups_only: false,
                }),
            ),
            (
                "Relay",
                UpstreamSource::Api {
                    base_url: "https://synthetic.invalid/v1".into(),
                    api_key: Some("synthetic-only".into()),
                    api_format: "openai_responses".into(),
                    models: (0..10).map(|index| json!({"model":format!("shared-{index}")})).collect(),
                    catalog: vec![],
                    mode: ModelMode::Picked,
                    groups_only: false,
                    auth_type: None,
                },
                None,
            ),
            (
                "Other vendor",
                UpstreamSource::Api {
                    base_url: "https://synthetic.invalid/v1".into(),
                    api_key: Some("synthetic-only".into()),
                    api_format: "openai_chat".into(),
                    models: (0..7).map(|index| json!({"model":format!("other-{index}")})).collect(),
                    catalog: vec![], mode: ModelMode::Picked, groups_only: false, auth_type: None,
                },
                None,
            ),
        ] {
            db.save_routing_upstream(RoutingUpstream {
                icon: None,
                schema_version: 1,
                id: String::new(),
                revision: 0,
                name: name.into(),
                enabled: true,
                has_api_key: false,
                preset_id: None,
                region: None,
                transport: None,
                codex_settings,
                source,
            })?;
        }
        let (providers, _) =
            super::super::catalog::prepare_providers(db.clone(), manager.clone(), false).await?;
        let catalog = db.intent_routing_catalog_with_providers(Some(providers.clone()))?;
        assert_eq!(catalog.groups.len(), 8);
        let menu = prepare(manager, providers, catalog.groups, None).await?;
        let official = menu
            .groups
            .values()
            .flat_map(|session| &session.targets)
            .find(|target| target.official)
            .ok_or("missing account target")?;
        assert!(
            !official.headers.contains_key(http::header::AUTHORIZATION),
            "catalog must not acquire access tokens"
        );
        let models = agent_connections::models(&menu);
        assert_eq!(models.len(), 33);
        let counts = models
            .iter()
            .fold(std::collections::BTreeMap::new(), |mut counts, model| {
                *counts.entry(model.group.as_str()).or_insert(0) += 1;
                counts
            });
        assert_eq!(
            counts,
            std::collections::BTreeMap::from([
                ("routing", 8),
                ("Subscription", 8),
                ("Relay", 10),
                ("Other vendor", 7)
            ])
        );
        assert!(models.iter().any(|model| model.group == "Subscription"
            && model.name == "Shared 0 · Subscription"
            && model.context_window == Some(500000)));
        assert!(models.iter().any(|model| model.group == "Relay"));
        let official_alias = models
            .iter()
            .find(|model| model.group == "Subscription")
            .ok_or("missing subscription model")?
            .id
            .clone();
        let payload = serde_json::to_value(&models)?;
        assert_eq!(
            payload
                .as_array()
                .ok_or("missing serialized catalog")?
                .len(),
            33
        );
        assert!(payload
            .as_array()
            .ok_or("missing serialized catalog")?
            .iter()
            .any(|row| row["group"] == "routing"));

        // A persisted login has no cached access token after an application restart.
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let (providers, _) =
            super::super::catalog::prepare_providers(db.clone(), manager.clone(), false).await?;
        let catalog = db.intent_routing_catalog_with_providers(Some(providers.clone()))?;
        let menu = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            prepare(manager.clone(), providers, catalog.groups, None),
        )
        .await??;
        assert_eq!(agent_connections::models(&menu).len(), 33);
        manager.remove_account("account").await?;
        let credentials = menu
            .groups
            .values()
            .flat_map(|session| &session.targets)
            .find_map(|target| target.credentials.as_ref())
            .ok_or("missing frozen identity")?;
        assert!(
            credentials.token().await.is_none(),
            "removed identities must remain unusable"
        );
        // Exercise the same authenticated global ingress used by connected Agents.
        let runtime = Arc::new(crate::proxy::intent::Runtime::default());
        runtime.set_global(menu, db.routing_generation())?;
        let key = db.create_gateway_key(crate::proxy::gateway::KeyPolicy {
            name: "Synthetic key".into(),
            models: vec![],
            all_models: true,
            token_limit: None,
            period: crate::proxy::gateway::Period::Never,
        })?;
        let server = crate::proxy::server::ProxyServer::new(
            crate::proxy::ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            db.clone(),
            None,
        )
        .with_intent_runtime(runtime);
        let info = server.start().await?;
        let client = reqwest::Client::builder().no_proxy().build()?;
        let root = format!("http://127.0.0.1:{}/gateway/v1", info.port);
        let listed: serde_json::Value = client
            .get(format!("{root}/models"))
            .bearer_auth(&key.secret)
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(
            listed["data"].as_array().ok_or("missing model list")?.len(),
            33
        );
        let response = client
            .post(format!("{root}/responses"))
            .bearer_auth(&key.secret)
            .json(&json!({"model":official_alias,"input":"synthetic","stream":true,"store":false}))
            .send()
            .await?;
        let status = response.status();
        let body: serde_json::Value = response.json().await?;
        assert!(
            body["error"]["code"] == "routing_account_changed_or_expired",
            "removed identity must fail closed"
        );
        assert_eq!(status, http::StatusCode::UNAUTHORIZED);
        server.stop().await?;
        let (providers, _) =
            super::super::catalog::prepare_providers(db.clone(), manager.clone(), false).await?;
        let catalog = db.intent_routing_catalog_with_providers(Some(providers.clone()))?;
        let menu = prepare(manager, providers, catalog.groups, None).await?;
        let models = agent_connections::models(&menu);
        assert_eq!(models.len(), 17);
        assert!(models
            .iter()
            .all(|model| matches!(model.group.as_str(), "Relay" | "Other vendor")));
        Ok(())
    }
    #[tokio::test]
    async fn discovered_groups_execute_and_disabled_discovery_keeps_only_an_unlisted_stand_in(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let sources = vec![provider("a"), provider("b")];
        let group = super::super::discover_groups(sources.clone()).remove(0);
        let alias = group.alias();
        let menu = prepare_with_stand_ins(
            manager.clone(),
            sources.clone(),
            vec![group.clone()],
            None,
            vec![],
        )
        .await?;
        assert_eq!(menu.groups[&alias].targets.len(), 2);
        assert!(menu.listed_aliases().any(|model| model == &alias));
        let mut stand_in = group;
        stand_in.members.truncate(1);
        stand_in.fallback_member_ids.clear();
        let menu = prepare_with_stand_ins(manager, sources, vec![], None, vec![stand_in]).await?;
        assert_eq!(menu.groups[&alias].targets.len(), 1);
        assert!(!menu.listed_aliases().any(|model| model == &alias));
        let direct = menu
            .stand_in_direct(&alias)
            .ok_or("missing direct stand-in")?;
        assert_eq!(menu.groups[direct].targets[0].provider_id, "a");
        assert_eq!(menu.groups[direct].targets[0].model, "shared-model");
        Ok(())
    }
    #[tokio::test]
    async fn provider_model_preferences_reach_every_shared_client(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut source = provider("a");
        source.settings_config["modelCatalog"]["models"][0]["modelPreferences"] =
            json!({"name":"My model","reasoningLevels":[],"images":false});
        let menu = prepare(manager, vec![source], vec![], None).await?;
        let models = crate::services::agent_connections::models(&menu);
        let model = models.first().ok_or("missing model")?;
        assert_eq!(model.name, "My model · a");
        assert!(!model.images);
        assert!(model.reasoning_levels.is_empty());
        let pi = crate::services::agent_connections::pi_model(model);
        assert_eq!(pi["name"], "My model · a");
        assert_eq!(pi["reasoning"], false);
        assert_eq!(pi["input"], json!(["text"]));
        let target = &menu
            .groups
            .values()
            .next()
            .ok_or("missing session")?
            .targets[0];
        let wire = super::super::effort::adapt_wire(
            br#"{"model":"exact-model","reasoning_effort":"high","messages":[]}"#,
            "openai_chat",
            &target.effort,
        )?;
        let body: serde_json::Value = serde_json::from_slice(&wire)?;
        assert!(body.get("reasoning_effort").is_none());
        Ok(())
    }
    #[tokio::test]
    async fn named_reasoning_levels_reach_shared_clients_without_disabling_images(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let source = provider("a");
        let mut group = direct_group(&source, "shared-model");
        group.id = "named-levels".into();
        group.enabled = true;
        group.levels = vec!["low".into(), "max".into()];
        let menu = prepare(manager, vec![source], vec![group], None).await?;
        let models = crate::services::agent_connections::models(&menu);
        let model = models
            .iter()
            .find(|model| model.id == "vary-auto/named-levels")
            .ok_or("missing group")?;
        assert_eq!(model.reasoning_levels, ["low", "max"]);
        assert!(model.images);
        let pi = crate::services::agent_connections::pi_model(model);
        assert_eq!(pi["reasoning"], true);
        assert_eq!(pi["input"], json!(["text", "image"]));
        assert_eq!(pi["thinkingLevelMap"]["max"], "max");
        assert!(pi["thinkingLevelMap"]["medium"].is_null());
        let opencode = crate::services::agent_connections::opencode_model(model);
        assert_eq!(opencode["variants"]["max"]["reasoningEffort"], "max");
        assert!(opencode["variants"].get("medium").is_none());
        Ok(())
    }
    #[tokio::test]
    async fn unlisted_models_still_execute_but_leave_every_published_menu(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let source = provider("a");
        let mut group = direct_group(&source, "shared-model");
        group.id = "shared".into();
        group.enabled = true;
        group.unlists = true;
        let hidden = direct_id("a", "shared-model");
        let alias = format!("vary-provider/{hidden}");
        let menu = prepare(
            manager.clone(),
            vec![source.clone()],
            vec![group.clone()],
            Some("a".into()),
        )
        .await?;
        assert!(menu.groups.contains_key(&alias));
        assert!(!menu.models.contains_key(&hidden));
        assert_eq!(menu.default_model.as_deref(), Some("vary-auto/shared"));
        assert_eq!(crate::services::agent_connections::models(&menu).len(), 1);
        let catalog = super::super::client::menu_catalog(&menu, "")?;
        assert_eq!(catalog["models"].as_array().ok_or("models")?.len(), 1);
        group.enabled = false;
        let menu = prepare(manager, vec![source], vec![group], None).await?;
        assert!(menu.models.contains_key(&hidden));
        Ok(())
    }
    #[tokio::test]
    async fn shared_catalog_publishes_every_provider_without_an_agent_selection() {
        let dir = tempfile::tempdir().expect("dir");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let menu = prepare(manager, vec![provider("a"), provider("b")], vec![], None)
            .await
            .expect("global direct menu");
        assert_eq!(menu.groups.len(), 2);
        assert!(menu.models.values().any(|m| m.label.ends_with(" · a")));
        assert!(menu.models.values().any(|m| m.label.ends_with(" · b")));
        assert!(menu.default_model.is_some());
    }

    #[tokio::test]
    async fn context_publication_preserves_direct_windows_and_uses_largest_group_member() {
        let dir = tempfile::tempdir().expect("dir");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut a = provider("a");
        a.settings_config["modelCatalog"]["models"] = json!([
            {"model":"shared-model","contextWindow":1000000,"maxContextWindow":1200000,
             "autoCompactTokenLimit":200000,"effectiveContextWindowPercent":95},
            {"model":"other-model","context_window":262144,"max_context_window":524288,
             "auto_compact_token_limit":180000,"effective_context_window_percent":90}
        ]);
        let mut b = provider("b");
        b.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(128000);
        let mut group = direct_group(&a, "shared-model");
        group.id = "mixed-windows".into();
        group.members.push(Member {
            effort: None,
            id: "small".into(),
            provider_id: "b".into(),
            model_id: "shared-model".into(),
            weight: 1,
            free: None,
        });
        group.fallback_member_ids.push("small".into());
        let mut menu = prepare(manager, vec![a, b], vec![group], Some("a".into()))
            .await
            .expect("menu");
        let projection = super::super::client::menu_catalog(&menu, "").expect("catalog");
        let entries = projection["models"].as_array().expect("models");
        let direct = entries
            .iter()
            .find(|e| e["display_name"] == "shared-model · a")
            .expect("direct");
        assert_eq!(direct["context_window"], 1000000);
        assert_eq!(direct["max_context_window"], 1200000);
        assert_eq!(direct["auto_compact_token_limit"], 200000);
        let other = entries
            .iter()
            .find(|e| e["display_name"] == "other-model · a")
            .expect("retargeted");
        assert_eq!(other["context_window"], 262144);
        assert_eq!(other["effective_context_window_percent"], 90);
        let routed = entries
            .iter()
            .find(|e| e["slug"] == "vary-auto/mixed-windows")
            .expect("group");
        assert_eq!(routed["context_window"], 1000000);
        assert!(routed.get("max_context_window").is_none());
        assert_eq!(routed["auto_compact_token_limit"], 200000);
        let session = Arc::make_mut(
            menu.groups
                .get_mut("vary-auto/mixed-windows")
                .expect("session"),
        );
        session.group.context_window = Some(-1);
        assert_eq!(
            super::super::context::Metadata::for_session(session).context_window,
            Some(128000)
        );
        session.group.context_window = Some(500000);
        assert_eq!(
            super::super::context::Metadata::for_session(session).context_window,
            Some(500000)
        );
    }

    #[tokio::test]
    async fn unknown_current_relay_does_not_borrow_standby_capacity(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let relay = provider("relay");
        let mut known = provider("known");
        known.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(500000);
        let mut group = direct_group(&relay, "shared-model");
        group.id = "mixed-unknown".into();
        group.members.push(Member {
            effort: None,
            id: "known".into(),
            provider_id: "known".into(),
            model_id: "shared-model".into(),
            weight: 1,
            free: None,
        });
        group.fallback_member_ids.push("known".into());
        let menu = prepare(
            manager,
            vec![relay, known],
            vec![group],
            Some("relay".into()),
        )
        .await?;
        let projection = super::super::client::menu_catalog(&menu, "")?;
        let entries = projection["models"].as_array().ok_or("missing models")?;
        assert_eq!(entries.len(), 3);
        for entry in entries {
            if entry["slug"] == "vary-auto/mixed-unknown"
                || entry["display_name"] == "shared-model · known"
            {
                assert_eq!(entry["context_window"], 500000);
            } else {
                assert!(entry.get("context_window").is_none());
            }
            assert_eq!(entry["effective_context_window_percent"], 95);
        }
        Ok(())
    }

    #[tokio::test]
    async fn context_only_refresh_preserves_static_destination_bindings() {
        let dir = tempfile::tempdir().expect("dir");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut source = provider("a");
        source.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(128000);
        let base_config = source.settings_config["config"]
            .as_str()
            .expect("config")
            .to_owned();
        source.settings_config["config"] = json!(format!(
            "model_auto_compact_token_limit=100000\n{base_config}"
        ));
        let previous = prepare(
            manager.clone(),
            vec![source.clone()],
            vec![],
            Some("a".into()),
        )
        .await
        .expect("previous");
        let alias = previous.default_model.clone().expect("alias");
        source.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(1000000);
        source.settings_config["config"] = json!(format!(
            "model_auto_compact_token_limit=200000\n{base_config}"
        ));
        let mut next = prepare(
            manager.clone(),
            vec![source.clone()],
            vec![],
            Some("a".into()),
        )
        .await
        .expect("next");
        next.preserve_from(&previous);
        assert!(Arc::ptr_eq(
            &next.groups[&alias].bindings,
            &previous.groups[&alias].bindings
        ));
        assert!(Arc::ptr_eq(
            &next.groups[&alias].targets[0].scheduling,
            &previous.groups[&alias].targets[0].scheduling
        ));
        let projection = super::super::client::menu_catalog(&next, "").expect("catalog");
        assert_eq!(projection["models"][0]["context_window"], 1000000);
        assert_eq!(projection["models"][0]["auto_compact_token_limit"], 200000);
        source.settings_config["auth"]["OPENAI_API_KEY"] = json!("synthetic-other-account");
        let mut changed = prepare(manager, vec![source], vec![], Some("a".into()))
            .await
            .expect("changed");
        changed.preserve_from(&next);
        assert!(!Arc::ptr_eq(
            &changed.groups[&alias].bindings,
            &next.groups[&alias].bindings
        ));
    }

    #[tokio::test]
    async fn context_manual_pick_publishes_only_the_selected_member() {
        let dir = tempfile::tempdir().expect("dir");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut a = provider("a");
        a.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(128000);
        let mut b = provider("b");
        b.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(1000000);
        let mut group = direct_group(&a, "shared-model");
        group.id = "manual-context".into();
        group.routing = super::super::RoutingStrategy::Manual;
        group.members.push(Member {
            effort: None,
            id: "large".into(),
            provider_id: "b".into(),
            model_id: "shared-model".into(),
            weight: 1,
            free: None,
        });
        group.manual_member_id = Some("large".into());
        let menu = prepare(manager, vec![a, b], vec![group], None)
            .await
            .expect("menu");
        let projection = super::super::client::menu_catalog(&menu, "").expect("catalog");
        assert_eq!(projection["models"][0]["context_window"], 1000000);
    }

    #[tokio::test]
    async fn codex_editor_menu_respects_account_pools_groups_only_and_usage(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::codex_provider::{self, ModelMode, Settings};
        let dir = tempfile::tempdir()?;
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        for id in ["one", "two"] {
            manager
                .add_test_account_with_user_identity(id, &format!("synthetic-{id}"), id)
                .await?;
        }
        let mut provider = Provider::with_id(
            "fixture".into(),
            "Synthetic".into(),
            json!({"modelCatalog":{"models":[{"model":"model-a"},{"model":"model-b"}]}}),
            None,
        );
        provider.category = Some("official".into());
        let mut policy = Settings {
            accounts: vec!["two".into(), "one".into()],
            mode: ModelMode::Picked,
            models: vec![json!({"model":"model-b"})],
            groups_only: false,
        };
        codex_provider::attach(&mut provider, &policy)?;
        codex_provider::filter_models(&mut provider);
        let mut group = direct_group(&provider, "model-b");
        group.id = "pool-fixture".into();
        let menu = prepare(
            manager.clone(),
            vec![provider.clone()],
            vec![group.clone()],
            None,
        )
        .await?;
        assert_eq!(menu.groups.len(), 2);
        assert!(menu.groups.values().all(|s| s.targets.len() == 2));
        assert!(!menu.models.contains_key(&direct_id("fixture", "model-a")));
        policy.groups_only = true;
        codex_provider::attach(&mut provider, &policy)?;
        let menu = prepare(
            manager.clone(),
            vec![provider.clone()],
            vec![group.clone()],
            None,
        )
        .await?;
        assert_eq!(menu.groups.len(), 1);
        assert_eq!(menu.groups[&group.alias()].targets.len(), 2);
        policy.mode = ModelMode::Usage;
        codex_provider::attach(&mut provider, &policy)?;
        assert!(
            matches!(prepare(manager, vec![provider], vec![group], None).await, Err(code) if code == "intentRouting.providerUnavailable")
        );
        Ok(())
    }

    #[tokio::test]
    async fn gemini_group_members_retarget_their_own_exact_url_model() {
        use crate::services::routing_upstreams::{
            resolve_provider, RoutingUpstream, UpstreamSource,
        };
        let dir = tempfile::tempdir().expect("dir");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let record = RoutingUpstream {
            icon: None,
            transport: None,
            schema_version: 1,
            preset_id: None,
            region: None,
            codex_settings: None,
            id: "gemini-fixture".into(),
            name: "Synthetic".into(),
            revision: 1,
            has_api_key: false,
            enabled: true,
            source: UpstreamSource::Api {
                catalog: vec![],
                mode: crate::services::codex_provider::ModelMode::Picked,
                groups_only: false,
                base_url: "https://synthetic.example/v1beta".into(),
                api_key: Some("synthetic-key".into()),
                api_format: "gemini_native".into(),
                models: vec![json!({"model":"model-a"}), json!({"model":"model-b"})],
                auth_type: None,
            },
        };
        let provider = resolve_provider(&record).expect("provider");
        let mut group = direct_group(&provider, "model-a");
        group.id = "gemini-models".into();
        group.members.push(Member {
            effort: None,
            id: "other".into(),
            provider_id: provider.id.clone(),
            model_id: "model-b".into(),
            weight: 1,
            free: None,
        });
        group.fallback_member_ids.push("other".into());
        let menu = prepare(manager, vec![provider], vec![group], None)
            .await
            .expect("menu");
        let targets = &menu.groups["vary-auto/gemini-models"].targets;
        assert_eq!(targets.len(), 2);
        assert!(targets[0].url.path().ends_with("/model-a:generateContent"));
        assert!(targets[1].url.path().ends_with("/model-b:generateContent"));
    }

    #[tokio::test]
    async fn manual_publishes_its_pick_when_unused_members_and_classifier_are_unavailable() {
        let dir = tempfile::tempdir().expect("dir");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let a = provider("a");
        let mut b = provider("b");
        b.settings_config["apiFormat"] = json!("unsupported");
        let group: RoutingGroup = serde_json::from_value(json!({
            "id":"manual-fixture","name":"Manual fixture","revision":0,"client":"codex","enabled":false,"routing":"manual",
            "members":[{"id":"b","providerId":"b","modelId":"unavailable"},{"id":"a","providerId":"a","modelId":"shared-model"}],
            "manualMemberId":"a","defaultMemberId":"b","fallbackMemberIds":["b"],"intentRecognitionEnabled":true,
            "classifierTarget":null,"classificationBudgetMs":2000,"rules":[]
        })).expect("group");
        let menu = prepare(manager, vec![a, b], vec![group], None)
            .await
            .expect("menu");
        let session = &menu.groups["vary-auto/manual-fixture"];
        assert_eq!(session.targets.len(), 1);
        assert_eq!(session.targets[0].provider_id, "a");
        assert!(session.classifier.is_none());
        assert_eq!(session.menu_targets().count(), 1);
    }
    #[tokio::test]
    async fn direct_models_and_groups_coexist_with_stable_isolated_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let a = provider("a");
        let b = provider("b");
        let mut group = direct_group(&a, "shared-model");
        group.id = "saved".into();
        group.name = "Saved".into();
        group.members.push(Member {
            effort: None,
            id: "other".into(),
            provider_id: b.id.clone(),
            model_id: "shared-model".into(),
            weight: 1,
            free: None,
        });
        group.fallback_member_ids.push("other".into());
        let menu = prepare(
            manager.clone(),
            vec![a.clone(), b.clone()],
            vec![group.clone()],
            Some("b".into()),
        )
        .await
        .unwrap();
        assert_eq!(menu.groups.len(), 3);
        assert!(menu.models.contains_key(&direct_id("a", "shared-model")));
        assert_eq!(menu.omitted_entries, 0);
        let alias = format!("vary-provider/{}", direct_id("b", "shared-model"));
        assert_eq!(menu.default_model, Some(alias.clone()));
        assert_eq!(menu.groups[&alias].targets.len(), 1);
        assert_eq!(menu.groups[&alias].targets[0].provider_id, "b");
        assert_eq!(menu.groups["vary-auto/saved"].targets.len(), 2);
        assert_eq!(
            menu.models[&direct_id("b", "shared-model")].label,
            "shared-model · b"
        );
        assert_eq!(menu.models["saved"].label, "Saved · routing group");
        assert!(!Arc::ptr_eq(
            &menu.groups[&alias].bindings,
            &menu.groups["vary-auto/saved"].bindings
        ));
        assert_ne!(
            direct_id("a", "shared-model"),
            direct_id("b", "shared-model")
        );
        assert_ne!(
            direct_id("a", "shared-model"),
            direct_id("a", "Shared-model")
        );
        let reordered = prepare(manager, vec![b, a], vec![group], Some("b".into()))
            .await
            .unwrap();
        assert_eq!(menu.default_model, reordered.default_model);
        let aliases = BTreeSet::from_iter(menu.groups.keys());
        assert_eq!(aliases, BTreeSet::from_iter(reordered.groups.keys()));
    }
    #[tokio::test]
    async fn shared_menu_keeps_all_catalogs_when_an_agent_changes_its_selection() {
        let dir = tempfile::tempdir().expect("fixture directory");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut current = provider("current");
        current.settings_config["modelCatalog"]["models"] = json!([{"model":"current-only"}]);
        let mut standby = provider("standby");
        standby.settings_config["modelCatalog"]["models"] = json!([{"model":"standby-only"}]);
        let mut group = direct_group(&standby, "vendor/Exact_Group.Model");
        group.id = "standby-group".into();
        let alias = group.alias();
        let menu = prepare(
            manager.clone(),
            vec![current.clone(), standby.clone()],
            vec![group.clone()],
            Some(current.id.clone()),
        )
        .await
        .expect("current menu");
        assert_eq!(menu.groups.len(), 5);
        assert_eq!(menu.omitted_entries, 0);
        assert!(menu
            .models
            .contains_key(&direct_id("current", "shared-model")));
        assert!(menu
            .models
            .contains_key(&direct_id("current", "current-only")));
        assert!(menu
            .models
            .contains_key(&direct_id("standby", "standby-only")));
        assert!(!menu
            .models
            .contains_key(&direct_id("standby", "vendor/Exact_Group.Model")));
        assert_eq!(menu.groups[&alias].targets[0].provider_id, "standby");
        assert_eq!(
            menu.groups[&alias].targets[0].model,
            "vendor/Exact_Group.Model"
        );
        let switched = prepare(
            manager.clone(),
            vec![current.clone(), standby.clone()],
            vec![group.clone()],
            Some(standby.id.clone()),
        )
        .await
        .expect("switched menu");
        assert!(switched
            .models
            .contains_key(&direct_id("current", "current-only")));
        assert!(switched
            .models
            .contains_key(&direct_id("standby", "standby-only")));
        assert!(!switched
            .models
            .contains_key(&direct_id("standby", "vendor/Exact_Group.Model")));
        assert_eq!(
            switched.groups[&alias].targets[0].model,
            "vendor/Exact_Group.Model"
        );
        for current_id in [None, Some("missing-provider".into())] {
            let groups_only = prepare(
                manager.clone(),
                vec![current.clone(), standby.clone()],
                vec![group.clone()],
                current_id,
            )
            .await
            .expect("groups only");
            assert_eq!(groups_only.groups.len(), 5);
            assert_eq!(groups_only.omitted_entries, 0);
            assert!(groups_only.groups.contains_key(&alias));
        }
    }

    #[tokio::test]
    async fn unavailable_provider_does_not_hide_other_providers() {
        let dir = tempfile::tempdir().expect("fixture directory");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut current = provider("current");
        current.settings_config["apiFormat"] = json!("unsupported");
        let standby = provider("standby");
        let mut group = direct_group(&standby, "shared-model");
        group.id = "available-group".into();
        let alias = group.alias();
        let menu = prepare(
            manager,
            vec![current, standby],
            vec![group],
            Some("current".into()),
        )
        .await
        .expect("working group");
        assert_eq!(menu.groups.len(), 2);
        assert_eq!(menu.omitted_entries, 1);
        assert!(menu.groups.contains_key(&alias));
    }

    #[tokio::test]
    async fn published_catalog_distinguishes_direct_models_from_single_member_groups() {
        let dir = tempfile::tempdir().expect("fixture directory");
        let oauth = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut provider = provider("fixture-provider");
        let exact_model = "vendor/Model_1.2-20261029";
        provider.settings_config["modelCatalog"]["models"] = json!([{"model":exact_model}]);
        let mut group = direct_group(&provider, exact_model);
        group.id = "saved-single-member".into();
        group.name = "Saved single member".into();
        let group_alias = group.alias();
        let direct_alias = format!("vary-provider/{}", direct_id(&provider.id, exact_model));
        let menu = prepare(
            oauth,
            vec![provider],
            vec![group],
            Some("fixture-provider".into()),
        )
        .await
        .expect("menu");
        assert_eq!(menu.groups[&direct_alias].targets[0].model, exact_model);
        assert_eq!(menu.groups[&group_alias].targets[0].model, exact_model);
        let runtime = Arc::new(crate::proxy::intent::Runtime::default());
        let config = dir.path().join("config.toml");
        let original = "# user configuration\nmodel='original-model'\nunrelated='retained'\n";
        std::fs::write(&config, original).expect("fixture config");
        let manager = super::super::client::Manager::new(
            config.clone(),
            dir.path().join("receipt.json"),
            runtime,
        );
        manager
            .activate_menu(Arc::new(menu), "http://127.0.0.1:1/intent/v1")
            .expect("publication");
        let installed: toml::Value = std::fs::read_to_string(&config)
            .expect("config")
            .parse()
            .expect("toml");
        let catalog: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                installed["model_catalog_json"]
                    .as_str()
                    .expect("catalog path"),
            )
            .expect("catalog"),
        )
        .expect("catalog json");
        let models = catalog["models"].as_array().expect("models");
        let direct = models
            .iter()
            .find(|m| m["slug"] == direct_alias)
            .expect("direct entry");
        assert_eq!(
            direct["display_name"],
            format!("{exact_model} · fixture-provider")
        );
        assert_eq!(direct["description"], "VaryAI direct provider model");
        let saved = models
            .iter()
            .find(|m| m["slug"] == group_alias)
            .expect("group entry");
        assert_eq!(saved["display_name"], "Saved single member · routing group");
        assert_eq!(saved["description"], "VaryAI routing group");
        manager.restore().expect("restore");
        assert_eq!(
            std::fs::read_to_string(config).expect("restored config"),
            original
        );
    }

    #[tokio::test]
    async fn large_provider_catalog_keeps_group_members_and_bounds_the_published_menu() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut a = provider("a");
        a.settings_config["modelCatalog"]["models"] = json!((0..300)
            .map(|i| json!({"model":format!("model-{i:03}")}))
            .collect::<Vec<_>>());
        let mut group = direct_group(&a, "model-299");
        group.id = "saved".into();
        let menu = prepare(manager, vec![a], vec![group], Some("a".into()))
            .await
            .unwrap();
        assert_eq!(menu.groups.len(), MAX_MENU_ENTRIES);
        assert_eq!(menu.omitted_entries, 46);
        assert_eq!(menu.groups["vary-auto/saved"].targets[0].model, "model-299");
    }
    #[tokio::test]
    async fn direct_only_catalog_exceeds_the_old_group_limit_and_restores_selected_alias() {
        let dir = tempfile::tempdir().unwrap();
        let oauth = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let mut p = provider("a");
        p.settings_config["modelCatalog"]["models"] = json!((0..40)
            .map(|i| json!({"model":format!("model-{i}")}))
            .collect::<Vec<_>>());
        let menu = prepare(oauth, vec![p], vec![], Some("a".into()))
            .await
            .unwrap();
        assert_eq!(menu.groups.len(), 41);
        let runtime = Arc::new(crate::proxy::intent::Runtime::default());
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "model='original'\n").unwrap();
        let manager = super::super::client::Manager::new(
            config.clone(),
            dir.path().join("receipt.json"),
            runtime,
        );
        let aliases = menu.groups.keys().cloned().collect::<BTreeSet<_>>();
        let default = menu.default_model.clone().unwrap();
        manager
            .activate_menu(Arc::new(menu), "http://127.0.0.1:1/intent/v1")
            .unwrap();
        let text = std::fs::read_to_string(&config).unwrap();
        let value = text.parse::<toml::Value>().unwrap();
        assert_eq!(value["model"].as_str(), Some(default.as_str()));
        let catalog: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(value["model_catalog_json"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        let actual = catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["slug"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, aliases);
        assert!(manager.status().unwrap().group_ids.is_empty());
        let chosen = aliases.iter().find(|alias| **alias != default).unwrap();
        let mut edited = text.parse::<toml_edit::DocumentMut>().unwrap();
        edited["model"] = toml_edit::value(chosen.clone());
        edited["unrelated"] = toml_edit::value("retained");
        std::fs::write(&config, edited.to_string()).unwrap();
        assert_eq!(
            std::fs::read_to_string(&config)
                .unwrap()
                .parse::<toml::Value>()
                .unwrap()["model"]
                .as_str(),
            Some(chosen.as_str())
        );
        manager.restore().unwrap();
        let restored = std::fs::read_to_string(config).unwrap();
        assert!(restored.contains("original"));
        assert!(!restored.contains("vary-provider/"));
        assert_eq!(
            restored.parse::<toml::Value>().unwrap()["unrelated"].as_str(),
            Some("retained")
        );
    }
    #[tokio::test]
    async fn unavailable_provider_does_not_remove_working_direct_models() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let a = provider("a");
        let mut bad = provider("bad");
        bad.settings_config["apiFormat"] = json!("unsupported");
        let group = direct_group(&bad, "shared-model");
        let menu = prepare(manager, vec![a, bad], vec![group], Some("a".into()))
            .await
            .unwrap();
        assert_eq!(menu.groups.len(), 1);
        assert_eq!(menu.omitted_entries, 2);
        assert!(menu
            .groups
            .keys()
            .all(|id| id.starts_with("vary-provider/")));
    }
}
