use super::*;
use crate::services::agent_connections::{Manager, Model, Receipt};
use base64::{engine::general_purpose::STANDARD, Engine};

struct Fixture {
    _home: tempfile::TempDir,
    paths: ClaudeDesktopPaths,
    manager: Manager,
}

impl Fixture {
    fn new(legacy: bool) -> Result<Self, String> {
        let home = tempfile::tempdir().map_err(|_| "fixture")?;
        let paths = paths_from_dirs(home.path().join("Claude"), home.path().join("Claude-3p"));
        write_json_file(&paths.normal_config_path, &json!({"theme":"light"}))
            .map_err(|_| "fixture")?;
        write_json_file(&paths.threep_config_path, &json!({"locale":"en"}))
            .map_err(|_| "fixture")?;
        let mut meta =
            json!({"entries":[{"id":"previous","name":"Previous"}],"appliedId":"previous"});
        if legacy {
            write_json_file(&paths.profile_path, &json!({
                "inferenceProvider":"gateway", "inferenceGatewayBaseUrl":"https://synthetic.invalid",
                "inferenceGatewayApiKey":"synthetic-original", "customSetting":"original"
            })).map_err(|_| "fixture")?;
            meta["entries"]
                .as_array_mut()
                .ok_or("fixture")?
                .push(json!({"id":PROFILE_ID,"name":"Legacy"}));
        }
        write_json_file(&paths.meta_path, &meta).map_err(|_| "fixture")?;
        let manager = Manager {
            root: home.path().join("receipts"),
        };
        Ok(Self {
            _home: home,
            paths,
            manager,
        })
    }

    fn receipt(&self) -> Result<Receipt, String> {
        self.receipt_with_token("synthetic-local")
    }

    fn receipt_with_token(&self, token: &str) -> Result<Receipt, String> {
        let model = Model {
            api: "openai-completions",
            native_model: None,
            output_tokens: None,
            cost: None,
            icon: None,
            id: "synthetic/first".into(),
            name: "First".into(),
            context_window: None,
            compact_at: None,
            group: "Synthetic".into(),
            reasoning_levels: vec![],
            reasoning: false,
            default_reasoning_level: None,
            images: true,
        };
        let previous = self.manager.load("claude-desktop")?;
        let (files, mapping) = shared_catalog_edits_at(
            self.paths.clone(),
            "http://127.0.0.1:1/connections/claude-desktop",
            token,
            previous.as_ref().map(|receipt| receipt.token.as_str()),
            &[model],
        )?;
        Ok(Receipt {
            app: "claude-desktop".into(),
            token: token.into(),
            model: "synthetic/first".into(),
            model_map: mapping,
            visible_models: None,
            files,
            suspended: false,
            applied_at_ms: None,
        })
    }

    fn install(&self) -> Result<(), String> {
        self.manager.install(self.receipt()?)
    }

    fn profile(&self) -> Result<Value, String> {
        read_json_file(&self.paths.profile_path).map_err(|_| "fixture".into())
    }

    fn edit_profile(&self, edit: impl FnOnce(&mut Value)) -> Result<(), String> {
        let mut profile = self.profile()?;
        edit(&mut profile);
        write_json_file(&self.paths.profile_path, &profile).map_err(|_| "fixture".into())
    }

    fn seal(&self) -> Result<(), String> {
        self.edit_profile(|v| {
            v["inferenceGatewayApiKey"] = json!(format!(
                "__sealed__:v1:{}",
                STANDARD.encode(b"synthetic opaque seal")
            ))
        })
    }
}

#[test]
fn desktop_new_connection_replaces_an_opaque_key_from_a_retired_connection() -> Result<(), String> {
    let f = Fixture::new(false)?;
    f.install()?;
    f.seal()?;
    f.edit_profile(|v| v["customSetting"] = json!("retained"))?;
    let before = fs::read(&f.paths.profile_path).map_err(|_| "fixture")?;
    let next = f.receipt_with_token("synthetic-reconnected")?;
    let prepared = serde_json::to_value(&next).map_err(|_| "fixture")?;
    assert!(
        prepared["files"][2]["installed"]["inferenceGatewayApiKey"] == "synthetic-reconnected",
        "an unreadable seal cannot establish ownership of a new connection credential"
    );
    assert_eq!(
        prepared["files"][2]["installed"]["customSetting"],
        "retained"
    );
    assert!(fs::read(&f.paths.profile_path).map_err(|_| "fixture")? == before);
    let replacement = Manager {
        root: f._home.path().join("new-connection-receipts"),
    };
    replacement.install(next)?;
    assert!(f.profile()?["inferenceGatewayApiKey"] == "synthetic-reconnected");
    assert!(replacement.status("claude-desktop")?.configured);
    Ok(())
}

