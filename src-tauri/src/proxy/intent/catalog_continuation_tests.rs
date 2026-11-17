//! Discovery metadata must not retire observed tool and conversation ownership.
use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture() -> Result<Menu, Box<dyn std::error::Error>> {
    let mut menu = global_tests::menu("http://synthetic.invalid/v1/responses");
    let session = Arc::make_mut(menu.groups.values_mut().next().ok_or("missing route")?);
    session.targets[0].upstream = Some(crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic upstream".into(),
        json!({"apiFormat":"openai_responses","base_url":"http://synthetic.invalid/v1","modelCatalog":{"models":[{"model":"exact-model","contextWindow":32000}]}}),
        None,
    ));
    Ok(menu)
}

fn refresh_catalog(menu: &mut Menu) -> TestResult {
    let session = Arc::make_mut(menu.groups.values_mut().next().ok_or("missing route")?);
    session.bindings = Arc::default();
    let provider = session.targets[0]
        .upstream
        .as_mut()
        .ok_or("missing provider")?;
    provider.settings_config["modelCatalog"] = json!({"models":[
        {"model":"exact-model","contextWindow":128000,"reasoningLevels":["low","high"]},
        {"model":"synthetic-unrelated-model"}
    ]});
    Ok(())
}

#[test]
fn catalog_continuation_refresh_preserves_destination_and_live_binding() -> TestResult {
    let original = fixture()?;
    let first = original.groups.values().next().ok_or("missing route")?;
    let mut refreshed = original.clone();
    refresh_catalog(&mut refreshed)?;
    refreshed.preserve_from(&original);
    let next = refreshed.groups.values().next().ok_or("missing route")?;
    assert!(
        first.targets[0].same_destination(&next.targets[0]),
        "discovery-only edits must not change the forwarding destination"
    );
    assert_eq!(
        first.targets[0].continuation_destination(),
        next.targets[0].continuation_destination(),
        "discovery-only edits must not invalidate persisted ownership"
    );
    assert!(Arc::ptr_eq(&first.bindings, &next.bindings));
    Ok(())
}

#[tokio::test]
async fn catalog_continuation_restart_restores_strict_tool_and_opaque_owners() -> TestResult {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("ownership.json");
    let key = dir.path().join("ownership.key");
    let store = continuation::Store::load(path.clone(), key.clone()).await?;
    let original = fixture()?;
    let first = original.groups.values().next().ok_or("missing route")?;
    let conversations = scopes::Conversations::default();
    let owner = conversations.acquire_durable(
        "agent:claude-desktop",
        first,
        Some("synthetic-session"),
        &json!({"input":"Synthetic opening"}),
        Some(store.clone()),
    )?;
    owner.update(|bindings| {
        bindings.bind_session("synthetic-session", 0);
        bindings.observe(
            &json!({"id":"synthetic-response","output":[
                {"type":"function_call","call_id":"synthetic-call"},
                {"type":"reasoning","id":"synthetic-reasoning","encrypted_content":"synthetic-seal"}
            ]}),
            0,
        );
    })?;
    store.flush().await?;
    drop(owner);
    drop(conversations);
    drop(store);
    let mut refreshed = original.clone();
    refresh_catalog(&mut refreshed)?;
    let next = refreshed.groups.values().next().ok_or("missing route")?;
    let body = json!({"previous_response_id":"synthetic-response","input":[
        {"type":"function_call_output","call_id":"synthetic-call","output":"Synthetic result"},
        {"type":"reasoning","id":"synthetic-reasoning","encrypted_content":"synthetic-seal"}
    ]});
    let restored = scopes::Conversations::default().acquire_durable(
        "agent:claude-desktop",
        next,
        Some("synthetic-session"),
        &body,
        Some(continuation::Store::load(path, key).await?),
    )?;
    restored.update(|bindings| {
        assert_eq!(
            bindings.resolve(&body, Some("synthetic-session")),
            Ok(Some(0))
        );
        assert!(bindings.known_opaque(&body));
    })?;
    Ok(())
}

