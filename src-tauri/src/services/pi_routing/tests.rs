use super::catalog::*;
use crate::provider::Provider;
use serde_json::json;

fn source(id: &str) -> Provider {
    Provider::with_id(
        id.into(),
        id.into(),
        json!({
            "api":"openai-completions", "baseUrl":"https://example.com/v1",
            "apiKey":"upstream-secret", "models":[{
                "id":"vendor/model:latest", "name":"Model", "contextWindow":123456,
                "maxTokens":4567, "compat":{"supportsStore":false}
            }]
        }),
        None,
    )
}

#[test]
fn pi_catalog_keeps_sources_distinct_and_exports_no_upstream_credentials() {
    let catalog = Catalog::build(
        &[source("a"), source("b")],
        &["a".into(), "b".into()],
        "http://127.0.0.1:12345",
        "local-token",
    )
    .expect("catalog");
    assert_eq!(catalog.targets.len(), 2);
    let exported = catalog.node.to_string();
    assert!(!exported.contains("upstream-secret"));
    assert!(!exported.contains("example.com"));
    for (alias, target) in &catalog.targets {
        assert!(alias.starts_with("vary_"));
        assert!(alias
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')));
        assert_eq!(target.model_id, "vendor/model:latest");
    }
    let model = &catalog.node["models"][0];
    assert_eq!(model["contextWindow"], 123456);
    assert_eq!(model["compat"]["supportsStore"], false);
    let other = Catalog::build(
        &[source("a")],
        &["a".into()],
        "http://127.0.0.1:23456",
        "another-local-token",
    )
    .expect("catalog");
    assert!(other
        .targets
        .keys()
        .all(|id| catalog.targets.contains_key(id)));
}

#[test]
fn pi_catalog_honors_model_protocol_endpoint_and_header_overrides() {
    let mut p = source("a");
    p.settings_config["headers"] = json!({"x-custom":"provider"});
    p.settings_config["models"][0]["api"] = json!("anthropic-messages");
    p.settings_config["models"][0]["baseUrl"] = json!("https://anthropic.example.com");
    p.settings_config["models"][0]["headers"] = json!({"x-custom":"model"});
    let c = Catalog::build(&[p], &["a".into()], "http://127.0.0.1:12345", "local-token")
        .expect("catalog");
    let target = c.targets.values().next().expect("target");
    assert_eq!(target.protocol, Protocol::Anthropic);
    assert_eq!(target.headers["x-custom"], "model");
    assert_eq!(c.node["models"][0]["baseUrl"], "http://127.0.0.1:12345/pi");
    assert_eq!(c.node["models"][0]["api"], "anthropic-messages");
}

#[test]
fn pi_catalog_rejects_dynamic_auth_unsupported_protocol_and_recursive_targets() {
    for config in [
        json!({"apiKey":"!credential-command"}),
        json!({"api":"bedrock-converse-stream"}),
        json!({"baseUrl":"http://127.0.0.1:12345/pi/v1"}),
        json!({"baseUrl":"http://localhost:12345/v1"}),
        json!({"modelOverrides":{"unknown":{"baseUrl":"https://other.example"}}}),
    ] {
        let mut p = source("a");
        for (k, v) in config.as_object().expect("object") {
            p.settings_config[k] = v.clone();
        }
        let c = Catalog::build(&[p], &["a".into()], "http://127.0.0.1:12345", "local-token")
            .expect("preview");
        assert!(c.targets.is_empty(), "unsafe model was exported");
        assert!(!c.issues.is_empty());
        assert!(!serde_json::to_string(&c.issues)
            .expect("issues")
            .contains("credential-command"));
    }
}

#[test]
#[serial_test::serial]
fn pi_manager_preserves_user_config_and_refuses_external_edits() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("settings");
    let manager = super::Manager::new(dir.path().join("pi-routing.json"));
    let db = crate::database::Database::memory().expect("db");
    db.save_provider("pi", &source("a")).expect("source");
    let other = json!({"api":"openai-completions","models":[{"id":"direct"}]});
    crate::pi_config::insert_pi_provider("other", &other).expect("other");
    manager
        .configure(&db, &["a".into()], "http://127.0.0.1:12345")
        .expect("enable");
    assert!(manager.enabled().expect("enabled"));
    let owned = crate::pi_config::read_pi_native_provider("a")
        .expect("read")
        .expect("owned");
    assert!(manager.owns_provider("a", &owned));
    let alias = alias("a", "vendor/model:latest");
    assert!(manager.owns_session("vary", &alias));
    let mut edited = owned.clone();
    edited["name"] = json!("User edit");
    crate::pi_config::replace_pi_provider("a", &owned, &edited).expect("external edit");
    assert_eq!(
        manager.disable().err().as_deref(),
        Some("piRouting.conflict")
    );
    assert_eq!(
        crate::pi_config::read_pi_native_provider("a").expect("read"),
        Some(edited.clone())
    );
    crate::pi_config::replace_pi_provider("a", &edited, &owned).expect("undo edit");
    manager.disable().expect("disable");
    assert!(manager.runtime.catalog.read().expect("runtime").is_none());
    assert!(
        manager.owns_session("vary", &alias),
        "historical identity retained"
    );
    assert_eq!(
        crate::pi_config::read_pi_native_provider("other").expect("read"),
        Some(other)
    );
    assert!(crate::pi_config::read_pi_native_provider("a")
        .expect("read")
        .is_none());
}

