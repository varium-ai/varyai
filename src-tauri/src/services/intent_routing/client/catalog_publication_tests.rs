use super::*;

const BASE: &str = "http://127.0.0.1:1/intent/v1";
const TOKEN: &str = "synthetic-catalog-token";

#[test]
fn changed_catalog_refreshes_keep_one_path_past_the_old_publication_limit(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    let mut path = None;
    for revision in 0..80 {
        let mut group = tests::group();
        group.name = format!("Synthetic revision {revision}");
        manager.write_projection(&[group], TOKEN, BASE)?;
        let receipt = manager.load()?.ok_or("missing receipt")?;
        assert_eq!(receipt.catalogs.len(), 1);
        let current = receipt.catalogs.last().ok_or("missing catalog")?;
        assert_eq!(path.get_or_insert_with(|| current.clone()), current);
        let catalog: Value = serde_json::from_slice(&fs::read(current)?)?;
        assert_eq!(
            catalog["models"][0]["display_name"],
            format!("Synthetic revision {revision} · routing group")
        );
    }
    assert_eq!(
        fs::read_dir(dir.path())?
            .filter_map(Result::ok)
            .filter(|entry| manager.owns_catalog_path(&entry.path()))
            .count(),
        1
    );
    Ok(())
}

fn seed_legacy_publications(manager: &Manager) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    manager.write_projection(&[tests::group()], TOKEN, BASE)?;
    let mut receipt = manager.load()?.ok_or("missing receipt")?;
    let catalog = fs::read(receipt.catalogs.last().ok_or("missing catalog")?)?;
    for index in 1..64 {
        let path = manager
            .receipt
            .parent()
            .ok_or("missing parent")?
            .join(format!("vary-routing-catalog-legacy-{index}.json"));
        fs::write(&path, &catalog)?;
        receipt.catalogs.push(path);
    }
    let mut installed: DocumentMut = receipt.installed.parse()?;
    installed["model_catalog_json"] = toml_edit::value(
        receipt
            .catalogs
            .last()
            .ok_or("missing catalog")?
            .to_string_lossy()
            .as_ref(),
    );
    receipt.installed = installed.to_string();
    fs::write(&manager.config, &receipt.installed)?;
    fs::write(&manager.receipt, serde_json::to_vec(&receipt)?)?;
    Ok(receipt.catalogs)
}

#[test]
fn legacy_64_catalogs_allow_refresh_and_reconnect_after_restart(
) -> Result<(), Box<dyn std::error::Error>> {
    for (disconnected, missing_catalog) in [(false, false), (true, false), (true, true)] {
        let (_dir, manager) = tests::setup();
        fs::write(&manager.config, "model='original'\nuser_setting='keep'\n")?;
        let paths = seed_legacy_publications(&manager)?;
        let previous = fs::read(&paths[0])?;
        let aliases = manager.load()?.ok_or("missing receipt")?.model_map;
        if disconnected {
            manager.disconnect()?;
            assert!(!manager.receipt.exists());
            assert_eq!(
                manager
                    .connection_identity()?
                    .ok_or("missing identity")?
                    .catalogs
                    .len(),
                64
            );
        }
        if missing_catalog {
            fs::remove_file(paths.last().ok_or("missing last path")?)?;
        }
        let restarted = Manager::new(
            manager.config.clone(),
            manager.receipt.clone(),
            Arc::default(),
        );
        let mut group = tests::group();
        group.name = "Changed after legacy publications".into();
        let alias = group.alias();
        let menu = Arc::new(crate::proxy::intent::Menu {
            token: if disconnected {
                "synthetic-restarted-runtime-token".into()
            } else {
                TOKEN.into()
            },
            groups: std::collections::HashMap::from([(
                alias.clone(),
                Arc::new(crate::proxy::intent::Session {
                    group,
                    targets: Vec::new(),
                    classifier: None,
                    client: reqwest::Client::builder().no_proxy().build()?,
                    created: std::time::Instant::now(),
                    bindings: Arc::default(),
                }),
            )]),
            models: Default::default(),
            default_model: Some(alias.clone()),
            omitted_entries: 0,
            context_settings: Default::default(),
        });
        restarted.activate_menu_with_selection(menu, BASE, None, disconnected, Some(&alias))?;
        assert!(restarted.status()?.enabled);
        assert_eq!(
            restarted
                .runtime
                .menu_snapshot()?
                .ok_or("missing active menu")?
                .token,
            TOKEN
        );
        let receipt = restarted.load()?.ok_or("missing new receipt")?;
        assert_eq!(receipt.catalogs, paths);
        assert_eq!(receipt.model_map, aliases);
        assert_eq!(fs::read(&paths[0])?, previous);
        assert_ne!(
            fs::read(paths.last().ok_or("missing last path")?)?,
            previous
        );
        assert!(paths.iter().all(|path| path.is_file()));
        let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        assert_eq!(doc["model_provider"].as_str(), Some(PROVIDER));
        assert_eq!(
            doc["model_providers"][PROVIDER]["experimental_bearer_token"].as_str(),
            Some(TOKEN)
        );
        restarted.disconnect()?;
        let restored: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        assert_eq!(restored["model"].as_str(), Some("original"));
        assert_eq!(restored["user_setting"].as_str(), Some("keep"));
    }
    Ok(())
}

#[test]
fn catalog_publication_rolls_back_when_configuration_cannot_commit(
) -> Result<(), Box<dyn std::error::Error>> {
    for previous in [None, Some(r#"{"models":[]}"#)] {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("vary-routing-catalog-fixture.json");
        if let Some(previous) = previous {
            fs::write(&path, previous)?;
        }
        let next = r#"{"models":[{"slug":"synthetic-new"}]}"#;
        let result = catalog_file::publish(&path, previous, next, || {
            assert_eq!(fs::read_to_string(&path).ok().as_deref(), Some(next));
            Err(FAILED.into())
        });
        assert_eq!(result, Err(FAILED.into()));
        assert_eq!(read(&path)?.as_deref(), previous);
    }
    Ok(())
}

#[test]
fn catalog_publication_preserves_concurrent_edits_on_commit_and_rollback(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("vary-routing-catalog-fixture.json");
    let previous = r#"{"models":[]}"#;
    let next = r#"{"models":[{"slug":"synthetic-new"}]}"#;
    let external = r#"{"models":[{"slug":"synthetic-external"}]}"#;
    fs::write(&path, external)?;
    let committed = std::cell::Cell::new(false);
    assert!(catalog_file::publish(&path, Some(previous), next, || {
        committed.set(true);
        Ok(())
    })
    .is_err());
    assert!(!committed.get());
    assert_eq!(fs::read_to_string(&path)?, external);

    fs::write(&path, previous)?;
    assert!(catalog_file::publish(&path, Some(previous), next, || {
        fs::write(&path, external).map_err(|_| FAILED)?;
        Err(FAILED.into())
    })
    .is_err());
    assert_eq!(fs::read_to_string(&path)?, external);
    Ok(())
}

#[test]
fn malformed_catalog_is_backed_up_before_repair() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("vary-routing-catalog-fixture.json");
    let previous = "synthetic-malformed-catalog";
    let next = r#"{"models":[]}"#;
    fs::write(&path, previous)?;
    catalog_file::publish(&path, Some(previous), next, || Ok(()))?;
    let backups = fs::read_dir(dir.path())?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("codex-catalog-recovery-")
        })
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read_to_string(backups[0].path())?, previous);
    assert_eq!(fs::read_to_string(&path)?, next);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(backups[0].path())?.permissions().mode() & 0o777,
            0o600
        );
    }
    Ok(())
}
