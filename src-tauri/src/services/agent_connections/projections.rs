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
        "kimi" | "qwen" | "zcode" => {
            crate::services::agent_clients::config_path(app).map_err(|_| FAILED)?
        }
        _ => return Err(FAILED.into()),
    };
    let wire = if app == "gemini" {
        catalog
            .iter()
            .map(|m| (wire_id(&m.id), m.id.clone()))
            .collect()
    } else {
        Default::default()
    };
    receipt.model_map = wire;
    receipt
        .files
        .push(edit(app, path, catalog, base, token, selected)?);
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
                config["name"] = json!("Vary AI");
                config["model"] = json!(selected);
                config["type"] = json!("openai-compatible");
                config["authType"] = json!("bearer");
                crate::qoder_config::validate_provider(&config).map_err(|_| FAILED)?;
                set(value, &["providers", "varyai"], config)?;
            }
            "kimi" | "qwen" | "zcode" => {
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
                let source = if app == "kimi" {
                    toml::to_string(value).map_err(|_| FAILED)?
                } else {
                    value.to_string()
                };
                let mut doc = crate::services::agent_clients::Document::parse(app, &source)
                    .map_err(|_| FAILED)?;
                doc.apply(
                    app,
                    "shared-catalog",
                    "Vary AI",
                    &client_config(catalog, base, token, selected),
                )
                .map_err(|_| FAILED)?;
                *value = doc.value;
                // Legacy adapters supply guessed capacities; shared catalogs must not.
                for m in catalog.iter().filter(|m| m.context_window.is_none()) {
                    if app == "kimi" {
                        if let Some(row) = value["models"]
                            [format!("varyai-shared-catalog-{}", m.id)]
                        .as_object_mut()
                        {
                            row.remove("max_context_size");
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
            reasoning_levels: vec![],
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
                        .all(|row| row.get("max_context_size").is_none())));
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