#[test]
#[serial_test::serial]
fn pi_source_deletion_revokes_new_requests_and_unowned_vary_is_preserved() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("settings");
    let manager = super::Manager::new(dir.path().join("pi-routing.json"));
    let db = crate::database::Database::memory().expect("db");
    db.save_provider("pi", &source("a")).expect("source");
    let user = json!({"api":"openai-completions","models":[{"id":"user"}]});
    crate::pi_config::insert_pi_provider("vary", &user).expect("user vary");
    assert!(manager
        .configure(&db, &["a".into()], "http://127.0.0.1:12345")
        .is_err());
    crate::pi_config::remove_pi_provider_if_matches("vary", &user).expect("remove user");
    manager
        .configure(&db, &["a".into()], "http://127.0.0.1:12345")
        .expect("enable");
    db.delete_provider("pi", "a").expect("delete");
    assert!(manager.refresh(&db, "http://127.0.0.1:12345").is_err());
    assert!(manager.runtime.catalog.read().expect("runtime").is_none());
}

#[test]
fn pi_alias_preserves_gemini_family_hints_and_chat_compatibility() {
    let id = alias("google", "gemini-2.5-pro");
    assert!(id.contains("gemini-2.5-pro"));
    let mut p = source("deepseek");
    p.settings_config["baseUrl"] = json!("https://api.deepseek.com/v1");
    let c = Catalog::build(
        &[p],
        &["deepseek".into()],
        "http://127.0.0.1:12345",
        "local-token",
    )
    .expect("catalog");
    assert_eq!(
        c.node["models"][0]["compat"]["maxTokensField"],
        "max_tokens"
    );
    assert_eq!(c.node["models"][0]["compat"]["thinkingFormat"], "deepseek");
    assert_eq!(
        c.node["models"][0]["compat"]["supportsDeveloperRole"],
        false
    );
}

#[tokio::test]
#[serial_test::serial]
async fn pi_listener_migration_retains_old_listener_on_bind_failure_and_app_timeouts() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let db = std::sync::Arc::new(crate::database::Database::memory().expect("db"));
    db.save_provider("pi", &source("a")).expect("source");
    let mut global = db.get_global_proxy_config().await.expect("global");
    global.listen_port = 0;
    db.update_global_proxy_config(global)
        .await
        .expect("random port");
    let mut codex = db.get_proxy_config_for_app("codex").await.expect("codex");
    codex.max_retries = 17;
    db.update_proxy_config_for_app(codex)
        .await
        .expect("timeouts");
    let service = crate::services::proxy::ProxyService::new(db.clone());
    service
        .configure_pi_routing(&["a".into()])
        .await
        .expect("enable");
    let previous = db.get_global_proxy_config().await.expect("previous");
    let old = crate::pi_config::read_pi_native_provider("a").expect("read");
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("occupied");
    let mut next = previous.clone();
    next.listen_port = occupied.local_addr().expect("port").port();
    assert_eq!(
        service.update_global_config(next).await.err().as_deref(),
        Some("piRouting.listenerFailed")
    );
    assert_eq!(
        db.get_global_proxy_config()
            .await
            .expect("unchanged")
            .listen_port,
        previous.listen_port
    );
    assert_eq!(
        crate::pi_config::read_pi_native_provider("a").expect("read"),
        old
    );
    assert!(
        reqwest::get(format!("http://127.0.0.1:{}/health", previous.listen_port))
            .await
            .expect("old alive")
            .status()
            .is_success()
    );
    let mut next = previous.clone();
    next.listen_port = 0;
    service
        .update_global_config(next)
        .await
        .expect("move listener");
    let actual = db
        .get_global_proxy_config()
        .await
        .expect("actual")
        .listen_port;
    assert_ne!(actual, 0);
    assert_ne!(actual, previous.listen_port);
    assert_eq!(
        db.get_proxy_config_for_app("codex")
            .await
            .expect("codex")
            .max_retries,
        17
    );
    let generated = crate::pi_config::read_pi_native_provider("a")
        .expect("read")
        .expect("vary");
    assert!(generated["baseUrl"]
        .as_str()
        .expect("url")
        .contains(&actual.to_string()));
    service
        .set_takeover_for_app("claude", false)
        .await
        .expect("other app stops");
    assert!(service.is_running().await, "Pi keeps shared listener alive");
    service.stop().await.expect("manual stop");
    assert!(service
        .pi_routing
        .runtime
        .catalog
        .read()
        .expect("runtime")
        .is_none());
    service
        .restore_pi_routing()
        .await
        .expect("restart restores enabled intent");
    assert!(service.pi_routing_status().await.expect("status").running);
    service.disable_pi_routing().await.expect("disable");
    service.stop().await.expect("stop");
}

