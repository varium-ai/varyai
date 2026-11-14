use super::*;
use std::sync::Arc;

fn contains_label(value: &Value, label: &str) -> bool {
    match value {
        Value::String(value) => value == label,
        Value::Array(values) => values.iter().any(|value| contains_label(value, label)),
        Value::Object(values) => values.values().any(|value| contains_label(value, label)),
        _ => false,
    }
}

#[tokio::test]
#[serial_test::serial]
async fn source_suffix_reaches_every_agent_projection_and_existing_connections(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Environment {
        fn drop(&mut self) {
            for (key, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            let _ = crate::settings::reload_settings();
        }
    }
    let _environment = Environment(
        ["CC_SWITCH_TEST_HOME", "HERMES_HOME", "LOCALAPPDATA"]
            .into_iter()
            .map(|key| {
                let previous = std::env::var_os(key);
                std::env::set_var(key, root.path().join(key));
                (key, previous)
            })
            .collect(),
    );
    crate::settings::reload_settings()?;
    let providers = [("source-a", "Relay A", "Model A"), ("source-b", "Relay B", "Model B")]
        .into_iter()
        .map(|(id, name, model)| {
            crate::provider::Provider::with_id(
                id.into(),
                name.into(),
                json!({
                    "auth":{"OPENAI_API_KEY":"synthetic-source-key"},
                    "apiFormat":"openai_responses",
                    "config":"model_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.invalid/v1'\nwire_api='responses'",
                    "modelCatalog":{"models":[{"model":model}]}
                }),
                None,
            )
        })
        .collect();
    let menu = crate::services::intent_routing::menu::prepare(
        Arc::new(
            crate::proxy::providers::codex_oauth_auth::CodexOAuthManager::new(
                root.path().join("accounts"),
            ),
        ),
        providers,
        vec![],
        None,
    )
    .await?;
    let catalog = models(&menu);
    for row in &catalog {
        assert_eq!(
            row.name,
            format!(
                "{} · {}",
                row.native_model.as_deref().ok_or("fixture model")?,
                row.group
            )
        );
    }
    let selected = &catalog.first().ok_or("fixture catalog")?.id;
    let codex = crate::services::intent_routing::client::menu_catalog(&menu, "")?;
    for row in &catalog {
        assert!(
            contains_label(&codex, &row.name),
            "Codex source label missing"
        );
    }
    for app in ["trae", "trae-intl"] {
        let config = projections::client_config(
            &catalog,
            &format!("http://127.0.0.1:1/connections/{app}"),
            "synthetic-source-key",
            selected,
        );
        for row in &catalog {
            assert!(
                contains_label(&config, &row.name),
                "{app} source label missing"
            );
        }
    }
    let mut legacy = menu.clone();
    for entry in legacy.models.values_mut() {
        entry.label = legacy.groups[&entry.alias].targets[0].model.clone();
    }
    for app in APPS.iter().copied().filter(|app| *app != "codex") {
        if app == "claude-desktop" && !cfg!(any(target_os = "macos", target_os = "windows")) {
            continue;
        }
        let config = root.path().join(app).join("config");
        crate::services::agent_clients::with_test_config(&config, || -> Result<(), String> {
            let initial = build(app, &menu, "http://127.0.0.1:1", selected)?;
            let previous = build(app, &legacy, "http://127.0.0.1:1", selected)?;
            let refreshed = build_with_token(
                app,
                &menu,
                "http://127.0.0.1:1",
                selected,
                previous.token.clone(),
                &ModelVisibility::default(),
                Some(&previous),
            )?;
            for receipt in [&initial, &refreshed] {
                for row in &catalog {
                    // Gemini reads display names from the shared discovery endpoint.
                    if app != "gemini" {
                        assert!(
                            receipt
                                .files
                                .iter()
                                .any(|file| contains_label(&file.installed, &row.name)),
                            "{app} source label missing"
                        );
                    }
                }
            }
            for (wire, alias) in &previous.model_map {
                assert_eq!(
                    refreshed.model_map.get(wire),
                    Some(alias),
                    "{app} lost a historical request alias"
                );
            }
            if app == "deepseek-harness" {
                let file = refreshed
                    .files
                    .iter()
                    .find(|file| matches!(file.format, Format::Cordis))
                    .ok_or(FAILED)?;
                let encoded = file
                    .format
                    .encode(file.original.as_deref(), &file.installed)?;
                let actual = crate::services::deepseek_harness::parse(&encoded)?;
                for row in &catalog {
                    assert!(
                        contains_label(&actual, &row.name),
                        "Harness serialized source label missing"
                    );
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

#[test]
fn source_suffix_refreshes_display_name_without_replacing_request_identity() {
    let model = Model {
        api: "openai-completions",
        native_model: None,
        output_tokens: None,
        cost: None,
        id: "vary-provider/fixture".into(),
        name: "Model A · Relay A".into(),
        context_window: Some(128000),
        compact_at: None,
        group: "Relay A".into(),
        reasoning_levels: vec!["high".into()],
        reasoning: true,
        default_reasoning_level: Some("high".into()),
        images: true,
        icon: None,
    };
    let mut aliases = BTreeMap::from([("Model A".into(), model.id.clone())]);
    let (projected, selected) =
        projections::named_catalog(std::slice::from_ref(&model), &model.id, &mut aliases);
    assert_eq!(selected, "Model A");
    assert_eq!(projected[0].id, "Model A");
    assert_eq!(projected[0].name, "Model A · Relay A");
    assert_eq!(projected[0].context_window, model.context_window);
    assert_eq!(projected[0].reasoning_levels, model.reasoning_levels);
    assert_eq!(aliases["Model A"], model.id);

    let mut second = model.clone();
    second.id = "vary-provider/second".into();
    aliases.insert("Model B".into(), second.id.clone());
    let (duplicates, _) =
        projections::named_catalog(&[model, second], "vary-provider/fixture", &mut aliases);
    assert_eq!(duplicates[0].id, "Model A");
    assert_eq!(duplicates[1].id, "Model B");
    assert_eq!(duplicates[0].name, "Model A · Relay A");
    assert_eq!(duplicates[1].name, "Model A · Relay A (2)");
}
