use super::*;

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
    assert!(!manager.config.exists());
    Ok(())
}
