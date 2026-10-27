//! Cross-agent profiles use synthetic providers and the isolated test HOME.
use cc_switch_lib::ProfileBundleService;
use cc_switch_lib::{AppType, ProfileService, Provider, ProviderService};
use serde_json::json;

#[path = "support.rs"]
mod support;

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    // Every case resets the test HOME and creates a fresh AppState. Keep the
    // original failure visible while allowing independent cases to run.
    let mutex = support::test_mutex();
    mutex.lock().unwrap_or_else(|poisoned| {
        mutex.clear_poison();
        poisoned.into_inner()
    })
}

fn seed() -> cc_switch_lib::AppState {
    support::reset_test_fs();
    let home = std::path::PathBuf::from(std::env::var("CC_SWITCH_TEST_HOME").unwrap());
    for directory in [
        ".pi",
        ".hermes",
        ".workbuddy-ai",
        ".qoder-cn",
        ".kimi-code",
        ".qwen",
        ".zcode",
        "Library",
        "AppData",
    ] {
        let path = home.join(directory);
        if path.exists() {
            std::fs::remove_dir_all(path).unwrap();
        }
    }
    let state = support::create_test_state().expect("test state");
    for (app, id, model) in [
        (AppType::Claude, "claude-a", "model-a"),
        (AppType::Claude, "claude-b", "model-b"),
        (AppType::Codex, "codex-a", "model-a"),
        (AppType::Codex, "codex-b", "model-b"),
    ] {
        let config = if app == AppType::Claude {
            json!({"env":{"ANTHROPIC_AUTH_TOKEN":"synthetic-key", "ANTHROPIC_BASE_URL":"https://example.test", "ANTHROPIC_MODEL":model}})
        } else {
            json!({"auth":{"OPENAI_API_KEY":"synthetic-key"}, "config":format!("model = \"{model}\"\nmodel_provider = \"fixture\"\n[model_providers.fixture]\nname = \"Fixture\"\nbase_url = \"https://example.test/v1\"\nwire_api = \"responses\"\n")})
        };
        state
            .db
            .save_provider(
                app.as_str(),
                &Provider::with_id(id.into(), id.into(), config, None),
            )
            .expect("seed provider");
    }
    ProviderService::switch(&state, AppType::Claude, "claude-a").expect("select Claude");
    ProviderService::switch(&state, AppType::Codex, "codex-a").expect("select Codex");
    state
}

#[test]
fn selected_bundle_is_independent_of_uncaptured_agents_and_restored_by_undo() {
    let _guard = test_guard();
    let state = seed();
    state
        .db
        .set_current_profile_id("kimi", Some("other-profile"))
        .unwrap();
    let first = ProfileBundleService::capture(
        &state,
        "First bundle",
        &["claude".into(), "codex".into()],
        None,
    )
    .unwrap();
    let second = ProfileBundleService::capture(
        &state,
        "Second bundle",
        &["claude".into(), "codex".into()],
        None,
    )
    .unwrap();
    for profile in [&first, &second] {
        let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
        assert!(
            ProfileBundleService::apply(&state, &profile.id, &preview.revision)
                .unwrap()
                .success
        );
        assert_eq!(
            state
                .db
                .get_current_profile_id("bundle")
                .unwrap()
                .as_deref(),
            Some(profile.id.as_str())
        );
        assert_eq!(
            state.db.get_current_profile_id("kimi").unwrap().as_deref(),
            Some("other-profile")
        );
    }
    let undo = ProfileBundleService::last_undo(&state).unwrap().unwrap();
    ProfileBundleService::undo(&state, &undo.id).unwrap();
    assert_eq!(
        state
            .db
            .get_current_profile_id("bundle")
            .unwrap()
            .as_deref(),
        Some(first.id.as_str())
    );
    ProfileService::delete(&state, &first.id).unwrap();
    assert_eq!(state.db.get_current_profile_id("bundle").unwrap(), None);
    let preview = ProfileBundleService::preview(&state, &second.id).unwrap();
    assert!(
        ProfileBundleService::apply(&state, &second.id, &preview.revision)
            .unwrap()
            .success
    );
    let path = cc_switch_lib::get_claude_settings_path();
    let contents = std::fs::read(&path).unwrap();
    ProfileBundleService::clear_current(&state).unwrap();
    assert_eq!(state.db.get_current_profile_id("bundle").unwrap(), None);
    assert_eq!(state.db.get_current_profile_id("claude").unwrap(), None);
    assert_eq!(state.db.get_current_profile_id("codex").unwrap(), None);
    assert_eq!(
        state.db.get_current_profile_id("kimi").unwrap().as_deref(),
        Some("other-profile")
    );
    assert_eq!(std::fs::read(&path).unwrap(), contents);
}

