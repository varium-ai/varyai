use super::*;
use std::sync::Arc;

#[tokio::test]
#[serial_test::serial]
async fn catalog_refresh_retains_selected_alias_across_reload_and_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let provider = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        json!({
            "apiFormat":"openai_responses",
            "auth":{"OPENAI_API_KEY":"synthetic-only"},
            "config":"model_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.invalid/v1'\nwire_api='responses'",
            "modelCatalog":{"models":[{"model":"first"},{"model":"second"},{"model":"third"}]}
        }),
        None,
    );
    let menu = crate::services::intent_routing::menu::prepare(
        Arc::new(
            crate::proxy::providers::codex_oauth_auth::CodexOAuthManager::new(
                dir.path().join("accounts"),
            ),
        ),
        vec![provider],
        vec![],
        None,
    )
    .await?;
    let catalog = models(&menu);
    assert_eq!(catalog.len(), 3);
    let default = menu.default_model.as_deref().ok_or("fixture default")?;
    let selected = &catalog
        .iter()
        .find(|model| model.id != default)
        .ok_or("second model")?
        .id;
    assert!(!menu.models.contains_key(selected));
    for app in ["kimi", "qwen"] {
        let path = dir.path().join(format!("{app}.config"));
        let manager = Manager {
            root: dir.path().join(format!("{app}-receipts")),
        };
        crate::services::agent_clients::with_test_config(&path, || -> Result<(), String> {
            manager.install(manager.build(app, &menu, "http://127.0.0.1:1", default)?)?;
            manager
                .update(app, &menu, "http://127.0.0.1:1", Some(selected))?
                .ok_or(FAILED)?;
            let selected_files = fs::read(&path).map_err(|_| FAILED)?;
            for _ in 0..3 {
                let reopened = Manager {
                    root: manager.root.clone(),
                };
                let refreshed = reopened
                    .refresh(app, &menu, "http://127.0.0.1:1")?
                    .ok_or(FAILED)?;
                assert_eq!(&refreshed.model, selected, "{app} refresh");
                assert_eq!(
                    reopened.status(app)?.model.as_ref(),
                    Some(selected),
                    "{app} status"
                );
                assert!(
                    fs::read(&path).map_err(|_| FAILED)? == selected_files,
                    "{app} native selection"
                );
            }
            manager.suspend(app)?;
            let resumed = manager
                .refresh(app, &menu, "http://127.0.0.1:1")?
                .ok_or(FAILED)?;
            assert_eq!(&resumed.model, selected, "{app} resume");
            let format = resumed.files[0].format;
            let previous = std::str::from_utf8(&selected_files).map_err(|_| FAILED)?;
            assert!(
                format.parse(read(&path)?.as_deref())? == format.parse(Some(previous))?,
                "{app} resumed native configuration"
            );

            let mut removed = menu.clone();
            removed.models.retain(|_, model| &model.alias != selected);
            removed.groups.remove(selected);
            let fallback = manager
                .refresh(app, &removed, "http://127.0.0.1:1")?
                .ok_or(FAILED)?;
            assert_eq!(fallback.model, default, "{app} removed model fallback");
            manager
                .update(app, &menu, "http://127.0.0.1:1", Some(selected))?
                .ok_or(FAILED)?;
            removed.default_model = None;
            let fallback = manager
                .refresh(app, &removed, "http://127.0.0.1:1")?
                .ok_or(FAILED)?;
            assert!(models(&removed)
                .iter()
                .any(|model| model.id == fallback.model));
            manager.disconnect(app)
        })?;
    }
    Ok(())
}

