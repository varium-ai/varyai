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
fn menu_publication_offers_named_levels_instead_of_the_intersection() {
    let mut group = group();
    group.levels = vec!["low".into(), "max".into()];
    let projection = catalog(
        std::slice::from_ref(&group),
        &std::collections::HashMap::from([(group.id.clone(), vec!["high".into()])]),
        "max",
        &Default::default(),
    )
    .expect("named levels");
    assert_eq!(
        projection["models"][0]["supported_reasoning_levels"],
        json!([
            {"effort":"low","description":"low"},
            {"effort":"max","description":"max"}
        ])
    );
    assert_eq!(projection["models"][0]["default_reasoning_level"], "max");
}

#[test]
fn projection_keeps_openai_login_visible_without_using_it_for_forwarding() {
    for auth in [
        json!({"auth_mode":"chatgpt", "tokens":{"access_token":"synthetic-oauth"}}),
        json!({"auth_mode":"chatgptAuthTokens", "tokens":{"access_token":"synthetic-oauth"}}),
        json!({"OPENAI_API_KEY":"synthetic-official-key"}),
    ] {
        let (dir, manager) = setup();
        let original = "model_provider='openai'\nmodel='original'\n";
        fs::write(&manager.config, original).expect("config fixture");
        let auth_path = dir.path().join("auth.json");
        let auth_bytes = serde_json::to_vec(&auth).expect("auth fixture");
        fs::write(&auth_path, &auth_bytes).expect("auth fixture");
        for _ in 0..2 {
            manager
                .write_projection(
                    &[group()],
                    "synthetic-local-token",
                    "http://127.0.0.1:1234/intent/v1",
                )
                .expect("publish and refresh");
            let installed: toml::Value = fs::read_to_string(&manager.config)
                .expect("installed config")
                .parse()
                .expect("installed toml");
            let provider = &installed["model_providers"][PROVIDER];
            assert_eq!(provider["requires_openai_auth"].as_bool(), Some(true));
            assert_eq!(
                provider["experimental_bearer_token"].as_str(),
                Some("synthetic-local-token")
            );
            assert_eq!(fs::read(&auth_path).expect("unchanged auth"), auth_bytes);
        }
        manager.restore().expect("restore");
        assert_eq!(
            fs::read_to_string(&manager.config).expect("restored"),
            original
        );
        assert_eq!(fs::read(auth_path).expect("unchanged auth"), auth_bytes);
    }
}

#[test]
fn projection_does_not_require_a_missing_or_unsupported_login() {
    for auth in [
        None,
        Some("not-json".to_owned()),
        Some(json!({"last_refresh":"synthetic-metadata"}).to_string()),
        Some(json!({"OPENAI_API_KEY":"PROXY_MANAGED"}).to_string()),
        Some(json!({"bedrock_api_key":"synthetic-bedrock"}).to_string()),
        Some(json!({"auth_mode":"headers", "tokens":{"access_token":"stale"}}).to_string()),
    ] {
        let (dir, manager) = setup();
        let path = dir.path().join("auth.json");
        if let Some(text) = &auth {
            fs::write(&path, text).expect("auth fixture");
        }
        manager
            .write_projection(
                &[group()],
                "synthetic-local-token",
                "http://127.0.0.1:1234/intent/v1",
            )
            .expect("keyless publication");
        let installed: toml::Value = fs::read_to_string(&manager.config)
            .expect("installed config")
            .parse()
            .expect("installed toml");
        assert_eq!(
            installed["model_providers"][PROVIDER]["requires_openai_auth"].as_bool(),
            Some(false)
        );
        assert_eq!(read(&path).expect("auth state"), auth);
    }
}

#[test]
fn projection_refresh_repairs_legacy_flag_and_tracks_file_login_changes() {
    let (dir, manager) = setup();
    let token = "synthetic-local-token";
    let base = "http://127.0.0.1:1234/intent/v1";
    manager
        .write_projection(&[group()], token, base)
        .expect("legacy signed-out projection");
    let mut receipt = manager.load().expect("receipt").expect("owned projection");
    let mut installed: DocumentMut = receipt.installed.parse().expect("toml");
    installed["model_providers"][PROVIDER]["requires_openai_auth"] = toml_edit::value(false);
    receipt.installed = installed.to_string();
    fs::write(&manager.config, &receipt.installed).expect("legacy config");
    fs::write(
        &manager.receipt,
        serde_json::to_vec(&receipt).expect("receipt json"),
    )
    .expect("legacy receipt");
    let auth = dir.path().join("auth.json");
    fs::write(
        &auth,
        json!({"tokens":{"access_token":"synthetic-oauth"}}).to_string(),
    )
    .expect("login fixture");
    for expected in [true, false] {
        manager
            .write_projection(&[group()], token, base)
            .expect("refresh owned projection");
        let installed: toml::Value = fs::read_to_string(&manager.config)
            .expect("config")
            .parse()
            .expect("toml");
        assert_eq!(
            installed["model_providers"][PROVIDER]["requires_openai_auth"].as_bool(),
            Some(expected)
        );
        if expected {
            fs::remove_file(&auth).expect("simulate native logout");
        }
    }
}

#[test]
fn projection_respects_auth_storage_instead_of_assuming_file_login() {
    for (store, previous, expected) in [
        ("file", false, true),
        ("ephemeral", true, false),
        ("auto", true, true),
        ("keyring", true, true),
        ("keyring", false, false),
    ] {
        let (dir, manager) = setup();
        fs::write(
            &manager.config,
            format!("cli_auth_credentials_store='{store}'\nmodel_provider='custom'\n[model_providers.custom]\nrequires_openai_auth={previous}\n"),
        )
        .expect("config fixture");
        fs::write(
            dir.path().join("auth.json"),
            json!({"tokens":{"access_token":"synthetic-oauth"}}).to_string(),
        )
        .expect("auth fixture");
        manager
            .write_projection(
                &[group()],
                "synthetic-local-token",
                "http://127.0.0.1:1234/intent/v1",
            )
            .expect("publish");
        let installed: toml::Value = fs::read_to_string(&manager.config)
            .expect("config")
            .parse()
            .expect("toml");
        assert_eq!(
            installed["model_providers"][PROVIDER]["requires_openai_auth"].as_bool(),
            Some(expected),
            "{store}"
        );
    }
}