#[test]
fn unchanged_provider_metadata_does_not_invalidate_profile_preview() {
    let _guard = test_guard();
    let state = seed();
    let mut provider = state
        .db
        .get_provider_by_id("claude-a", "claude")
        .unwrap()
        .unwrap();
    let routes = (0..8)
        .map(|index| {
            (
                format!("route-{index}"),
                json!({"model": format!("model-{index}")}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    provider.meta =
        Some(serde_json::from_value(json!({"claudeDesktopModelRoutes": routes})).unwrap());
    state.db.save_provider("claude", &provider).unwrap();
    let profile = ProfileBundleService::capture(
        &state,
        "Stable metadata",
        &["claude".into(), "codex".into()],
        None,
    )
    .unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    for _ in 0..8 {
        assert_eq!(
            preview.revision,
            ProfileBundleService::preview(&state, &profile.id)
                .unwrap()
                .revision
        );
    }
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
}

#[test]
fn profiles_capture_only_the_current_supplier_for_clients_with_one_selection() {
    let _guard = test_guard();
    for app in [
        AppType::WorkBuddy,
        AppType::Qoder,
        AppType::Kimi,
        AppType::Qwen,
        AppType::ZCode,
    ] {
        let state = seed();
        for (id, model) in [("extra-a", "model-a"), ("extra-b", "model-b")] {
            state
                .db
                .save_provider(
                    app.as_str(),
                    &Provider::with_id(id.into(), id.into(), extra_config(&app, model), None),
                )
                .unwrap();
            ProviderService::switch(&state, app.clone(), id).unwrap();
        }
        let profile = ProfileBundleService::capture(
            &state,
            "Current supplier",
            &["claude".into(), app.as_str().into()],
            None,
        )
        .unwrap();
        let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
        let change = preview
            .changes
            .iter()
            .find(|c| c.app == app.as_str())
            .unwrap();
        assert_eq!(
            change.target_provider,
            "extra-b",
            "{} captured inactive suppliers",
            app.as_str()
        );
        assert!(change.models.contains(&"model-b".into()));
        assert!(
            !change.models.iter().any(|m| m.contains("model-a")),
            "{} captured inactive models",
            app.as_str()
        );
        let path = match app {
            AppType::WorkBuddy => support::ensure_test_home().join(".workbuddy-ai/models.json"),
            AppType::Qoder => support::ensure_test_home().join(".qoder-cn/settings.json"),
            AppType::Kimi => support::ensure_test_home().join(".kimi-code/config.toml"),
            AppType::Qwen => support::ensure_test_home().join(".qwen/settings.json"),
            AppType::ZCode => support::ensure_test_home().join(".zcode/v2/provider_config.json"),
            _ => unreachable!(),
        };
        let saved_native = std::fs::read(&path).unwrap();
        ProviderService::switch(&state, app.clone(), "extra-a").unwrap();
        let mut provider = state
            .db
            .get_provider_by_id("extra-b", app.as_str())
            .unwrap()
            .unwrap();
        provider.settings_config = extra_config(&app, "edited-model-b");
        ProviderService::update(&state, app.clone(), None, provider).unwrap();
        let before = std::fs::read(&path).unwrap();
        let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
        let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
        assert!(
            result.success,
            "{} failed to restore saved selection",
            app.as_str()
        );
        assert_eq!(
            ProviderService::current(&state, app.clone()).unwrap(),
            "extra-b"
        );
        let applied = std::fs::read_to_string(&path).unwrap();
        assert!(applied.contains("model-b"));
        assert!(!applied.contains("edited-model-b"));
        assert!(
            !applied.contains("model-a"),
            "{} left the previous supplier active",
            app.as_str()
        );
        if matches!(app, AppType::Kimi | AppType::Qwen | AppType::ZCode) {
            let selection = |bytes: &[u8]| {
                let value: serde_json::Value = if app == AppType::Kimi {
                    serde_json::to_value(
                        toml::from_str::<toml::Value>(std::str::from_utf8(bytes).unwrap()).unwrap(),
                    )
                    .unwrap()
                } else {
                    serde_json::from_slice(bytes).unwrap()
                };
                match app {
                    AppType::Kimi => value["default_model"].clone(),
                    AppType::Qwen => {
                        json!({"model":value["model"]["name"],"auth":value["security"]["auth"]["selectedType"]})
                    }
                    _ => value["config"]["defaultModelSelection"].clone(),
                }
            };
            assert_eq!(selection(applied.as_bytes()), selection(&saved_native));
        }
        ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}

#[test]
fn undo_remains_available_after_provider_lists_are_read() {
    let _guard = test_guard();
    let state = seed();
    for app in [AppType::WorkBuddy, AppType::Qoder, AppType::Kimi] {
        for (id, model) in [("extra-a", "model-a"), ("extra-b", "model-b")] {
            state
                .db
                .save_provider(
                    app.as_str(),
                    &Provider::with_id(id.into(), id.into(), extra_config(&app, model), None),
                )
                .unwrap();
        }
        ProviderService::switch(&state, app, "extra-a").unwrap();
    }
    let apps = ["claude", "codex", "workbuddy", "qoder", "kimi"].map(String::from);
    let profile = ProfileBundleService::capture(&state, "Read-only refresh", &apps, None).unwrap();
    ProviderService::switch(&state, AppType::Codex, "codex-b").unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    for app in [
        AppType::Claude,
        AppType::Codex,
        AppType::WorkBuddy,
        AppType::Qoder,
        AppType::Kimi,
    ] {
        ProviderService::list(&state, app.clone()).unwrap();
        ProviderService::current(&state, app).unwrap();
    }
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap())
        .expect("read-only UI refresh must not conflict with undo");
}

#[test]
fn pi_bundle_merges_native_model_fields_missing_from_saved_provider_and_can_undo() {
    let _guard = test_guard();
    let state = seed();
    let provider = Provider::with_id(
        "company".into(),
        "Company".into(),
        extra_config(&AppType::Pi, "saved-model"),
        None,
    );
    state.db.save_provider("pi", &provider).unwrap();
    ProviderService::switch(&state, AppType::Pi, "company").unwrap();
    let path = support::ensure_test_home().join(".pi/agent/models.json");
    let mut native: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    native["providers"]["company"]["models"][0]["api"] = json!("openai-completions");
    native["providers"]["company"]["models"][0]["compat"] = json!({"supportsDeveloperRole": true});
    std::fs::write(&path, serde_json::to_vec_pretty(&native).unwrap()).unwrap();
    let before_provider = state
        .db
        .get_provider_by_id("company", "pi")
        .unwrap()
        .unwrap();
    assert!(before_provider.settings_config["models"][0]
        .get("api")
        .is_none());
    assert!(before_provider.settings_config["models"][0]
        .get("compat")
        .is_none());
    let before_bytes = std::fs::read(&path).unwrap();
    let before_config = before_provider.settings_config;
    let profile =
        ProfileBundleService::capture(&state, "Work", &["claude".into(), "pi".into()], None)
            .unwrap();
    assert!(!profile.payload.contains("synthetic-secret"));

    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    assert_eq!(
        ProfileBundleService::preview(&state, &profile.id)
            .unwrap()
            .revision,
        preview.revision,
        "repeated previews must work without poisoning the operation lock"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before_bytes);
    assert_eq!(
        state
            .db
            .get_provider_by_id("company", "pi")
            .unwrap()
            .unwrap()
            .settings_config,
        before_config,
        "preview must not modify saved provider configuration"
    );

    let applied = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(applied.success);
    let config = state
        .db
        .get_provider_by_id("company", "pi")
        .unwrap()
        .unwrap()
        .settings_config;
    assert_eq!(config["models"][0]["api"], "openai-completions");
    assert_eq!(config["models"][0]["compat"]["supportsDeveloperRole"], true);
    assert_eq!(config["apiKey"], "synthetic-secret");
    ProfileBundleService::undo(&state, applied.undo_id.as_deref().unwrap()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before_bytes);
    assert_eq!(
        state
            .db
            .get_provider_by_id("company", "pi")
            .unwrap()
            .unwrap()
            .settings_config,
        before_config
    );
}

#[test]
fn captures_every_provider_capable_agent_instead_of_only_the_original_three() {
    let _guard = test_guard();
    support::reset_test_fs();
    let state = support::create_test_state().unwrap();
    let apps: Vec<_> = AppType::all()
        .filter(|app| *app != AppType::DoubaoWork)
        .collect();
    for app in &apps {
        let config = if *app == AppType::Codex {
            json!({"config":"model = \"fixture\"\n"})
        } else {
            json!({"env":{"ANTHROPIC_MODEL":"fixture"},"model":"fixture","models":[{"id":"fixture","model":"fixture"}]})
        };
        state
            .db
            .save_provider(
                app.as_str(),
                &Provider::with_id("fixture".into(), "Fixture".into(), config, None),
            )
            .unwrap();
        state
            .db
            .set_current_provider(app.as_str(), "fixture")
            .unwrap();
    }
    let profile = ProfileBundleService::capture(
        &state,
        "All tools",
        &apps
            .iter()
            .map(|app| app.as_str().to_owned())
            .collect::<Vec<_>>(),
        None,
    )
    .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&profile.payload).unwrap();
    assert_eq!(
        payload["bundle"]["selections"].as_array().unwrap().len(),
        15
    );
    for selection in payload["bundle"]["selections"].as_array().unwrap() {
        assert!(
            selection.get("library").is_some(),
            "library state missing for {}",
            selection["app"]
        );
    }
}

#[test]
fn bundle_captures_live_instructions_alongside_provider_models() {
    let _guard = test_guard();
    let state = seed();
    let home = std::path::PathBuf::from(std::env::var("CC_SWITCH_TEST_HOME").unwrap());
    std::fs::write(
        home.join(".claude/CLAUDE.md"),
        "Use Chinese; run the tests.",
    )
    .unwrap();
    let profile = ProfileBundleService::capture(
        &state,
        "With instructions",
        &["claude".into(), "codex".into()],
        None,
    )
    .unwrap();
    assert!(profile.payload.contains("Use Chinese; run the tests."));
}

#[test]
fn single_agent_snapshot_applies_and_undoes_without_changing_other_agents() {
    let _guard = test_guard();
    let state = seed();
    let profile = ProfileBundleService::capture(&state, "Current agent", &["claude".into()], None)
        .expect("capture one agent");
    ProviderService::switch(&state, AppType::Claude, "claude-b").unwrap();
    ProviderService::switch(&state, AppType::Codex, "codex-b").unwrap();
    let codex_path = cc_switch_lib::get_codex_config_path();
    let codex_before = std::fs::read(&codex_path).unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    assert_eq!(preview.changes.len(), 1);
    assert_eq!(preview.changes[0].app, "claude");
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        "claude-a"
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        "codex-b"
    );
    assert_eq!(std::fs::read(&codex_path).unwrap(), codex_before);
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        "claude-b"
    );
    assert_eq!(std::fs::read(codex_path).unwrap(), codex_before);
    for apps in [
        vec![],
        vec!["claude".into(), "claude".into()],
        vec!["unknown".into()],
    ] {
        assert!(ProfileBundleService::capture(&state, "Invalid scope", &apps, None).is_err());
    }
}

#[test]
fn bundle_previews_and_restores_provider_and_model_for_two_agents() {
    let _guard = test_guard();
    let state = seed();
    let profile =
        ProfileBundleService::capture(&state, "Work", &["claude".into(), "codex".into()], None)
            .expect("capture");
    ProviderService::switch(&state, AppType::Claude, "claude-b").expect("switch Claude");
    ProviderService::switch(&state, AppType::Codex, "codex-b").expect("switch Codex");
    let preview = ProfileBundleService::preview(&state, &profile.id).expect("preview");
    assert_eq!(preview.changes.len(), 2);
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        "claude-b"
    );
    assert!(!serde_json::to_string(&preview)
        .unwrap()
        .contains("synthetic-key"));
    let result =
        ProfileBundleService::apply(&state, &profile.id, &preview.revision).expect("apply");
    assert!(result.success, "{result:?}");
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        "claude-a"
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        "codex-a"
    );
    ProfileBundleService::undo(&state, result.undo_id.as_deref().expect("undo id"))
        .expect("restore previous configuration");
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        "claude-b"
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        "codex-b"
    );
}