#[test]
fn sealed_desktop_key_keeps_connection_current_and_disconnect_restores_baseline(
) -> Result<(), String> {
    let f = Fixture::new(true)?;
    let original = fs::read(&f.paths.profile_path).map_err(|_| "fixture")?;
    f.install()?;
    f.seal()?;
    assert!(
        f.manager.status("claude-desktop")?.configured,
        "Desktop encryption is not configuration drift"
    );
    f.manager.disconnect("claude-desktop")?;
    assert!(fs::read(&f.paths.profile_path).map_err(|_| "fixture")? == original);
    assert!(!f.manager.status("claude-desktop")?.enabled);
    Ok(())
}

#[test]
fn desktop_projection_keeps_sealed_key_and_user_settings_byte_for_byte() -> Result<(), String> {
    let f = Fixture::new(false)?;
    f.install()?;
    f.seal()?;
    f.edit_profile(|v| {
        v["coworkEgressAllowedHosts"] = json!(["synthetic.invalid"]);
        v["disableDeploymentModeChooser"] = json!(false);
        v["customSetting"] = json!(true);
    })?;
    let before = fs::read(&f.paths.profile_path).map_err(|_| "fixture")?;
    let next = f.receipt()?;
    let prepared = serde_json::to_value(&next).map_err(|_| "fixture")?;
    assert!(prepared["files"][2]["installed"] == f.profile()?);
    let other = Manager {
        root: f._home.path().join("second-receipts"),
    };
    other.install(next)?;
    assert!(
        fs::read(&f.paths.profile_path).map_err(|_| "fixture")? == before,
        "unchanged sync must not reformat ciphertext"
    );
    Ok(())
}

#[test]
fn desktop_disconnect_merges_profile_entries_and_preserves_external_selection_and_mode(
) -> Result<(), String> {
    let f = Fixture::new(false)?;
    f.install()?;
    f.seal()?;
    write_json_file(
        &f.paths.normal_config_path,
        &json!({"deploymentMode":"consumer","theme":"dark"}),
    )
    .map_err(|_| "fixture")?;
    write_json_file(&f.paths.meta_path, &json!({
        "entries":[{"id":"previous","name":"Renamed"},{"id":PROFILE_ID,"name":"Vary AI"},{"id":"added","extra":true}],
        "appliedId":"added", "customMeta":true
    })).map_err(|_| "fixture")?;
    f.manager.disconnect("claude-desktop")?;
    let meta: Value = read_json_file(&f.paths.meta_path).map_err(|_| "fixture")?;
    assert_eq!(
        meta,
        json!({"entries":[{"id":"previous","name":"Renamed"},{"id":"added","extra":true}],"appliedId":"added","customMeta":true})
    );
    let normal: Value = read_json_file(&f.paths.normal_config_path).map_err(|_| "fixture")?;
    assert_eq!(normal, json!({"deploymentMode":"consumer","theme":"dark"}));
    assert!(!f.paths.profile_path.exists());
    Ok(())
}

#[test]
fn desktop_disconnect_chooses_remaining_profile_if_prior_selection_was_removed(
) -> Result<(), String> {
    let f = Fixture::new(false)?;
    f.install()?;
    write_json_file(
        &f.paths.meta_path,
        &json!({"entries":[{"id":PROFILE_ID},{"id":"added"}],"appliedId":PROFILE_ID}),
    )
    .map_err(|_| "fixture")?;
    f.manager.disconnect("claude-desktop")?;
    let meta: Value = read_json_file(&f.paths.meta_path).map_err(|_| "fixture")?;
    assert_eq!(
        meta,
        json!({"entries":[{"id":"added"}],"appliedId":"added"})
    );
    Ok(())
}

