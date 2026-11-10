use super::*;

const BASE: &str = "http://127.0.0.1:1234/intent/v1";
const TOKEN: &str = "fixture-alias-token";

fn publish(manager: &Manager, groups: &[RoutingGroup], runtime: bool) -> Result<(), String> {
    if runtime {
        use crate::proxy::intent::{Menu, Session};
        let sessions = groups
            .iter()
            .map(|group| {
                (
                    group.alias(),
                    Arc::new(Session {
                        group: group.clone(),
                        targets: vec![],
                        classifier: None,
                        client: reqwest::Client::new(),
                        created: std::time::Instant::now(),
                        bindings: Arc::default(),
                    }),
                )
            })
            .collect();
        manager.runtime.set_menu(Arc::new(Menu {
            token: TOKEN.into(),
            groups: sessions,
            models: Default::default(),
            default_model: groups.first().map(RoutingGroup::alias),
            omitted_entries: 0,
            context_settings: Default::default(),
        }))?;
    }
    manager.write_projection(groups, TOKEN, BASE)
}

fn provider(home: &Path, id: &str) -> Result<String, rusqlite::Error> {
    rusqlite::Connection::open(home.join("state_5.sqlite"))?.query_row(
        "SELECT model_provider FROM threads WHERE id=?1",
        [id],
        |row| row.get(0),
    )
}

#[test]
fn provider_migration_preserves_current_retired_and_unknown_model_identities(
) -> Result<(), Box<dyn std::error::Error>> {
    for runtime in [false, true] {
        for paginated in [false, true] {
            let (dir, manager) = tests::setup();
            fs::write(
                &manager.config,
                "model_provider='fixture-source'\nmodel='original'\n",
            )?;
            let mut group = tests::group();
            let canonical = group.alias();
            let mut retired = group.clone();
            retired.id = "retired-fixture".into();
            retired.name = "Retired fixture".into();
            publish(&manager, &[group.clone(), retired.clone()], runtime)?;
            let old = manager.load()?.ok_or("missing receipt")?;
            let original = old
                .model_aliases
                .iter()
                .find(|wire| old.model_map.get(*wire) == Some(&canonical))
                .ok_or("missing original alias")?
                .clone();
            let retired_name = old
                .model_aliases
                .iter()
                .find(|wire| old.model_map.get(*wire) == Some(&retired.alias()))
                .ok_or("missing retired alias")?
                .clone();
            group.name = "Renamed fixture".into();
            let mut replacement = retired.clone();
            replacement.id = "replacement-fixture".into();
            publish(&manager, &[group, replacement.clone()], runtime)?;
            let receipt = manager.load()?.ok_or("missing receipt")?;
            let current = receipt
                .model_aliases
                .iter()
                .find(|wire| receipt.model_map.get(*wire) == Some(&canonical))
                .ok_or("missing current alias")?
                .clone();
            assert_ne!(original, current);
            assert!(!receipt.model_aliases.contains(&retired_name));
            for (id, model) in [
                ("canonical", canonical.as_str()),
                ("current", current.as_str()),
                ("renamed", original.as_str()),
                ("retired", retired_name.as_str()),
                ("removed-id", retired.alias().as_str()),
                ("unknown", "unpublished-fixture"),
            ] {
                let path = mode_tests::thread_model_fixture(
                    dir.path(),
                    id,
                    "fixture-source",
                    paginated,
                    Some(model),
                    Some(model),
                )?;
                let before = fs::read(&path)?;
                manager.sync_threads()?;
                assert_eq!(
                    provider(dir.path(), id)?,
                    PROVIDER,
                    "runtime={runtime}, paginated={paginated}, id={id}"
                );
                if paginated {
                    assert_eq!(fs::read(&path)?, before);
                }
                let stored: String = rusqlite::Connection::open(dir.path().join("state_5.sqlite"))?
                    .query_row("SELECT model FROM threads WHERE id=?1", [id], |row| {
                        row.get(0)
                    })?;
                assert_eq!(stored, model);
                assert!(
                    !manager.thread_sync_pending(),
                    "runtime={runtime}, paginated={paginated}, id={id}"
                );
            }
            if !runtime {
                let restarted = Manager::new(
                    manager.config.clone(),
                    manager.receipt.clone(),
                    Arc::default(),
                );
                mode_tests::thread_model_fixture(
                    dir.path(),
                    "restart-readable",
                    "fixture-source",
                    paginated,
                    Some(&original),
                    Some(&original),
                )?;
                restarted.sync_threads()?;
                assert_eq!(provider(dir.path(), "restart-readable")?, PROVIDER);
            }
        }
    }
    Ok(())
}

