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
    provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(|v| v.as_array())
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["model"].as_str().is_some_and(|id| id.trim() == model))
        })
        .map(super::effort::Capability::parse)
        .unwrap_or_default()
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
    let mut menu = Menu {
        token: format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ),
        groups: HashMap::new(),
        models: HashMap::new(),
        default_model: None,
        omitted_entries: 0,
    };
    let mut targets: HashMap<(String, String), Target> = HashMap::new();
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
            .filter(|model| direct_group(provider, model).validate().is_ok())
            .collect::<Vec<_>>();
        models.sort_by_key(|model| (Some(model) != default.as_ref(), model.clone()));
        let Some(first) = models.first() else {
            continue;
        };
        let prepared = execution::prepare_with_providers(
            manager.clone(),
            direct_group(provider, first),
            vec![provider.clone()],
        )
        .await;
        let Ok(prepared) = prepared else {
            menu.omitted_entries += models.len();
            continue;
        };
        let Some(mut template) = prepared.targets.first().cloned() else {
            continue;
        };
        if let Some(previous) = accounts.iter().find(|t| t.same_account(&template)) {
            template.scheduling = Arc::new(previous.scheduling.for_model());
        }
        accounts.push(template.clone());
        http_client.get_or_insert_with(|| prepared.client.clone());
        for model in models {
            let key = (provider.id.clone(), model.clone());
            let publish = menu.groups.len() < direct_limit;
            if !publish && !required.contains(&key) {
                menu.omitted_entries += 1;
                continue;
            }
            let mut target = template.clone();
            target.model = model.clone();
            target.effort = declared_capability(provider, &model);
            target.scheduling = Arc::new(template.scheduling.for_model());
            if required.contains(&key) {
                targets.insert(key, target.clone());
            }
            if !publish {
                menu.omitted_entries += 1;
                continue;
            }
            let group = direct_group(provider, &model);
            let alias = format!("vary-provider/{}", group.id);
            let label = format!("{} · {}", model, provider.name);
            if menu.default_model.is_none() && Some(&provider.id) == current_provider.as_ref() {
                menu.default_model = Some(alias.clone());
            }
            menu.models.insert(
                group.id.clone(),
                MenuModel {
                    alias: alias.clone(),
                    label,
                },
            );
            menu.groups.insert(
                alias,
                Arc::new(Session {
                    group,
                    targets: vec![target],
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
        let plan = available.into_iter().flatten().collect();
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
    if menu.groups.is_empty() {
        return Err("intentRouting.providerUnavailable".into());
    }
    Ok(menu)
}

pub(crate) async fn refresh_active(service: &crate::services::ProxyService) {
    if service.intent_runtime.menu_active()
        && service.connect_codex_menu(false, false).await.is_err()
    {
        // Persistence succeeded; the service revoked stale targets under its switch lock.
        log::warn!("Codex model menu refresh failed; reconnect is required");
    }
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
    async fn manual_publishes_its_pick_when_unused_members_and_classifier_are_unavailable() {
        let dir = tempfile::tempdir().expect("dir");
        let manager = Arc::new(CodexOAuthManager::new(dir.path().to_owned()));
        let a = provider("a");
        let mut b = provider("b");
        b.settings_config["apiFormat"] = json!("openai_chat");
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
        bad.settings_config["apiFormat"] = json!("openai_chat");
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