#[test]
fn changed_live_file_invalidates_preview_without_overwriting_external_edits() {
    let _guard = test_guard();
    let state = seed();
    let profile =
        ProfileBundleService::capture(&state, "Work", &["claude".into(), "codex".into()], None)
            .unwrap();
    ProviderService::switch(&state, AppType::Claude, "claude-b").unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let path = cc_switch_lib::get_claude_settings_path();
    let external = b"{\"env\":{},\"external\":true}";
    std::fs::write(&path, external).unwrap();
    assert!(ProfileBundleService::apply(&state, &profile.id, &preview.revision).is_err());
    assert_eq!(std::fs::read(path).unwrap(), external);
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        "claude-b"
    );
}

#[test]
fn saving_a_bundle_keeps_existing_single_agent_profile_behavior() {
    let _guard = test_guard();
    let state = seed();
    let legacy =
        ProfileService::create(&state, "Project", cc_switch_lib::ProfileScope::Claude).unwrap();
    ProfileBundleService::capture(&state, "Work", &["claude".into(), "codex".into()], None)
        .unwrap();
    ProviderService::switch(&state, AppType::Claude, "claude-b").unwrap();
    ProfileService::apply(&state, &legacy.id, cc_switch_lib::ProfileScope::Claude).unwrap();
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        "claude-a"
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        "codex-a"
    );
}

#[test]
fn captures_live_model_selection_without_saving_credentials() {
    let _guard = test_guard();
    let state = seed();
    let path = cc_switch_lib::get_claude_settings_path();
    let mut live: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    live["env"]["ANTHROPIC_MODEL"] = json!("live-model");
    std::fs::write(&path, serde_json::to_vec(&live).unwrap()).unwrap();
    let profile =
        ProfileBundleService::capture(&state, "Work", &["claude".into(), "codex".into()], None)
            .unwrap();
    assert!(profile.payload.contains("live-model"));
    assert!(!profile.payload.contains("synthetic-key"));
    ProviderService::switch(&state, AppType::Claude, "claude-b").unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    let applied: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(applied["env"]["ANTHROPIC_MODEL"], "live-model");
}

#[test]
fn second_agent_failure_restores_first_agent_and_original_files() {
    let _guard = test_guard();
    let state = seed();
    let profile =
        ProfileBundleService::capture(&state, "Work", &["claude".into(), "codex".into()], None)
            .unwrap();
    ProviderService::switch(&state, AppType::Claude, "claude-b").unwrap();
    ProviderService::switch(&state, AppType::Codex, "codex-b").unwrap();
    let mut provider = state
        .db
        .get_provider_by_id("codex-a", "codex")
        .unwrap()
        .unwrap();
    provider.settings_config["auth"] = json!({});
    provider.settings_config["config"] = json!("");
    provider.category = Some("official".into());
    provider.meta = Some(
        serde_json::from_value(json!({"authBinding": {
            "source":"managed_account", "authProvider":"codex_oauth", "accountId":"missing-account"
        }}))
        .unwrap(),
    );
    state.db.save_provider("codex", &provider).unwrap();
    let path = cc_switch_lib::get_claude_settings_path();
    let before = std::fs::read(&path).unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(!result.success);
    assert!(result.undo_id.is_none());
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        "claude-b"
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Codex).unwrap(),
        "codex-b"
    );
}

