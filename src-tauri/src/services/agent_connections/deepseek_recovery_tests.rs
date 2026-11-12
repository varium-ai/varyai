use super::*;

fn install(manager: &Manager, path: &Path) -> Result<(), String> {
    let file = FileEdit::prepare_format(path.into(), Format::Cordis, |value| {
        crate::services::deepseek_harness::apply(
            value,
            "shared-catalog",
            "VaryAI",
            &json!({"baseUrl":"http://127.0.0.1:1/v1","protocol":"openai","apiKey":"synthetic-only","models":[{"model":"shared-model"}]}),
        )
    })?;
    manager.install(Receipt {
        app: "deepseek-harness".into(),
        token: "synthetic-only".into(),
        model: "shared-model".into(),
        model_map: Default::default(),
        visible_models: None,
        files: vec![file],
        suspended: false,
        applied_at_ms: None,
    })
}

fn damage(path: &Path, change: impl FnOnce(&mut Value)) -> Result<String, String> {
    let source = read(path)?.ok_or(FAILED)?;
    let mut entries: Vec<Value> = serde_yaml::from_str(&source).map_err(|_| FAILED)?;
    let adapter = entries
        .iter()
        .position(|row| row["id"] == "llm-pi-ai")
        .ok_or(FAILED)?;
    let adapter = entries.remove(adapter);
    let selection = entries
        .iter()
        .position(|row| row["id"] == "agent-default-model")
        .ok_or(FAILED)?;
    let selection = entries.remove(selection);
    let mut managed = json!([adapter, selection]);
    change(&mut managed);
    let mut saved = managed.as_array().cloned().ok_or(FAILED)?;
    saved.extend(entries);
    let text = format!(
        "# Native customization\n{}",
        serde_yaml::to_string(&saved).map_err(|_| FAILED)?.replacen(
            "enabled: true",
            "enabled: false",
            1
        )
    );
    fs::write(path, &text).map_err(|_| FAILED)?;
    Ok(text)
}

#[test]
fn explicit_reapply_keeps_structural_client_saves_and_the_other_managers_disconnect(
) -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|_| FAILED)?;
    let manager = Manager {
        root: root.path().join("receipts"),
    };
    let path = root.path().join("cordis.patch.yml");
    fs::write(&path, "# Native customization\n- id: another-plugin\n  config:\n    enabled: true\n- id: llm-pi-ai\n  config:\n    providers:\n      native:\n        baseURL: https://native.invalid/v1\n").map_err(|_| FAILED)?;
    install(&manager, &path)?;
    let damaged = damage(&path, |entries| {
        entries[0]["config"]["providers"]
            .as_object_mut()
            .unwrap()
            .remove("native");
    })?;
    assert!(manager.status("deepseek-harness")?.configured);
    assert_eq!(
        read(&path)?.as_deref(),
        Some(damaged.as_str()),
        "polling must not repair external edits"
    );
    manager.release_for_reapply("deepseek-harness")?;
    let clean = read(&path)?.ok_or(FAILED)?;
    assert!(clean.starts_with("# Native customization\n"));
    assert!(clean.contains("enabled: false"));
    let parsed = crate::services::deepseek_harness::parse(&clean)?;
    assert_eq!(parsed["providers"], json!({}));
    assert!(
        !clean.contains("https://native.invalid/"),
        "a removed native provider must not return"
    );
    let backups = fs::read_dir(&manager.root)
        .map_err(|_| FAILED)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| FAILED)?;
    assert_eq!(backups.len(), 1);
    assert_eq!(read(&backups[0].path())?.as_deref(), Some(damaged.as_str()));
    install(&manager, &path)?;
    assert!(manager.status("deepseek-harness")?.configured);
    manager.disconnect("deepseek-harness")?;
    assert_eq!(read(&path)?.as_deref(), Some(clean.as_str()));
    Ok(())
}

#[test]
fn explicit_reapply_rewrites_changed_owned_fields() -> Result<(), String> {
    for field in ["baseURL", "headers", "models"] {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: root.path().join("receipts"),
        };
        let path = root.path().join("cordis.patch.yml");
        install(&manager, &path)?;
        let damaged = damage(&path, |entries| {
            entries[0]["config"]["providers"]["varyai-shared-catalog"][field] =
                json!("synthetic-change");
        })?;
        assert!(manager.status("deepseek-harness")?.recovery_pending);
        assert_eq!(read(&path)?.as_deref(), Some(damaged.as_str()));
        manager.release_for_reapply("deepseek-harness")?;
        assert_eq!(fs::read_dir(&manager.root).map_err(|_| FAILED)?.count(), 1);
        let clean = crate::services::deepseek_harness::parse(&read(&path)?.ok_or(FAILED)?)?;
        assert_eq!(clean["providers"], json!({}));
        install(&manager, &path)?;
        assert!(manager.status("deepseek-harness")?.configured);
    }
    Ok(())
}

#[test]
fn recovery_rejects_ambiguous_boundaries_and_invalid_documents() -> Result<(), String> {
    for case in [
        "duplicate_route",
        "invalid_yaml",
        "foreign_plugin",
        "foreign_provider",
    ] {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let manager = Manager {
            root: root.path().join("receipts"),
        };
        let path = root.path().join("cordis.patch.yml");
        install(&manager, &path)?;
        let damaged = damage(&path, |entries| {
            if case == "foreign_plugin" {
                entries[0]["name"] = json!("another-plugin");
            } else if case == "foreign_provider" {
                let routes = entries[0]["config"]["providers"].as_object_mut().unwrap();
                let provider = routes.remove("varyai-shared-catalog").unwrap();
                routes.insert("foreign-provider".into(), provider);
            }
        })?;
        let damaged = match case {
            "duplicate_route" => {
                let mut entries: Vec<Value> = serde_yaml::from_str(&damaged).map_err(|_| FAILED)?;
                entries.push(entries[0].clone());
                serde_yaml::to_string(&entries).map_err(|_| FAILED)?
            }
            "invalid_yaml" => format!("invalid: [\n{damaged}"),
            _ => damaged,
        };
        fs::write(&path, &damaged).map_err(|_| FAILED)?;
        assert!(manager.release_for_reapply("deepseek-harness").is_err());
        assert_eq!(read(&path)?.as_deref(), Some(damaged.as_str()));
        assert!(manager.path("deepseek-harness")?.is_file());
        assert_eq!(fs::read_dir(&manager.root).map_err(|_| FAILED)?.count(), 1);
    }
    Ok(())
}

#[test]
fn explicit_reapply_keeps_a_native_choice_for_later_disconnect() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|_| FAILED)?;
    let manager = Manager {
        root: root.path().join("receipts"),
    };
    let path = root.path().join("cordis.patch.yml");
    install(&manager, &path)?;
    damage(&path, |entries| {
        entries[1]["config"] = json!({"provider":"deepseek-official","model":"native-model"});
    })?;
    manager.release_for_reapply("deepseek-harness")?;
    let native = crate::services::deepseek_harness::parse(&read(&path)?.ok_or(FAILED)?)?;
    assert_eq!(
        crate::services::deepseek_harness::selection(&native),
        json!({"provider":"deepseek-official","model":"native-model"})
    );
    install(&manager, &path)?;
    manager.disconnect("deepseek-harness")?;
    let restored = crate::services::deepseek_harness::parse(&read(&path)?.ok_or(FAILED)?)?;
    assert_eq!(
        crate::services::deepseek_harness::selection(&restored),
        crate::services::deepseek_harness::selection(&native)
    );
    Ok(())
}