#[test]
#[serial_test::serial]
fn pi_generated_provider_is_excluded_from_native_membership_and_import() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let state = crate::store::AppState::new(std::sync::Arc::new(
        crate::database::Database::memory().expect("db"),
    ));
    state.db.save_provider("pi", &source("a")).expect("source");
    state
        .proxy_service
        .pi_routing
        .configure(&state.db, &["a".into()], "http://127.0.0.1:12345")
        .expect("enable");
    let generated = Catalog::build(
        &[source("a")],
        &["a".into()],
        "http://127.0.0.1:12345",
        "fixture",
    )
    .expect("catalog")
    .node;
    state
        .db
        .save_provider(
            "pi",
            &Provider::with_id("vary".into(), "Generated".into(), generated, None),
        )
        .expect("previous import");
    let providers = crate::services::ProviderService::list(&state, crate::app_config::AppType::Pi)
        .expect("list");
    assert!(!providers.contains_key("vary"));
    assert!(!crate::services::pi_state::PiStateService::current(&state)
        .expect("current")
        .enabled_provider_ids
        .contains(&"vary".to_string()));
}

#[test]
#[serial_test::serial]
fn pi_pending_receipt_recovers_a_crash_between_atomic_writes() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let directory = tempfile::tempdir().expect("settings");
    let path = directory.path().join("pi-routing.json");
    let manager = super::Manager::new(path.clone());
    let db = crate::database::Database::memory().expect("db");
    db.save_provider("pi", &source("a")).expect("source");
    manager
        .configure(&db, &["a".into()], "http://127.0.0.1:12345")
        .expect("first");
    let mut pending = manager.read().expect("settings");
    pending.previous_providers = pending.providers.clone();
    let mut new_node = pending.providers["a"].clone();
    new_node["baseUrl"] = json!("http://127.0.0.1:23456/pi/v1");
    pending.providers.insert("a".into(), new_node);
    manager
        .write(&pending)
        .expect("pending persisted before crash");
    let restarted = super::Manager::new(path);
    restarted
        .refresh(&db, "http://127.0.0.1:23456")
        .expect("recover previous receipt");
    assert!(
        restarted
            .status(&db, true, "http://127.0.0.1:23456")
            .expect("status")
            .configured
    );
    restarted.disable().expect("disable");
}

#[tokio::test]
#[serial_test::serial]
async fn pi_shared_routing_toggle_uses_all_saved_sources_and_refreshes_new_sources() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let db = std::sync::Arc::new(crate::database::Database::memory().expect("db"));
    let mut global = db.get_global_proxy_config().await.expect("global");
    global.listen_port = 0;
    db.update_global_proxy_config(global)
        .await
        .expect("random port");
    let service = crate::services::proxy::ProxyService::new(db.clone());
    assert_eq!(
        service
            .set_takeover_for_app("pi", true)
            .await
            .err()
            .as_deref(),
        Some("piRouting.noAvailableModels")
    );
    assert!(
        !service.is_running().await,
        "empty sources must not start a listener"
    );
    db.save_provider("pi", &source("a")).expect("source");
    db.save_provider("pi", &source("b")).expect("source");
    service
        .set_takeover_for_app("pi", true)
        .await
        .expect("enable from shared control");
    assert!(service.get_takeover_status().await.expect("takeover").pi);
    assert_eq!(
        service
            .pi_routing_status()
            .await
            .expect("status")
            .model_count,
        2
    );
    db.save_provider("pi", &source("c")).expect("new source");
    // Provider mutations call this while holding the Pi switch lock.
    service.refresh_pi_routing_locked();
    assert_eq!(
        service
            .pi_routing_status()
            .await
            .expect("refreshed")
            .model_count,
        3
    );
    service
        .set_takeover_for_app("pi", false)
        .await
        .expect("disable from shared control");
    assert!(!service.pi_routing_status().await.expect("disabled").enabled);
    assert!(crate::pi_config::read_pi_native_provider("vary")
        .expect("read")
        .is_none());
    service
        .set_takeover_for_app("pi", true)
        .await
        .expect("enable again");
    service.stop_with_restore().await.expect("master stop");
    assert!(
        !service
            .pi_routing_status()
            .await
            .expect("master disabled")
            .enabled
    );
    assert!(!service.is_running().await);
    assert!(crate::pi_config::read_pi_native_provider("vary")
        .expect("read")
        .is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn pi_config_conflict_does_not_prevent_master_gateway_shutdown() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let db = std::sync::Arc::new(crate::database::Database::memory().expect("db"));
    let mut global = db.get_global_proxy_config().await.expect("global");
    global.listen_port = 0;
    db.update_global_proxy_config(global)
        .await
        .expect("random port");
    db.save_provider("pi", &source("a")).expect("source");
    let service = crate::services::proxy::ProxyService::new(db);
    service
        .set_takeover_for_app("pi", true)
        .await
        .expect("enable");
    let original = crate::pi_config::read_pi_native_provider("a")
        .expect("read")
        .expect("vary");
    let mut edited = original.clone();
    edited["name"] = json!("User edit");
    crate::pi_config::replace_pi_provider("a", &original, &edited).expect("edit");
    assert_eq!(
        service.stop_with_restore().await.err().as_deref(),
        Some("piRouting.conflict")
    );
    let running = service.is_running().await;
    if running {
        service.stop().await.expect("cleanup");
    }
    assert!(
        !running,
        "config cleanup errors must not prevent an explicit shutdown"
    );
    assert!(service
        .pi_routing
        .runtime
        .catalog
        .read()
        .expect("catalog")
        .is_none());
    assert_eq!(
        crate::pi_config::read_pi_native_provider("a").expect("read"),
        Some(edited)
    );
}

