use super::*;

const KEY: &str = "VARYAI_HARNESS_GATEWAY_KEY";

#[test]
fn deepseek_native_credentials_follow_the_connection_lifecycle() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|_| FAILED)?;
    let profile = root.path().join("profiles/desktop/cordis.patch.yml");
    let credentials = root.path().join(".credentials.yaml");
    let original = "# Native credentials\nversion: 1\nrefs:\n  NATIVE_API_KEY: synthetic-native\n  MAGPIE_GATEWAY_KEY: synthetic-other-manager\n";
    fs::write(&credentials, original).map_err(|_| FAILED)?;
    let manager = Manager {
        root: root.path().join("receipts"),
    };
    let menu = tests::fixture_menu();
    let selected = menu.default_model.as_deref().ok_or(FAILED)?;
    crate::services::agent_clients::with_test_config(&profile, || -> Result<(), String> {
        let receipt = manager.build("deepseek-harness", &menu, "http://127.0.0.1:1", selected)?;
        assert_eq!(
            receipt.files.len(),
            2,
            "the native credential store must be journaled"
        );
        let token = receipt.token.clone();
        manager.install(receipt)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&credentials)
                    .map_err(|_| FAILED)?
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let stored: Value =
            serde_yaml::from_str(&read(&credentials)?.ok_or(FAILED)?).map_err(|_| FAILED)?;
        assert!(stored["refs"][KEY].as_str() == Some(token.as_str()));
        assert!(stored["refs"]["NATIVE_API_KEY"] == "synthetic-native");
        assert!(stored["refs"]["MAGPIE_GATEWAY_KEY"] == "synthetic-other-manager");
        let doc = crate::services::deepseek_harness::parse(&read(&profile)?.ok_or(FAILED)?)?;
        let route = &doc["providers"]["varyai-provider-shared-catalog"]["config"]["providers"]
            ["varyai-shared-catalog"];
        assert_eq!(route["apiKeyEnv"], KEY);
        assert!(route["headers"].get("Authorization").is_none());
        let before = fs::read(&credentials).map_err(|_| FAILED)?;
        manager.refresh("deepseek-harness", &menu, "http://127.0.0.1:1")?;
        assert!(fs::read(&credentials).map_err(|_| FAILED)? == before);
        assert!(manager.status("deepseek-harness")?.configured);
        manager.disconnect("deepseek-harness")?;
        assert!(read(&credentials)?.as_deref() == Some(original));
        assert!(!profile.exists());
        Ok(())
    })
}

#[test]
fn deepseek_native_credentials_reject_ambiguous_stores_without_writes() -> Result<(), String> {
    for source in [
        "version: 2\nrefs: {}\n",
        "version: 1\nrefs: []\n",
        "version: 1\nrefs: {NATIVE_API_KEY: 42}\n",
        "version: 1\nrefs: {NATIVE_API_KEY: ''}\n",
        "version: 1\nrefs: {invalid-reference: synthetic-invalid}\n",
        "refs: {NATIVE_API_KEY: synthetic-native}\n",
        "version: 1\nrefs: [\n",
        "version: 1\nrefs: {VARYAI_HARNESS_GATEWAY_KEY: synthetic-foreign}\n",
    ] {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let profile = root.path().join("profiles/desktop/cordis.patch.yml");
        let credentials = root.path().join(".credentials.yaml");
        fs::write(&credentials, source).map_err(|_| FAILED)?;
        let manager = Manager {
            root: root.path().join("receipts"),
        };
        let menu = tests::fixture_menu();
        let selected = menu.default_model.as_deref().ok_or(FAILED)?;
        crate::services::agent_clients::with_test_config(&profile, || {
            assert!(manager
                .build("deepseek-harness", &menu, "http://127.0.0.1:1", selected)
                .is_err());
        });
        assert!(read(&credentials)?.as_deref() == Some(source));
        assert!(!profile.exists());
    }
    Ok(())
}