#[test]
fn desktop_disconnect_keeps_user_settings_for_next_connection() -> Result<(), String> {
    let f = Fixture::new(false)?;
    f.install()?;
    f.seal()?;
    f.edit_profile(|v| {
        v["customSetting"] = json!("user");
        v["coworkEgressAllowedHosts"] = json!([]);
    })?;
    f.manager.disconnect("claude-desktop")?;
    assert!(!f.paths.profile_path.exists());
    f.install()?;
    let profile = f.profile()?;
    assert_eq!(profile["customSetting"], "user");
    assert_eq!(profile["coworkEgressAllowedHosts"], json!([]));
    assert_eq!(
        profile["inferenceGatewayApiKey"], "synthetic-local",
        "cached ciphertext must not reuse a retired connection token"
    );
    Ok(())
}

#[test]
fn desktop_sealed_key_does_not_hide_real_gateway_changes_or_malformed_metadata(
) -> Result<(), String> {
    for field in [
        "inferenceGatewayApiKey",
        "inferenceGatewayBaseUrl",
        "inferenceProvider",
    ] {
        let f = Fixture::new(false)?;
        f.install()?;
        f.seal()?;
        f.edit_profile(|v| v[field] = json!("synthetic-external"))?;
        let before = fs::read(&f.paths.profile_path).map_err(|_| "fixture")?;
        assert!(!f.manager.status("claude-desktop")?.configured);
        assert!(f.manager.disconnect("claude-desktop").is_err());
        assert!(fs::read(&f.paths.profile_path).map_err(|_| "fixture")? == before);
        assert!(f.manager.load("claude-desktop")?.is_some());
    }
    let f = Fixture::new(false)?;
    f.install()?;
    f.seal()?;
    fs::write(
        &f.paths.meta_path,
        r#"{"entries":{},"appliedId":"external"}"#,
    )
    .map_err(|_| "fixture")?;
    assert!(f.manager.disconnect("claude-desktop").is_err());
    let normal: Value = read_json_file(&f.paths.normal_config_path).map_err(|_| "fixture")?;
    assert_eq!(
        normal["deploymentMode"], "3p",
        "all documents must validate before restoring any"
    );
    Ok(())
}

#[test]
fn desktop_suspend_accepts_sealed_key_and_can_finish_disconnect() -> Result<(), String> {
    let f = Fixture::new(false)?;
    f.install()?;
    f.seal()?;
    f.manager.suspend("claude-desktop")?;
    assert!(f
        .manager
        .load("claude-desktop")?
        .is_some_and(|r| r.suspended));
    f.manager.disconnect("claude-desktop")?;
    assert!(!f.manager.status("claude-desktop")?.enabled);
    Ok(())
}

#[test]
fn desktop_old_receipt_with_no_policy_recovers_sealed_key() -> Result<(), String> {
    let f = Fixture::new(true)?;
    f.install()?;
    let path = f.manager.root.join("claude-desktop.json");
    let mut receipt: Value = read_json_file(&path).map_err(|_| "fixture")?;
    for file in receipt["files"].as_array_mut().ok_or("fixture")? {
        file.as_object_mut().ok_or("fixture")?.remove("desktop");
    }
    write_json_file(&path, &receipt).map_err(|_| "fixture")?;
    f.seal()?;
    assert!(f.manager.status("claude-desktop")?.configured);
    f.manager.disconnect("claude-desktop")?;
    Ok(())
}

#[test]
fn sealed_keys_are_not_a_global_field_ownership_exception() -> Result<(), String> {
    let f = Fixture::new(false)?;
    let mut receipt = f.receipt()?;
    let mut raw = serde_json::to_value(&receipt).map_err(|_| "fixture")?;
    raw["app"] = json!("claude");
    for file in raw["files"].as_array_mut().ok_or("fixture")? {
        file.as_object_mut().ok_or("fixture")?.remove("desktop");
    }
    receipt = serde_json::from_value(raw).map_err(|_| "fixture")?;
    f.manager.install(receipt)?;
    f.seal()?;
    assert!(!f.manager.status("claude")?.configured);
    assert!(f.manager.disconnect("claude").is_err());
    Ok(())
}

