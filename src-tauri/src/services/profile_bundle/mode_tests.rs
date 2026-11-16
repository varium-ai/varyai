//! Scheme mode changes use the existing guarded disconnect and native switch paths.
use super::*;
use crate::proxy::intent::{Menu, MenuModel, Session, Target};
use std::{collections::HashMap, sync::Arc, time::Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Home {
    _directory: tempfile::TempDir,
    previous: Option<std::ffi::OsString>,
}
impl Home {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", directory.path());
        crate::settings::reload_settings()?;
        Ok(Self {
            _directory: directory,
            previous,
        })
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        let _ = crate::settings::reload_settings();
    }
}

fn setup() -> Result<(Home, AppState, Profile), Box<dyn std::error::Error>> {
    let home = Home::new()?;
    let mut state = AppState::new(Arc::new(crate::database::Database::memory()?));
    // Match production: routing and native suppliers own the same client file.
    state.proxy_service.intent_client =
        Arc::new(super::super::intent_routing::client::Manager::new(
            crate::codex_config::get_codex_config_path(),
            crate::config::get_app_config_dir().join("intent-routing-client.json"),
            state.proxy_service.intent_runtime.clone(),
        ));
    for (id, name, model) in [
        ("native-a", "Vendor A", "model-a"),
        ("native-b", "Vendor B", "model-b"),
    ] {
        let provider = Provider::with_id(
            id.into(),
            name.into(),
            json!({
                "auth":{"OPENAI_API_KEY":"synthetic-key"},
                "config":format!("model='{model}'\nmodel_provider='fixture'\n[model_providers.fixture]\nname='Fixture'\nbase_url='https://example.test/v1'\nwire_api='responses'\n")
            }),
            None,
        );
        state.db.save_provider("codex", &provider)?;
    }
    ProviderService::switch(&state, AppType::Codex, "native-a")?;
    let profile = ProfileBundleService::capture(&state, "Native scheme", &["codex".into()], None)?;
    ProviderService::switch(&state, AppType::Codex, "native-b")?;
    // Seed the client-owned native document explicitly; provider selection may
    // defer projection until the client is connected.
    let native = state
        .db
        .get_provider_by_id("native-b", "codex")?
        .ok_or("missing native fixture")?;
    crate::config::atomic_write_private(
        &crate::codex_config::get_codex_config_path(),
        native.settings_config["config"]
            .as_str()
            .ok_or("missing native config fixture")?
            .as_bytes(),
    )?;
    crate::config::write_json_file(
        &crate::codex_config::get_codex_auth_path(),
        &json!({"auth_mode":"api_key", "OPENAI_API_KEY":"synthetic-existing"}),
    )?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let origin = format!("http://{}", listener.local_addr()?);
    let group = serde_json::from_value::<super::super::intent_routing::RoutingGroup>(json!({
        "id":"synthetic-group","name":"Shared route","revision":0,"client":"codex","enabled":true,
        "members":[{"id":"member","providerId":"native-a","modelId":"model-a"}],
        "defaultMemberId":"member","fallbackMemberIds":[],"intentRecognitionEnabled":false,
        "classifierTarget":null,"classificationBudgetMs":2000,"rules":[],"levels":[]
    }))?;
    let alias = group.alias();
    let menu = Arc::new(Menu {
        token: "synthetic-token".into(),
        default_model: Some(alias.clone()),
        omitted_entries: 0,
        context_settings: Default::default(),
        models: HashMap::from([(
            "synthetic-group".into(),
            MenuModel {
                alias: alias.clone(),
                label: "Shared route".into(),
                description: "VaryAI routing group",
                order: 0,
            },
        )]),
        groups: HashMap::from([(
            alias.clone(),
            Arc::new(Session {
                group,
                targets: vec![Target {
                    provider_id: "native-a".into(),
                    model: "model-a".into(),
                    fixed_effort: None,
                    upstream: None,
                    effort: Default::default(),
                    url: format!("{origin}/responses").parse()?,
                    headers: Default::default(),
                    official: false,
                    credentials: None,
                    scheduling: Arc::default(),
                }],
                classifier: None,
                client: reqwest::Client::builder().no_proxy().build()?,
                created: Instant::now(),
                bindings: Arc::default(),
            }),
        )]),
    });
    state
        .proxy_service
        .intent_runtime
        .set_global((*menu).clone(), state.db.routing_generation())?;
    state
        .proxy_service
        .intent_client
        .activate_menu_with_native(
            menu,
            &format!("{origin}/intent/v1"),
            Some(&format!("{origin}/backend-api/codex")),
            false,
        )?;
    state
        .proxy_service
        .agent_connections
        .set_codex_intent(Some(&alias), true)?;
    Ok((home, state, profile))
}