#[test]
fn deepseek_native_credentials_upgrade_header_receipts_and_restore_the_original_baseline(
) -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|_| FAILED)?;
    let profile = root.path().join("profiles/desktop/cordis.patch.yml");
    let credentials = root.path().join(".credentials.yaml");
    let manager = Manager {
        root: root.path().join("receipts"),
    };
    let menu = tests::fixture_menu();
    let selected = menu.default_model.as_deref().ok_or(FAILED)?;
    let file = FileEdit::prepare_format(profile.clone(), Format::Cordis, |value| {
        crate::services::deepseek_harness::apply(
            value,
            "shared-catalog",
            "Vary AI",
            &json!({
                "baseUrl":"http://127.0.0.1:1/v1", "protocol":"openai", "apiKey":"synthetic-only", "models":[{"model":"Fixture"}]
            }),
        )
    })?;
    manager.install(Receipt {
        app: "deepseek-harness".into(),
        token: "synthetic-only".into(),
        model: selected.into(),
        model_map: BTreeMap::new(),
        visible_models: None,
        files: vec![file],
        suspended: false,
        applied_at_ms: None,
    })?;
    assert!(!credentials.exists());
    crate::services::agent_clients::with_test_config(&profile, || -> Result<(), String> {
        let next = manager
            .refresh("deepseek-harness", &menu, "http://127.0.0.1:1")?
            .ok_or(FAILED)?;
        assert_eq!(next.files.len(), 2);
        assert!(manager.status("deepseek-harness")?.configured);
        let source = read(&profile)?.ok_or(FAILED)?;
        assert!(source.contains("apiKeyEnv: VARYAI_HARNESS_GATEWAY_KEY"));
        assert!(!source.contains("Bearer synthetic-only"));
        manager.refresh("deepseek-harness", &menu, "http://127.0.0.1:1")?;
        manager.disconnect("deepseek-harness")
    })?;
    assert!(!profile.exists());
    assert!(!credentials.exists());
    Ok(())
}

#[test]
fn deepseek_native_credentials_preserve_keys_added_after_connecting() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|_| FAILED)?;
    let profile = root.path().join("profiles/desktop/cordis.patch.yml");
    let credentials = root.path().join(".credentials.yaml");
    let manager = Manager {
        root: root.path().join("receipts"),
    };
    let menu = tests::fixture_menu();
    let selected = menu.default_model.as_deref().ok_or(FAILED)?;
    crate::services::agent_clients::with_test_config(&profile, || -> Result<(), String> {
        manager.install(manager.build(
            "deepseek-harness",
            &menu,
            "http://127.0.0.1:1",
            selected,
        )?)?;
        let mut stored: Value =
            serde_yaml::from_str(&read(&credentials)?.ok_or(FAILED)?).map_err(|_| FAILED)?;
        stored["refs"]["NATIVE_API_KEY"] = json!("synthetic-added");
        fs::write(
            &credentials,
            serde_yaml::to_string(&stored).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED)?;
        manager.disconnect("deepseek-harness")?;
        let stored: Value =
            serde_yaml::from_str(&read(&credentials)?.ok_or(FAILED)?).map_err(|_| FAILED)?;
        assert_eq!(stored["version"], 1);
        assert!(stored["refs"].get(KEY).is_none());
        assert!(stored["refs"]["NATIVE_API_KEY"] == "synthetic-added");
        Ok(())
    })
}