#[test]
fn provider_migration_does_not_rewrite_or_depend_on_model_catalog_contents(
) -> Result<(), Box<dyn std::error::Error>> {
    for tamper in [
        "extra",
        "missing-hidden",
        "visible",
        "unrelated-path",
        "mapping",
    ] {
        let (dir, manager) = tests::setup();
        fs::write(&manager.config, "model_provider='fixture-source'\n")?;
        publish(&manager, &[tests::group()], false)?;
        let mut receipt = manager.load()?.ok_or("missing receipt")?;
        let model = receipt.model_aliases[0].clone();
        let path = receipt.catalogs.last().ok_or("missing catalog")?.clone();
        let mut catalog: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        match tamper {
            "extra" => catalog["models"]
                .as_array_mut()
                .ok_or("models")?
                .push(json!({"slug":"injected","visibility":"hide"})),
            "missing-hidden" => {
                catalog["models"]
                    .as_array_mut()
                    .ok_or("models")?
                    .retain(|entry| entry["visibility"] != "hide");
            }
            "visible" => catalog["models"][0]["slug"] = json!("injected"),
            "mapping" => {
                receipt
                    .model_map
                    .insert(model.clone(), "vary-provider/retired".into());
                fs::write(&manager.receipt, serde_json::to_vec(&receipt)?)?;
            }
            "unrelated-path" => {
                let external = dir.path().join("external").join("catalog.json");
                fs::create_dir_all(external.parent().ok_or("parent")?)?;
                fs::write(&external, serde_json::to_vec(&catalog)?)?;
                let mut installed: DocumentMut = receipt.installed.parse()?;
                installed["model_catalog_json"] =
                    toml_edit::value(external.to_string_lossy().as_ref());
                receipt.installed = installed.to_string();
                receipt.catalogs.push(external);
                fs::write(&manager.config, &receipt.installed)?;
                fs::write(&manager.receipt, serde_json::to_vec(&receipt)?)?;
            }
            _ => unreachable!(),
        }
        if matches!(tamper, "extra" | "missing-hidden" | "visible") {
            fs::write(&path, serde_json::to_vec(&catalog)?)?;
        }
        mode_tests::thread_model_fixture(
            dir.path(),
            "untrusted",
            "fixture-source",
            true,
            Some(&model),
            None,
        )?;
        let config_before = fs::read(&manager.config)?;
        let catalog_before = fs::read(&path)?;
        manager.sync_threads()?;
        assert_eq!(provider(dir.path(), "untrusted")?, PROVIDER, "{tamper}");
        assert_eq!(fs::read(&manager.config)?, config_before);
        assert_eq!(fs::read(&path)?, catalog_before);
    }
    Ok(())
}

#[test]
fn alias_integration_legacy_receipts_keep_exact_canonical_history_support(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(&manager.config, "model_provider='fixture-source'\n")?;
    let group = tests::group();
    publish(&manager, &[group.clone()], false)?;
    let mut receipt = manager.load()?.ok_or("missing receipt")?;
    let canonical = group.alias();
    receipt.model_map.clear();
    receipt.model_aliases = vec![canonical.clone()];
    let path = receipt.catalogs.last().ok_or("missing catalog")?;
    fs::write(
        path,
        serde_json::to_vec(&catalog(
            &[group],
            &Default::default(),
            "",
            &Default::default(),
        )?)?,
    )?;
    fs::write(&manager.receipt, serde_json::to_vec(&receipt)?)?;
    mode_tests::thread_model_fixture(
        dir.path(),
        "legacy-receipt",
        "fixture-source",
        true,
        Some(&canonical),
        None,
    )?;
    manager.sync_threads()?;
    assert_eq!(provider(dir.path(), "legacy-receipt")?, PROVIDER);
    Ok(())
}

#[test]
fn provider_migration_preserves_history_when_runtime_credentials_rotate(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    fs::write(&manager.config, "model_provider='fixture-source'\n")?;
    let group = tests::group();
    publish(&manager, &[group.clone()], true)?;
    let receipt = manager.load()?.ok_or("missing receipt")?;
    let mut menu = (*manager.runtime.menu_snapshot()?.ok_or("menu")?).clone();
    menu.token = "different-fixture-token".into();
    manager.runtime.set_menu(Arc::new(menu))?;
    for (id, model) in [
        ("canonical", group.alias()),
        ("readable", receipt.model_aliases[0].clone()),
    ] {
        mode_tests::thread_model_fixture(
            dir.path(),
            id,
            "fixture-source",
            true,
            Some(&model),
            None,
        )?;
        manager.sync_threads()?;
        assert_eq!(provider(dir.path(), id)?, PROVIDER);
    }
    Ok(())
}