#[test]
#[serial_test::serial]
fn pi_routing_reuses_original_provider_and_model_and_restores_direct_config() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("settings");
    let manager = super::Manager::new(dir.path().join("pi-routing.json"));
    let db = crate::database::Database::memory().expect("db");
    let original = source("company");
    db.save_provider("pi", &original).expect("source");
    crate::pi_config::insert_pi_provider("company", &original.settings_config).expect("native");
    let settings = crate::pi_config::get_pi_settings_path().expect("settings path");
    std::fs::write(
        &settings,
        r#"{"defaultProvider":"company","defaultModel":"vendor/model:latest"}"#,
    )
    .expect("defaults");
    manager
        .configure(&db, &["company".into()], "http://127.0.0.1:12345")
        .expect("enable");
    let native = crate::pi_config::read_pi_native_providers().expect("providers");
    assert_eq!(native.len(), 1, "routing must not add a second provider");
    let routed = &native["company"];
    assert_eq!(routed["models"][0]["id"], "vendor/model:latest");
    assert_eq!(routed["models"][0]["name"], "Model");
    assert!(routed["baseUrl"]
        .as_str()
        .expect("url")
        .starts_with("http://127.0.0.1:12345/pi/"));
    assert!(!routed.to_string().contains("upstream-secret"));
    assert_eq!(
        db.get_provider_by_id("company", "pi")
            .expect("db")
            .expect("source")
            .settings_config,
        original.settings_config
    );
    manager
        .refresh(&db, "http://127.0.0.1:23456")
        .expect("port migration");
    manager.disable().expect("disable");
    assert_eq!(
        crate::pi_config::read_pi_native_provider("company").expect("restored"),
        Some(original.settings_config)
    );
    assert_eq!(
        std::fs::read_to_string(settings).expect("defaults"),
        r#"{"defaultProvider":"company","defaultModel":"vendor/model:latest"}"#
    );
}

#[test]
#[serial_test::serial]
fn pi_routing_migrates_the_legacy_duplicate_and_preserves_unrelated_nodes() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("settings");
    let path = dir.path().join("pi-routing.json");
    let db = crate::database::Database::memory().expect("db");
    let original = source("company");
    db.save_provider("pi", &original).expect("source");
    let legacy = Catalog::build(
        &[original.clone()],
        &["company".into()],
        "http://127.0.0.1:12345",
        "local-token",
    )
    .expect("catalog")
    .node;
    crate::pi_config::insert_pi_provider("company", &original.settings_config).expect("source");
    crate::pi_config::insert_pi_provider("vary", &legacy).expect("legacy");
    let other = json!({"futureField":true});
    crate::pi_config::insert_pi_provider("other", &other).expect("other");
    let settings = super::Settings {
        enabled: true,
        provider_ids: vec!["company".into()],
        directory: crate::pi_config::get_pi_agent_dir().expect("dir"),
        token: "local-token".into(),
        receipt: Some(legacy),
        known_models: vec![alias("company", "vendor/model:latest")],
        ..Default::default()
    };
    let manager = super::Manager::new(path);
    manager.write(&settings).expect("legacy settings");
    manager
        .refresh(&db, "http://127.0.0.1:23456")
        .expect("migrate");
    assert!(crate::pi_config::read_pi_native_provider("vary")
        .expect("read")
        .is_none());
    assert!(
        manager
            .status(&db, true, "http://127.0.0.1:23456")
            .expect("status")
            .configured
    );
    manager.disable().expect("disable");
    assert_eq!(
        crate::pi_config::read_pi_native_provider("company").expect("read"),
        Some(original.settings_config)
    );
    assert_eq!(
        crate::pi_config::read_pi_native_provider("other").expect("read"),
        Some(other)
    );
}

