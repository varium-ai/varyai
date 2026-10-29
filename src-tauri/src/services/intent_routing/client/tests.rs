use super::*;
use serde_json::json;

fn setup() -> (tempfile::TempDir, Manager) {
    let dir = tempfile::tempdir().expect("fixture directory");
    let manager = Manager::new(
        dir.path().join("config.toml"),
        dir.path().join("receipt.json"),
        Arc::default(),
    );
    (dir, manager)
}
fn group() -> RoutingGroup {
    super::super::tests::fixture()
}

#[test]
fn menu_catalog_has_distinct_aliases_and_human_names() {
    let a = group();
    let mut b = a.clone();
    b.id = "second".into();
    b.name = "Second".into();
    let catalog = catalog(&[a, b], &Default::default(), "", &Default::default()).expect("catalog");
    assert_eq!(catalog["models"][0]["slug"], "vary-auto/development");
    assert_eq!(
        catalog["models"][1]["display_name"],
        "Second · routing group"
    );
    assert!(catalog["models"][0]["base_instructions"].is_string());
    assert_eq!(catalog["models"][0]["shell_type"], "shell_command");
    // Unknown capabilities must not inherit the unrelated binary-thinking template.
    assert_eq!(
        catalog["models"][0]["supported_reasoning_levels"],
        json!([])
    );
    assert_eq!(catalog["models"][0]["supports_reasoning_summaries"], false);
}

#[test]
fn restores_original_config_without_touching_auth_or_unrelated_edits() {
    let (dir, manager) = setup();
    let original = "# user comment\nmodel = 'original'\nmodel_provider = 'custom'\n[model_providers.custom]\nbase_url = 'https://synthetic.example/v1'\n";
    fs::write(&manager.config, original).expect("config");
    let auth = dir.path().join("auth.json");
    fs::write(&auth, "synthetic-auth").expect("auth");
    manager
        .write_projection(
            &[group()],
            "synthetic-local-token",
            "http://127.0.0.1:1234/intent/v1",
        )
        .expect("project");
    let active = fs::read_to_string(&manager.config).expect("active");
    assert!(active.contains("varyai-routing"));
    fs::write(&manager.config, format!("new_setting = true\n{active}")).expect("external edit");
    manager.restore().expect("restore");
    let restored: toml::Value = fs::read_to_string(&manager.config)
        .expect("restored")
        .parse()
        .expect("toml");
    assert_eq!(restored["model"].as_str(), Some("original"));
    assert_eq!(restored["new_setting"].as_bool(), Some(true));
    assert!(restored["model_providers"].get(PROVIDER).is_none());
    assert_eq!(fs::read_to_string(auth).expect("auth"), "synthetic-auth");
    assert!(!manager.receipt.exists());
}

#[test]
fn recovery_is_durable_and_preserves_newer_owned_field_edits() {
    let (_dir, manager) = setup();
    fs::write(&manager.config, "model = 'original'\n").expect("config");
    manager
        .write_projection(&[group()], "local-token", "http://127.0.0.1:1234/intent/v1")
        .expect("project");
    let mut doc: DocumentMut = fs::read_to_string(&manager.config)
        .expect("active")
        .parse()
        .expect("toml");
    doc["model"] = toml_edit::value("user-new-model");
    fs::write(&manager.config, doc.to_string()).expect("external edit");
    let restarted = Manager::new(
        manager.config.clone(),
        manager.receipt.clone(),
        Arc::default(),
    );
    restarted.restore().expect("recover");
    let text = fs::read_to_string(&manager.config).expect("restored");
    assert!(text.contains("user-new-model"));
    assert!(!text.contains("varyai-routing"));
}

#[test]
fn updating_catalog_keeps_original_backup_and_detects_external_takeover() {
    let (_dir, manager) = setup();
    let original = "model = 'original'\n";
    fs::write(&manager.config, original).expect("config");
    manager
        .write_projection(&[group()], "token-one", "http://127.0.0.1:1234/intent/v1")
        .expect("first");
    let mut second = group();
    second.id = "second".into();
    manager
        .write_projection(&[second], "token-two", "http://127.0.0.1:1234/intent/v1")
        .expect("second");
    manager.restore().expect("restore");
    assert_eq!(
        fs::read_to_string(&manager.config).expect("config"),
        original
    );
    manager
        .write_projection(&[group()], "token", "http://127.0.0.1:1234/intent/v1")
        .expect("first");
    fs::write(&manager.config, "model_provider='external'\n").expect("external");
    assert!(manager
        .write_projection(&[group()], "new", "http://127.0.0.1:1234/intent/v1")
        .is_err());
}

