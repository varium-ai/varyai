use super::*;

const BASE: &str = "http://127.0.0.1:1234/intent/v1";
const NATIVE: &str = "http://127.0.0.1:1234/backend-api/codex";
const STALE: &str = "1970-01-01T00:00:00Z";

fn cache_fixture(dir: &Path) -> Result<(PathBuf, Value), Box<dyn std::error::Error>> {
    let path = dir.join("models_cache.json");
    let value = json!({
        "fetched_at": "2099-01-01T00:00:00Z",
        "etag": "W/\"synthetic+magpie-old\"",
        "client_version": "0.166.0",
        "models": [{"slug": "synthetic-old-route", "display_name": "Synthetic QA · routing group"}],
        "future_field": {"preserved": true}
    });
    fs::write(&path, serde_json::to_vec_pretty(&value)?)?;
    Ok((path, value))
}

#[test]
fn takeover_expires_previous_gateway_model_cache_without_removing_metadata(
) -> Result<(), Box<dyn std::error::Error>> {
    for native in [false, true] {
        let (dir, manager) = tests::setup();
        let original = if native {
            "model='synthetic-old-route'\nopenai_base_url='http://127.0.0.1:4321/backend-api/codex'\n"
        } else {
            "model='synthetic-old-route'\nmodel_provider='magpie'\nmodel_catalog_json='magpie-models.json'\n[model_providers.magpie]\nbase_url='http://127.0.0.1:4321/v1'\n"
        };
        fs::write(&manager.config, original)?;
        if native {
            fs::write(
                dir.path().join("auth.json"),
                r#"{"auth_mode":"chatgpt","tokens":{"access_token":"synthetic"}}"#,
            )?;
        }
        let (path, mut expected) = cache_fixture(dir.path())?;
        manager.write_projection_for_intent(
            &[tests::group()],
            "synthetic-local-token",
            BASE,
            native.then_some(NATIVE),
            ApplyIntent::Reapply,
            None,
        )?;
        expected["fetched_at"] = json!(STALE);
        let actual: Value = serde_json::from_slice(&fs::read(&path)?)?;
        assert_eq!(actual, expected);
        let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        if native {
            assert!(doc.get("model_catalog_json").is_none());
            assert_eq!(doc["openai_base_url"].as_str(), Some(NATIVE));
        } else {
            assert_eq!(doc["model_provider"].as_str(), Some(PROVIDER));
        }
        manager.restore()?;
        tests::assert_restored_config(&manager, original);
    }
    Ok(())
}

#[test]
fn model_cache_refresh_is_idempotent_but_catalog_changes_and_reapply_expire_it(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    manager.write_projection(&[tests::group()], "synthetic-local-token", BASE)?;
    let (path, mut expected) = cache_fixture(dir.path())?;
    let before = fs::read(&path)?;
    manager.write_projection(&[tests::group()], "synthetic-local-token", BASE)?;
    assert_eq!(fs::read(&path)?, before);
    let mut changed = tests::group();
    changed.name = "Renamed synthetic route".into();
    manager.write_projection(&[changed.clone()], "synthetic-local-token", BASE)?;
    expected["fetched_at"] = json!(STALE);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path)?)?,
        expected
    );
    let (path, mut expected) = cache_fixture(dir.path())?;
    manager.write_projection_for_intent(
        &[changed],
        "synthetic-local-token",
        BASE,
        None,
        ApplyIntent::Reapply,
        None,
    )?;
    expected["fetched_at"] = json!(STALE);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&path)?)?,
        expected
    );
    Ok(())
}

#[test]
fn disconnect_expires_owned_cache_but_preserves_a_foreign_takeover_cache(
) -> Result<(), Box<dyn std::error::Error>> {
    for foreign in [false, true] {
        let (dir, manager) = tests::setup();
        manager.write_projection(&[tests::group()], "synthetic-local-token", BASE)?;
        let (path, mut expected) = cache_fixture(dir.path())?;
        if foreign {
            let mut doc: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
            doc["model_provider"] = toml_edit::value("synthetic-external");
            doc["model_catalog_json"] = toml_edit::value("external-models.json");
            fs::write(&manager.config, doc.to_string())?;
        }
        manager.disconnect()?;
        if !foreign {
            expected["fetched_at"] = json!(STALE);
        }
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(&path)?)?,
            expected
        );
    }
    Ok(())
}

#[test]
fn malformed_or_unwritable_model_cache_never_blocks_connection(
) -> Result<(), Box<dyn std::error::Error>> {
    for malformed in [false, true] {
        let (dir, manager) = tests::setup();
        let path = dir.path().join("models_cache.json");
        if malformed {
            fs::write(&path, "synthetic invalid JSON")?;
        } else {
            fs::create_dir(&path)?;
        }
        manager.write_projection(&[tests::group()], "synthetic-local-token", BASE)?;
        assert!(manager.status()?.configuration_owned);
        if malformed {
            assert_eq!(fs::read_to_string(path)?, "synthetic invalid JSON");
        } else {
            assert!(path.is_dir());
        }
    }
    Ok(())
}

#[test]
fn rejected_takeover_does_not_expire_model_cache() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(
        &manager.config,
        "openai_base_url='https://synthetic.example/codex'\n",
    )?;
    let (path, _) = cache_fixture(dir.path())?;
    let before = fs::read(&path)?;
    assert!(manager
        .write_projection_with_native(
            &[tests::group()],
            "synthetic-local-token",
            BASE,
            Some(NATIVE),
        )
        .is_err());
    assert_eq!(fs::read(&path)?, before);
    Ok(())
}
