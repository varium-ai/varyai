use super::*;
use std::sync::Arc;

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
    let selected = menu.default_model.as_deref().ok_or("fixture default")?;
    let claude_path = crate::config::get_claude_settings_path();
    fs::create_dir_all(claude_path.parent().ok_or("parent")?)?;
    let original = json!({"env":{"CLAUDE_CODE_USE_BEDROCK":"1","ANTHROPIC_MODEL":"native"},"model":"native-choice"}).to_string();
    fs::write(&claude_path, &original)?;
    let receipt = build("claude", &menu, "http://127.0.0.1:1", selected)?;
    let installed = &receipt.files[0].installed;
    assert_eq!(installed["env"]["CLAUDE_CODE_USE_BEDROCK"], "0");
    assert!(installed["env"].get("ANTHROPIC_MODEL").is_none());
    assert_eq!(installed["model"], selected);
    assert_eq!(installed["modelPicker"]["options"][0]["model"], selected);
    receipt.files[0].write()?;
    receipt.files[0].restore()?;
    assert_eq!(fs::read_to_string(&claude_path)?, original);
    let json_path = crate::opencode_config::get_opencode_dir().join("opencode.json");
    let jsonc = json_path.with_extension("jsonc");
    fs::create_dir_all(jsonc.parent().ok_or("parent")?)?;
    fs::write(&jsonc, "{ // retained comment\n theme: 'synthetic',\n}")?;
    let receipt = build("opencode", &menu, "http://127.0.0.1:1", selected)?;
    assert_eq!(receipt.files[0].path, jsonc);
    receipt.files[0].write()?;
    assert!(fs::read_to_string(&jsonc)?.contains("// retained comment"));
    assert!(!json_path.exists());
    for app in ["kimi", "qwen", "zcode", "workbuddy"] {
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
