use super::*;
use std::sync::Arc;

#[tokio::test]
#[serial_test::serial]
async fn model_identities_preserve_legacy_choices_and_survive_display_label_changes(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    struct Home(Option<std::ffi::OsString>);
    impl Drop for Home {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }
    let _home = Home(std::env::var_os("CC_SWITCH_TEST_HOME"));
    std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
    crate::settings::reload_settings()?;
    crate::gemini_config::write_gemini_env_text_atomic(
        "GEMINI_CLI_CUSTOM_HEADERS=\"X-Fixture: a, b\"\n",
    )?;
    let provider = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        json!({"auth":{"OPENAI_API_KEY":"synthetic-only"},"apiFormat":"openai_chat","config":"model='model-a'\nmodel_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.invalid/v1'\nwire_api='chat'","modelCatalog":{"models":[{"model":"model-a","displayName":"Zeta Model A"},{"model":"model-b","displayName":"Zeta Model B"}]}}),
        None,
    );
    let menu = crate::services::intent_routing::menu::prepare(
        Arc::new(
            crate::proxy::providers::codex_oauth_auth::CodexOAuthManager::new(
                dir.path().to_owned(),
            ),
        ),
        vec![provider],
        vec![],
        None,
    )
    .await?;
    let catalog = models(&menu);
    assert_eq!(catalog.len(), 2);
    let first = &catalog[0].id;
    let second = &catalog[1].id;
    for app in [
        "gemini",
        "opencode",
        "openclaw",
        "qoder",
        "deepseek-harness",
        "t3code",
    ] {
        let mut old = build(app, &menu, "http://127.0.0.1:1", first)?;
        // Reproduce a configuration from before readable publication, with a
        // different selected model from the receipt's original starting model.
        old.model_map.clear();
        let legacy = if app == "gemini" {
            let wire = projections::wire_id(second);
            old.model_map.insert(wire.clone(), second.clone());
            wire
        } else {
            second.clone()
        };
        for file in &mut old.files {
            match app {
                "gemini" if matches!(file.format, Format::Env) => {
                    file.installed["GEMINI_MODEL"] = json!(legacy)
                }
                "gemini" => file.installed["model"]["name"] = json!(legacy),
                "opencode" => file.installed["model"] = json!(format!("varyai/{legacy}")),
                "openclaw" => {
                    file.installed["agents"]["defaults"]["model"]["primary"] =
                        json!(format!("varyai/{legacy}"))
                }
                "qoder" => file.installed["providers"]["varyai"]["model"] = json!(legacy),
                "deepseek-harness" if matches!(file.format, Format::Cordis) => {
                    file.installed["selection"][0]["config"]["model"] = json!(legacy)
                }
                "t3code" if file.path.ends_with("settings.json") => {
                    file.installed["defaultModelSelection"]["model"] = json!(legacy)
                }
                "t3code" if matches!(file.format, Format::Toml) => {
                    file.installed["model"] = json!(legacy)
                }
                _ => {}
            }
        }
        let manager = Manager {
            root: dir.path().join(format!("receipts-{app}")),
        };
        manager.install(old)?;
        let upgraded = manager
            .refresh(app, &menu, "http://127.0.0.1:1")?
            .ok_or("missing receipt")?;
        let name = model_wire(
            &upgraded.model_map,
            upgraded.visible_models.as_deref(),
            second,
        );
        assert_eq!(upgraded.model_map.get(name), Some(second));
        assert!(name.contains("Zeta"), "{app} unreadable model");
        if app == "t3code" {
            assert!(name.is_ascii() && !name.contains(char::is_whitespace));
        }
        if app == "gemini" {
            assert_eq!(upgraded.model_map.get(&legacy), Some(second));
        }
        for file in &upgraded.files {
            match app {
                "gemini" if matches!(file.format, Format::Env) => {
                    assert_eq!(file.installed["GEMINI_MODEL"], name)
                }
                "gemini" => {
                    assert_eq!(file.installed["model"]["name"], name);
                    assert_eq!(
                        file.installed["experimental"]["dynamicModelConfiguration"],
                        true
                    );
                    for model in &catalog {
                        let id = model_wire(
                            &upgraded.model_map,
                            upgraded.visible_models.as_deref(),
                            &model.id,
                        );
                        assert_eq!(
                            file.installed["modelConfigs"]["modelDefinitions"][id]["displayName"],
                            id
                        );
                        assert_eq!(
                            file.installed["modelConfigs"]["modelDefinitions"][id]["isVisible"],
                            true
                        );
                    }
                }
                "opencode" => assert_eq!(file.installed["model"], format!("varyai/{name}")),
                "openclaw" => assert_eq!(
                    file.installed["agents"]["defaults"]["model"]["primary"],
                    format!("varyai/{name}")
                ),
                "qoder" => assert_eq!(file.installed["providers"]["varyai"]["model"], name),
                "deepseek-harness" if matches!(file.format, Format::Cordis) => {
                    assert_eq!(file.installed["selection"][0]["config"]["model"], name)
                }
                "t3code" if file.path.ends_with("settings.json") => {
                    assert_eq!(file.installed["defaultModelSelection"]["model"], name)
                }
                "t3code" if matches!(file.format, Format::Toml) => {
                    assert_eq!(file.installed["model"], name)
                }
                _ => {}
            }
        }
        let repeated = manager
            .refresh(app, &menu, "http://127.0.0.1:1")?
            .ok_or("missing repeated receipt")?;
        assert_eq!(
            upgraded.model_map, repeated.model_map,
            "{app} unstable names"
        );
        for (before, after) in upgraded.files.iter().zip(&repeated.files) {
            assert!(
                before.installed == after.installed,
                "{app} unstable projection"
            );
        }
        if matches!(app, "gemini" | "t3code") {
            let mut renamed = menu.clone();
            for model in renamed.models.values_mut() {
                model.label = "Renamed · 我的模型".into();
            }
            let current = manager
                .refresh(app, &renamed, "http://127.0.0.1:1")?
                .ok_or("missing renamed receipt")?;
            let visible = current
                .visible_models
                .as_deref()
                .ok_or("missing visible names")?;
            assert_eq!(visible.len(), 2);
            assert_ne!(visible[0], visible[1]);
            if app == "gemini" {
                assert!(visible.iter().any(|wire| wire == "Renamed · 我的模型"));
                assert_eq!(current.model_map.get(name), Some(second));
                let env = current
                    .files
                    .iter()
                    .find(|file| matches!(file.format, Format::Env));
                assert_eq!(
                    env.ok_or("missing Gemini .env")?.installed["GEMINI_CLI_CUSTOM_HEADERS"],
                    "X-Fixture: a, b, User-Agent: GeminiCLI-VaryAI"
                );
            } else {
                assert_eq!(Some(visible), upgraded.visible_models.as_deref());
                assert!(visible
                    .iter()
                    .all(|wire| wire.is_ascii() && !wire.contains(char::is_whitespace)));
            }
            let selected = model_wire(&current.model_map, Some(visible), second);
            for file in &current.files {
                if matches!(file.format, Format::Toml) {
                    assert_eq!(file.installed["model"], selected);
                } else if matches!(file.format, Format::Env) {
                    assert_eq!(file.installed["GEMINI_MODEL"], selected);
                } else if file.path.ends_with("settings.json") {
                    let model = if app == "gemini" {
                        &file.installed["model"]["name"]
                    } else {
                        &file.installed["defaultModelSelection"]["model"]
                    };
                    assert_eq!(model, selected);
                }
            }
            renamed.models.retain(|_, model| &model.alias != second);
            renamed.groups.remove(second);
            renamed.default_model = Some(first.clone());
            let reduced = manager
                .refresh(app, &renamed, "http://127.0.0.1:1")?
                .ok_or("missing reduced receipt")?;
            let selected = model_wire(&reduced.model_map, reduced.visible_models.as_deref(), first);
            if app == "gemini" {
                let definitions = &reduced.files[1].installed["modelConfigs"]["modelDefinitions"];
                assert_eq!(definitions.as_object().map(|rows| rows.len()), Some(1));
                assert_eq!(definitions[selected]["displayName"], selected);
            }
            assert_eq!(serde_json::to_value(native::get(app)?)?["model"], selected);
        }
        manager.disconnect(app)?;
        for file in upgraded.files {
            assert_eq!(read(&file.path)?, file.original, "{app} restore");
        }
    }
    let mut gemini_group = catalog[0].clone();
    gemini_group.id = "vary-auto/synthetic-group".into();
    gemini_group.name = "中文路由组 · routing group".into();
    let mut receipt = Receipt {
        app: "gemini".into(),
        token: "synthetic-only".into(),
        model: gemini_group.id.clone(),
        model_map: BTreeMap::from([("routing-group".into(), gemini_group.id.clone())]),
        visible_models: None,
        files: vec![],
        suspended: false,
        applied_at_ms: None,
    };
    projections::build(
        "gemini",
        std::slice::from_ref(&gemini_group),
        "http://127.0.0.1:1/connections/gemini",
        "synthetic-only",
        &gemini_group.id,
        &mut receipt,
        None,
    )?;
    let wire = gemini_group.name.as_str();
    assert_eq!(receipt.files[0].installed["GEMINI_MODEL"], wire);
    assert_eq!(receipt.files[1].installed["model"]["name"], wire);
    assert_eq!(
        receipt.files[1].installed["modelConfigs"]["modelDefinitions"][wire]["displayName"],
        gemini_group.name
    );
    assert_eq!(receipt.model_map["routing-group"], gemini_group.id);
    Ok(())
}