#[test]
#[serial_test::serial]
fn pi_routing_sync_and_edits_keep_the_saved_upstream_and_restore_latest_changes() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let state = crate::store::AppState::new(std::sync::Arc::new(
        crate::database::Database::memory().expect("db"),
    ));
    let original = source("company");
    state.db.save_provider("pi", &original).expect("source");
    crate::pi_config::insert_pi_provider("company", &original.settings_config).expect("native");
    state
        .proxy_service
        .pi_routing
        .configure(&state.db, &["company".into()], "http://127.0.0.1:12345")
        .expect("enable");
    let listed = crate::services::ProviderService::list(&state, crate::app_config::AppType::Pi)
        .expect("list");
    assert_eq!(listed["company"].settings_config, original.settings_config);
    assert_eq!(
        crate::services::pi_state::PiStateService::current(&state)
            .expect("membership")
            .enabled_provider_ids,
        vec!["company"]
    );
    let mut edited = original.clone();
    edited.settings_config["apiKey"] = json!("new-upstream-secret");
    state.db.save_provider("pi", &edited).expect("edit source");
    state
        .proxy_service
        .pi_routing
        .refresh(&state.db, "http://127.0.0.1:12345")
        .expect("refresh");
    state.proxy_service.pi_routing.disable().expect("disable");
    assert_eq!(
        crate::pi_config::read_pi_native_provider("company").expect("restored"),
        Some(edited.settings_config)
    );
}

#[test]
fn pi_native_provider_scopes_keep_identical_model_ids_distinct() {
    let catalog = Catalog::build(
        &[source("a"), source("b")],
        &["a".into(), "b".into()],
        "http://127.0.0.1:12345",
        "local-token",
    )
    .expect("catalog");
    assert_eq!(
        catalog.providers["a"]["models"][0]["id"],
        "vendor/model:latest"
    );
    assert_eq!(
        catalog.providers["b"]["models"][0]["id"],
        "vendor/model:latest"
    );
    assert_ne!(
        catalog.providers["a"]["baseUrl"],
        catalog.providers["b"]["baseUrl"]
    );
}

#[test]
#[serial_test::serial]
fn pi_batch_projection_conflict_does_not_partially_replace_providers() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let original = json!({"models":[{"id":"original"}]});
    crate::pi_config::insert_pi_provider("a", &original).expect("a");
    let snapshot = crate::pi_config::read_pi_native_providers().expect("read");
    let external = json!({"models":[{"id":"external"}]});
    crate::pi_config::insert_pi_provider("b", &external).expect("external edit");
    let mut replacement = snapshot.clone();
    replacement["a"] = json!({"models":[{"id":"replacement"}]});
    assert!(crate::pi_config::replace_pi_providers(&snapshot, &replacement).is_err());
    assert_eq!(
        crate::pi_config::read_pi_native_provider("a").expect("read"),
        Some(original)
    );
    assert_eq!(
        crate::pi_config::read_pi_native_provider("b").expect("read"),
        Some(external)
    );
}

// Simulate an older release importing routed models.json back into the source DB.
fn overwrite_with_projection(db: &crate::database::Database, id: &str) -> serde_json::Value {
    let projected = crate::pi_config::read_pi_native_provider(id)
        .expect("native")
        .expect("projection");
    db.update_provider_settings_config("pi", id, &projected)
        .expect("old import");
    projected
}

fn remove_source_snapshots(path: &std::path::Path) {
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("settings")).expect("json");
    settings
        .as_object_mut()
        .expect("object")
        .remove("sourceSnapshots");
    std::fs::write(path, serde_json::to_vec(&settings).expect("json")).expect("old settings");
}

#[test]
#[serial_test::serial]
fn pi_history_recovers_contaminated_source_from_original_without_changing_metadata() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("pi-routing.json");
    let manager = super::Manager::new(path.clone());
    let db = crate::database::Database::memory().expect("db");
    let mut original = source("company");
    original.notes = Some("user note".into());
    db.save_provider("pi", &original).expect("source");
    crate::pi_config::insert_pi_provider("company", &original.settings_config).expect("native");
    manager
        .configure(&db, &["company".into()], "http://127.0.0.1:12345")
        .expect("enable");
    let projection = overwrite_with_projection(&db, "company");
    remove_source_snapshots(&path);
    // A new process starts with an empty runtime and only the historical files.
    let restarted = super::Manager::new(path);
    restarted
        .refresh(&db, "http://127.0.0.1:12345")
        .expect("recover startup");
    let restored = db
        .get_provider_by_id("company", "pi")
        .expect("db")
        .expect("provider");
    assert_eq!(restored.settings_config, original.settings_config);
    assert_eq!(restored.notes, original.notes);
    assert_eq!(
        crate::pi_config::read_pi_native_provider("company").expect("native"),
        Some(projection)
    );
    assert_eq!(
        restarted
            .status(&db, true, "http://127.0.0.1:12345")
            .expect("status")
            .model_count,
        1
    );
    restarted
        .refresh(&db, "http://127.0.0.1:12345")
        .expect("idempotent");
    restarted.disable().expect("disable");
    assert_eq!(
        crate::pi_config::read_pi_native_provider("company").expect("restored"),
        Some(original.settings_config)
    );
}