#[test]
fn undo_refuses_to_overwrite_an_external_edit() {
    let _guard = test_guard();
    let state = seed();
    let profile =
        ProfileBundleService::capture(&state, "Work", &["claude".into(), "codex".into()], None)
            .unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    let path = cc_switch_lib::get_claude_settings_path();
    let external = br#"{"env":{},"external":true}"#;
    std::fs::write(&path, external).unwrap();
    let err = ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap_err();
    assert!(err.to_string().contains("conflict"));
    assert_eq!(std::fs::read(path).unwrap(), external);
}

fn extra_config(app: &AppType, model: &str) -> serde_json::Value {
    match app {
        AppType::Gemini => {
            json!({"env":{"GEMINI_API_KEY":"synthetic-secret","GOOGLE_GEMINI_BASE_URL":"https://example.test","GEMINI_MODEL":model},"config":{}})
        }
        AppType::GrokBuild => {
            json!({"config":format!("[models]\ndefault = \"custom\"\n[model.custom]\nname = \"Fixture\"\nmodel = \"{model}\"\nbase_url = \"https://example.test/v1\"\napi_key = \"synthetic-secret\"\napi_backend = \"openai\"\ncontext_window = 8192\n")})
        }
        AppType::OpenCode => {
            json!({"npm":"@ai-sdk/openai-compatible","options":{"apiKey":"synthetic-secret","baseURL":"https://example.test/v1"},"models":{model:{"name":model}}})
        }
        AppType::OpenClaw => {
            json!({"baseUrl":"https://example.test/v1","apiKey":"synthetic-secret","api":"openai-completions","models":[{"id":model,"name":model}]})
        }
        AppType::Hermes => {
            json!({"base_url":"https://example.test/v1","api_key":"synthetic-secret","models":[{"id":model,"name":model}]})
        }
        AppType::Pi => {
            json!({"baseUrl":"https://example.test/v1","apiKey":"synthetic-secret","api":"openai-completions","models":[{"id":model,"name":model,"contextWindow":8192,"maxTokens":1024,"input":["text"]}]})
        }
        AppType::WorkBuddy => {
            json!({"id":model,"name":model,"url":"https://example.test/v1/chat/completions","apiKey":"synthetic-secret"})
        }
        AppType::Qoder => {
            json!({"protocol":"openai","model":model,"baseUrl":"https://example.test/v1","apiKey":"synthetic-secret","models":[{"model":model}]})
        }
        _ => {
            json!({"baseUrl":"https://example.test/v1","apiKey":"synthetic-secret","protocol":"openai","models":[{"model":model}]})
        }
    }
}

#[test]
fn added_agents_apply_and_undo_through_their_real_configuration_adapters() {
    let _guard = test_guard();
    for app in [
        AppType::Gemini,
        AppType::GrokBuild,
        AppType::OpenCode,
        AppType::OpenClaw,
        AppType::Hermes,
        AppType::Pi,
        AppType::WorkBuddy,
        AppType::Qoder,
        AppType::Kimi,
        AppType::Qwen,
        AppType::ZCode,
    ] {
        let state = seed();
        for (id, model) in [("extra-a", "extra-model-a"), ("extra-b", "extra-model-b")] {
            state
                .db
                .save_provider(
                    app.as_str(),
                    &Provider::with_id(id.into(), id.into(), extra_config(&app, model), None),
                )
                .unwrap();
        }
        ProviderService::switch(&state, app.clone(), "extra-a")
            .unwrap_or_else(|e| panic!("{} initial switch: {e}", app.as_str()));
        let home = std::path::PathBuf::from(std::env::var("CC_SWITCH_TEST_HOME").unwrap());
        let path = home.join(match app {
            AppType::Gemini => ".gemini/.env",
            AppType::GrokBuild => ".grok/config.toml",
            AppType::OpenCode => ".config/opencode/opencode.json",
            AppType::OpenClaw => ".openclaw/openclaw.json",
            AppType::Hermes if cfg!(windows) => "AppData/Local/hermes/config.yaml",
            AppType::Hermes => ".hermes/config.yaml",
            AppType::Pi => ".pi/agent/models.json",
            AppType::WorkBuddy => ".workbuddy-ai/models.json",
            AppType::Qoder => ".qoder-cn/settings.json",
            AppType::Kimi => ".kimi-code/config.toml",
            AppType::Qwen => ".qwen/settings.json",
            AppType::ZCode => ".zcode/v2/provider_config.json",
            _ => unreachable!(),
        });
        let profile = ProfileBundleService::capture(
            &state,
            "Extra",
            &["claude".into(), app.as_str().into()],
            None,
        )
        .unwrap_or_else(|e| panic!("{} capture: {e}", app.as_str()));
        assert!(
            !profile.payload.contains("synthetic-secret"),
            "{} leaked a credential",
            app.as_str()
        );
        ProviderService::switch(&state, app.clone(), "extra-b").unwrap();
        let before_bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{} path {}: {e}", app.as_str(), path.display()));
        let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
        if app == AppType::Pi {
            let change = preview.changes.iter().find(|c| c.app == "pi").unwrap();
            assert!(
                change.current_provider.contains("extra-b"),
                "Pi preview must show enabled providers even without a current-provider pointer"
            );
            assert!(change.current_models.contains(&"extra-model-b".into()));
        }
        let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision)
            .unwrap_or_else(|e| panic!("{} apply: {e}", app.as_str()));
        assert!(
            result.success,
            "{} rolled back: {:?}",
            app.as_str(),
            result.warnings
        );
        let applied = std::fs::read_to_string(&path).unwrap();
        assert!(
            applied.contains("extra-model-a"),
            "{} model not applied: {applied}",
            app.as_str()
        );
        assert!(
            !applied.contains("extra-model-b"),
            "{} left unwanted model/provider enabled: {applied}",
            app.as_str()
        );
        assert_eq!(
            state
                .db
                .get_current_profile_id(app.as_str())
                .unwrap()
                .as_deref(),
            Some(profile.id.as_str())
        );
        ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap())
            .unwrap_or_else(|e| panic!("{} undo: {e}", app.as_str()));
        assert_eq!(state.db.get_current_profile_id(app.as_str()).unwrap(), None);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before_bytes,
            "{} undo did not restore exact file",
            app.as_str()
        );
    }
}