#[test]
fn orphan_cleanup_recognizes_login_preserving_projection() {
    let (_dir, manager) = setup();
    orphan(&manager);
    let mut doc: DocumentMut = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    doc["model_providers"][PROVIDER]["requires_openai_auth"] = toml_edit::value(true);
    fs::write(&manager.config, doc.to_string()).expect("login-preserving orphan");
    assert!(manager.status().expect("status").orphan_cleanup_available);
    manager.cleanup_orphan(true).expect("explicit cleanup");
    assert!(!manager.status().expect("status").orphaned_config);
}

fn orphan(manager: &Manager) -> String {
    let original = "# private fixture\nmodel='user-model'\nmodel_provider='custom'\n[model_providers.custom]\nbase_url='https://synthetic.example/v1'\n";
    fs::write(&manager.config, original).expect("config");
    manager
        .write_projection(
            &[group()],
            "synthetic-orphan-token",
            "http://127.0.0.1:1234/intent/v1",
        )
        .expect("project");
    let mut doc: DocumentMut = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    doc["model"] = toml_edit::value("user-model");
    doc["model_provider"] = toml_edit::value("custom");
    doc.as_table_mut().remove("model_catalog_json");
    doc.as_table_mut().remove("web_search");
    let text = doc.to_string();
    fs::write(&manager.config, &text).expect("switched config");
    fs::remove_file(&manager.receipt).expect("lost receipt");
    text
}

#[test]
fn orphaned_inactive_projection_has_explicit_private_backup_cleanup() {
    let (dir, manager) = setup();
    let before = orphan(&manager);
    let auth = dir.path().join("auth.json");
    fs::write(&auth, "synthetic-login").expect("auth");
    let status = manager.status().expect("status");
    assert!(!status.enabled);
    assert!(status.recovery_pending && status.orphaned_config && status.orphan_cleanup_available);
    assert!(!serde_json::to_string(&status)
        .expect("status json")
        .contains("synthetic-orphan-token"));
    assert_eq!(fs::read_to_string(&manager.config).expect("config"), before);
    // Ordinary shutdown without a receipt must not clean up unowned configuration.
    manager.restore().expect("no receipt");
    assert_eq!(fs::read_to_string(&manager.config).expect("config"), before);
    assert!(manager.cleanup_orphan(false).is_err());
    manager.cleanup_orphan(true).expect("explicit cleanup");
    let after = fs::read_to_string(&manager.config).expect("config");
    let mut expected: DocumentMut = before.parse().expect("toml");
    expected["model_providers"]
        .as_table_like_mut()
        .expect("providers")
        .remove(PROVIDER);
    assert_eq!(after, expected.to_string());
    assert_eq!(fs::read_to_string(auth).expect("auth"), "synthetic-login");
    let backups = fs::read_dir(dir.path())
        .expect("files")
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("vary-routing-orphan-backup-")
        })
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        fs::read_to_string(backups[0].path()).expect("backup"),
        before
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(backups[0].path())
                .expect("mode")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let status = manager.status().expect("cleaned status");
    assert!(!status.recovery_pending && !status.orphaned_config);
    manager
        .write_projection(
            &[group()],
            "new-local-token",
            "http://127.0.0.1:1234/intent/v1",
        )
        .expect("reconnect");
    manager.restore().expect("restore");
    assert_eq!(fs::read_to_string(&manager.config).expect("config"), after);
}

#[test]
fn explicit_disconnect_removes_receiptless_routes_and_keeps_provider_and_login(
) -> Result<(), Box<dyn std::error::Error>> {
    for profile in [false, true] {
        let (dir, manager) = setup();
        fs::write(&manager.config, "model='original'\ntheme='keep-user'\n")?;
        let auth = dir.path().join("auth.json");
        fs::write(&auth, "synthetic-login")?;
        manager.write_projection_with_native(
            &[group()],
            "synthetic-disconnect-token",
            "http://127.0.0.1:1234/intent/v1",
            Some("http://127.0.0.1:1234/backend-api/codex"),
        )?;
        let mut doc: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
        if profile {
            doc["profile"] = toml_edit::value("work");
            doc["profiles"]["work"]["model_provider"] = toml_edit::value(PROVIDER);
            doc["profiles"]["work"]["model"] = doc["model"].clone();
            doc["profiles"]["work"]["model_catalog_json"] = doc["model_catalog_json"].clone();
            doc["profiles"]["other"]["model"] = toml_edit::value("user-other");
        }
        let before = doc.to_string();
        fs::write(&manager.config, &before)?;
        fs::remove_file(&manager.receipt)?;
        manager.restore()?;
        assert_eq!(fs::read_to_string(&manager.config)?, before);
        manager.disconnect()?;
        let after: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        for field in [
            "model_provider",
            "model",
            "model_catalog_json",
            "openai_base_url",
        ] {
            assert!(after.get(field).is_none(), "{field}");
        }
        assert_eq!(after["theme"].as_str(), Some("keep-user"));
        assert!(after["model_providers"].get(PROVIDER).is_some());
        if profile {
            for field in ["model_provider", "model", "model_catalog_json"] {
                assert!(after["profiles"]["work"].get(field).is_none(), "{field}");
            }
            assert_eq!(
                after["profiles"]["other"]["model"].as_str(),
                Some("user-other")
            );
        }
        assert_eq!(fs::read_to_string(auth)?, "synthetic-login");
        let after = fs::read_to_string(&manager.config)?;
        manager.disconnect()?;
        assert_eq!(fs::read_to_string(&manager.config)?, after);
        let backups = fs::read_dir(dir.path())?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("vary-routing-orphan-backup-")
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read_to_string(backups[0].path())?, before);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(backups[0].path())?.permissions().mode() & 0o777,
                0o600
            );
        }
    }
    Ok(())
}