#[test]
#[serial_test::serial]
fn pi_history_snapshots_sources_even_when_native_provider_was_absent() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("pi-routing.json");
    let manager = super::Manager::new(path.clone());
    let db = crate::database::Database::memory().expect("db");
    let original = source("company");
    db.save_provider("pi", &original).expect("source");
    manager
        .configure(&db, &["company".into()], "http://127.0.0.1:12345")
        .expect("enable");
    overwrite_with_projection(&db, "company");
    let restarted = super::Manager::new(path);
    restarted
        .refresh(&db, "http://127.0.0.1:12345")
        .expect("recover saved-only source");
    assert_eq!(
        db.get_provider_by_id("company", "pi")
            .expect("db")
            .expect("provider")
            .settings_config,
        original.settings_config
    );
    restarted.disable().expect("disable");
    assert!(
        crate::pi_config::read_pi_native_provider("company")
            .expect("native")
            .is_none(),
        "native membership must remain absent after disable"
    );
}

#[test]
#[serial_test::serial]
fn pi_history_preserves_unrecoverable_sources_while_restoring_healthy_routes() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("pi-routing.json");
    let manager = super::Manager::new(path.clone());
    let db = crate::database::Database::memory().expect("db");
    for id in ["healthy", "lost"] {
        let p = source(id);
        db.save_provider("pi", &p).expect("source");
        if id == "healthy" {
            crate::pi_config::insert_pi_provider(id, &p.settings_config).expect("native");
        }
    }
    manager
        .configure(
            &db,
            &["healthy".into(), "lost".into()],
            "http://127.0.0.1:12345",
        )
        .expect("enable");
    overwrite_with_projection(&db, "healthy");
    let lost = overwrite_with_projection(&db, "lost");
    remove_source_snapshots(&path);
    let restarted = super::Manager::new(path.clone());
    restarted
        .refresh(&db, "http://127.0.0.1:12345")
        .expect("healthy route restored");
    let status = restarted
        .status(&db, true, "http://127.0.0.1:12345")
        .expect("status");
    assert!(status.running);
    assert_eq!(status.model_count, 1);
    assert!(status
        .issues
        .iter()
        .any(|i| i.provider_id == "lost" && i.code == "piRouting.sourceRecoveryRequired"));
    assert_eq!(
        crate::pi_config::read_pi_native_provider("lost").expect("native"),
        Some(lost.clone())
    );
    assert_eq!(
        db.get_provider_by_id("lost", "pi")
            .expect("db")
            .expect("provider")
            .settings_config,
        lost
    );
    assert!(
        restarted
            .read()
            .expect("settings")
            .providers
            .contains_key("lost"),
        "keep receipt for later repair"
    );
    let mut repaired = source("lost");
    repaired.settings_config["apiKey"] = json!("new-key");
    db.save_provider("pi", &repaired).expect("manual repair");
    restarted
        .refresh(&db, "http://127.0.0.1:12345")
        .expect("repaired route");
    assert!(restarted
        .status(&db, true, "http://127.0.0.1:12345")
        .expect("status")
        .issues
        .is_empty());
}

#[test]
#[serial_test::serial]
fn pi_history_reports_missing_backups_without_guessing_or_rewriting_config() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("pi-routing.json");
    let manager = super::Manager::new(path.clone());
    let db = crate::database::Database::memory().expect("db");
    db.save_provider("pi", &source("lost")).expect("source");
    manager
        .configure(&db, &["lost".into()], "http://127.0.0.1:12345")
        .expect("enable");
    let lost = overwrite_with_projection(&db, "lost");
    remove_source_snapshots(&path);
    let restarted = super::Manager::new(path.clone());
    let before = std::fs::read(&path).expect("settings");
    assert_eq!(
        restarted
            .refresh(&db, "http://127.0.0.1:12345")
            .err()
            .as_deref(),
        Some("piRouting.sourceRecoveryRequired")
    );
    assert_eq!(std::fs::read(&path).expect("settings"), before);
    assert_eq!(
        crate::pi_config::read_pi_native_provider("lost").expect("native"),
        Some(lost)
    );
    assert_eq!(
        restarted
            .status(&db, true, "http://127.0.0.1:12345")
            .expect("status")
            .error_code
            .as_deref(),
        Some("piRouting.sourceRecoveryRequired")
    );
}

#[test]
#[serial_test::serial]
fn pi_history_never_overwrites_new_upstream_or_restores_deleted_sources() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("pi-routing.json");
    let manager = super::Manager::new(path.clone());
    let db = crate::database::Database::memory().expect("db");
    for id in ["edited", "deleted"] {
        db.save_provider("pi", &source(id)).expect("source");
    }
    manager
        .configure(
            &db,
            &["edited".into(), "deleted".into()],
            "http://127.0.0.1:12345",
        )
        .expect("enable");
    let mut edited = source("edited");
    edited.settings_config["baseUrl"] = json!("https://new.example.com/v1");
    edited.settings_config["apiKey"] = json!("new-secret");
    db.save_provider("pi", &edited).expect("edit");
    db.delete_provider("pi", "deleted").expect("delete");
    let restarted = super::Manager::new(path);
    restarted
        .refresh(&db, "http://127.0.0.1:12345")
        .expect("refresh");
    assert_eq!(
        db.get_provider_by_id("edited", "pi")
            .expect("db")
            .expect("provider")
            .settings_config,
        edited.settings_config
    );
    assert!(db
        .get_provider_by_id("deleted", "pi")
        .expect("db")
        .is_none());
}