#[test]
#[serial_test::serial]
fn native_scheme_preview_describes_actual_codex_connection() -> TestResult {
    let (_home, state, profile) = setup()?;
    let preview = ProfileBundleService::preview(&state, &profile.id)?;
    assert_eq!(preview.changes[0].current_provider, "Vary AI");
    assert_eq!(preview.changes[0].current_models, vec!["Shared route"]);
    assert_eq!(preview.changes[0].target_provider, "Vendor A");
    assert_eq!(preview.changes[0].models, vec!["model-a"]);
    Ok(())
}

#[test]
#[serial_test::serial]
fn shared_scheme_preview_identifies_the_supplier_beside_the_model() -> TestResult {
    let (_home, state, _) = setup()?;
    let mut menu = (*state
        .proxy_service
        .intent_runtime
        .global_snapshot()?
        .ok_or("missing menu")?)
    .clone();
    let model = menu.models.values_mut().next().ok_or("missing model")?;
    let alias = model.alias.clone();
    model.label = "Identical model name".into();
    model.description = "VaryAI upstream model".into();
    let session = Arc::make_mut(menu.groups.get_mut(&alias).ok_or("missing session")?);
    session.targets[0].upstream = Some(Provider::with_id(
        "supplier".into(),
        "Configured supplier".into(),
        json!({}),
        None,
    ));
    state
        .proxy_service
        .intent_runtime
        .set_global(menu, state.db.routing_generation())?;
    for app in [
        AppType::Codex,
        AppType::Claude,
        AppType::Gemini,
        AppType::Pi,
    ] {
        let selection = ProfileSelection {
            app: app.clone(),
            provider_id: String::new(),
            models: json!({}),
            connection: None,
            providers: None,
            native_models: None,
            library: None,
        };
        let preview = shared::preview(
            &state,
            &selection,
            &shared::Connection {
                model: alias.clone(),
                visibility: None,
            },
        )?;
        assert_eq!(
            preview.models,
            vec!["Identical model name · Configured supplier"]
        );
        if app == AppType::Codex {
            assert_eq!(preview.current_models, preview.models);
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn routed_codex_can_apply_native_scheme_and_undo_reinstates_route() -> TestResult {
    let (_home, state, profile) = setup()?;
    let config = crate::codex_config::get_codex_config_path();
    let freshness = state
        .proxy_service
        .agent_connections
        .profile_paths("codex")?
        .into_iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name == "codex-startup-freshness.json")
        })
        .ok_or("missing startup snapshot path")?;
    let freshness_before = std::fs::read(&freshness)?;
    let before = std::fs::read(&config)?;
    let auth = std::fs::read(crate::codex_config::get_codex_auth_path())?;
    let preview = ProfileBundleService::preview(&state, &profile.id)?;
    let applied = ProfileBundleService::apply(&state, &profile.id, &preview.revision)?;
    assert!(applied.success, "{:?}", applied.failure);
    let document: toml::Value = std::fs::read_to_string(&config)?.parse()?;
    assert_eq!(document["model"].as_str(), Some("model-a"));
    assert!(!state.proxy_service.intent_client.active_or_pending());
    assert!(!state.proxy_service.intent_runtime.menu_active());
    assert_ne!(std::fs::read(&freshness)?, freshness_before);
    ProfileBundleService::undo(&state, applied.undo_id.as_deref().ok_or("missing undo")?)?;
    assert_eq!(std::fs::read(&config)?, before);
    assert_eq!(std::fs::read(&freshness)?, freshness_before);
    assert_eq!(
        std::fs::read(crate::codex_config::get_codex_auth_path())?,
        auth
    );
    assert!(
        state
            .proxy_service
            .intent_client
            .status()?
            .configuration_owned
    );
    assert!(state.proxy_service.intent_runtime.menu_active());
    assert!(state.proxy_service.intent_runtime.native.menu().is_ok());
    Ok(())
}