#[test]
fn unchanged_menu_refresh_reuses_catalog_without_exhausting_publications() {
    let (_dir, manager) = setup();
    fs::write(&manager.config, "model='original'\n").expect("config");
    for _ in 0..70 {
        manager
            .write_projection(&[group()], "same-token", "http://127.0.0.1:1234/intent/v1")
            .expect("unchanged refresh");
    }
    assert_eq!(
        manager
            .load()
            .expect("receipt")
            .expect("active")
            .catalogs
            .len(),
        1
    );
    manager.restore().expect("restore");
}

#[test]
fn compare_write_refuses_newer_file_and_does_not_leak_token_to_status() {
    let (_dir, manager) = setup();
    fs::write(&manager.config, "newer").expect("config");
    assert!(replace_checked(&manager.config, Some("older"), Some("replacement")).is_err());
    assert_eq!(fs::read_to_string(&manager.config).expect("file"), "newer");
    fs::write(&manager.config, "").expect("config");
    manager
        .write_projection(
            &[group()],
            "synthetic-local-secret",
            "http://127.0.0.1:1234/intent/v1",
        )
        .expect("project");
    let status = serde_json::to_value(manager.status().expect("status")).expect("json");
    assert!(!status.to_string().contains("synthetic-local-secret"));
    assert_eq!(status["groupIds"], json!(["development"]));
}

#[test]
fn no_original_file_and_private_receipt_are_recoverable() {
    let (_dir, manager) = setup();
    manager
        .write_projection(
            &[group()],
            "local-secret",
            "http://127.0.0.1:1234/intent/v1",
        )
        .expect("project");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&manager.config)
                .expect("config")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&manager.receipt)
                .expect("receipt")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    manager.restore().expect("recover");
    assert!(!manager.config.exists());
}

#[test]
fn partial_update_recovery_restores_old_selected_alias_and_preserves_comments() {
    let (_dir, manager) = setup();
    fs::write(&manager.config, "model = 'original'\n").expect("config");
    let a = group();
    let mut b = a.clone();
    b.id = "other".into();
    manager
        .write_projection(&[a, b.clone()], "old", "http://127.0.0.1:1234/intent/v1")
        .expect("first");
    let mut doc: DocumentMut = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    doc["model"] = toml_edit::value("vary-auto/other");
    let old = format!("# user edit\n{doc}");
    fs::write(&manager.config, &old).expect("selection");
    manager
        .write_projection(&[group()], "new", "http://127.0.0.1:1234/intent/v1")
        .expect("update");
    // Simulate a crash after journaling but before replacing the config.
    fs::write(&manager.config, old).expect("partial update");
    manager.restore().expect("recover");
    let text = fs::read_to_string(&manager.config).expect("config");
    assert!(text.contains("# user edit"));
    assert!(!text.contains("vary-auto/"));
    assert!(text.contains("original"));
}

#[tokio::test]
async fn gateway_stop_recovers_menu_config_without_an_active_server() {
    let db = Arc::new(crate::database::Database::memory().expect("database"));
    let service = crate::services::ProxyService::new(db);
    let manager = &service.intent_client;
    fs::create_dir_all(manager.config.parent().expect("parent")).expect("directory");
    fs::write(&manager.config, "model='original'\n").expect("config");
    manager
        .write_projection(&[group()], "token", "http://127.0.0.1:1234/intent/v1")
        .expect("project");
    assert!(service.set_takeover_for_app("codex", true).await.is_err());
    let _ = service.stop().await;
    assert_eq!(
        fs::read_to_string(&manager.config).expect("restored"),
        "model='original'\n"
    );
    assert!(!manager.receipt.exists());
}

#[test]
fn missing_config_after_interrupted_install_recovers_journaled_original() {
    let (_dir, manager) = setup();
    fs::write(&manager.config, "model='original'\n").expect("original");
    manager
        .write_projection(&[group()], "token", "http://127.0.0.1:1234/intent/v1")
        .expect("project");
    fs::remove_file(&manager.config).expect("simulate interrupted replacement");
    manager.restore().expect("recovery");
    assert_eq!(
        fs::read_to_string(&manager.config).expect("config"),
        "model='original'\n"
    );
}

#[test]
fn menu_exports_shared_levels_and_preserves_user_default() {
    let capabilities = [(
        group().id,
        vec!["low".into(), "medium".into(), "high".into(), "xhigh".into()],
    )]
    .into_iter()
    .collect();
    let value = catalog(
        &[group()],
        &capabilities,
        "xhigh",
        &[(group().id, true)].into_iter().collect(),
    )
    .expect("catalog");
    assert_eq!(
        value["models"][0]["supported_reasoning_levels"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(value["models"][0]["default_reasoning_level"], "xhigh");
    assert_eq!(value["models"][0]["supports_reasoning_summaries"], true);
    assert_eq!(
        value["models"][0]["input_modalities"],
        json!(["text", "image"])
    );
}