#[test]
#[ignore = "requires installed OpenCode CLI"]
fn installed_opencode_accepts_generated_jsonc() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("fixture.json");
    fs::write(
        &path,
        "{ // retained\n \"$schema\":\"https://opencode.ai/config.json\",\n}",
    )?;
    let model = Model {
        api: "openai-completions",
        native_model: None,
        id: "synthetic".into(),
        name: "Synthetic".into(),
        context_window: Some(272000),
        output_tokens: None,
        cost: None,
        compact_at: None,
        group: "Synthetic".into(),
        reasoning_levels: vec!["low".into(), "high".into()],
        reasoning: true,
        default_reasoning_level: None,
        images: true,
        icon: None,
    };
    let file = FileEdit::prepare_format(path.clone(), Format::Jsonc, |value| {
        value["provider"] = json!({
            "fixture": {
                "npm":"@ai-sdk/openai-compatible",
                "options":{"baseURL":"http://127.0.0.1:1/v1","apiKey":"synthetic-only"},
                "models":{"synthetic":opencode_model(&model)}
            }
        });
        value["model"] = json!("fixture/synthetic");
        Ok(())
    })?;
    file.write()?;
    let output = std::process::Command::new("opencode")
        .args(["debug", "config"])
        .current_dir(dir.path())
        .env("XDG_CONFIG_HOME", dir.path().join("config"))
        .env("XDG_DATA_HOME", dir.path().join("data"))
        .env("XDG_CACHE_HOME", dir.path().join("cache"))
        .env("OPENCODE_CONFIG_DIR", dir.path().join("opencode"))
        .env("OPENCODE_CONFIG", &path)
        .env("OPENCODE_DISABLE_PROJECT_CONFIG", "true")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "true")
        .output()?;
    assert!(
        output.status.success(),
        "OpenCode rejected the synthetic projection"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn parity_native_projection_preserves_stable_ids_picker_and_jsonc(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    struct Home(Option<std::ffi::OsString>);
    impl Drop for Home {
        fn drop(&mut self) {
            match self.0.take() {
                Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }
    let _home = Home(std::env::var_os("CC_SWITCH_TEST_HOME"));
    std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
    crate::settings::reload_settings()?;
    let provider = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        json!({"auth":{"OPENAI_API_KEY":"synthetic-only"},"apiFormat":"openai_chat","config":"model='synthetic-model'\nmodel_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.invalid/v1'\nwire_api='chat'","modelCatalog":{"models":[{"model":"synthetic-model","name":"First label"}]}}),
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
    let fixture = menu
        .groups
        .values()
        .next()
        .and_then(|session| session.targets.first())
        .ok_or("fixture target")?;
    let mut chat = fixture.clone();
    chat.upstream
        .as_mut()
        .ok_or("fixture provider")?
        .meta
        .get_or_insert_with(Default::default)
        .api_format = Some("openai_chat".into());
    let mut responses = chat.clone();
    responses
        .upstream
        .as_mut()
        .ok_or("fixture provider")?
        .meta
        .as_mut()
        .ok_or("fixture metadata")?
        .api_format = Some("openai_responses".into());
    assert_eq!(
        preferred_api([&responses, &responses].into_iter()),
        "openai-responses"
    );
    assert_eq!(
        preferred_api([&chat, &responses].into_iter()),
        "openai-completions"
    );
    let mut official = chat.clone();
    official.official = true;
    assert_eq!(preferred_api([&official].into_iter()), "openai-responses");
    let selected = menu.default_model.as_deref().ok_or("fixture default")?;
    let claude_path = crate::config::get_claude_settings_path();
    fs::create_dir_all(claude_path.parent().ok_or("parent")?)?;
    let original = json!({"env":{"CLAUDE_CODE_USE_BEDROCK":"1","ANTHROPIC_MODEL":"native"},"model":"native-choice","availableModels":["sonnet"],"modelPicker":{"options":[{"model":"native-extra","label":"Native extra"}]}}).to_string();
    fs::write(&claude_path, &original)?;
    let receipt = build("claude", &menu, "http://127.0.0.1:1", selected)?;
    let installed = &receipt.files[0].installed;
    assert_eq!(installed["env"]["CLAUDE_CODE_USE_BEDROCK"], "0");
    assert!(installed["env"].get("ANTHROPIC_MODEL").is_none());
    for model in models(&menu) {
        assert!(
            installed["availableModels"]
                .as_array()
                .ok_or("allowlist")?
                .contains(&json!(model.name)),
            "shared model missing from the user allowlist"
        );
    }
    let name = &models(&menu)[0].name;
    assert_eq!(installed["model"], name.as_str());
    assert_eq!(
        installed["modelPicker"]["options"][1]["model"],
        name.as_str()
    );
    assert_eq!(
        receipt.model_map.get(name).map(String::as_str),
        Some(selected)
    );
    for role in ["OPUS", "SONNET", "HAIKU"] {
        assert_eq!(
            installed["env"][format!("ANTHROPIC_DEFAULT_{role}_MODEL")],
            name.as_str()
        );
        assert_eq!(
            installed["env"][format!("ANTHROPIC_DEFAULT_{role}_MODEL_NAME")],
            name.as_str()
        );
    }
    receipt.files[0].write()?;
    receipt.files[0].restore()?;
    assert_eq!(fs::read_to_string(&claude_path)?, original);
    let mut legacy = receipt.clone();
    legacy.model_map.clear();
    legacy.files[0] = FileEdit::prepare(claude_path.clone(), |value| {
        *value = installed.clone();
        value["model"] = json!(selected);
        value["modelPicker"]["options"][1]["model"] = json!(selected);
        for role in ["OPUS", "SONNET", "HAIKU"] {
            value["env"][format!("ANTHROPIC_DEFAULT_{role}_MODEL")] = json!(selected);
            value["env"]
                .as_object_mut()
                .ok_or("env")?
                .remove(&format!("ANTHROPIC_DEFAULT_{role}_MODEL_NAME"));
        }
        Ok(())
    })?;
    let manager = Manager {
        root: dir.path().join("receipts"),
    };
    manager.install(legacy)?;
    let upgraded = manager
        .refresh("claude", &menu, "http://127.0.0.1:1")?
        .ok_or("upgrade")?;
    assert_eq!(upgraded.files[0].installed, *installed);
    let refreshed = manager
        .refresh("claude", &menu, "http://127.0.0.1:1")?
        .ok_or("refresh")?;
    assert_eq!(refreshed.files[0].installed, *installed);
    manager.disconnect("claude")?;
    assert_eq!(fs::read_to_string(&claude_path)?, original);
    let json_path = crate::opencode_config::get_opencode_dir().join("opencode.json");
    fs::create_dir_all(json_path.parent().ok_or("parent")?)?;
    let original = r#"{"small_model":"mine/small","provider":{"mine":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://synthetic.invalid/v1","apiKey":"synthetic-only"},"models":{"small":{"name":"Small"}}}}}"#;
    fs::write(&json_path, original)?;
    let receipt = build("opencode", &menu, "http://127.0.0.1:1", selected)?;
    receipt.files[0].write()?;
    let installed: Value = serde_json::from_str(&fs::read_to_string(&json_path)?)?;
    assert_eq!(installed["model"], format!("varyai/{name}"));
    assert_eq!(installed["small_model"], "mine/small");
    assert_eq!(
        installed["provider"]["mine"]["models"]["small"]["name"],
        "Small"
    );
    receipt.files[0].restore()?;
    assert_eq!(fs::read_to_string(&json_path)?, original);
    let legacy_manager = Manager {
        root: dir.path().join("legacy-opencode-receipts"),
    };
    let mut legacy = build("opencode", &menu, "http://127.0.0.1:1", selected)?;
    legacy.files[0].format = Format::Json5;
    legacy_manager.install(legacy)?;
    assert!(serde_json::from_str::<Value>(&fs::read_to_string(&json_path)?).is_err());
    let migrated = legacy_manager.load("opencode")?.ok_or("legacy receipt")?;
    assert!(matches!(migrated.files[0].format, Format::Jsonc));
    legacy_manager.refresh("opencode", &menu, "http://127.0.0.1:1")?;
    serde_json::from_str::<Value>(&fs::read_to_string(&json_path)?)?;
    legacy_manager.disconnect("opencode")?;
    assert_eq!(fs::read_to_string(&json_path)?, original);
    fs::remove_file(&json_path)?;
    let jsonc = json_path.with_extension("jsonc");
    fs::create_dir_all(jsonc.parent().ok_or("parent")?)?;
    fs::write(
        &jsonc,
        "{ // retained comment\n \"theme\": \"synthetic\",\n}",
    )?;
    let receipt = build("opencode", &menu, "http://127.0.0.1:1", selected)?;
    assert_eq!(receipt.files[0].path, jsonc);
    receipt.files[0].write()?;
    assert!(fs::read_to_string(&jsonc)?.contains("// retained comment"));
    crate::services::agent_connections::jsonc::parse(&fs::read_to_string(&jsonc)?)?;
    assert!(!json_path.exists());
    receipt.files[0].restore()?;
    let manager = Manager {
        root: dir.path().join("opencode-receipts"),
    };
    manager.install(receipt)?;
    manager.refresh("opencode", &menu, "http://127.0.0.1:1")?;
    let live = fs::read_to_string(&jsonc)?;
    crate::services::agent_connections::jsonc::parse(&live)?;
    let edited = live.replace("\"theme\": \"synthetic\"", "\"theme\": \"edited\"");
    assert_ne!(live, edited);
    fs::write(&jsonc, edited)?;
    manager.disconnect("opencode")?;
    let restored = crate::services::agent_connections::jsonc::parse(&fs::read_to_string(&jsonc)?)?;
    assert_eq!(restored["theme"], "edited");
    assert!(restored.get("model").is_none());
    assert!(fs::read_to_string(&jsonc)?.contains("// retained comment"));
    for app in ["claude", "kimi", "qwen", "zcode", "workbuddy"] {
        let first = build(app, &menu, "http://127.0.0.1:1", selected)?;
        assert!(!first.model_map.is_empty(), "{app}");
        let mut renamed = menu.clone();
        for model in renamed.models.values_mut() {
            model.label = "Renamed".into();
        }
        let second = build_with_token(
            app,
            &renamed,
            "http://127.0.0.1:1",
            selected,
            first.token.clone(),
            &ModelVisibility::default(),
            Some(&first),
        )?;
        assert_eq!(first.model_map, second.model_map, "{app}");
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    assert!(matches!(
        build("claude-desktop", &menu, "http://127.0.0.1:1", selected),
        Err(error) if error == FAILED
    ));
    #[cfg(any(target_os = "macos", windows))]
    {
        let desktop_dir = if cfg!(windows) {
            dir.path().join("AppData/Local")
        } else {
            dir.path().join("Library/Application Support")
        };
        let normal_dir = if cfg!(windows) {
            dir.path().join("AppData/Roaming/Claude")
        } else {
            desktop_dir.join("Claude")
        };
        let normal = normal_dir.join("claude_desktop_config.json");
        let threep = desktop_dir.join("Claude-3p/claude_desktop_config.json");
        fs::create_dir_all(normal.parent().ok_or("parent")?)?;
        fs::write(&normal,json!({"mcpServers":{"edited":{"command":"synthetic-original"},"removed":{"command":"synthetic-original"}}}).to_string())?;
        fs::write(
            normal.with_file_name("config.json"),
            json!({"locale":"ja-JP"}).to_string(),
        )?;
        let manager = Manager {
            root: dir.path().join("desktop-receipts"),
        };
        manager.install(manager.build(
            "claude-desktop",
            &menu,
            "http://127.0.0.1:1",
            selected,
        )?)?;
        let mut live: Value = serde_json::from_str(&fs::read_to_string(&threep)?)?;
        assert_eq!(
            live["mcpServers"]["edited"]["command"],
            "synthetic-original"
        );
        assert_eq!(
            live["mcpServers"]["removed"]["command"],
            "synthetic-original"
        );
        let locale: Value =
            serde_json::from_str(&fs::read_to_string(threep.with_file_name("config.json"))?)?;
        assert_eq!(locale["locale"], "ja-JP");
        live["mcpServers"]["edited"]["command"] = json!("synthetic-user-change");
        live["mcpServers"]
            .as_object_mut()
            .ok_or("servers")?
            .remove("removed");
        fs::write(&threep, live.to_string())?;
        fs::remove_file(threep.with_file_name("config.json"))?;
        fs::remove_file(normal.with_file_name("config.json"))?;
        manager.refresh("claude-desktop", &menu, "http://127.0.0.1:1")?;
        let refreshed: Value = serde_json::from_str(&fs::read_to_string(&threep)?)?;
        assert!(refreshed["mcpServers"].get("removed").is_none());
        assert!(!threep.with_file_name("config.json").exists());
        manager.disconnect("claude-desktop")?;
        let restored: Value = serde_json::from_str(&fs::read_to_string(&threep)?)?;
        assert_eq!(
            restored["mcpServers"]["edited"]["command"],
            "synthetic-user-change"
        );
        assert!(restored["mcpServers"].get("removed").is_none());
        let profile = desktop_dir
            .join("Claude-3p/configLibrary")
            .join(format!("{}.json", crate::claude_desktop_config::PROFILE_ID));
        fs::write(
            &profile,
            json!({"toolSearchEnabled":false,"customSetting":true}).to_string(),
        )?;
        manager.install(manager.build(
            "claude-desktop",
            &menu,
            "http://127.0.0.1:1",
            selected,
        )?)?;
        let mut value: Value = serde_json::from_str(&fs::read_to_string(&profile)?)?;
        assert_eq!(value["toolSearchEnabled"], false);
        value
            .as_object_mut()
            .ok_or("profile")?
            .remove("toolSearchEnabled");
        fs::write(&profile, value.to_string())?;
        manager.refresh("claude-desktop", &menu, "http://127.0.0.1:1")?;
        manager.disconnect("claude-desktop")?;
        let restored: Value = serde_json::from_str(&fs::read_to_string(&profile)?)?;
        assert!(restored.get("toolSearchEnabled").is_none());
        manager.install(manager.build(
            "claude-desktop",
            &menu,
            "http://127.0.0.1:1",
            selected,
        )?)?;
        let reconnected: Value = serde_json::from_str(&fs::read_to_string(&profile)?)?;
        assert!(reconnected.get("toolSearchEnabled").is_none());
        manager.disconnect("claude-desktop")?;
    }
    Ok(())
}

#[tokio::test]
async fn qoder_refresh_ages_only_effective_startup_changes(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let provider = crate::provider::Provider::with_id(
        "freshness-fixture".into(),
        "Synthetic freshness".into(),
        json!({"auth":{"OPENAI_API_KEY":"synthetic-only"},"apiFormat":"openai_chat",
            "config":"model_provider='fixture'\n[model_providers.fixture]\nbase_url='https://synthetic.invalid/v1'\nwire_api='chat'",
            "modelCatalog":{"models":[{"model":"alpha"},{"model":"beta"}]}}),
        None,
    );
    let menu = crate::services::intent_routing::menu::prepare(
        Arc::new(
            crate::proxy::providers::codex_oauth_auth::CodexOAuthManager::new(
                dir.path().join("accounts"),
            ),
        ),
        vec![provider],
        vec![],
        None,
    )
    .await?;
    let selected = menu.default_model.as_deref().ok_or("default")?;
    let manager = Manager {
        root: dir.path().join("receipts"),
    };
    crate::qoder_config::with_test_dir(&dir.path().join("qoder"), || -> Result<(), String> {
        let mut receipt = manager.build("qoder", &menu, "http://127.0.0.1:1", selected)?;
        receipt.applied_at_ms = Some(1000);
        manager.install(receipt)?;
        let plan_dir = manager.root.join("model-control").join("qoder");
        atomic_write_private(&plan_dir.join("consent.json"), br#"{"version":1}"#)
            .map_err(|_| FAILED)?;
        let old = manager.load("qoder")?.ok_or(FAILED)?;
        manager.sync_model_control(&old, false)?;
        let plan: Value = serde_json::from_str(&read(&plan_dir.join("plan.json"))?.ok_or(FAILED)?)
            .map_err(|_| FAILED)?;
        atomic_write_private(
            &plan_dir.join("status.json"),
            &serde_json::to_vec(&json!({"revision":plan["revision"],"ready":true}))
                .map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED)?;
        assert!(!manager.model_control_status("qoder")?.pending);

        let mut renamed = menu.clone();
        for row in renamed.models.values_mut() {
            row.label = format!("New label {}", row.label);
        }
        let refreshed = manager
            .refresh("qoder", &renamed, "http://127.0.0.1:1")?
            .ok_or(FAILED)?;
        assert_eq!(refreshed.applied_at_ms, Some(1000));
        assert!(!manager.model_control_status("qoder")?.pending);
        let reopened = Manager {
            root: manager.root.clone(),
        };
        assert_eq!(
            reopened
                .refresh("qoder", &renamed, "http://127.0.0.1:1")?
                .ok_or(FAILED)?
                .applied_at_ms,
            Some(1000)
        );

        // Upstream-only routing changes do not alter the client-facing configuration.
        let mut rerouted = renamed.clone();
        for session in rerouted.groups.values_mut() {
            for target in &mut Arc::make_mut(session).targets {
                if let Some(provider) = target.upstream.as_mut() {
                    provider.settings_config["auth"]["OPENAI_API_KEY"] =
                        json!("synthetic-upstream-replacement");
                }
            }
        }
        assert_eq!(
            manager
                .refresh("qoder", &rerouted, "http://127.0.0.1:1")?
                .ok_or(FAILED)?
                .applied_at_ms,
            Some(1000)
        );
        assert!(!manager.model_control_status("qoder")?.pending);

        // Removing a visible model changes the catalog the client reads at startup.
        let mut fewer = renamed.clone();
        fewer.models.retain(|_, row| row.alias == selected);
        fewer.groups.retain(|alias, _| alias == selected);
        let changed = manager
            .refresh("qoder", &fewer, "http://127.0.0.1:1")?
            .ok_or(FAILED)?;
        assert!(changed.applied_at_ms.is_some_and(|time| time > 1000));
        assert!(manager.model_control_status("qoder")?.pending);
        let changed_time = changed.applied_at_ms;
        assert_eq!(
            manager
                .refresh("qoder", &fewer, "http://127.0.0.1:1")?
                .ok_or(FAILED)?
                .applied_at_ms,
            changed_time
        );
        Ok(())
    })?;
    Ok(())
}