#[test]
fn receiptless_disconnect_preserves_external_fields_and_refuses_unknown_provider(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, manager) = setup();
    orphan(&manager);
    let mut doc: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
    doc["model_provider"] = toml_edit::value(PROVIDER);
    doc["model"] = toml_edit::value("user-model");
    doc["model_catalog_json"] = toml_edit::value("/synthetic/user-models.json");
    doc["openai_base_url"] = toml_edit::value("https://synthetic.example/backend-api/codex");
    fs::write(&manager.config, doc.to_string())?;
    manager.disconnect()?;
    let after: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert!(after.get("model_provider").is_none());
    assert_eq!(after["model"].as_str(), Some("user-model"));
    assert_eq!(
        after["model_catalog_json"].as_str(),
        Some("/synthetic/user-models.json")
    );
    assert_eq!(
        after["openai_base_url"].as_str(),
        Some("https://synthetic.example/backend-api/codex")
    );

    doc["model_providers"][PROVIDER]["base_url"] =
        toml_edit::value("https://synthetic.example/intent/v1");
    let unknown = doc.to_string();
    fs::write(&manager.config, &unknown)?;
    assert_eq!(manager.disconnect(), Err(CONFLICT.into()));
    assert_eq!(fs::read_to_string(&manager.config)?, unknown);
    Ok(())
}

#[test]
fn explicit_disconnect_detaches_an_orphan_baseline_after_reconnect(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, manager) = setup();
    orphan(&manager);
    let mut doc: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
    doc["model_provider"] = toml_edit::value(PROVIDER);
    doc["model"] = toml_edit::value(group().alias());
    fs::write(&manager.config, doc.to_string())?;
    manager.write_projection_inner(
        &[group()],
        "synthetic-reconnect-token",
        "http://127.0.0.1:4321/intent/v1",
        None,
        true,
        None,
    )?;
    assert!(manager.receipt.exists());
    manager.disconnect()?;
    let after: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert!(after.get("model_provider").is_none());
    assert!(after.get("model").is_none());
    assert!(!manager.receipt.exists());
    Ok(())
}

#[test]
fn receiptless_disconnect_requires_a_recovery_location_and_accepts_legacy_fields(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = setup();
    orphan(&manager);
    let mut doc: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
    doc["model_provider"] = toml_edit::value(PROVIDER);
    doc["model"] = toml_edit::value(group().alias());
    let provider = doc["model_providers"][PROVIDER]
        .as_table_like_mut()
        .ok_or("provider table")?;
    provider.remove("supports_websockets");
    provider.remove("supports_standalone_web_search");
    let before = doc.to_string();
    fs::write(&manager.config, &before)?;
    let manager = Manager::new(
        manager.config.clone(),
        dir.path().join("blocked/receipt.json"),
        Arc::default(),
    );
    fs::write(dir.path().join("blocked"), "synthetic-file")?;
    assert!(manager.disconnect().is_err());
    assert_eq!(fs::read_to_string(&manager.config)?, before);
    fs::remove_file(dir.path().join("blocked"))?;
    manager.disconnect()?;
    let after: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert!(after.get("model_provider").is_none());
    assert!(after.get("model").is_none());
    Ok(())
}

#[test]
fn orphan_cleanup_never_guesses_active_or_custom_ownership() {
    for change in [
        "active",
        "profile",
        "nested-reference",
        "alias",
        "catalog",
        "unknown-field",
        "remote",
        "url-query",
        "token",
        "name",
    ] {
        let (_dir, manager) = setup();
        orphan(&manager);
        let mut doc: DocumentMut = fs::read_to_string(&manager.config)
            .expect("config")
            .parse()
            .expect("toml");
        match change {
            "active" => doc["model_provider"] = toml_edit::value(PROVIDER),
            "profile" => doc["profiles"]["selected"]["model_provider"] = toml_edit::value(PROVIDER),
            "nested-reference" => {
                let nested: DocumentMut = "external = [{ model_provider = 'varyai-routing' }]"
                    .parse()
                    .expect("nested fixture");
                doc["external"] = nested["external"].clone();
            }
            "alias" => doc["model"] = toml_edit::value("vary-provider/model-synthetic"),
            "catalog" => {
                doc["model_catalog_json"] =
                    toml_edit::value("/synthetic/vary-routing-catalog-orphan.json")
            }
            "unknown-field" => {
                doc["model_providers"][PROVIDER]["custom_flag"] = toml_edit::value(true)
            }
            "remote" => {
                doc["model_providers"][PROVIDER]["base_url"] =
                    toml_edit::value("https://synthetic.example/intent/v1")
            }
            "url-query" => {
                doc["model_providers"][PROVIDER]["base_url"] =
                    toml_edit::value("http://127.0.0.1:1234/intent/v1?custom=1")
            }
            "token" => {
                doc["model_providers"][PROVIDER]["experimental_bearer_token"] =
                    toml_edit::value("user-provided credential")
            }
            "name" => {
                doc["model_providers"][PROVIDER]["name"] = toml_edit::value("User-owned provider")
            }
            _ => unreachable!(),
        }
        let before = doc.to_string();
        fs::write(&manager.config, &before).expect("config");
        let status = manager.status().expect("status");
        assert!(
            status.orphaned_config && status.recovery_pending,
            "{change}"
        );
        assert!(!status.orphan_cleanup_available, "{change}");
        assert!(manager.cleanup_orphan(true).is_err(), "{change}");
        assert_eq!(
            fs::read_to_string(&manager.config).expect("unchanged"),
            before
        );
    }
}