#[test]
fn deepseek_native_credentials_detect_external_changes_and_recover_only_on_reconnect(
) -> Result<(), String> {
    for change in ["credential", "reference", "missing-store", "auth-header"] {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let profile = root.path().join("profiles/desktop/cordis.patch.yml");
        let credentials = root.path().join(".credentials.yaml");
        let manager = Manager {
            root: root.path().join("receipts"),
        };
        let menu = tests::fixture_menu();
        let selected = menu.default_model.as_deref().ok_or(FAILED)?;
        crate::services::agent_clients::with_test_config(&profile, || -> Result<(), String> {
            manager.install(manager.build(
                "deepseek-harness",
                &menu,
                "http://127.0.0.1:1",
                selected,
            )?)?;
            if change == "auth-header" {
                let source = read(&profile)?.ok_or(FAILED)?;
                let mut doc = crate::services::deepseek_harness::parse(&source)?;
                doc["providers"]["varyai-provider-shared-catalog"]["config"]["providers"]
                    ["varyai-shared-catalog"]["headers"]["authorization"] =
                    json!("Bearer synthetic-external");
                fs::write(
                    &profile,
                    crate::services::deepseek_harness::encode(&source, &doc)?,
                )
                .map_err(|_| FAILED)?;
            } else if change == "reference" {
                let source = read(&profile)?
                    .ok_or(FAILED)?
                    .replace(KEY, "SYNTHETIC_OTHER_KEY");
                fs::write(&profile, source).map_err(|_| FAILED)?;
            } else if change == "missing-store" {
                fs::remove_file(&credentials).map_err(|_| FAILED)?;
            } else {
                let mut stored: Value = serde_yaml::from_str(&read(&credentials)?.ok_or(FAILED)?)
                    .map_err(|_| FAILED)?;
                stored["refs"][KEY] = json!("synthetic-edited");
                stored["refs"]["NATIVE_API_KEY"] = json!("synthetic-added");
                fs::write(
                    &credentials,
                    serde_yaml::to_string(&stored).map_err(|_| FAILED)?,
                )
                .map_err(|_| FAILED)?;
            }
            let before = (read(&profile)?, read(&credentials)?);
            assert!(manager.status("deepseek-harness")?.recovery_pending);
            assert!(manager
                .refresh("deepseek-harness", &menu, "http://127.0.0.1:1")
                .is_err());
            assert!((read(&profile)?, read(&credentials)?) == before);
            // Older receipts may predate the separate model preference cache.
            fs::remove_file(
                manager
                    .model_choice_path("deepseek-harness")
                    .ok_or(FAILED)?,
            )
            .map_err(|_| FAILED)?;
            manager.release_for_reapply("deepseek-harness")?;
            assert_eq!(
                manager.remembered_model("deepseek-harness").as_deref(),
                Some(selected)
            );
            manager.install(manager.build(
                "deepseek-harness",
                &menu,
                "http://127.0.0.1:1",
                selected,
            )?)?;
            assert!(manager.status("deepseek-harness")?.configured);
            manager.disconnect("deepseek-harness")?;
            if change == "credential" {
                let stored: Value = serde_yaml::from_str(&read(&credentials)?.ok_or(FAILED)?)
                    .map_err(|_| FAILED)?;
                assert!(stored["refs"].get(KEY).is_none());
                assert!(stored["refs"]["NATIVE_API_KEY"] == "synthetic-added");
            }
            Ok(())
        })?;
    }
    Ok(())
}

#[test]
fn deepseek_native_credentials_corrupt_store_blocks_recovery_before_any_profile_write(
) -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|_| FAILED)?;
    let profile = root.path().join("profiles/desktop/cordis.patch.yml");
    let credentials = root.path().join(".credentials.yaml");
    let manager = Manager {
        root: root.path().join("receipts"),
    };
    let menu = tests::fixture_menu();
    let selected = menu.default_model.as_deref().ok_or(FAILED)?;
    crate::services::agent_clients::with_test_config(&profile, || -> Result<(), String> {
        manager.install(manager.build(
            "deepseek-harness",
            &menu,
            "http://127.0.0.1:1",
            selected,
        )?)?;
        let before = read(&profile)?;
        fs::write(&credentials, "version: 2\nrefs: {}\n").map_err(|_| FAILED)?;
        assert!(manager.release_for_reapply("deepseek-harness").is_err());
        assert!(read(&profile)? == before);
        assert!(read(&credentials)?.as_deref() == Some("version: 2\nrefs: {}\n"));
        assert!(manager.load("deepseek-harness")?.is_some());
        Ok(())
    })
}

#[test]
fn deepseek_native_credentials_install_race_rolls_back_the_profile() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|_| FAILED)?;
    let profile = root.path().join("profiles/desktop/cordis.patch.yml");
    let credentials = root.path().join(".credentials.yaml");
    let manager = Manager {
        root: root.path().join("receipts"),
    };
    let menu = tests::fixture_menu();
    let selected = menu.default_model.as_deref().ok_or(FAILED)?;
    crate::services::agent_clients::with_test_config(&profile, || -> Result<(), String> {
        let receipt = manager.build("deepseek-harness", &menu, "http://127.0.0.1:1", selected)?;
        let changed = "version: 1\nrefs: {VARYAI_HARNESS_GATEWAY_KEY: synthetic-race}\n";
        fs::write(&credentials, changed).map_err(|_| FAILED)?;
        assert!(manager.install(receipt).is_err());
        assert!(!profile.exists());
        assert!(read(&credentials)?.as_deref() == Some(changed));
        Ok(())
    })
}