#[test]
fn instructions_mcp_and_skill_assignments_apply_and_restore_with_the_bundle() {
    use cc_switch_lib::{McpService, SkillService};
    let _guard = test_guard();
    let state = seed();
    let home = std::path::PathBuf::from(std::env::var("CC_SWITCH_TEST_HOME").unwrap());
    let server=serde_json::from_value(json!({"id":"fixture-mcp","name":"Fixture MCP","server":{"command":"fixture","args":[]},"apps":{"claude":true}})).unwrap();
    McpService::upsert_server(&state, server).unwrap();
    let skill=serde_json::from_value(json!({"id":"local:fixture","name":"Fixture Skill","directory":"fixture","apps":{},"installedAt":1})).unwrap();
    state.db.save_skill(&skill).unwrap();
    let ssot = SkillService::get_ssot_dir().unwrap().join("fixture");
    std::fs::create_dir_all(&ssot).unwrap();
    std::fs::write(ssot.join("SKILL.md"), "# Fixture\nDo the task.").unwrap();
    SkillService::toggle_app(&state.db, "local:fixture", &AppType::Claude, true).unwrap();
    std::fs::write(home.join(".claude/CLAUDE.md"), "Saved instruction").unwrap();
    let profile = ProfileBundleService::capture(
        &state,
        "Resources",
        &["claude".into(), "codex".into()],
        None,
    )
    .unwrap();
    McpService::toggle_app(&state, "fixture-mcp", AppType::Claude, false).unwrap();
    SkillService::toggle_app(&state.db, "local:fixture", &AppType::Claude, false).unwrap();
    std::fs::write(home.join(".claude/CLAUDE.md"), "Before switch").unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    assert!(
        state.db.get_all_mcp_servers().unwrap()["fixture-mcp"]
            .apps
            .claude
    );
    assert!(
        state.db.get_all_installed_skills().unwrap()["local:fixture"]
            .apps
            .claude
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(),
        "Saved instruction"
    );
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
    assert!(
        !state.db.get_all_mcp_servers().unwrap()["fixture-mcp"]
            .apps
            .claude
    );
    assert!(
        !state.db.get_all_installed_skills().unwrap()["local:fixture"]
            .apps
            .claude
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(),
        "Before switch"
    );
    assert!(!SkillService::get_app_skills_dir(&AppType::Claude)
        .unwrap()
        .join("fixture")
        .exists());
}

#[test]
fn failed_resource_projection_rolls_back_prior_mcp_and_instruction_changes() {
    use cc_switch_lib::{McpService, SkillService};
    let _guard = test_guard();
    let state = seed();
    let home = std::path::PathBuf::from(std::env::var("CC_SWITCH_TEST_HOME").unwrap());
    McpService::upsert_server(
        &state,
        serde_json::from_value(
            json!({"id":"mcp","name":"MCP","server":{"command":"fixture"},"apps":{"claude":true}}),
        )
        .unwrap(),
    )
    .unwrap();
    state.db.save_skill(&serde_json::from_value(json!({"id":"local:lost","name":"Lost","directory":"lost","apps":{"claude":true},"installedAt":1})).unwrap()).unwrap();
    std::fs::write(home.join(".claude/CLAUDE.md"), "Saved").unwrap();
    let profile = ProfileBundleService::capture(
        &state,
        "Missing source",
        &["claude".into(), "codex".into()],
        None,
    )
    .unwrap();
    McpService::toggle_app(&state, "mcp", AppType::Claude, false).unwrap();
    let mut skill = state.db.get_all_installed_skills().unwrap()["local:lost"].clone();
    skill.apps.claude = false;
    state.db.update_skill_apps(&skill.id, &skill.apps).unwrap();
    std::fs::write(home.join(".claude/CLAUDE.md"), "Before").unwrap();
    let before = std::fs::read(home.join(".claude.json")).unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(!result.success);
    assert!(!state.db.get_all_mcp_servers().unwrap()["mcp"].apps.claude);
    assert!(
        !state.db.get_all_installed_skills().unwrap()["local:lost"]
            .apps
            .claude
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".claude/CLAUDE.md")).unwrap(),
        "Before"
    );
    assert_eq!(std::fs::read(home.join(".claude.json")).unwrap(), before);
    assert!(!SkillService::get_app_skills_dir(&AppType::Claude)
        .unwrap()
        .join("lost")
        .exists());
}

#[test]
fn closed_trae_does_not_block_profile_preview_apply_or_undo() {
    let _guard = test_guard();
    let state = seed();
    for (id, model) in [("trae-a", "model-a"), ("trae-b", "model-b")] {
        state
            .db
            .save_provider(
                "trae",
                &Provider::with_id(
                    id.into(),
                    id.into(),
                    extra_config(&AppType::Trae, model),
                    None,
                ),
            )
            .unwrap();
    }
    state.db.set_current_provider("trae", "trae-a").unwrap();
    let profile =
        ProfileBundleService::capture(&state, "Offline", &["claude".into(), "trae".into()], None)
            .unwrap();
    ProviderService::switch(&state, AppType::Claude, "claude-b").unwrap();
    state.db.set_current_provider("trae", "trae-b").unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id)
        .expect("preview must not require a connected client");
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    assert!(result
        .warnings
        .iter()
        .any(|w| w == "profiles.bundle.clientSyncPending"));
    assert!(state.db.get_bool_flag("profile_trae_sync_pending").unwrap());
    assert!(ProfileBundleService::preview(&state, &profile.id)
        .unwrap()
        .changes
        .iter()
        .any(|change| change.app == "trae" && change.client_sync_pending));
    assert_eq!(
        state.db.get_current_provider("claude").unwrap().as_deref(),
        Some("claude-a")
    );
    assert_eq!(
        state.db.get_current_provider("trae").unwrap().as_deref(),
        Some("trae-a")
    );
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
    assert_eq!(
        state.db.get_current_provider("claude").unwrap().as_deref(),
        Some("claude-b")
    );
    assert_eq!(
        state.db.get_current_provider("trae").unwrap().as_deref(),
        Some("trae-b")
    );
}