#[test]
fn explicit_reconnect_repairs_partial_menu_without_modifying_login(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = setup();
    let before = "model_provider='varyai-routing'\nmodel='synthetic-raw-model'\n[model_providers.varyai-routing]\nname='VaryAI models and routing groups'\nbase_url='http://127.0.0.1:1234/intent/v1'\nwire_api='responses'\nrequires_openai_auth=false\nexperimental_bearer_token='synthetic-old-token'\n";
    fs::write(&manager.config, before)?;
    let auth = serde_json::to_vec(
        &json!({"auth_mode":"chatgpt","tokens":{"access_token":"synthetic-login"}}),
    )?;
    fs::write(dir.path().join("auth.json"), &auth)?;
    assert_eq!(
        manager.write_projection(
            &[group()],
            "synthetic-new-token",
            "http://127.0.0.1:4321/intent/v1"
        ),
        Err(CONFLICT.into())
    );
    manager.write_projection_inner(
        &[group()],
        "synthetic-new-token",
        "http://127.0.0.1:4321/intent/v1",
        None,
        true,
        None,
    )?;
    let installed: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert_eq!(installed["model"].as_str(), Some(group().alias().as_str()));
    assert_eq!(
        installed["model_providers"][PROVIDER]["requires_openai_auth"].as_bool(),
        Some(true)
    );
    let catalog = installed["model_catalog_json"]
        .as_str()
        .ok_or("missing catalog")?;
    let catalog: Value = serde_json::from_slice(&fs::read(catalog)?)?;
    assert!(catalog["models"]
        .as_array()
        .ok_or("missing models")?
        .iter()
        .any(|m| m["slug"] == group().alias()));
    assert_eq!(fs::read(dir.path().join("auth.json"))?, auth);
    assert!(manager.status()?.enabled == manager.runtime.menu_active());
    manager.restore()?;
    assert_eq!(fs::read_to_string(&manager.config)?, before);
    assert_eq!(fs::read(dir.path().join("auth.json"))?, auth);
    Ok(())
}

#[test]
fn explicit_reconnect_adopts_restored_or_modified_provider_and_preserves_baseline() {
    for active in [false, true] {
        let (dir, manager) = setup();
        orphan(&manager);
        let mut doc: DocumentMut = read(&manager.config)
            .expect("config")
            .expect("present")
            .parse()
            .expect("toml");
        if active {
            doc["model_provider"] = toml_edit::value(PROVIDER);
            doc["model"] = toml_edit::value("auto/restored-model");
            doc["model_catalog_json"] =
                toml_edit::value("/synthetic/vary-routing-catalog-old.json");
        }
        doc["profiles"]["restored"]["model_provider"] = toml_edit::value(PROVIDER);
        doc["model_providers"][PROVIDER]["custom_flag"] = toml_edit::value(true);
        doc["openai_base_url"] = toml_edit::value("http://127.0.0.1:1234/backend-api/codex");
        let before = doc.to_string();
        fs::write(&manager.config, &before).expect("restored configuration");
        let auth = dir.path().join("auth.json");
        fs::write(&auth, "synthetic-login").expect("auth");

        // A background refresh cannot acquire a restored configuration.
        assert_eq!(
            manager.write_projection(&[group()], "fresh-token", "http://127.0.0.1:4321/intent/v1"),
            Err(CONFLICT.into())
        );
        assert_eq!(
            read(&manager.config).expect("unchanged"),
            Some(before.clone())
        );
        assert!(!manager.receipt.exists());
        manager
            .write_projection_inner(
                &[group()],
                "fresh-token",
                "http://127.0.0.1:4321/intent/v1",
                None,
                true,
                None,
            )
            .expect("explicit reconnect");
        let receipt = manager.load().expect("receipt").expect("new owner");
        assert_eq!(receipt.original.as_deref(), Some(before.as_str()));
        assert!(receipt.native.is_none());
        let installed: toml::Value = read(&manager.config)
            .expect("config")
            .expect("present")
            .parse()
            .expect("toml");
        assert_eq!(installed["model_provider"].as_str(), Some(PROVIDER));
        assert_eq!(installed["model"].as_str(), Some(group().alias().as_str()));
        assert_eq!(
            installed["model_providers"][PROVIDER]["base_url"].as_str(),
            Some("http://127.0.0.1:4321/intent/v1")
        );
        assert_eq!(
            installed["model_providers"][PROVIDER]["experimental_bearer_token"].as_str(),
            Some("fresh-token")
        );
        assert_eq!(
            installed["profiles"]["restored"]["model_provider"].as_str(),
            Some(PROVIDER)
        );
        assert_eq!(
            installed["openai_base_url"].as_str(),
            Some("http://127.0.0.1:1234/backend-api/codex")
        );
        let backups = fs::read_dir(dir.path())
            .expect("files")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("vary-routing-orphan-backup-")
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            read(&backups[0].path()).expect("backup"),
            Some(before.clone())
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(backups[0].path())
                    .expect("backup mode")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        manager.restore().expect("disconnect");
        assert_eq!(
            read(&manager.config).expect("baseline restored"),
            Some(before)
        );
        assert_eq!(
            read(&auth).expect("unchanged login").as_deref(),
            Some("synthetic-login")
        );
    }
}

#[test]
fn orphan_cleanup_refuses_unreadable_recovery_location() {
    let (dir, manager) = setup();
    let before = orphan(&manager);
    let blocked_parent = dir.path().join("not-a-directory");
    fs::write(&blocked_parent, "fixture").expect("block recovery directory");
    let manager = Manager::new(
        manager.config.clone(),
        blocked_parent.join("receipt.json"),
        Arc::default(),
    );
    assert_eq!(manager.cleanup_orphan(true), Err(FAILED.into()));
    assert_eq!(
        fs::read_to_string(&manager.config).expect("unchanged"),
        before
    );
}