#[test]
fn deepseek_native_credentials_accept_empty_native_documents() -> Result<(), String> {
    for original in [
        "",
        "# Empty native store\n",
        "{}\n",
        "version: 1\nrefs: null\n",
        "version: 1\nrefs: {}\n",
    ] {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let profile = root.path().join("profiles/desktop/cordis.patch.yml");
        let credentials = root.path().join(".credentials.yaml");
        fs::write(&credentials, original).map_err(|_| FAILED)?;
        let manager = Manager {
            root: root.path().join("receipts"),
        };
        let menu = tests::fixture_menu();
        let selected = menu.default_model.as_deref().ok_or(FAILED)?;
        crate::services::agent_clients::with_test_config(&profile, || -> Result<(), String> {
            manager.install(manager.build(
                "deepseek-harness",
                &menu,
                "http://127.0.0.1:1",
                selected,
            )?)?;
            assert!(manager.status("deepseek-harness")?.configured);
            manager.disconnect("deepseek-harness")?;
            assert!(read(&credentials)?.as_deref() == Some(original));
            Ok(())
        })?;
    }
    Ok(())
}

#[test]
fn deepseek_orphan_gateway_credentials_can_be_replaced_and_restored() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|_| FAILED)?;
    let profile = root.path().join("profiles/desktop/cordis.patch.yml");
    let credentials = root.path().join(".credentials.yaml");
    let manager = Manager {
        root: root.path().join("receipts"),
    };
    let menu = tests::fixture_menu();
    let selected = menu.default_model.as_deref().ok_or(FAILED)?;
    crate::services::agent_clients::with_test_config(&profile, || -> Result<(), String> {
        let first = manager.build("deepseek-harness", &menu, "http://127.0.0.1:1", selected)?;
        let old_token = first.token.clone();
        manager.install(first)?;
        let baseline = (read(&profile)?, read(&credentials)?);
        fs::remove_file(manager.path("deepseek-harness")?).map_err(|_| FAILED)?;
        let replacement =
            manager.build("deepseek-harness", &menu, "http://127.0.0.1:2", selected)?;
        assert!(replacement.token != old_token);
        let token = replacement.token.clone();
        manager.install(replacement)?;
        let store = harness_credentials::parse(read(&credentials)?.as_deref())?;
        assert!(store["refs"][KEY].as_str() == Some(token.as_str()));
        assert!(manager.status("deepseek-harness")?.configured);
        manager.disconnect("deepseek-harness")?;
        assert!((read(&profile)?, read(&credentials)?) == baseline);
        Ok(())
    })
}

#[test]
fn deepseek_orphan_key_is_not_claimed_by_foreign_profiles() -> Result<(), String> {
    for (id, route, base) in [
        (
            "varyai-provider-shared-catalog",
            "varyai-shared-catalog",
            "https://synthetic.invalid/connections/deepseek-harness/v1",
        ),
        (
            "native-provider",
            "native",
            "http://127.0.0.1:1/connections/deepseek-harness/v1",
        ),
        (
            "mixed",
            "varyai-shared-catalog",
            "http://127.0.0.1:1/connections/deepseek-harness/v1",
        ),
        (
            "varyai-provider-shared-catalog",
            "varyai-shared-catalog",
            "http://127.0.0.1:1/native/v1",
        ),
    ] {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let profile = root.path().join("cordis.patch.yml");
        let credentials = root.path().join(".credentials.yaml");
        let original = "version: 1\nrefs: {VARYAI_HARNESS_GATEWAY_KEY: synthetic-foreign}\n";
        fs::write(&credentials, original).map_err(|_| FAILED)?;
        let adapter = if id == "native-provider" {
            id
        } else {
            "llm-pi-ai"
        };
        let mut document =
            json!([{"id":adapter,"config":{"providers":{route:{"apiKeyEnv":KEY,"baseURL":base}}}}]);
        if id == "mixed" {
            document[0]["config"]["providers"]["native"] =
                json!({"apiKeyEnv":KEY,"baseURL":"https://synthetic.invalid/v1"});
        }
        fs::write(
            &profile,
            serde_yaml::to_string(&document).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED)?;
        assert!(harness_credentials::prepare(&profile, "synthetic-new").is_err());
        assert!(read(&credentials)?.as_deref() == Some(original));
    }
    Ok(())
}