#[cfg(unix)]
#[test]
fn switching_profiles_reuses_unchanged_agents_without_rewriting_their_files() {
    use std::os::unix::fs::MetadataExt;
    let _guard = test_guard();
    let state = seed();
    let apps = ["claude".into(), "codex".into()];
    let work = ProfileBundleService::capture(&state, "Work", &apps, None).unwrap();
    ProviderService::switch(&state, AppType::Claude, "claude-b").unwrap();
    let personal = ProfileBundleService::capture(&state, "Personal", &apps, None).unwrap();
    let preview = ProfileBundleService::preview(&state, &work.id).unwrap();
    assert!(
        ProfileBundleService::apply(&state, &work.id, &preview.revision)
            .unwrap()
            .success
    );
    let path = support::ensure_test_home().join(".codex/config.toml");
    let inode = std::fs::metadata(&path).unwrap().ino();
    let preview = ProfileBundleService::preview(&state, &personal.id).unwrap();
    let result = ProfileBundleService::apply(&state, &personal.id, &preview.revision).unwrap();
    assert!(result.success);
    assert_eq!(
        std::fs::metadata(&path).unwrap().ino(),
        inode,
        "unchanged Codex configuration was rewritten"
    );
    assert_eq!(
        state.db.get_current_provider("claude").unwrap().as_deref(),
        Some("claude-b")
    );
    assert_eq!(
        state.db.get_current_profile_id("codex").unwrap().as_deref(),
        Some(personal.id.as_str())
    );
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
    assert_eq!(
        state.db.get_current_provider("claude").unwrap().as_deref(),
        Some("claude-a")
    );
    assert_eq!(
        state.db.get_current_profile_id("codex").unwrap().as_deref(),
        Some(work.id.as_str())
    );
    // A supplier edited outside the scheme must invalidate reuse, even when
    // its ID and selected model are unchanged.
    let preview = ProfileBundleService::preview(&state, &personal.id).unwrap();
    assert!(
        ProfileBundleService::apply(&state, &personal.id, &preview.revision)
            .unwrap()
            .success
    );
    let inode = std::fs::metadata(&path).unwrap().ino();
    let mut provider = state
        .db
        .get_provider_by_id("codex-a", "codex")
        .unwrap()
        .unwrap();
    provider.settings_config["config"] = json!(provider.settings_config["config"]
        .as_str()
        .unwrap()
        .replace("example.test", "changed.example.test"));
    state.db.save_provider("codex", &provider).unwrap();
    let preview = ProfileBundleService::preview(&state, &work.id).unwrap();
    assert!(
        ProfileBundleService::apply(&state, &work.id, &preview.revision)
            .unwrap()
            .success
    );
    assert_ne!(std::fs::metadata(&path).unwrap().ino(), inode);
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("changed.example.test"));
}

#[cfg(unix)]
#[test]
fn trae_profiles_deliver_when_connected_without_binding_preview_to_runtime() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
    };
    let _guard = test_guard();
    // macOS's default temporary directory can exceed Unix socket SUN_LEN.
    support::ensure_test_home();
    struct RestoreHome(std::ffi::OsString);
    impl Drop for RestoreHome {
        fn drop(&mut self) {
            std::env::set_var("CC_SWITCH_TEST_HOME", &self.0);
        }
    }
    let _restore_home = RestoreHome(std::env::var_os("CC_SWITCH_TEST_HOME").unwrap());
    let short_home = tempfile::Builder::new()
        .prefix("vary-profile-")
        .tempdir_in("/tmp")
        .unwrap();
    std::env::set_var("CC_SWITCH_TEST_HOME", short_home.path());
    let state = seed();
    let home = std::path::PathBuf::from(std::env::var("CC_SWITCH_TEST_HOME").unwrap());
    let dir = home.join(".cc-switch/trae-bridge");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bridge-agent-profile.sock");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let models = Arc::new(Mutex::new(std::collections::BTreeMap::<
        String,
        serde_json::Value,
    >::new()));
    let running = Arc::new(AtomicBool::new(true));
    let (server_models, run) = (models.clone(), running.clone());
    let worker = std::thread::spawn(move || {
        while run.load(Ordering::Relaxed) {
            let (mut socket, _) = match listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    continue;
                }
                Err(_) => break,
            };
            let mut line = String::new();
            BufReader::new(socket.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            let result=match request["op"].as_str().unwrap(){
            "health"=>json!({"ready":true,"enabled":false}),
            "list"=>json!(server_models.lock().unwrap().iter().map(|(id,c)|json!({"id":id,"name":c["models"][0]["model"],"displayName":format!("VaryAI/{id}/{}",c["models"][0]["model"].as_str().unwrap()),"baseUrl":c["baseUrl"]})).collect::<Vec<_>>()),
            "apply"=>{
                let id = request["id"].as_str().unwrap();
                let mut models = server_models.lock().unwrap();
                if models.iter().any(|(other, config)| other != id && config["models"][0]["model"] == request["config"]["models"][0]["model"]) {
                    writeln!(socket, "{}", json!({"error":"synthetic duplicate request model"})).unwrap();
                    continue;
                }
                models.insert(id.into(),request["config"].clone());json!(true)
            },
            "remove"=>{server_models.lock().unwrap().remove(request["id"].as_str().unwrap());json!(true)},
            _=>panic!("unexpected operation"),
        };
            writeln!(socket, "{}", json!({"result":result})).unwrap();
        }
    });
    struct Stop(Arc<AtomicBool>, Option<std::thread::JoinHandle<()>>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Relaxed);
            self.1.take().unwrap().join().unwrap();
        }
    }
    let _stop = Stop(running, Some(worker));
    for (id, model) in [("trae-a", "shared-model"), ("trae-b", "shared-model")] {
        state
            .db
            .save_provider(
                "trae",
                &Provider::with_id(
                    id.into(),
                    id.into(),
                    extra_config(&AppType::Trae, model),
                    None,
                ),
            )
            .unwrap();
    }
    ProviderService::switch(&state, AppType::Trae, "trae-a").unwrap();
    let profile =
        ProfileBundleService::capture(&state, "TRAE", &["claude".into(), "trae".into()], None)
            .unwrap();
    ProviderService::switch(&state, AppType::Trae, "trae-b").unwrap();
    let before = models.lock().unwrap().clone();
    // Losing the client after review must not invalidate local selection.
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let offline_path = path.with_extension("offline");
    std::fs::rename(&path, &offline_path).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    assert!(result
        .warnings
        .iter()
        .any(|w| w == "profiles.bundle.clientSyncPending"));
    assert_eq!(*models.lock().unwrap(), before);
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
    std::fs::rename(&offline_path, &path).unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    assert!(!state.db.get_bool_flag("profile_trae_sync_pending").unwrap());
    assert!(models.lock().unwrap().contains_key("trae-a"));
    assert!(!models.lock().unwrap().contains_key("trae-b"));
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
    assert_eq!(*models.lock().unwrap(), before);
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    models.lock().unwrap().get_mut("trae-b").unwrap()["models"][0]["model"] =
        json!("external-edit");
    // Runtime changes do not invalidate the local scheme preview. External
    // changes to VaryAI's saved provider configuration still reject apply.
    assert_eq!(
        ProfileBundleService::preview(&state, &profile.id)
            .unwrap()
            .revision,
        preview.revision
    );
    let mut provider = state
        .db
        .get_provider_by_id("trae-a", "trae")
        .unwrap()
        .unwrap();
    provider.settings_config = extra_config(&AppType::Trae, "external-config-edit");
    state.db.save_provider("trae", &provider).unwrap();
    assert!(ProfileBundleService::apply(&state, &profile.id, &preview.revision).is_err());
    assert_eq!(
        models.lock().unwrap()["trae-b"]["models"][0]["model"],
        "external-edit"
    );
}