#[test]
#[serial_test::serial]
fn pi_history_does_not_trust_modified_projection_or_local_backup() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("pi-routing.json");
    let manager = super::Manager::new(path.clone());
    let db = crate::database::Database::memory().expect("db");
    let original = source("company");
    db.save_provider("pi", &original).expect("source");
    crate::pi_config::insert_pi_provider("company", &original.settings_config).expect("native");
    manager
        .configure(&db, &["company".into()], "http://127.0.0.1:12345")
        .expect("enable");
    let projection = overwrite_with_projection(&db, "company");
    remove_source_snapshots(&path);
    let mut modified = projection.clone();
    modified["apiKey"] = json!("user-edited-key");
    db.update_provider_settings_config("pi", "company", &modified)
        .expect("external edit");
    assert_eq!(
        manager
            .refresh(&db, "http://127.0.0.1:12345")
            .err()
            .as_deref(),
        Some("piRouting.sourceRecoveryRequired")
    );
    assert_eq!(
        db.get_provider_by_id("company", "pi")
            .expect("db")
            .expect("provider")
            .settings_config,
        modified
    );
    db.update_provider_settings_config("pi", "company", &projection)
        .expect("undo");
    let mut settings = manager.read().expect("settings");
    settings
        .originals
        .insert("company".into(), Some(projection.clone()));
    manager.write(&settings).expect("bad old backup");
    assert_eq!(
        manager
            .refresh(&db, "http://127.0.0.1:12345")
            .err()
            .as_deref(),
        Some("piRouting.sourceRecoveryRequired")
    );
    assert_eq!(
        db.get_provider_by_id("company", "pi")
            .expect("db")
            .expect("provider")
            .settings_config,
        projection
    );
}

#[test]
fn pi_history_detects_local_model_overrides_without_rejecting_other_local_services() {
    let mut original = source("company");
    original.settings_config["models"][0]["baseUrl"] = json!("http://localhost:12345/pi/v1");
    assert!(is_local_projection(&original.settings_config));
    original.settings_config["models"][0]["baseUrl"] = json!("http://localhost:11434/v1");
    assert!(!is_local_projection(&original.settings_config));
}

#[test]
#[serial_test::serial]
fn pi_history_import_and_list_repair_sources_before_showing_upstream_urls() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let state = crate::store::AppState::new(std::sync::Arc::new(
        crate::database::Database::memory().expect("db"),
    ));
    let original = source("company");
    state.db.save_provider("pi", &original).expect("source");
    crate::pi_config::insert_pi_provider("company", &original.settings_config).expect("native");
    let manager = &state.proxy_service.pi_routing;
    manager
        .configure(&state.db, &["company".into()], "http://127.0.0.1:12345")
        .expect("enable");
    overwrite_with_projection(&state.db, "company");
    assert_eq!(
        crate::services::provider::import_pi_providers_from_live(&state).expect("startup import"),
        1
    );
    let listed = crate::services::ProviderService::list(&state, crate::app_config::AppType::Pi)
        .expect("list");
    assert_eq!(listed["company"].settings_config, original.settings_config);
    assert_eq!(
        crate::services::provider::import_pi_providers_from_live(&state).expect("repeat import"),
        0
    );
}

#[test]
#[serial_test::serial]
#[cfg(unix)]
fn pi_history_backup_write_failure_keeps_database_unchanged_and_retryable() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("pi-routing.json");
    let manager = super::Manager::new(path.clone());
    let db = crate::database::Database::memory().expect("db");
    let original = source("company");
    db.save_provider("pi", &original).expect("source");
    manager
        .configure(&db, &["company".into()], "http://127.0.0.1:12345")
        .expect("enable");
    let projection = overwrite_with_projection(&db, "company");
    // A readable path whose parent cannot be used for an atomic replacement.
    // /dev/fd aliases stay readable but are not writable as config destinations.
    let file = std::fs::File::open(&path).expect("settings");
    use std::os::fd::AsRawFd;
    let unwritable = super::Manager::new(std::path::PathBuf::from(format!(
        "/dev/fd/{}",
        file.as_raw_fd()
    )));
    assert!(unwritable.recover_sources(&db).is_err());
    assert_eq!(
        db.get_provider_by_id("company", "pi")
            .expect("db")
            .expect("provider")
            .settings_config,
        projection
    );
    manager.recover_sources(&db).expect("retry");
    assert_eq!(
        db.get_provider_by_id("company", "pi")
            .expect("db")
            .expect("provider")
            .settings_config,
        original.settings_config
    );
    assert_eq!(
        manager.read().expect("private backup").recovery_backup["company"],
        projection
    );
}

