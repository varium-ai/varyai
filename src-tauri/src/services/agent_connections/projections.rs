use super::{put, FileEdit, Format, Model, Receipt, CONFLICT, FAILED};
use serde_json::{json, Value};
use std::path::PathBuf;

pub(super) fn build(
    app: &str,
    catalog: &[Model],
    base: &str,
    token: &str,
    selected: &str,
    receipt: &mut Receipt,
) -> Result<(), String> {
    let path = match app {
        "gemini" => crate::gemini_config::get_gemini_env_path(),
        "grokbuild" => crate::grok_config::get_grok_config_path(),
        "hermes" => crate::hermes_config::get_hermes_config_path(),
        "openclaw" => crate::openclaw_config::get_openclaw_config_path(),
        "qoder" => crate::qoder_config::path(),
        "kimi" | "qwen" | "zcode" | "deepseek-harness" => {
            crate::services::agent_clients::config_path(app).map_err(|_| FAILED)?
        }
        _ => return Err(FAILED.into()),
    };
    receipt.model_map.clear();
    let mut projected = catalog.to_vec();
    let mut chosen = selected.to_owned();
    if ["kimi", "qwen", "zcode"].contains(&app) {
        for model in &mut projected {
            let internal = model.id.clone();
            let mut name = model.name.clone();
            let mut duplicate = 2;
            while receipt.model_map.contains_key(&name) {
                name = format!("{} ({duplicate})", model.name);
                duplicate += 1;
            }
            if internal == selected {
                chosen = name.clone();
            }
            receipt.model_map.insert(name.clone(), internal);
            model.id = name.clone();
            model.name = name;
        }
    } else if app == "gemini" {
        receipt.model_map = catalog
            .iter()
            .map(|m| (wire_id(&m.id), m.id.clone()))
            .collect();
    }
    receipt
        .files
        .push(edit(app, path, &projected, base, token, &chosen)?);
    if app == "gemini" {
        receipt.files.push(FileEdit::prepare(
            crate::gemini_config::get_gemini_settings_path(),
            |value| {
                set(
                    value,
                    &["security", "auth", "selectedType"],
                    json!("gemini-api-key"),
                )?;
                set(value, &["model", "name"], json!(wire_id(selected)))
            },
        )?);
    }
    Ok(())
}
pub(super) fn wire_id(alias: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("vary-{:x}", Sha256::digest(alias.as_bytes()))
}
fn set(value: &mut Value, path: &[&str], next: Value) -> Result<(), String> {
    put(
        value,
        &path.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
        Some(next),
    )
}
fn owned(value: Option<&Value>, token: &str, key: &str) -> Result<(), String> {
    if value.is_some_and(|v| v[key].as_str() != Some(token)) {
        return Err(CONFLICT.into());
    }
    Ok(())
}
fn edit(
    app: &str,
    path: PathBuf,
    catalog: &[Model],
    base: &str,
    token: &str,
    selected: &str,
) -> Result<FileEdit, String> {
    let format = match app {
        "gemini" => Format::Env,
        "grokbuild" | "kimi" => Format::Toml,
        "hermes" => Format::Yaml,
        "deepseek-harness" => Format::Cordis,
        "openclaw" => Format::Json5,
        _ => Format::Json,
    };
    FileEdit::prepare_format(path, format, |value| {
        match app {
            "gemini" => {
                for (key, next) in [
                    ("GOOGLE_GEMINI_BASE_URL", base.to_owned()),
                    ("GEMINI_API_KEY", token.to_owned()),
                    ("GEMINI_MODEL", wire_id(selected)),
                ] {
                    value[key] = json!(next);
                }
            }
            "grokbuild" => {
                let mut desired = serde_json::Map::new();
                for m in catalog {
                    let profile = wire_id(&m.id);
                    owned(
                        value.get("model").and_then(|v| v.get(&profile)),
                        token,
                        "api_key",
                    )?;
                    let mut row = json!({"model":m.id,"name":m.name,"base_url":format!("{base}/v1"),"api_key":token,"api_backend":"responses"});
                    if !m.reasoning_levels.is_empty() {
                        row["reasoning_efforts"] = json!(m.reasoning_levels);
                    }
                    if let Some(window) = super::working_window(m) {
                        row["context_window"] = json!(window);
                    }
                    desired.insert(profile, row);
                }
                let mut rows = value
                    .get("model")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                rows.retain(|_, row| row["api_key"].as_str() != Some(token));
                rows.extend(desired);
                value["model"] = json!(rows);
                set(value, &["models", "default"], json!(wire_id(selected)))?;
            }
            "openclaw" => {
                owned(value.pointer("/models/providers/varyai"), token, "apiKey")?;
                let previous = value
                    .pointer("/models/providers/varyai/models")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m["id"].as_str())
                    .map(|id| format!("varyai/{id}"))
                    .collect::<Vec<_>>();
                let rows = catalog.iter().map(|m| {
                    let mut row = json!({"id":m.id,"name":m.name,"input":if m.images {vec!["text","image"]} else {vec!["text"]}});
                    row["reasoning"] = json!(m.reasoning || m.reasoning_levels.iter().any(|level| level != "none"));
                    if let Some(w) = m.context_window { row["contextWindow"] = json!(w); }
                    row
                }).collect::<Vec<_>>();
                set(value, &["models", "mode"], json!("merge"))?;
                set(
                    value,
                    &["models", "providers", "varyai"],
                    json!({"baseUrl":format!("{base}/v1"),"apiKey":token,"api":"openai-completions","models":rows}),
                )?;
                set(
                    value,
                    &["agents", "defaults", "model", "primary"],
                    json!(format!("varyai/{selected}")),
                )?;
                // Extend a native allowlist without hiding the user's own models.
                if value.pointer("/agents/defaults/models").is_some() {
                    let allowlist = value
                        .pointer_mut("/agents/defaults/models")
                        .and_then(Value::as_object_mut)
                        .ok_or(FAILED)?;
                    for alias in previous {
                        allowlist.remove(&alias);
                    }
                    for m in catalog {
                        set(
                            value,
                            &["agents", "defaults", "models", &format!("varyai/{}", m.id)],
                            json!({"alias":m.name}),
                        )?;
                    }
                }
            }
            "hermes" => {
                let mut rows = match value.get("custom_providers") {
                    Some(v) => v.as_array().cloned().ok_or(FAILED)?,
                    None => vec![],
                };
                for row in rows.iter().filter(|r| r["name"] == "varyai") {
                    owned(Some(row), token, "api_key")?;
                }
                rows.retain(|r| r["name"] != "varyai");
                let models = catalog
                    .iter()
                    .map(|m| {
                        (
                            m.id.clone(),
                            m.context_window
                                .map_or(json!({}), |w| json!({"context_length":w})),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>();
                rows.push(json!({"name":"varyai","base_url":format!("{base}/v1"),"api_key":token,"model":selected,"models":models}));
                value["custom_providers"] = json!(rows);
                for (key, next) in [
                    ("default", json!(selected)),
                    ("provider", json!("varyai")),
                    ("base_url", json!(format!("{base}/v1"))),
                ] {
                    set(value, &["model", key], next)?;
                }
                if let Some(window) = catalog
                    .iter()
                    .find(|model| model.id == selected)
                    .and_then(|model| model.context_window)
                {
                    set(value, &["model", "context_length"], json!(window))?;
                } else {
                    super::put(value, &["model".into(), "context_length".into()], None)?;
                }
            }
            "qoder" => {
                owned(value.pointer("/providers/varyai"), token, "apiKey")?;
                let mut config = client_config(catalog, base, token, selected);
                if let Some(rows) = config["models"].as_array_mut() {
                    for row in rows {
                        if let Some(model) = catalog.iter().find(|model| row["model"] == model.id) {
                            super::reasoning::qoder(row, model);
                        }
                    }
                }
                config["name"] = json!("Vary AI");
                config["model"] = json!(selected);
                config["type"] = json!("openai-compatible");
                config["authType"] = json!("bearer");
                crate::qoder_config::validate_provider(&config).map_err(|_| FAILED)?;
                set(value, &["providers", "varyai"], config)?;
            }
            "kimi" | "qwen" | "zcode" | "deepseek-harness" => {
                match app {
                    "kimi" => owned(value.pointer("/providers/Vary AI"), token, "api_key")?,
                    "qwen" => {
                        if value
                            .pointer("/env/VARYAI_shared_catalog_API_KEY")
                            .is_some_and(|v| v.as_str() != Some(token))
                        {
                            return Err(CONFLICT.into());
                        }
                    }
                    "zcode" => {
                        for row in value
                            .pointer("/config/providerConfigRules/providerRules")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter(|r| r["providerId"] == "varyai-shared-catalog")
                        {
                            owned(row.pointer("/config/access"), token, "apiKey")?;
                        }
                    }
                    _ => {}
                }
                if app == "deepseek-harness" {
                    if let Some(provider) = value.pointer("/providers/varyai-provider-shared-catalog/config/providers/varyai-shared-catalog") {
                        if provider["headers"]["Authorization"] != format!("Bearer {token}") {
                            return Err(CONFLICT.into());
                        }
                    }
                }
                let source = if app == "deepseek-harness" {
                    crate::services::deepseek_harness::encode("", value)?
                } else if app == "kimi" {
                    toml::to_string(value).map_err(|_| FAILED)?
                } else {
                    value.to_string()
                };
                let mut doc = crate::services::agent_clients::Document::parse(app, &source)
                    .map_err(|_| FAILED)?;
                let mut config = client_config(catalog, base, token, selected);
                if app == "deepseek-harness" {
                    for row in config["models"].as_array_mut().ok_or(FAILED)? {
                        let model = catalog
                            .iter()
                            .find(|model| row["model"] == model.id)
                            .ok_or(FAILED)?;
                        row["input"] = if model.images {
                            json!(["text", "image"])
                        } else {
                            json!(["text"])
                        };
                        if !model.reasoning_levels.is_empty() {
                            let levels = model
                                .reasoning_levels
                                .iter()
                                .filter_map(|level| {
                                    match level.as_str() {
                                        "none" => Some(("off".to_owned(), Value::Null)),
                                        // Harness has a fixed selector enum; its wire values
                                        // can still request the upstream's strongest effort.
                                        "ultra" => Some(("max".to_owned(), json!(level))),
                                        "off" | "minimal" | "low" | "medium" | "high" | "xhigh"
                                        | "max" => Some((level.clone(), json!(level))),
                                        _ => None,
                                    }
                                })
                                .collect::<serde_json::Map<_, _>>();
                            if !levels.is_empty() {
                                row["reasoningEfforts"] = json!(levels);
                            }
                        }
                    }
                }
                doc.apply(app, "shared-catalog", "Vary AI", &config)
                    .map_err(|_| FAILED)?;
                *value = doc.value;
                for model in catalog {
                    if app == "kimi" {
                        super::reasoning::kimi(&mut value["models"][model.id.clone()], model);
                    } else if app == "zcode" {
                        for row in value
                            .pointer_mut("/config/modelConfigRules/providerModelRules")
                            .and_then(Value::as_array_mut)
                            .into_iter()
                            .flatten()
                            .filter(|row| {
                                row["providerId"] == "varyai-shared-catalog"
                                    && row["modelId"] == model.id
                            })
                        {
                            super::reasoning::zcode(&mut row["config"], model);
                        }
                    }
                }
                // Kimi requires a positive local ceiling even when upstream capacity
                // is unknown. Optional ZCode metadata can remain absent.
                for m in catalog.iter().filter(|m| m.context_window.is_none()) {
                    if app == "kimi" {
                        if let Some(row) = value["models"][m.id.clone()].as_object_mut() {
                            row.insert("max_context_size".into(), json!(32_000));
                        }
                    } else if app == "zcode" {
                        for row in value
                            .pointer_mut("/config/modelConfigRules/providerModelRules")
                            .and_then(Value::as_array_mut)
                            .into_iter()
                            .flatten()
                            .filter(|r| {
                                r["providerId"] == "varyai-shared-catalog" && r["modelId"] == m.id
                            })
                        {
                            if let Some(props) = row["config"]["properties"].as_object_mut() {
                                props.remove("contextWindow");
                            }
                        }
                    }
                }
            }
            _ => return Err(FAILED.into()),
        }
        Ok(())
    })
}
pub(super) fn client_config(catalog: &[Model], base: &str, token: &str, selected: &str) -> Value {
    let mut rows = catalog
        .iter()
        .map(|m| {
            let mut row = json!({"model":m.id,"displayName":m.name});
            if let Some(w) = m.context_window {
                row["contextWindow"] = json!(w);
            }
            row
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|m| m["model"].as_str() != Some(selected));
    json!({"baseUrl":format!("{base}/v1"),"apiKey":token,"protocol":"openai","models":rows})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepseek_desktop_projection_is_readable_reversible_and_keeps_foreign_edits(
    ) -> Result<(), String> {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = root.path().join("cordis.patch.yml");
        let original =
            "# Desktop customization\n- id: another-plugin\n  config:\n    enabled: true\n";
        std::fs::write(&path, original).map_err(|_| FAILED)?;
        let catalog = vec![Model {
            id: "vary-provider/synthetic-route".into(),
            name: "Readable Model · Supplier · VaryAI".into(),
            context_window: Some(128000),
            compact_at: None,
            group: "supplier".into(),
            reasoning_levels: vec!["none".into(), "high".into(), "max".into(), "ultra".into()],
            reasoning: true,
            default_reasoning_level: Some("high".into()),
            images: true,
            icon: None,
        }];
        let projection = edit(
            "deepseek-harness",
            path.clone(),
            &catalog,
            "http://127.0.0.1:1/connections/deepseek-harness",
            "synthetic-only",
            &catalog[0].id,
        )
        .map_err(|_| "DeepSeek projection preparation failed")?;
        projection
            .write()
            .map_err(|_| "DeepSeek projection write failed")?;
        let live = std::fs::read_to_string(&path).map_err(|_| FAILED)?;
        assert!(live.starts_with(original));
        let doc = crate::services::deepseek_harness::parse(&live)?;
        assert!(
            live.contains("id: agent-default-model"),
            "the chosen starting model reaches Harness"
        );
        let provider = &doc["providers"]["varyai-provider-shared-catalog"]["config"]["providers"]
            ["varyai-shared-catalog"];
        assert_eq!(provider["api"], "openai-completions");
        assert_eq!(provider["models"][0]["name"], catalog[0].name);
        assert_eq!(provider["models"][0]["input"], json!(["text", "image"]));
        assert_eq!(
            provider["models"][0]["reasoningEfforts"],
            json!({"off":null,"high":"high","max":"ultra"})
        );
        assert!(projection
            .intact()
            .map_err(|_| "DeepSeek initial ownership read failed")?);
        let changed = live.replacen("enabled: true", "enabled: false", 1);
        std::fs::write(&path, &changed).map_err(|_| FAILED)?;
        assert!(projection
            .intact()
            .map_err(|_| "DeepSeek foreign-edit ownership read failed")?);
        projection
            .restore()
            .map_err(|_| "DeepSeek projection restore failed")?;
        assert_eq!(
            std::fs::read_to_string(&path).map_err(|_| FAILED)?,
            original.replacen("enabled: true", "enabled: false", 1)
        );
        let missing = root.path().join("missing-profile.yml");
        let empty = edit(
            "deepseek-harness",
            missing.clone(),
            &catalog,
            "http://127.0.0.1:1",
            "synthetic-only",
            &catalog[0].id,
        )?;
        empty.write()?;
        empty.restore()?;
        assert!(
            !missing.exists(),
            "disconnect restores an absent profile file"
        );
        let baseline = "- id: agent-default-model\n  config:\n    provider: deepseek-official\n    model: deepseek-flash\n";
        std::fs::write(&path, baseline).map_err(|_| FAILED)?;
        let selected = edit(
            "deepseek-harness",
            path.clone(),
            &catalog,
            "http://127.0.0.1:1",
            "synthetic-only",
            &catalog[0].id,
        )?;
        selected.write()?;
        selected.restore()?;
        assert_eq!(
            std::fs::read_to_string(&path).map_err(|_| FAILED)?,
            baseline
        );
        let mut models = catalog.clone();
        let mut second = models[0].clone();
        second.id = "vary-provider/another-model".into();
        second.name = "Another Model · VaryAI".into();
        models.push(second);
        let choice = edit(
            "deepseek-harness",
            path.clone(),
            &models,
            "http://127.0.0.1:1",
            "synthetic-only",
            &models[1].id,
        )?;
        let manager = super::super::Manager {
            root: root.path().join("receipts"),
        };
        manager.install(Receipt {
            app: "deepseek-harness".into(),
            token: "synthetic-only".into(),
            model: models[1].id.clone(),
            model_map: Default::default(),
            visible_models: None,
            files: vec![choice],
            suspended: false,
            applied_at_ms: None,
        })?;
        let text = std::fs::read_to_string(&path).map_err(|_| FAILED)?;
        let mut native = crate::services::deepseek_harness::parse(&text)?;
        assert_eq!(
            crate::services::deepseek_harness::selection(&native),
            json!({"provider":"varyai-shared-catalog","model":models[1].id})
        );
        let external = json!({"provider":"deepseek-official","model":"native-choice"});
        crate::services::deepseek_harness::set_selection(&mut native, &external)?;
        std::fs::write(
            &path,
            crate::services::deepseek_harness::encode(&text, &native)?,
        )
        .map_err(|_| FAILED)?;
        assert!(manager.status("deepseek-harness")?.configured);
        manager.release_for_reapply("deepseek-harness")?;
        let restored = crate::services::deepseek_harness::parse(
            &std::fs::read_to_string(&path).map_err(|_| FAILED)?,
        )?;
        assert_eq!(
            crate::services::deepseek_harness::selection(&restored),
            external
        );
        assert!(restored["providers"].as_object().ok_or(FAILED)?.is_empty());
        let choice = edit(
            "deepseek-harness",
            path.clone(),
            &models,
            "http://127.0.0.1:1",
            "synthetic-only",
            &models[1].id,
        )?;
        manager.install(Receipt {
            app: "deepseek-harness".into(),
            token: "synthetic-only".into(),
            model: models[1].id.clone(),
            model_map: Default::default(),
            visible_models: None,
            files: vec![choice],
            suspended: false,
            applied_at_ms: None,
        })?;
        let text = std::fs::read_to_string(&path).map_err(|_| FAILED)?;
        let mut native = crate::services::deepseek_harness::parse(&text)?;
        crate::services::deepseek_harness::set_selection(
            &mut native,
            &json!({"provider":"varyai-shared-catalog","model":models[0].id}),
        )?;
        std::fs::write(
            &path,
            crate::services::deepseek_harness::encode(&text, &native)?,
        )
        .map_err(|_| FAILED)?;
        manager.release_for_reapply("deepseek-harness")?;
        let restored = crate::services::deepseek_harness::parse(
            &std::fs::read_to_string(&path).map_err(|_| FAILED)?,
        )?;
        assert_eq!(
            crate::services::deepseek_harness::selection(&restored),
            external,
            "removing a managed provider must not leave its default dangling"
        );
        Ok(())
    }
    #[test]
    fn shared_kimi_qwen_and_zcode_use_readable_models_and_keep_route_mappings() -> Result<(), String>
    {
        let catalog = vec![
            Model {
                icon: None,
                id: "vary-provider/route-hash-a".into(),
                name: "Synthetic Model · Supplier · VaryAI".into(),
                context_window: Some(200000),
                compact_at: None,
                group: "supplier".into(),
                reasoning_levels: vec!["low".into(), "high".into()],
                reasoning: true,
                default_reasoning_level: Some("low".into()),
                images: true,
            },
            Model {
                icon: None,
                id: "vary-provider/route-hash-b".into(),
                name: "Synthetic Model · Supplier · VaryAI".into(),
                context_window: None,
                compact_at: None,
                group: "supplier".into(),
                reasoning_levels: vec![],
                reasoning: false,
                default_reasoning_level: None,
                images: false,
            },
        ];
        for app in ["kimi", "qwen", "zcode"] {
            let dir = tempfile::tempdir().map_err(|_| FAILED)?;
            let path = dir.path().join("client-config");
            let original = if app == "kimi" {
                "theme='light'\n"
            } else if app == "zcode" {
                r#"{"schemaVersion":1,"config":{"providerConfigRules":{"providerRules":[]},"modelConfigRules":{"providerModelRules":[]}}}"#
            } else {
                r#"{"theme":"light"}"#
            };
            std::fs::write(&path, original).map_err(|_| FAILED)?;
            let legacy = edit(
                app,
                path.clone(),
                &catalog,
                "http://127.0.0.1:1/connections/test",
                "synthetic-token",
                &catalog[0].id,
            )?;
            legacy.write()?;
            let mut receipt = Receipt {
                app: app.into(),
                token: "synthetic-token".into(),
                model: catalog[0].id.clone(),
                model_map: Default::default(),
                visible_models: None,
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            };
            crate::services::agent_clients::with_test_config(&path, || {
                build(
                    app,
                    &catalog,
                    "http://127.0.0.1:1/connections/test",
                    "synthetic-token",
                    &catalog[0].id,
                    &mut receipt,
                )
            })?;
            assert_eq!(
                receipt.model_map.get(&catalog[0].name),
                Some(&catalog[0].id),
                "{app}"
            );
            assert_eq!(
                receipt.model_map.get(&format!("{} (2)", catalog[1].name)),
                Some(&catalog[1].id)
            );
            let installed = &receipt.files[0].installed;
            assert!(!installed.to_string().contains("route-hash"), "{app}");
            match app {
                "kimi" => {
                    let selected = installed["default_model"].as_str().ok_or(FAILED)?;
                    assert_eq!(selected, catalog[0].name);
                    assert!(installed["models"]
                        .as_object()
                        .ok_or(FAILED)?
                        .values()
                        .all(|row| row["max_context_size"].as_u64().is_some_and(|n| n > 0)));
                    assert_eq!(
                        installed["models"][format!("{} (2)", catalog[1].name)]["max_context_size"],
                        json!(32_000)
                    );
                    assert_eq!(catalog[1].context_window, None);
                    assert_eq!(installed["models"][selected]["model"], catalog[0].name);
                    assert_eq!(
                        installed["models"][selected]["support_efforts"],
                        json!(["low", "high"])
                    );
                }
                "qwen" => assert_eq!(installed["model"]["name"], catalog[0].name),
                "zcode" => assert_eq!(
                    installed["config"]["defaultModelSelection"]["modelId"],
                    catalog[0].name
                ),
                _ => unreachable!(),
            }
            receipt.files[0].write()?;
            let mut repeated = receipt.clone();
            repeated.files.clear();
            crate::services::agent_clients::with_test_config(&path, || {
                build(
                    app,
                    &catalog,
                    "http://127.0.0.1:1/connections/test",
                    "synthetic-token",
                    &catalog[0].id,
                    &mut repeated,
                )
            })?;
            assert_eq!(
                receipt.files[0].installed, repeated.files[0].installed,
                "{app} refresh"
            );
        }
        Ok(())
    }
    #[test]
    fn grok_long_conversations_use_compaction_without_shrinking_the_shared_capacity(
    ) -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let mut catalog = vec![Model {
            icon: None,
            id: "auto/shared".into(),
            name: "Shared".into(),
            context_window: Some(1_000_000),
            compact_at: Some(272_000),
            group: "routing".into(),
            reasoning_levels: vec![],
            reasoning: false,
            default_reasoning_level: None,
            images: true,
        }];
        let path = dir.path().join("grok.toml");
        let projection = edit(
            "grokbuild",
            path.clone(),
            &catalog,
            "http://127.0.0.1:1",
            "synthetic-local",
            &catalog[0].id,
        )?;
        let key = wire_id(&catalog[0].id);
        assert_eq!(
            projection.installed["model"][&key]["context_window"],
            272_000
        );
        assert_eq!(catalog[0].context_window, Some(1_000_000));
        catalog[0].compact_at = None;
        let full = edit(
            "grokbuild",
            path,
            &catalog,
            "http://127.0.0.1:1",
            "synthetic-local",
            &catalog[0].id,
        )?;
        assert_eq!(full.installed["model"][&key]["context_window"], 1_000_000);
        Ok(())
    }
    #[test]
    fn eight_native_projections_are_private_reversible_and_refreshable() -> Result<(), String> {
        let catalog = vec![Model {
            icon: None,
            id: "auto/shared".into(),
            name: "Shared".into(),
            context_window: Some(1_000_000),
            compact_at: None,
            group: "routing".into(),
            reasoning_levels: vec!["none".into(), "low".into(), "high".into()],
            reasoning: true,
            default_reasoning_level: None,
            images: true,
        }];
        for app in [
            "gemini",
            "grokbuild",
            "openclaw",
            "hermes",
            "qoder",
            "kimi",
            "qwen",
            "zcode",
        ] {
            let dir = tempfile::tempdir().map_err(|_| FAILED)?;
            let path = dir.path().join("native");
            let original = match app {
                "gemini" => "# keep\nTHEME=light\n",
                "grokbuild" | "kimi" => "# keep\ntheme='light'\n",
                "hermes" => "# keep\ntheme: light\n",
                _ => "{\"theme\":\"light\"}",
            };
            std::fs::write(&path, original).map_err(|_| FAILED)?;
            if app == "zcode" {
                std::fs::write(&path, r#"{"schemaVersion":1,"config":{"providerConfigRules":{"providerRules":[]},"modelConfigRules":{"providerModelRules":[]}}}"#).map_err(|_| FAILED)?;
            }
            let baseline = std::fs::read_to_string(&path).map_err(|_| FAILED)?;
            let projection = edit(
                app,
                path.clone(),
                &catalog,
                "http://127.0.0.1:1/connections/test",
                "synthetic-local",
                &catalog[0].id,
            )?;
            projection.write()?;
            assert!(projection.intact()?, "{app}");
            let installed = &projection.installed;
            match app {
                "grokbuild" => assert_eq!(
                    installed["model"][wire_id(&catalog[0].id)]["reasoning_efforts"],
                    json!(["none", "low", "high"])
                ),
                "openclaw" => assert_eq!(
                    installed["models"]["providers"]["varyai"]["models"][0]["reasoning"],
                    true
                ),
                "qoder" => assert_eq!(
                    installed["providers"]["varyai"]["models"][0]["capabilities"]["thinking"]
                        ["supportedEffortLevels"],
                    json!(["low", "high"])
                ),
                "kimi" => assert_eq!(
                    installed["models"][catalog[0].id.clone()]["support_efforts"],
                    json!(["low", "high"])
                ),
                "zcode" => assert_eq!(
                    installed["config"]["modelConfigRules"]["providerModelRules"][0]["config"]
                        ["optionSpecs"]["reasoningLevel"]["values"],
                    json!(["disabled", "low", "high"])
                ),
                _ => {}
            }
            let repeated = edit(
                app,
                path.clone(),
                &catalog,
                "http://127.0.0.1:1/connections/test",
                "synthetic-local",
                &catalog[0].id,
            )?;
            assert!(projection.installed == repeated.installed, "{app}");
            let mut renamed = catalog.clone();
            renamed[0].id = "provider/other".into();
            renamed[0].context_window = None;
            let mut refreshed = edit(
                app,
                path.clone(),
                &renamed,
                "http://127.0.0.1:2/connections/test",
                "synthetic-local",
                &renamed[0].id,
            )?;
            refreshed.write()?;
            if app == "kimi" {
                assert!(refreshed.installed["models"]
                    .as_object()
                    .is_some_and(|rows| rows
                        .values()
                        .filter(|row| row["provider"] == "Vary AI")
                        .all(|row| row["max_context_size"] == json!(32_000))));
            }
            if app == "zcode" {
                assert!(refreshed
                    .installed
                    .pointer("/config/modelConfigRules/providerModelRules")
                    .and_then(Value::as_array)
                    .is_some_and(|rows| rows
                        .iter()
                        .filter(|row| row["providerId"] == "varyai-shared-catalog")
                        .all(|row| row.pointer("/config/properties/contextWindow").is_none())));
            }
            refreshed.original = projection.original.clone();
            refreshed.fields.extend(projection.fields.iter().cloned());
            refreshed.fields.sort();
            refreshed.fields.dedup();
            refreshed.restore()?;
            assert_eq!(
                std::fs::read_to_string(&path).map_err(|_| FAILED)?,
                baseline,
                "{app} refresh"
            );
            projection.write()?;
            projection.restore()?;
            assert_eq!(
                std::fs::read_to_string(&path).map_err(|_| FAILED)?,
                baseline,
                "{app}"
            );
        }
        Ok(())
    }
}