#[test]
fn catalog_continuation_keeps_transport_credentials_protocol_and_nested_settings_exact(
) -> TestResult {
    let menu = fixture()?;
    let original = &menu.groups.values().next().ok_or("missing route")?.targets[0];
    let settings = original
        .upstream
        .as_ref()
        .ok_or("missing provider")?
        .settings_config
        .clone();
    for kind in [
        "provider",
        "model",
        "url",
        "header",
        "effort",
        "credential",
        "protocol",
        "metadata",
        "nested_catalog",
    ] {
        let mut changed = original.clone();
        match kind {
            "provider" => changed.provider_id = "synthetic-other-provider".into(),
            "model" => changed.model = "synthetic-other-model".into(),
            "url" => changed.url = "http://synthetic.invalid/other/responses".parse()?,
            "header" => {
                changed.headers.insert(
                    header::AUTHORIZATION,
                    http::HeaderValue::from_static("Bearer synthetic-other"),
                );
            }
            "effort" => changed.fixed_effort = Some("high".into()),
            _ => {
                let provider = changed.upstream.as_mut().ok_or("missing provider")?;
                match kind {
                    "credential" => {
                        provider.settings_config["auth"] =
                            json!({"OPENAI_API_KEY":"synthetic-other"})
                    }
                    "protocol" => provider.settings_config["apiFormat"] = json!("openai_chat"),
                    "metadata" => {
                        provider.meta = Some(crate::provider::ProviderMeta {
                            prompt_cache_key: Some("synthetic-other".into()),
                            ..Default::default()
                        })
                    }
                    _ => {
                        provider.settings_config["requestParameters"] =
                            json!({"modelCatalog":"synthetic-request-data"})
                    }
                }
            }
        }
        assert!(!original.same_destination(&changed), "{kind}");
        assert_ne!(
            original.continuation_destination(),
            changed.continuation_destination(),
            "{kind}"
        );
    }
    assert_eq!(
        original
            .upstream
            .as_ref()
            .ok_or("missing provider")?
            .settings_config,
        settings
    );
    let mut without_catalog = original.clone();
    without_catalog
        .upstream
        .as_mut()
        .ok_or("missing provider")?
        .settings_config
        .as_object_mut()
        .ok_or("missing settings")?
        .remove("modelCatalog");
    assert_eq!(
        without_catalog.continuation_destination(),
        without_catalog.legacy_continuation_destination()
    );
    assert!(original.same_destination(&without_catalog));
    assert_eq!(
        original.continuation_destination(),
        without_catalog.continuation_destination()
    );
    Ok(())
}

#[tokio::test]
async fn catalog_continuation_legacy_records_migrate_only_on_an_exact_configuration_match(
) -> TestResult {
    let dir = tempfile::tempdir()?;
    let store =
        continuation::Store::load(dir.path().join("state.json"), dir.path().join("state.key"))
            .await?;
    let menu = fixture()?;
    let session = menu.groups.values().next().ok_or("missing route")?;
    let legacy = session.targets[0]
        .legacy_continuation_destination()
        .ok_or("missing legacy destination")?;
    let stable = session.targets[0]
        .continuation_destination()
        .ok_or("missing stable destination")?;
    assert_ne!(legacy, stable);
    let body = json!({"input":[{"type":"function_call_output","call_id":"synthetic-call","output":"Synthetic result"}]});
    for agent in crate::services::agent_connections::APPS
        .iter()
        .copied()
        .chain(["trae", "trae-intl"])
    {
        let consumer = format!("agent:{agent}");
        let key = continuation::hash(
            "scope",
            &serde_json::to_string(&(&consumer, &session.group.id, session.group.alias()))?,
        );
        let id = continuation::hash("conversation", "synthetic-session");
        store.record(
            &key,
            &id,
            continuation::Snapshot {
                targets: vec![legacy.clone()],
                calls: std::collections::BTreeMap::from([(
                    continuation::hash("call", "synthetic-call"),
                    Some(legacy.clone()),
                )]),
                sessions: std::collections::BTreeMap::from([(
                    continuation::hash("session", "synthetic-session"),
                    Some(legacy.clone()),
                )]),
                ..Default::default()
            },
        );
        for change in ["url", "credential", "unknown_legacy_catalog"] {
            let mut changed = (**session).clone();
            changed.bindings = Arc::default();
            match change {
                "url" => {
                    changed.targets[0].url = "http://synthetic.invalid/other/responses".parse()?
                }
                "credential" => {
                    changed.targets[0].headers.insert(
                        header::AUTHORIZATION,
                        http::HeaderValue::from_static("Bearer synthetic-other"),
                    );
                }
                _ => {
                    changed.targets[0]
                        .upstream
                        .as_mut()
                        .ok_or("missing provider")?
                        .settings_config["modelCatalog"] =
                        json!({"models":[{"model":"synthetic-unknown-old-catalog"}]})
                }
            }
            let result = scopes::Conversations::default().acquire_durable(
                &consumer,
                &changed,
                Some("synthetic-session"),
                &body,
                Some(store.clone()),
            );
            assert_eq!(
                result.err(),
                Some("routing_unknown_continuation"),
                "{agent}: {change}"
            );
        }
        let owner = scopes::Conversations::default().acquire_durable(
            &consumer,
            session,
            Some("synthetic-session"),
            &body,
            Some(store.clone()),
        )?;
        owner.update(|bindings| {
            assert_eq!(
                bindings.resolve(&body, Some("synthetic-session")),
                Ok(Some(0))
            );
            let mut unknown = body.clone();
            unknown["input"][0]["call_id"] = json!("synthetic-unobserved-call");
            assert!(bindings
                .resolve(&unknown, Some("synthetic-session"))
                .is_err());
            // A normal observed answer rewrites the old record with stable keys.
            bindings.observe(&json!({"id":"synthetic-new-response"}), 0);
        })?;
        assert!(store
            .scope(&key)
            .iter()
            .all(|(_, snapshot)| snapshot.targets == vec![stable.clone()]));
    }
    Ok(())
}