#[test]
fn profile_mcp_assignments_survive_proxy_restore_and_undo_restores_backup() {
    use cc_switch_lib::McpService;
    let _guard = test_guard();
    let state = seed();
    let server = serde_json::from_value(json!({"id":"profile-mcp","name":"Profile MCP","server":{"command":"fixture"},"apps":{"codex":true}})).unwrap();
    McpService::upsert_server(&state, server).unwrap();
    let profile = ProfileBundleService::capture(
        &state,
        "Proxy resources",
        &["claude".into(), "codex".into()],
        None,
    )
    .unwrap();
    McpService::toggle_app(&state, "profile-mcp", AppType::Codex, false).unwrap();
    let config =
        std::fs::read_to_string(support::ensure_test_home().join(".codex/config.toml")).unwrap();
    let backup = json!({"auth":{"OPENAI_API_KEY":"synthetic-key"},"config":config}).to_string();
    futures::executor::block_on(state.db.save_live_backup("codex", &backup)).unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    let updated = futures::executor::block_on(state.db.get_live_backup("codex"))
        .unwrap()
        .unwrap();
    let saved: serde_json::Value = serde_json::from_str(&updated.original_config).unwrap();
    let doc: toml::Value = saved["config"].as_str().unwrap().parse().unwrap();
    assert!(
        doc["mcp_servers"].get("profile-mcp").is_some(),
        "stopping takeover must retain the profile's MCP assignment"
    );
    assert_eq!(saved["auth"]["OPENAI_API_KEY"], "synthetic-key");
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
    assert_eq!(
        futures::executor::block_on(state.db.get_live_backup("codex"))
            .unwrap()
            .unwrap()
            .original_config,
        backup
    );
}

#[test]
fn empty_native_model_selections_apply_without_enabling_fallback_suppliers() {
    let _guard = test_guard();
    let state = seed();
    for app in [AppType::Kimi, AppType::Qwen, AppType::ZCode] {
        state
            .db
            .save_provider(
                app.as_str(),
                &Provider::with_id(
                    "configured".into(),
                    "Configured".into(),
                    extra_config(&app, "model"),
                    None,
                ),
            )
            .unwrap();
    }
    let profile = ProfileBundleService::capture(
        &state,
        "Empty native",
        &[
            "claude".into(),
            "kimi".into(),
            "qwen".into(),
            "zcode".into(),
        ],
        None,
    )
    .unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success, "empty native selections must be valid");
    for app in ["kimi", "qwen", "zcode"] {
        assert!(state.db.get_current_provider(app).unwrap().is_none());
    }
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
}

#[test]
fn supported_file_based_agents_apply_as_one_bundle_and_undo_together() {
    let _guard = test_guard();
    let state = seed();
    let apps: Vec<_> = AppType::all()
        .filter(|app| !matches!(app, AppType::DoubaoWork | AppType::Trae))
        .filter(|app| *app != AppType::ClaudeDesktop || cfg!(any(target_os = "macos", windows)))
        .collect();
    assert_eq!(
        apps.len(),
        if cfg!(any(target_os = "macos", windows)) {
            14
        } else {
            13
        }
    );
    if apps.contains(&AppType::ClaudeDesktop) {
        let mut official = Provider::with_id(
            "claude-desktop-official".into(),
            "Desktop Official".into(),
            json!({"env":{}}),
            None,
        );
        official.category = Some("official".into());
        state.db.save_provider("claude-desktop", &official).unwrap();
        ProviderService::switch(&state, AppType::ClaudeDesktop, &official.id).unwrap();
    }
    for app in apps.iter().filter(|app| {
        !matches!(
            app,
            AppType::Claude | AppType::Codex | AppType::ClaudeDesktop
        )
    }) {
        for (id, model) in [("extra-a", "model-a"), ("extra-b", "model-b")] {
            state
                .db
                .save_provider(
                    app.as_str(),
                    &Provider::with_id(id.into(), id.into(), extra_config(app, model), None),
                )
                .unwrap();
        }
        ProviderService::switch(&state, app.clone(), "extra-a").unwrap();
    }
    let profile = ProfileBundleService::capture(
        &state,
        "All file agents",
        &apps
            .iter()
            .map(|app| app.as_str().into())
            .collect::<Vec<_>>(),
        None,
    )
    .unwrap();
    for app in &apps {
        if *app == AppType::ClaudeDesktop {
            continue;
        }
        let id = match app {
            AppType::Claude => "claude-b",
            AppType::Codex => "codex-b",
            _ => "extra-b",
        };
        ProviderService::switch(&state, app.clone(), id).unwrap();
    }
    let before: Vec<_> = apps
        .iter()
        .map(|app| state.db.get_all_providers(app.as_str()).unwrap())
        .collect();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    assert_eq!(preview.changes.len(), apps.len());
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success, "all adapters must apply together");
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
    for (app, expected) in apps.iter().zip(before) {
        assert_eq!(
            serde_json::to_value(state.db.get_all_providers(app.as_str()).unwrap()).unwrap(),
            serde_json::to_value(expected).unwrap(),
            "{} provider metadata was not restored",
            app.as_str()
        );
    }
}

#[test]
fn workbuddy_model_edit_is_replaced_by_saved_catalog_without_orphaning_old_ids() {
    let _guard = test_guard();
    let state = seed();
    let provider = Provider::with_id(
        "workbuddy".into(),
        "WorkBuddy".into(),
        extra_config(&AppType::WorkBuddy, "saved-model"),
        None,
    );
    state.db.save_provider("workbuddy", &provider).unwrap();
    ProviderService::switch(&state, AppType::WorkBuddy, "workbuddy").unwrap();
    let profile = ProfileBundleService::capture(
        &state,
        "Catalog",
        &["claude".into(), "workbuddy".into()],
        None,
    )
    .unwrap();
    let mut edited = state
        .db
        .get_provider_by_id("workbuddy", "workbuddy")
        .unwrap()
        .unwrap();
    edited.settings_config["id"] = json!("later-model");
    ProviderService::update(&state, AppType::WorkBuddy, None, edited).unwrap();
    let path = support::ensure_test_home().join(".workbuddy-ai/models.json");
    let before = std::fs::read(&path).unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("saved-model"));
    assert!(
        !text.contains("later-model"),
        "profile must remove the superseded native model ID"
    );
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn undo_restores_an_instruction_file_symlink_and_its_existing_target() {
    let _guard = test_guard();
    let state = seed();
    let home = support::ensure_test_home();
    let target = home.join(".cc-switch/shared-instructions.md");
    let path = home.join(".claude/CLAUDE.md");
    std::fs::write(&target, "Saved instructions").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let profile = ProfileBundleService::capture(
        &state,
        "Linked instructions",
        &["claude".into(), "codex".into()],
        None,
    )
    .unwrap();
    std::fs::write(&target, "Before switch").unwrap();
    let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
    let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
    assert!(result.success);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "Saved instructions"
    );
    ProfileBundleService::undo(&state, result.undo_id.as_deref().unwrap()).unwrap();
    assert_eq!(std::fs::read_link(&path).unwrap(), target);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "Before switch");
}