#[test]
fn orphan_cleanup_refuses_receipt_backed_ownership() {
    let (dir, manager) = setup();
    orphan(&manager);
    manager.cleanup_orphan(true).expect("clean orphan");
    manager
        .write_projection(
            &[group()],
            "synthetic-owned-token",
            "http://127.0.0.1:1234/intent/v1",
        )
        .expect("connect");
    let before = fs::read_to_string(&manager.config).expect("config");
    let receipt = fs::read_to_string(&manager.receipt).expect("receipt");
    assert!(manager.cleanup_orphan(true).is_err());
    let status = manager.status().expect("status");
    assert!(!status.orphaned_config && !status.orphan_cleanup_available);
    assert_eq!(
        fs::read_to_string(&manager.config).expect("unchanged"),
        before
    );
    assert_eq!(
        fs::read_to_string(&manager.receipt).expect("receipt"),
        receipt
    );
    assert_eq!(
        fs::read_dir(dir.path())
            .expect("files")
            .filter_map(Result::ok)
            .filter(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with("vary-routing-orphan-backup-"))
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn orphan_cleanup_refuses_symlinked_configuration() {
    let (dir, manager) = setup();
    let before = orphan(&manager);
    let target = dir.path().join("external.toml");
    fs::rename(&manager.config, &target).expect("move fixture");
    std::os::unix::fs::symlink(&target, &manager.config).expect("symlink");
    assert!(manager.status().is_err());
    assert!(manager.cleanup_orphan(true).is_err());
    assert!(fs::symlink_metadata(&manager.config)
        .expect("link")
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(target).expect("unchanged"), before);
    assert!(!fs::read_dir(dir.path())
        .expect("files")
        .filter_map(Result::ok)
        .any(|e| e
            .file_name()
            .to_string_lossy()
            .starts_with("vary-routing-orphan-backup-")));
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
        "Second · routing group · VaryAI"
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
fn status_distinguishes_an_inactive_runtime_from_changed_projection_fields() {
    let (_dir, manager) = setup();
    manager
        .write_projection(
            &[group()],
            "synthetic-status-only",
            "http://127.0.0.1:1234/intent/v1",
        )
        .expect("projection");
    let status = serde_json::to_value(manager.status().expect("status")).expect("safe status");
    assert_eq!(status["configurationOwned"], true);
    assert_eq!(status["enabled"], false);
    assert_eq!(status["recoveryPending"], true);
    let mut config: DocumentMut = fs::read_to_string(&manager.config)
        .expect("fixture")
        .parse()
        .expect("toml");
    config["model_provider"] = toml_edit::value("external");
    fs::write(&manager.config, config.to_string()).expect("external change");
    let status = serde_json::to_value(manager.status().expect("status")).expect("safe status");
    assert_eq!(status["configurationOwned"], false);
    assert!(!status.to_string().contains("synthetic-status-only"));
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

fn legacy_receipt_with_added_url(manager: &Manager) -> (String, Vec<PathBuf>) {
    fs::write(&manager.config, "model='original'\n").expect("original");
    manager
        .write_projection(
            &[group()],
            "synthetic-token",
            "http://127.0.0.1:1234/intent/v1",
        )
        .expect("project");
    let mut receipt: Value =
        serde_json::from_str(&fs::read_to_string(&manager.receipt).expect("receipt"))
            .expect("json");
    let installed = format!(
        "# user URL\nopenai_base_url='https://synthetic.example/backend-api/codex'\n{}",
        receipt["installed"].as_str().expect("installed")
    );
    receipt["version"] = json!(1);
    receipt["installed"] = json!(installed);
    receipt.as_object_mut().expect("receipt").remove("native");
    receipt
        .as_object_mut()
        .expect("receipt")
        .remove("pre_write");
    let catalogs = receipt["catalogs"]
        .as_array()
        .expect("catalogs")
        .iter()
        .map(|path| PathBuf::from(path.as_str().expect("path")))
        .collect();
    fs::write(
        &manager.receipt,
        serde_json::to_vec(&receipt).expect("json"),
    )
    .expect("receipt");
    fs::write(&manager.config, &installed).expect("config");
    (installed, catalogs)
}

#[test]
fn legacy_receipt_exact_installed_preserves_unowned_user_added_native_url() {
    let (_dir, manager) = setup();
    legacy_receipt_with_added_url(&manager);
    manager.restore().expect("restore");
    let text = fs::read_to_string(&manager.config).expect("config");
    let doc: toml::Value = text.parse().expect("toml");
    assert_eq!(
        doc["openai_base_url"].as_str(),
        Some("https://synthetic.example/backend-api/codex")
    );
    assert_eq!(doc["model"].as_str(), Some("original"));
    assert!(text.contains("# user URL"));
    assert!(!manager.receipt.exists());
}

#[test]
fn legacy_receipt_missing_config_keeps_ambiguous_url_recovery_evidence() {
    let (_dir, manager) = setup();
    let (_, catalogs) = legacy_receipt_with_added_url(&manager);
    let receipt = fs::read_to_string(&manager.receipt).expect("receipt");
    fs::remove_file(&manager.config).expect("interrupted replacement");
    assert_eq!(manager.restore(), Err(CONFLICT.into()));
    assert!(!manager.config.exists());
    assert_eq!(
        fs::read_to_string(&manager.receipt).expect("receipt"),
        receipt
    );
    assert!(catalogs.iter().all(|path| path.exists()));
}

const NATIVE_ONE: &str = "http://127.0.0.1:1234/backend-api/codex";
const NATIVE_TWO: &str = "http://127.0.0.1:4321/backend-api/codex";
const MENU_BASE: &str = "http://127.0.0.1:1234/intent/v1";

#[test]
fn native_projection_refuses_any_unowned_url_including_identical_loopback() {
    for url in [NATIVE_ONE, "https://synthetic.example/backend-api/codex"] {
        let (_dir, manager) = setup();
        let original = format!("# user setting\nopenai_base_url='{url}'\nmodel='original'\n");
        fs::write(&manager.config, &original).expect("config");
        assert_eq!(
            manager.write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_ONE)),
            Err("intentRouting.nativeConflict".into())
        );
        assert_eq!(
            fs::read_to_string(&manager.config).expect("config"),
            original
        );
        assert!(!manager.receipt.exists());
        manager
            .write_projection(&[group()], "token", MENU_BASE)
            .expect("custom menu remains usable");
        assert_eq!(manager.native_owned_url().expect("ownership"), None);
        let receipt = fs::read_to_string(&manager.receipt).expect("receipt");
        assert_eq!(
            manager.write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_ONE)),
            Err("intentRouting.nativeConflict".into())
        );
        assert_eq!(
            fs::read_to_string(&manager.receipt).expect("receipt"),
            receipt
        );
        manager.restore().expect("restore");
        assert_eq!(
            fs::read_to_string(&manager.config).expect("config"),
            original
        );
    }
}