#[test]
#[serial_test::serial]
fn native_scheme_failure_restores_codex_route_and_receipts() -> TestResult {
    let (_home, state, profile) = setup()?;
    let config = crate::codex_config::get_codex_config_path();
    let before = std::fs::read(&config)?;
    let preview = ProfileBundleService::preview(&state, &profile.id)?;
    let mut native_applied = false;
    let result = ProfileBundleService::apply_using(
        &state,
        &profile.id,
        &preview.revision,
        |state, app, id| {
            ProviderService::switch_with_lock_held(state, app, id)?;
            native_applied = true;
            Err(error("operationFailed"))
        },
    )?;
    assert!(
        native_applied,
        "native selection was rejected before exercising rollback"
    );
    assert!(!result.success);
    assert_eq!(std::fs::read(&config)?, before);
    assert!(
        state
            .proxy_service
            .intent_client
            .status()?
            .configuration_owned
    );
    assert!(state.proxy_service.intent_runtime.menu_active());
    assert!(state.proxy_service.intent_runtime.native.menu().is_ok());
    Ok(())
}

#[test]
#[serial_test::serial]
fn native_scheme_mode_change_preserves_external_edit_conflicts() -> TestResult {
    let (_home, state, profile) = setup()?;
    let preview = ProfileBundleService::preview(&state, &profile.id)?;
    let config = crate::codex_config::get_codex_config_path();
    let edited = std::fs::read_to_string(&config)? + "\n# Synthetic external edit\n";
    std::fs::write(&config, &edited)?;
    assert!(ProfileBundleService::apply(&state, &profile.id, &preview.revision).is_err());
    assert_eq!(std::fs::read_to_string(&config)?, edited);
    assert!(state.proxy_service.intent_client.active_or_pending());
    Ok(())
}

#[test]
#[serial_test::serial]
fn native_scheme_undo_restores_existing_codex_conversation_provider() -> TestResult {
    for paginated in [false, true] {
        let (_home, state, profile) = setup()?;
        let root = crate::codex_config::get_codex_config_path()
            .parent()
            .ok_or("missing fixture directory")?
            .to_owned();
        let receipt_paths = state.proxy_service.intent_client.profile_paths()?;
        let receipt: Value = serde_json::from_slice(&std::fs::read(
            receipt_paths.get(1).ok_or("missing receipt fixture")?,
        )?)?;
        let original: toml::Value = receipt["original"]
            .as_str()
            .ok_or("missing original fixture")?
            .parse()?;
        let source = original
            .get("model_provider")
            .and_then(toml::Value::as_str)
            .unwrap_or("openai");
        let directory = root.join("sessions").join("2026").join("11").join("16");
        std::fs::create_dir_all(&directory)?;
        let id = "019a0000-0000-7000-8000-0000000000c1";
        let path = directory.join(format!("rollout-{id}.jsonl"));
        let contents = if paginated {
            format!(
                "{}\n",
                json!({"type":"event_msg","payload":{"message":"Synthetic suffix"}})
            )
        } else {
            format!(
                "{}\n{}\n",
                json!({"type":"session_meta","payload":{"id":id,"model_provider":source}}),
                json!({"type":"turn_context","payload":{"model":"model-a"}})
            )
        };
        std::fs::write(&path, contents)?;
        std::fs::File::options()
            .write(true)
            .open(&path)?
            .set_times(std::fs::FileTimes::new().set_modified(
                std::time::SystemTime::now() - std::time::Duration::from_secs(120),
            ))?;
        let db_path = root.join("state_5.sqlite");
        {
            let db = rusqlite::Connection::open(&db_path)?;
            db.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT NOT NULL, model_provider TEXT NOT NULL, model TEXT, history_mode TEXT NOT NULL DEFAULT 'legacy')")?;
            db.execute(
                "INSERT INTO threads VALUES (?1,?2,?3,'model-a',?4)",
                rusqlite::params![
                    id,
                    path.to_string_lossy(),
                    source,
                    if paginated { "paginated" } else { "legacy" }
                ],
            )?;
        }
        let provider = || -> Result<String, rusqlite::Error> {
            rusqlite::Connection::open(&db_path)?.query_row(
                "SELECT model_provider FROM threads WHERE id=?1",
                [id],
                |row| row.get(0),
            )
        };
        state.proxy_service.intent_client.sync_threads()?;
        assert_eq!(
            provider()?,
            "varyai",
            "fixture was not imported into routing"
        );
        let preview = ProfileBundleService::preview(&state, &profile.id)?;
        let applied = ProfileBundleService::apply(&state, &profile.id, &preview.revision)?;
        assert!(applied.success, "{:?}", applied.failure);
        assert_eq!(provider()?, source);
        ProfileBundleService::undo(&state, applied.undo_id.as_deref().ok_or("missing undo")?)?;
        assert_eq!(
            provider()?,
            "varyai",
            "undo left the existing conversation outside its restored route"
        );
    }
    Ok(())
}