#[test]
fn alias_integration_catalog_repair_preserves_renamed_capabilities_and_selection(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, manager) = tests::setup();
    let mut group = tests::group();
    group.levels = vec!["low".into(), "high".into()];
    publish(&manager, &[group.clone()], true)?;
    group.name = "Renamed fixture".into();
    publish(&manager, &[group.clone()], true)?;
    let receipt = manager.load()?.ok_or("missing receipt")?;
    let path = receipt.catalogs.last().ok_or("missing catalog")?;
    let before = fs::read(path)?;
    // This path is the private catalog recorded by the temporary fixture receipt.
    assert_eq!(path.parent(), manager.receipt.parent());
    fs::remove_file(path)?;
    manager.sync_configuration(BASE)?;
    assert!(
        fs::read(path)? == before,
        "repair must reproduce the complete published catalog"
    );
    assert_eq!(manager.selected_model()?, Some(group.alias()));
    assert_eq!(
        manager.load()?.ok_or("missing receipt")?.model_map,
        receipt.model_map
    );
    Ok(())
}

#[test]
fn alias_integration_catalog_repair_rejects_stale_or_unpublished_mapping(
) -> Result<(), Box<dyn std::error::Error>> {
    for tamper in ["token", "mapping", "visible", "menu", "owner"] {
        let (_dir, manager) = tests::setup();
        let group = tests::group();
        publish(&manager, &[group.clone()], true)?;
        let mut receipt = manager.load()?.ok_or("missing receipt")?;
        let path = receipt.catalogs.last().ok_or("missing catalog")?.clone();
        match tamper {
            "token" => {
                let mut menu = (*manager.runtime.menu_snapshot()?.ok_or("menu")?).clone();
                menu.token = "different-fixture-token".into();
                manager.runtime.set_menu(Arc::new(menu))?;
            }
            "menu" => {
                let mut menu = (*manager.runtime.menu_snapshot()?.ok_or("menu")?).clone();
                let session = menu.groups.get_mut(&group.alias()).ok_or("session")?;
                Arc::make_mut(session).group.name = "Changed unpublished name".into();
                manager.runtime.set_menu(Arc::new(menu))?;
            }
            "mapping" => {
                receipt.model_map.clear();
                fs::write(&manager.receipt, serde_json::to_vec(&receipt)?)?;
            }
            "visible" => {
                receipt.model_aliases = vec!["unpublished".into()];
                fs::write(&manager.receipt, serde_json::to_vec(&receipt)?)?;
            }
            "owner" => {
                let mut doc: DocumentMut = fs::read_to_string(&manager.config)?.parse()?;
                doc["model_provider"] = toml_edit::value("external");
                fs::write(&manager.config, doc.to_string())?;
            }
            _ => unreachable!(),
        }
        assert_eq!(path.parent(), manager.receipt.parent());
        fs::remove_file(&path)?;
        let _ = manager.sync_configuration(BASE);
        assert!(!path.exists(), "{tamper}");
    }
    Ok(())
}

#[test]
fn readable_selection_survives_rename_restart_and_legacy_selection() -> Result<(), String> {
    let (_dir, manager) = tests::setup();
    let mut group = tests::group();
    let canonical = group.alias();
    let base = "http://127.0.0.1:1/intent/v1";
    manager.write_projection(std::slice::from_ref(&group), "synthetic-token", base)?;
    let configured = || -> String {
        let doc: toml::Value = fs::read_to_string(&manager.config)
            .unwrap()
            .parse()
            .unwrap();
        doc["model"].as_str().unwrap().to_owned()
    };
    let original_name = format!("{}-routing-group", group.name.replace(' ', "-"));
    assert_eq!(configured(), original_name);
    assert_eq!(manager.selected_model()?, Some(canonical.clone()));
    group.name = "Renamed synthetic".into();
    manager.write_projection(std::slice::from_ref(&group), "synthetic-token", base)?;
    let renamed = format!("{}-routing-group", group.name.replace(' ', "-"));
    assert_eq!(configured(), renamed);
    let restarted = Manager::new(
        manager.config.clone(),
        manager.receipt.clone(),
        Arc::default(),
    );
    for selection in [&original_name, &canonical] {
        let mut doc: DocumentMut = fs::read_to_string(&manager.config)
            .unwrap()
            .parse()
            .unwrap();
        doc["model"] = toml_edit::value(selection.as_str());
        fs::write(&manager.config, doc.to_string()).unwrap();
        assert_eq!(restarted.selected_model()?, Some(canonical.clone()));
        restarted.write_projection(std::slice::from_ref(&group), "synthetic-token", base)?;
        assert_eq!(configured(), renamed);
    }
    restarted.restore()?;
    tests::assert_restored_config(&manager, "");
    Ok(())
}