#[test]
fn native_projection_validates_only_uncredentialed_loopback_codex_base() {
    for url in [
        "https://127.0.0.1:1234/backend-api/codex",
        "http://synthetic.example:1234/backend-api/codex",
        "http://user@127.0.0.1:1234/backend-api/codex",
        "http://@127.0.0.1:1234/backend-api/codex",
        "http://127.0.0.1:1234/other/../backend-api/codex",
        " http://127.0.0.1:1234/backend-api/codex",
        "http://127.0.0.1:1234/backend-api/codex?query=1",
        "http://127.0.0.1:1234/backend-api/codex#fragment",
        "http://127.0.0.1:1234/backend-api/codex/",
        "http://127.0.0.1:1234/intent/v1",
    ] {
        let (_dir, manager) = setup();
        assert!(manager
            .write_projection_with_native(&[group()], "token", MENU_BASE, Some(url))
            .is_err());
        assert!(!manager.config.exists());
        assert!(!manager.receipt.exists());
    }
}

#[test]
fn native_v2_install_refresh_and_restore_preserve_ownership_without_implicit_acquisition() {
    let (_dir, manager) = setup();
    let original = "# user comment\nmodel='original'\n";
    fs::write(&manager.config, original).expect("config");
    manager
        .write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_ONE))
        .expect("native install");
    assert_eq!(
        manager.load().expect("receipt").expect("installed").version,
        2
    );
    assert_eq!(
        manager.native_owned_url().expect("ownership"),
        Some(NATIVE_ONE.into())
    );
    manager
        .write_projection(&[group()], "token", MENU_BASE)
        .expect("ordinary refresh");
    assert_eq!(
        manager.native_owned_url().expect("ownership"),
        Some(NATIVE_ONE.into())
    );
    manager
        .write_projection_with_native(&[group()], "new-token", MENU_BASE, Some(NATIVE_TWO))
        .expect("owned port refresh");
    assert_eq!(
        manager.native_owned_url().expect("ownership"),
        Some(NATIVE_TWO.into())
    );
    manager.restore().expect("restore");
    assert_eq!(
        fs::read_to_string(&manager.config).expect("restored"),
        original
    );
    manager
        .write_projection(&[group()], "token", MENU_BASE)
        .expect("menu only");
    assert_eq!(manager.native_owned_url().expect("ownership"), None);
    assert!(!fs::read_to_string(&manager.config)
        .expect("config")
        .contains("openai_base_url"));
}

#[test]
fn native_user_edit_ends_ownership_and_survives_refresh_and_restore() {
    let (_dir, manager) = setup();
    fs::write(&manager.config, "model='original'\n").expect("config");
    manager
        .write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_ONE))
        .expect("install");
    let mut doc: DocumentMut = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    doc["openai_base_url"] = toml_edit::value("https://synthetic.example/user-codex");
    let text = format!("# external change\n{doc}");
    fs::write(&manager.config, &text).expect("user edit");
    assert_eq!(manager.native_owned_url().expect("ownership"), None);
    assert_eq!(
        manager.write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_TWO)),
        Err("intentRouting.nativeConflict".into())
    );
    assert_eq!(fs::read_to_string(&manager.config).expect("config"), text);
    manager
        .write_projection(&[group()], "token", MENU_BASE)
        .expect("menu refresh");
    assert_eq!(manager.native_owned_url().expect("ownership"), None);
    manager.restore().expect("restore");
    let text = fs::read_to_string(&manager.config).expect("restored");
    let doc: toml::Value = text.parse().expect("toml");
    assert_eq!(doc["model"].as_str(), Some("original"));
    assert_eq!(
        doc["openai_base_url"].as_str(),
        Some("https://synthetic.example/user-codex")
    );
    assert!(text.contains("# external change"));
}

#[test]
fn late_native_acquisition_restores_acquisition_baseline_not_original_user_url() {
    let (_dir, manager) = setup();
    fs::write(
        &manager.config,
        "openai_base_url='https://synthetic.example/old'\nmodel='original'\n",
    )
    .expect("config");
    manager
        .write_projection(&[group()], "token", MENU_BASE)
        .expect("menu only");
    let mut doc: DocumentMut = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    doc.as_table_mut().remove("openai_base_url");
    fs::write(&manager.config, doc.to_string()).expect("user removes URL");
    manager
        .write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_ONE))
        .expect("late acquisition");
    manager.restore().expect("restore");
    let doc: toml::Value = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    assert_eq!(doc["model"].as_str(), Some("original"));
    assert!(doc.get("openai_base_url").is_none());
}