#[test]
fn desktop_legacy_profile_user_edits_survive_disconnect_and_reconnect() -> Result<(), String> {
    let f = Fixture::new(true)?;
    f.install()?;
    assert_eq!(
        f.profile()?["customSetting"],
        "original",
        "connecting must not discard legacy user settings"
    );
    f.seal()?;
    f.edit_profile(|v| {
        v["customSetting"] = json!("edited");
        v["coworkEgressAllowedHosts"] = json!(["synthetic.invalid"]);
    })?;
    f.manager.disconnect("claude-desktop")?;
    let profile = f.profile()?;
    assert_eq!(profile["customSetting"], "edited");
    assert_eq!(
        profile["coworkEgressAllowedHosts"],
        json!(["synthetic.invalid"])
    );
    assert_eq!(
        profile["inferenceGatewayBaseUrl"],
        "https://synthetic.invalid"
    );
    f.install()?;
    assert_eq!(f.profile()?["customSetting"], "edited");
    Ok(())
}

#[test]
fn desktop_sealed_key_restores_a_partial_legacy_profile_without_leaking_projection_fields(
) -> Result<(), String> {
    let f = Fixture::new(false)?;
    let original = json!({"inferenceProvider":"gateway", "customSetting":true});
    write_json_file(&f.paths.profile_path, &original).map_err(|_| "fixture")?;
    f.install()?;
    f.seal()?;
    f.manager.disconnect("claude-desktop")?;
    assert!(
        f.profile()? == original,
        "key ownership must be assessed before restoring its gateway address"
    );
    Ok(())
}

#[test]
fn parity_desktop_carries_native_settings_and_preserves_later_user_edits() -> Result<(), String> {
    let f = Fixture::new(false)?;
    write_json_file(&f.paths.normal_config_path,&json!({"mcpServers":{"copied":{"command":"synthetic-a","args":["synthetic"]},"same":{"command":"normal"}}})).map_err(|_|"fixture")?;
    write_json_file(
        &f.paths.threep_config_path,
        &json!({"mcpServers":{"same":{"command":"threep"}}}),
    )
    .map_err(|_| "fixture")?;
    let normal_locale = f.paths.normal_config_path.with_file_name("config.json");
    let threep_locale = f.paths.threep_config_path.with_file_name("config.json");
    write_json_file(&normal_locale, &json!({"locale":"ja-JP","unrelated":true}))
        .map_err(|_| "fixture")?;
    f.install()?;
    assert_eq!(f.profile()?["toolSearchEnabled"], true);
    let mut native: Value = read_json_file(&f.paths.threep_config_path).map_err(|_| "fixture")?;
    assert_eq!(native["mcpServers"]["copied"]["command"], "synthetic-a");
    assert_eq!(native["mcpServers"]["same"]["command"], "threep");
    let locale: Value = read_json_file(&threep_locale).map_err(|_| "fixture")?;
    assert_eq!(locale["locale"], "ja-JP");
    native["mcpServers"]["copied"]["command"] = json!("user-changed");
    write_json_file(&f.paths.threep_config_path, &native).map_err(|_| "fixture")?;
    f.edit_profile(|v| {
        v.as_object_mut().map(|v| v.remove("toolSearchEnabled"));
    })?;
    let refreshed = f.receipt()?;
    let projected = serde_json::to_value(refreshed).map_err(|_| "fixture")?;
    assert!(projected["files"][2]["installed"]
        .get("toolSearchEnabled")
        .is_none());
    f.manager.disconnect("claude-desktop")?;
    let restored: Value = read_json_file(&f.paths.threep_config_path).map_err(|_| "fixture")?;
    assert_eq!(restored["mcpServers"]["copied"]["command"], "user-changed");
    assert_eq!(
        restored["mcpServers"]["copied"]["args"],
        json!(["synthetic"])
    );
    assert!(!threep_locale.exists());
    Ok(())
}

#[test]
fn parity_windows_packages_include_both_native_directory_sets() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|_| "fixture")?;
    let local = root.path().join("Local");
    let package = local.join("Packages/AnthropicPBC.Claude_synthetic/LocalCache");
    fs::create_dir_all(package.join("Roaming/Claude")).map_err(|_| "fixture")?;
    fs::create_dir_all(package.join("Local/Claude-3p")).map_err(|_| "fixture")?;
    let paths = windows_path_sets(&local, &root.path().join("Roaming"));
    assert_eq!(paths.len(), 2);
    assert!(paths.iter().any(|paths| paths
        .threep_config_path
        .starts_with(package.join("Local/Claude-3p"))));
    assert!(paths.iter().any(|paths| paths
        .threep_config_path
        .starts_with(local.join("Claude-3p"))));
    Ok(())
}