#[test]
#[serial_test::serial]
fn pi_history_reports_orphaned_saved_projections_even_before_routing_is_enabled() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("dir");
    let manager = super::Manager::new(dir.path().join("pi-routing.json"));
    let db = crate::database::Database::memory().expect("db");
    let original = source("orphaned");
    let catalog = Catalog::build(
        &[original.clone()],
        &["orphaned".into()],
        "http://127.0.0.1:12345",
        "local-token",
    )
    .expect("catalog");
    let mut orphaned = original;
    orphaned.settings_config = catalog.providers["orphaned"].clone();
    db.save_provider("pi", &orphaned)
        .expect("old imported projection");
    let status = manager
        .status(&db, false, "http://127.0.0.1:12345")
        .expect("status");
    assert!(status
        .issues
        .iter()
        .any(|issue| issue.provider_id == "orphaned"
            && issue.code == "piRouting.sourceRecoveryRequired"));
}

#[test]
fn pi_history_stale_repair_cannot_overwrite_a_concurrent_user_edit() {
    let db = crate::database::Database::memory().expect("db");
    let original = source("company");
    let mut stale = original.settings_config.clone();
    stale["baseUrl"] = json!("http://127.0.0.1:12345/pi/v1");
    db.save_provider("pi", &original)
        .expect("newer user configuration");
    assert!(!db
        .update_provider_settings_config_if_matches(
            "pi",
            "company",
            &stale,
            &source("older").settings_config
        )
        .expect("stale repair"));
    assert_eq!(
        db.get_provider_by_id("company", "pi")
            .expect("db")
            .expect("provider")
            .settings_config,
        original.settings_config
    );
}

#[tokio::test]
#[serial_test::serial]
async fn pi_history_restored_gateway_forwards_with_original_credentials_after_restart() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("upstream");
    let addr = listener.local_addr().expect("address");
    let app = axum::Router::new().route("/v1/chat/completions", axum::routing::post(
        |headers: http::HeaderMap, axum::Json(body): axum::Json<serde_json::Value>| async move {
            assert_eq!(headers["authorization"], "Bearer upstream-secret");
            assert_eq!(body["model"], "vendor/model:latest");
            axum::Json(json!({"id":"test","choices":[{"message":{"role":"assistant","content":"restored"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}}))
        }
    ));
    let upstream = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("pi-routing.json");
    let db = std::sync::Arc::new(crate::database::Database::memory().expect("db"));
    let mut original = source("company");
    original.settings_config["baseUrl"] = json!(format!("http://{addr}/v1"));
    db.save_provider("pi", &original).expect("source");
    crate::pi_config::insert_pi_provider("company", &original.settings_config).expect("native");
    let manager = super::Manager::new(path.clone());
    manager
        .configure(&db, &["company".into()], "http://127.0.0.1:12345")
        .expect("previous release");
    let projection = overwrite_with_projection(&db, "company");
    remove_source_snapshots(&path);
    let restarted = super::Manager::new(path);
    let gateway = crate::proxy::server::ProxyServer::new(
        crate::proxy::ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    )
    .with_pi_runtime(restarted.runtime.clone());
    let info = gateway.start().await.expect("gateway");
    let origin = format!("http://127.0.0.1:{}", info.port);
    let endpoint = format!("{origin}/pi/p/{}/v1/chat/completions", alias("company", ""));
    let client = reqwest::Client::new();
    let send = || {
        client.post(&endpoint).bearer_auth(projection["apiKey"].as_str().expect("token"))
        .json(&json!({"model":"vendor/model:latest","messages":[{"role":"user","content":"hello"}]}))
    };
    assert_eq!(
        send()
            .send()
            .await
            .expect("request before repair")
            .status()
            .as_u16(),
        503
    );
    // The same startup refresh used by ProxyService rebuilds the runtime.
    restarted.refresh(&db, &origin).expect("restore history");
    let response = send().send().await.expect("request after repair");
    assert_eq!(response.status().as_u16(), 200);
    assert!(response
        .text()
        .await
        .expect("response")
        .contains("restored"));
    gateway.stop().await.expect("stop");
    upstream.abort();
}

#[test]
#[serial_test::serial]
fn pi_history_recovery_preserves_saved_display_name_in_restored_native_config() {
    let _agent = crate::pi_config::test_support::TestAgentDir::new();
    let dir = tempfile::tempdir().expect("dir");
    let manager = super::Manager::new(dir.path().join("pi-routing.json"));
    let db = crate::database::Database::memory().expect("db");
    let mut original = source("company");
    original.settings_config["name"] = json!("Company");
    db.save_provider("pi", &original).expect("source");
    crate::pi_config::insert_pi_provider("company", &original.settings_config).expect("native");
    manager
        .configure(&db, &["company".into()], "http://127.0.0.1:12345")
        .expect("enable");
    let mut renamed = original;
    renamed.name = "Company backup".into();
    db.save_provider("pi", &renamed)
        .expect("saved display name");
    overwrite_with_projection(&db, "company");
    manager
        .refresh(&db, "http://127.0.0.1:12345")
        .expect("recover");
    manager.disable().expect("restore native config");
    let listed = db
        .get_provider_by_id("company", "pi")
        .expect("db")
        .expect("provider");
    assert_eq!(listed.name, "Company backup");
    assert_eq!(listed.settings_config["name"], "Company backup");
    assert_eq!(
        crate::pi_config::read_pi_native_provider("company")
            .expect("native")
            .expect("provider")["name"],
        "Company backup"
    );
}