#[test]
fn interrupted_refresh_uses_actual_prewrite_and_previous_owned_port() {
    for missing in [false, true] {
        let (_dir, manager) = setup();
        fs::write(&manager.config, "model='original'\n").expect("config");
        manager
            .write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_ONE))
            .expect("first install");
        let pre_write = format!(
            "# new user comment\nuser_option=true\n{}",
            fs::read_to_string(&manager.config).expect("config")
        );
        fs::write(&manager.config, &pre_write).expect("external unrelated edit");
        manager
            .write_projection_with_native(&[group()], "new-token", MENU_BASE, Some(NATIVE_TWO))
            .expect("refresh");
        if missing {
            fs::remove_file(&manager.config).expect("interrupted replacement");
        } else {
            fs::write(&manager.config, pre_write).expect("journal before replacement");
        }
        manager.restore().expect("recover");
        let text = fs::read_to_string(&manager.config).expect("restored");
        let doc: toml::Value = text.parse().expect("toml");
        assert_eq!(doc["model"].as_str(), Some("original"));
        assert_eq!(doc["user_option"].as_bool(), Some(true));
        assert!(doc.get("openai_base_url").is_none());
        assert!(text.contains("# new user comment"));
    }
}

#[test]
fn v1_to_v2_refresh_journals_unowned_url_for_missing_config_recovery() {
    let (_dir, manager) = setup();
    legacy_receipt_with_added_url(&manager);
    manager
        .write_projection(&[group()], "next-token", MENU_BASE)
        .expect("refresh");
    assert_eq!(manager.native_owned_url().expect("ownership"), None);
    fs::remove_file(&manager.config).expect("interrupted replacement");
    manager
        .restore()
        .expect("recover from actual pre-write snapshot");
    let doc: toml::Value = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    assert_eq!(
        doc["openai_base_url"].as_str(),
        Some("https://synthetic.example/backend-api/codex")
    );
    assert_eq!(doc["model"].as_str(), Some("original"));
}

#[test]
fn default_or_v1_native_metadata_does_not_claim_field_ownership() {
    for version in [1, 2] {
        let (_dir, manager) = setup();
        manager
            .write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_ONE))
            .expect("install");
        let mut receipt: Value =
            serde_json::from_str(&fs::read_to_string(&manager.receipt).expect("receipt"))
                .expect("json");
        receipt["version"] = json!(version);
        if version == 2 {
            receipt["native"] = json!({});
        }
        fs::write(
            &manager.receipt,
            serde_json::to_vec(&receipt).expect("json"),
        )
        .expect("receipt");
        assert_eq!(manager.native_owned_url().expect("not owned"), None);
        assert_eq!(
            manager.write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_ONE)),
            Err("intentRouting.nativeConflict".into())
        );
        manager.restore().expect("preserve unowned URL");
        let doc: toml::Value = fs::read_to_string(&manager.config)
            .expect("config")
            .parse()
            .expect("toml");
        assert_eq!(doc["openai_base_url"].as_str(), Some(NATIVE_ONE));
    }
}

#[test]
fn explicit_native_metadata_requires_a_matching_installed_projection() {
    let (_dir, manager) = setup();
    manager
        .write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_ONE))
        .expect("install");
    let mut receipt: Value =
        serde_json::from_str(&fs::read_to_string(&manager.receipt).expect("receipt"))
            .expect("json");
    receipt["native"]["installed"] = json!(NATIVE_TWO);
    fs::write(
        &manager.receipt,
        serde_json::to_vec(&receipt).expect("json"),
    )
    .expect("receipt");
    assert_eq!(manager.native_owned_url().expect("not owned"), None);
    manager.restore().expect("preserve unowned URL");
    let doc: toml::Value = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    assert_eq!(doc["openai_base_url"].as_str(), Some(NATIVE_ONE));
}

#[test]
fn legacy_exact_previous_document_preserves_unowned_url() {
    let (_dir, manager) = setup();
    let (installed, _) = legacy_receipt_with_added_url(&manager);
    let mut receipt: Value =
        serde_json::from_str(&fs::read_to_string(&manager.receipt).expect("receipt"))
            .expect("json");
    receipt["previous"] = json!(installed);
    receipt["installed"] = json!(receipt["installed"].as_str().expect("installed").replace(
        "https://synthetic.example/backend-api/codex",
        "https://synthetic.example/next"
    ));
    fs::write(
        &manager.receipt,
        serde_json::to_vec(&receipt).expect("json"),
    )
    .expect("receipt");
    manager.restore().expect("restore exact previous");
    let doc: toml::Value = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    assert_eq!(
        doc["openai_base_url"].as_str(),
        Some("https://synthetic.example/backend-api/codex")
    );
}

#[test]
fn missing_legacy_config_refuses_conflicting_previous_url_provenance() {
    let (_dir, manager) = setup();
    let (_, catalogs) = legacy_receipt_with_added_url(&manager);
    let mut receipt: Value =
        serde_json::from_str(&fs::read_to_string(&manager.receipt).expect("receipt"))
            .expect("json");
    receipt["original"] =
        json!("openai_base_url='https://synthetic.example/backend-api/codex'\nmodel='original'\n");
    receipt["previous"] = json!(receipt["installed"].as_str().expect("installed").replace(
        "https://synthetic.example/backend-api/codex",
        "https://synthetic.example/previous"
    ));
    fs::write(
        &manager.receipt,
        serde_json::to_vec(&receipt).expect("json"),
    )
    .expect("receipt");
    fs::remove_file(&manager.config).expect("missing config");
    assert_eq!(manager.restore(), Err(CONFLICT.into()));
    assert!(manager.receipt.exists());
    assert!(catalogs.iter().all(|path| path.exists()));
}