#[test]
fn two_named_bundles_can_be_edited_and_switched_without_single_provider_changes_rewriting_them() {
    let _guard = test_guard();
    let state = seed();
    let apps = ["claude".into(), "codex".into()];
    let work = ProfileBundleService::capture(&state, "Work", &apps, None).unwrap();
    ProviderService::switch(&state, AppType::Claude, "claude-b").unwrap();
    ProviderService::switch(&state, AppType::Codex, "codex-b").unwrap();
    let personal = ProfileBundleService::capture(&state, "Personal", &apps, None).unwrap();
    ProfileService::update(&state, &personal.id, Some("Budget".into()), false, None).unwrap();
    // Edit the live model, then explicitly replace that bundle's saved state.
    let claude_path = cc_switch_lib::get_claude_settings_path();
    let mut live: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&claude_path).unwrap()).unwrap();
    live["env"]["ANTHROPIC_MODEL"] = json!("budget-model");
    std::fs::write(&claude_path, serde_json::to_vec(&live).unwrap()).unwrap();
    let edited =
        ProfileBundleService::capture(&state, "Budget", &apps, Some(&personal.id)).unwrap();
    assert_eq!(edited.id, personal.id);
    assert_eq!(state.db.get_all_profiles().unwrap().len(), 2);
    // Exercise both directions twice, checking the actual native files each time.
    for (profile, suffix, claude_model) in [
        (&work, "a", "model-a"),
        (&edited, "b", "budget-model"),
        (&work, "a", "model-a"),
        (&edited, "b", "budget-model"),
    ] {
        let preview = ProfileBundleService::preview(&state, &profile.id).unwrap();
        assert_eq!(preview.changes.len(), 2);
        let result = ProfileBundleService::apply(&state, &profile.id, &preview.revision).unwrap();
        assert!(result.success, "{result:?}");
        for app in [AppType::Claude, AppType::Codex] {
            assert_eq!(
                ProviderService::current(&state, app.clone()).unwrap(),
                format!("{}-{suffix}", app.as_str())
            );
        }
        let live: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&claude_path).unwrap()).unwrap();
        assert_eq!(live["env"]["ANTHROPIC_MODEL"], claude_model);
        assert_eq!(live["env"]["ANTHROPIC_BASE_URL"], "https://example.test");
        let codex: toml_edit::DocumentMut =
            std::fs::read_to_string(cc_switch_lib::get_codex_config_path())
                .unwrap()
                .parse()
                .unwrap();
        assert_eq!(
            codex["model"].as_str(),
            Some(format!("model-{suffix}").as_str())
        );
        assert_eq!(
            codex["model_providers"]["fixture"]["base_url"].as_str(),
            Some("https://example.test/v1")
        );
    }
    let profiles_before = state.db.get_all_profiles().unwrap();
    let codex_before = std::fs::read(cc_switch_lib::get_codex_config_path()).unwrap();
    ProviderService::switch(&state, AppType::Claude, "claude-a").unwrap();
    assert_eq!(
        std::fs::read(cc_switch_lib::get_codex_config_path()).unwrap(),
        codex_before
    );
    for profile in profiles_before {
        assert_eq!(
            state.db.get_profile(&profile.id).unwrap().unwrap().payload,
            profile.payload
        );
    }
}

#[test]
fn later_custom_suppliers_can_be_captured_without_changing_old_profiles_or_copying_credentials() {
    let _guard = test_guard();
    let state = seed();
    let apps = ["claude".into(), "codex".into()];
    let old = ProfileBundleService::capture(&state, "Original", &apps, None).unwrap();
    // A vendor unknown when the profile was created, with arbitrary IDs and models.
    let id = "new-vendor-arbitrary-id";
    let custom = Provider::with_id(
        id.into(),
        "New vendor".into(),
        json!({"env": {
            "ANTHROPIC_BASE_URL": "https://new-vendor.example.test",
            "ANTHROPIC_AUTH_TOKEN": "new-synthetic-secret",
            "ANTHROPIC_MODEL": "new-vendor-custom-model"
        }}),
        None,
    );
    state.db.save_provider("claude", &custom).unwrap();
    ProviderService::switch(&state, AppType::Claude, id).unwrap();
    assert_eq!(
        state.db.get_profile(&old.id).unwrap().unwrap().payload,
        old.payload
    );
    let newer = ProfileBundleService::capture(&state, "New vendor", &apps, None).unwrap();
    assert!(newer.payload.contains("new-vendor-custom-model"));
    assert!(!newer.payload.contains("new-synthetic-secret"));
    let preview = ProfileBundleService::preview(&state, &old.id).unwrap();
    assert!(
        ProfileBundleService::apply(&state, &old.id, &preview.revision)
            .unwrap()
            .success
    );
    assert_eq!(
        ProviderService::current(&state, AppType::Claude).unwrap(),
        "claude-a"
    );
    assert!(state.db.get_provider_by_id(id, "claude").unwrap().is_some());
    // Rotate connection details later: the profile must use current provider data.
    let mut rotated = custom;
    rotated.settings_config["env"]["ANTHROPIC_BASE_URL"] = json!("https://rotated.example.test");
    rotated.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"] = json!("rotated-synthetic-secret");
    state.db.save_provider("claude", &rotated).unwrap();
    let preview = ProfileBundleService::preview(&state, &newer.id).unwrap();
    assert_eq!(preview.changes[0].target_provider, "New vendor");
    assert!(
        ProfileBundleService::apply(&state, &newer.id, &preview.revision)
            .unwrap()
            .success
    );
    let live: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cc_switch_lib::get_claude_settings_path()).unwrap())
            .unwrap();
    assert_eq!(live["env"]["ANTHROPIC_MODEL"], "new-vendor-custom-model");
    assert_eq!(
        live["env"]["ANTHROPIC_BASE_URL"],
        "https://rotated.example.test"
    );
    assert_eq!(
        live["env"]["ANTHROPIC_AUTH_TOKEN"],
        "rotated-synthetic-secret"
    );
    // Updating an older profile to include the new vendor is explicit.
    let updated = ProfileBundleService::capture(&state, "Original", &apps, Some(&old.id)).unwrap();
    assert!(updated.payload.contains(id));
}