#[cfg(unix)]
#[test]
fn native_ownership_and_recovery_refuse_symlinked_config_and_receipt() {
    for symlink_receipt in [false, true] {
        let (dir, manager) = setup();
        manager
            .write_projection_with_native(&[group()], "token", MENU_BASE, Some(NATIVE_ONE))
            .expect("install");
        let path = if symlink_receipt {
            &manager.receipt
        } else {
            &manager.config
        };
        let target = dir.path().join("synthetic-external");
        fs::rename(path, &target).expect("move fixture");
        let before = fs::read_to_string(&target).expect("target");
        std::os::unix::fs::symlink(&target, path).expect("symlink");
        assert!(manager.native_owned_url().is_err());
        assert!(manager.restore().is_err());
        assert_eq!(fs::read_to_string(&target).expect("unchanged"), before);
        assert!(manager.receipt.exists());
    }
}

#[test]
fn shared_menu_catalog_matches_disk_projection_rich_metadata_and_capabilities() {
    use crate::proxy::intent::{Menu, MenuModel, Session, Target};
    use std::collections::HashMap;
    let (_dir, manager) = setup();
    let mut direct = group();
    direct.id = "z-direct".into();
    let mut routed = group();
    routed.id = "a-routed".into();
    let client = reqwest::Client::new();
    let session = |group: RoutingGroup, images: bool| {
        let member = &group.members[0];
        Arc::new(Session {
            targets: vec![Target {
                fixed_effort: None,
                provider_id: member.provider_id.clone(),
                model: member.model_id.clone(),
                upstream: None,
                effort: super::super::effort::Capability::parse(&json!({
                    "supported_reasoning_levels":["low","high"],
                    "context_window": if images { 1000000 } else { 128000 },
                    "max_context_window": if images { 1200000 } else { 128000 },
                    "auto_compact_token_limit": 90000,
                    "effective_context_window_percent": 90,
                    "input_modalities": if images { vec!["text", "image"] } else { vec!["text"] },
                })),
                url: "http://127.0.0.1:1/responses"
                    .parse()
                    .expect("synthetic URL"),
                headers: Default::default(),
                official: false,
                credentials: None,
                scheduling: Arc::default(),
            }],
            group,
            classifier: None,
            client: client.clone(),
            created: std::time::Instant::now(),
            bindings: Arc::default(),
        })
    };
    let menu = Arc::new(Menu {
        token: "synthetic-menu-token".into(),
        default_model: Some("vary-provider/exact-id".into()),
        omitted_entries: 0,
        context_settings: Default::default(),
        groups: HashMap::from([
            (
                "vary-provider/exact-id".into(),
                session(direct.clone(), true),
            ),
            ("auto/exact-group".into(), session(routed.clone(), false)),
        ]),
        models: HashMap::from([
            (
                direct.id,
                MenuModel {
                    alias: "vary-provider/exact-id".into(),
                    label: "Exact direct label".into(),
                    description: "Direct provider model",
                    order: 1,
                },
            ),
            (
                routed.id,
                MenuModel {
                    alias: "auto/exact-group".into(),
                    label: "Exact group label".into(),
                    description: "Routing group description",
                    order: 0,
                },
            ),
        ]),
    });
    fs::write(&manager.config, "model_reasoning_effort='high'\n").expect("config");
    manager
        .activate_menu_with_native(menu.clone(), MENU_BASE, Some(NATIVE_ONE), false)
        .expect("activate");
    let expected = menu_catalog(&menu, "high").expect("rich catalog");
    assert_eq!(expected["models"][0]["slug"], "auto/exact-group");
    assert_eq!(expected["models"][1]["slug"], "vary-provider/exact-id");
    let receipt = manager.load().expect("receipt").expect("installed");
    let actual: Value = serde_json::from_str(
        &fs::read_to_string(receipt.catalogs.last().expect("catalog path")).expect("catalog"),
    )
    .expect("json");
    assert_eq!(actual, expected);
    let (published_menu, native_catalog) = manager.runtime.native.menu().expect("native snapshot");
    assert_eq!(native_catalog, actual);
    assert_eq!(published_menu.token, menu.token);
    assert_eq!(actual["models"][0]["slug"], "auto/exact-group");
    assert_eq!(
        actual["models"][0]["description"],
        "Routing group description"
    );
    assert_eq!(actual["models"][0]["input_modalities"], json!(["text"]));
    assert_eq!(actual["models"][1]["slug"], "vary-provider/exact-id");
    assert_eq!(actual["models"][1]["display_name"], "Exact direct label");
    assert_eq!(
        actual["models"][1]["supported_reasoning_levels"]
            .as_array()
            .expect("efforts")
            .len(),
        2
    );
    assert_eq!(actual["models"][0]["context_window"], 128000);
    assert_eq!(actual["models"][1]["context_window"], 1000000);
    assert_eq!(actual["models"][1]["max_context_window"], 1200000);
    assert_eq!(actual["models"][1]["auto_compact_token_limit"], 90000);
    assert_eq!(actual["models"][1]["effective_context_window_percent"], 90);
    assert_eq!(actual["models"][1]["default_reasoning_level"], "high");
    assert_eq!(
        actual["models"][1]["input_modalities"],
        json!(["text", "image"])
    );
    manager
        .activate_menu_with_selection(
            menu.clone(),
            MENU_BASE,
            None,
            true,
            Some("auto/exact-group"),
        )
        .expect("explicit Agent model selection");
    let selected: toml::Value = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    assert_eq!(selected["model"].as_str(), Some("auto/exact-group"));
    manager
        .activate_menu_with_native(menu.clone(), MENU_BASE, None, false)
        .expect("background refresh");
    let selected: toml::Value = fs::read_to_string(&manager.config)
        .expect("config")
        .parse()
        .expect("toml");
    assert_eq!(selected["model"].as_str(), Some("auto/exact-group"));
    let before = fs::read(&manager.config).expect("config");
    assert!(manager
        .activate_menu_with_selection(menu, MENU_BASE, None, true, Some("not-in-catalog"))
        .is_err());
    assert_eq!(fs::read(&manager.config).expect("config"), before);
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
