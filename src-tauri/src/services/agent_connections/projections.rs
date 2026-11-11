use super::{put, FileEdit, Format, Model, Receipt, CONFLICT, FAILED};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub(super) fn named_catalog(
    catalog: &[Model],
    selected: &str,
    model_map: &mut BTreeMap<String, String>,
) -> (Vec<Model>, String) {
    let mut projected = catalog.to_vec();
    let mut chosen = selected.to_owned();
    for model in &mut projected {
        let internal = model.id.clone();
        let mut name = model_map
            .iter()
            .find(|(_, alias)| *alias == &internal)
            .map(|(wire, _)| wire.clone())
            .unwrap_or_else(|| model.name.clone());
        let mut duplicate = 2;
        while model_map.get(&name).is_some_and(|alias| alias != &internal) {
            name = format!("{} ({duplicate})", model.name);
            duplicate += 1;
        }
        if internal == selected {
            chosen = name.clone();
        }
        model_map.insert(name.clone(), internal);
        model.id = name.clone();
        model.name = name;
    }
    (projected, chosen)
}

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
    let (projected, chosen) = if ["grokbuild", "hermes", "kimi", "qwen", "zcode"].contains(&app) {
        named_catalog(catalog, selected, &mut receipt.model_map)
    } else {
        (catalog.to_vec(), selected.to_owned())
    };
    if app == "gemini" {
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
        "qwen" => Format::Jsonc,
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
                    let profile = m.name.clone();
                    owned(
                        value.get("model").and_then(|v| v.get(&profile)),
                        token,
                        "api_key",
                    )?;
                    let backend = match m.api {
                        "openai-responses" => "responses",
                        "anthropic-messages" => "messages",
                        _ => "chat_completions",
                    };
                    let mut row = json!({"model":m.id,"name":m.name,"base_url":format!("{base}/v1"),"api_key":token,"api_backend":backend});
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
                let profile = catalog
                    .iter()
                    .find(|m| m.id == selected)
                    .map(|m| m.name.as_str())
                    .unwrap_or(selected);
                set(value, &["models", "default"], json!(profile))?;
                set(value, &["features", "campaigns"], json!(false))?;
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
                let model_path = if value
                    .pointer("/agents/defaults/model")
                    .is_some_and(Value::is_string)
                {
                    vec!["agents", "defaults", "model"]
                } else {
                    vec!["agents", "defaults", "model", "primary"]
                };
                set(value, &model_path, json!(format!("varyai/{selected}")))?;
                if let Some(allow) = value.pointer_mut("/agents/defaults/modelPolicy/allow") {
                    let allow = allow.as_array_mut().ok_or(FAILED)?;
                    if !allow
                        .iter()
                        .any(|entry| matches!(entry.as_str(), Some("*") | Some("varyai/*")))
                    {
                        allow.push(json!("varyai/*"));
                    }
                }
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
                // Current Hermes reads named entries under providers, not custom_providers.
                if let Some(rows) = value.get_mut("custom_providers") {
                    let rows = rows.as_array_mut().ok_or(FAILED)?;
                    for row in rows.iter().filter(|row| row["name"] == "varyai") {
                        owned(Some(row), token, "api_key")?;
                    }
                    rows.retain(|row| row["name"] != "varyai");
                    if rows.is_empty() {
                        value
                            .as_object_mut()
                            .ok_or(FAILED)?
                            .remove("custom_providers");
                    }
                }
                let split = !value
                    .pointer("/providers/varyai/api_mode")
                    .is_some_and(|mode| mode != "chat_completions")
                    && catalog
                        .iter()
                        .any(|model| model.api == "anthropic-messages");
                for (id, mode) in [
                    ("varyai", "chat_completions"),
                    ("varyai-anthropic", "anthropic_messages"),
                ] {
                    owned(
                        value
                            .get("providers")
                            .and_then(|providers| providers.get(id)),
                        token,
                        "api_key",
                    )?;
                    if id == "varyai-anthropic" && !split {
                        if let Some(providers) =
                            value.get_mut("providers").and_then(Value::as_object_mut)
                        {
                            providers.remove(id);
                        }
                        continue;
                    }
                    let mut row = value
                        .get("providers")
                        .and_then(|providers| providers.get(id))
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let mode = row["api_mode"]
                        .as_str()
                        .filter(|mode| {
                            matches!(
                                *mode,
                                "chat_completions" | "anthropic_messages" | "responses"
                            )
                        })
                        .unwrap_or(mode)
                        .to_owned();
                    row["name"] = json!(id);
                    row["base_url"] = json!(if mode == "anthropic_messages" {
                        base.to_owned()
                    } else {
                        format!("{base}/v1")
                    });
                    row["api_key"] = json!(token);
                    row["api_mode"] = json!(mode);
                    if row.get("extra_headers").is_none() {
                        row["extra_headers"] = json!({});
                    }
                    row["extra_headers"]["User-Agent"] = json!("hermes-agent");
                    row["models"] = json!(catalog
                        .iter()
                        .filter(|model| !split
                            || (id == "varyai-anthropic") == (model.api == "anthropic-messages"))
                        .map(|model| model.id.clone())
                        .collect::<Vec<_>>());
                    set(value, &["providers", id], row)?;
                }
                let provider = if split
                    && catalog
                        .iter()
                        .any(|model| model.id == selected && model.api == "anthropic-messages")
                {
                    "varyai-anthropic"
                } else {
                    "varyai"
                };
                set(value, &["model", "default"], json!(selected))?;
                set(value, &["model", "provider"], json!(provider))?;
                // Hermes resolves the endpoint from the selected provider entry.
                super::put(value, &["model".into(), "base_url".into()], None)?;
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
                    crate::services::deepseek_harness::project(
                        value, catalog, base, token, selected,
                    )?;
                    return Ok(());
                }
                let source = if app == "kimi" {
                    toml::to_string(value).map_err(|_| FAILED)?
                } else {
                    value.to_string()
                };
                let mut doc = crate::services::agent_clients::Document::parse(app, &source)
                    .map_err(|_| FAILED)?;
                let config = client_config(catalog, base, token, selected);
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
                            row.insert("max_context_size".into(), json!(128_000));
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
            if let Some(tokens) = super::output_limit(m) {
                row["maxOutputTokens"] = json!(tokens);
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
    fn harness_legacy_receipts_survive_unmarked_client_saves_and_disconnect() -> Result<(), String>
    {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = root.path().join("cordis.patch.yml");
        let source = "# Native profile\n- id: llm-pi-ai\n  config:\n    providers:\n      native:\n        baseURL: https://synthetic.invalid/v1\n        models: [{id: native-model}]\n- id: agent-default-model\n  config: {provider: native, model: native-model}\n";
        std::fs::write(&path, source).map_err(|_| FAILED)?;
        let mut doc = crate::services::deepseek_harness::parse(source)?;
        crate::services::deepseek_harness::apply(
            &mut doc,
            "shared-catalog",
            "Vary AI",
            &json!({"baseUrl":"http://127.0.0.1:1/v1","protocol":"openai","apiKey":"synthetic-only","models":[{"model":"shared-model"}]}),
        )?;
        let mut file = FileEdit::prepare_format(path.clone(), Format::Cordis, |value| {
            *value = doc.clone();
            Ok(())
        })?;
        let native: Value = serde_yaml::from_str(source).map_err(|_| FAILED)?;
        let mut legacy = doc.clone();
        legacy["patches"] = native.clone();
        file.installed = legacy;
        let mut rows = native.as_array().cloned().ok_or(FAILED)?;
        let mut route = rows[0].clone();
        route["name"] = json!("@deepseek-ai/dsh-llm-pi-ai");
        route["config"]["providers"]["varyai-shared-catalog"] = doc["providers"]
            ["varyai-provider-shared-catalog"]["config"]["providers"]["varyai-shared-catalog"]
            .clone();
        rows.push(route);
        rows.push(doc["selection"][0].clone());
        let unmarked = format!(
            "{}# END VaryAI model providers\n",
            serde_yaml::to_string(&rows).map_err(|_| FAILED)?
        );
        std::fs::write(&path, unmarked).map_err(|_| FAILED)?;
        let manager = super::super::Manager {
            root: root.path().join("receipts"),
        };
        std::fs::create_dir_all(&manager.root).map_err(|_| FAILED)?;
        let receipt = Receipt {
            app: "deepseek-harness".into(),
            token: "synthetic-only".into(),
            model: "shared-model".into(),
            model_map: Default::default(),
            visible_models: None,
            files: vec![file],
            suspended: false,
            applied_at_ms: None,
        };
        std::fs::write(
            manager.root.join("deepseek-harness.json"),
            serde_json::to_vec(&receipt).map_err(|_| FAILED)?,
        )
        .map_err(|_| FAILED)?;
        assert!(manager.status("deepseek-harness")?.configured);
        manager.disconnect("deepseek-harness")?;
        assert_eq!(std::fs::read_to_string(path).map_err(|_| FAILED)?, source);
        Ok(())
    }

    #[test]
    fn qwen_jsonc_provider_edits_retain_comments_and_context() -> Result<(), String> {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = root.path().join("settings.json");
        let source = "{\n // Native preference\n \"theme\":\"synthetic\",\n}\n";
        std::fs::write(&path, source).map_err(|_| FAILED)?;
        let config = json!({"baseUrl":"http://127.0.0.1:1/v1","protocol":"openai","apiKey":"synthetic-only","models":[{"model":"shared-model","contextWindow":128000}]});
        let mut doc =
            crate::services::agent_clients::Document::parse("qwen", source).map_err(|_| FAILED)?;
        doc.apply("qwen", "shared-catalog", "Vary AI", &config)
            .map_err(|_| FAILED)?;
        let encoded = doc.encode().map_err(|_| FAILED)?;
        assert!(encoded.contains("// Native preference"));
        assert_eq!(
            super::super::jsonc::parse(&encoded)?["modelProviders"]["openai"][0]
                ["generationConfig"]["contextWindowSize"],
            128000
        );
        Ok(())
    }

    #[test]
    fn magpie_harness_protocol_routes_limits_and_native_provider_schema() -> Result<(), String> {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let catalog = [
            "openai-completions",
            "openai-responses",
            "anthropic-messages",
        ]
        .map(|api| Model {
            api,
            native_model: if api == "anthropic-messages" {
                Some("claude-opus-5-5".into())
            } else {
                None
            },
            output_tokens: Some(393216),
            cost: None,
            id: api.into(),
            name: api.into(),
            context_window: Some(500000),
            compact_at: None,
            group: "Synthetic".into(),
            reasoning_levels: vec!["none".into(), "high".into()],
            reasoning: true,
            default_reasoning_level: None,
            images: true,
            icon: None,
        });
        let path = root.path().join("cordis.patch.yml");
        let file = edit(
            "deepseek-harness",
            path.clone(),
            &catalog,
            "http://127.0.0.1:1",
            "synthetic-only",
            "anthropic-messages",
        )?;
        file.write()?;
        let doc = crate::services::deepseek_harness::parse(
            &std::fs::read_to_string(&path).map_err(|_| FAILED)?,
        )?;
        assert_eq!(
            crate::services::deepseek_harness::selection(&doc)["provider"],
            "varyai-shared-catalog-anthropic"
        );
        for (suffix, api) in [
            ("", "openai-completions"),
            ("-responses", "openai-responses"),
            ("-anthropic", "anthropic-messages"),
        ] {
            let profile = &doc["providers"][format!("varyai-provider-shared-catalog{suffix}")]
                ["config"]["providers"][format!("varyai-shared-catalog{suffix}")];
            assert_eq!(profile["api"], api);
            assert_eq!(profile["streamIdleTimeoutMs"], 600000);
            assert_eq!(profile["models"][0]["maxTokens"], 184464);
            if api == "anthropic-messages" {
                assert_eq!(
                    profile["models"][0]["compat"]["forceAdaptiveThinking"],
                    true
                );
            }
            assert_eq!(
                profile["baseURL"],
                if api == "anthropic-messages" {
                    "http://127.0.0.1:1"
                } else {
                    "http://127.0.0.1:1/v1"
                }
            );
        }
        assert_eq!(
            super::super::pi_model(&catalog[2])["compat"]["forceAdaptiveThinking"],
            true
        );
        let mut saved = doc.clone();
        let profile = &mut saved["providers"]["varyai-provider-shared-catalog"]["config"]
            ["providers"]["varyai-shared-catalog"];
        profile["api"] = json!("openai-responses");
        profile["streamIdleTimeoutMs"] = json!(120000);
        profile["headers"]["User-Agent"] = json!("synthetic-client");
        profile["models"][0]["compat"] = json!({"supportsStore":false});
        let mut adopted = crate::services::deepseek_harness::adopt(&doc, &saved);
        assert_eq!(adopted, saved);
        crate::services::deepseek_harness::project(
            &mut adopted,
            &catalog,
            "http://127.0.0.1:1",
            "synthetic-only",
            "anthropic-messages",
        )?;
        let profile = &adopted["providers"]["varyai-provider-shared-catalog"]["config"]
            ["providers"]["varyai-shared-catalog"];
        assert_eq!(profile["api"], "openai-responses");
        assert_eq!(profile["streamIdleTimeoutMs"], 120000);
        assert_eq!(profile["models"][0]["compat"]["supportsStore"], false);
        assert_eq!(profile["models"][0]["compat"]["supportsStrictMode"], true);
        for field in ["baseURL", "credential", "catalog"] {
            let mut changed = saved.clone();
            let profile = &mut changed["providers"]["varyai-provider-shared-catalog"]["config"]
                ["providers"]["varyai-shared-catalog"];
            match field {
                "baseURL" => profile["baseURL"] = json!("https://synthetic.invalid/v1"),
                "credential" => {
                    profile["headers"]["Authorization"] = json!("Bearer synthetic-foreign")
                }
                _ => profile["models"][0]["id"] = json!("synthetic-foreign-model"),
            }
            assert_ne!(
                crate::services::deepseek_harness::adopt(&doc, &changed),
                changed
            );
        }
        let hermes = edit(
            "hermes",
            root.path().join("hermes.yaml"),
            &catalog,
            "http://127.0.0.1:1",
            "synthetic-only",
            "anthropic-messages",
        )?;
        assert_eq!(
            hermes.installed["providers"]["varyai"]["api_mode"],
            "chat_completions"
        );
        assert_eq!(
            hermes.installed["providers"]["varyai-anthropic"]["api_mode"],
            "anthropic_messages"
        );
        assert_eq!(hermes.installed["model"]["provider"], "varyai-anthropic");
        assert!(hermes.installed.get("custom_providers").is_none());
        let kimi = edit(
            "kimi",
            root.path().join("kimi.toml"),
            &catalog,
            "http://127.0.0.1:1",
            "synthetic-only",
            "openai-completions",
        )?;
        assert_eq!(kimi.installed["providers"]["Vary AI"]["type"], "kimi");
        let qwen = edit(
            "qwen",
            root.path().join("qwen.json"),
            &catalog,
            "http://127.0.0.1:1",
            "synthetic-only",
            "openai-completions",
        )?;
        assert_eq!(
            qwen.installed["modelProviders"]["openai"][0]["generationConfig"]["contextWindowSize"],
            500000
        );
        Ok(())
    }

    #[test]
    fn magpie_grok_native_protocol_and_campaign_contract() -> Result<(), String> {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = root.path().join("config.toml");
        let original = "[features]\ncampaigns = true\n[models]\ndefault = 'native'\n";
        std::fs::write(&path, original).map_err(|_| FAILED)?;
        let model = Model {
            api: "openai-completions",
            native_model: None,
            output_tokens: None,
            cost: None,
            id: "synthetic".into(),
            name: "Synthetic".into(),
            context_window: None,
            compact_at: None,
            group: "Synthetic".into(),
            reasoning_levels: vec![],
            reasoning: false,
            default_reasoning_level: None,
            images: false,
            icon: None,
        };
        let file = edit(
            "grokbuild",
            path.clone(),
            &[model],
            "http://127.0.0.1:1",
            "synthetic-only",
            "synthetic",
        )?;
        assert_eq!(file.installed["features"]["campaigns"], false);
        assert_eq!(
            file.installed["model"]["Synthetic"]["api_backend"],
            "chat_completions"
        );
        file.write()?;
        file.restore()?;
        assert_eq!(std::fs::read_to_string(path).map_err(|_| FAILED)?, original);
        Ok(())
    }

    #[test]
    fn parity_readable_ids_survive_renames_and_are_never_reassigned() {
        let mut first = Model {
            api: "openai-completions",
            native_model: None,
            output_tokens: None,
            cost: None,
            id: "vary-provider/a".into(),
            name: "Shared label".into(),
            context_window: None,
            compact_at: None,
            group: "Synthetic".into(),
            reasoning_levels: vec![],
            reasoning: false,
            default_reasoning_level: None,
            images: false,
            icon: None,
        };
        let mut second = first.clone();
        second.id = "vary-provider/b".into();
        let mut map = BTreeMap::new();
        let (initial, _) = named_catalog(&[first.clone(), second.clone()], &first.id, &mut map);
        first.name = "Renamed".into();
        let (renamed, _) = named_catalog(&[first.clone(), second.clone()], &first.id, &mut map);
        assert_eq!(initial[0].id, renamed[0].id);
        let (remaining, _) = named_catalog(&[second], &first.id, &mut map);
        assert_eq!(remaining[0].id, initial[1].id);
        assert_eq!(map[&initial[0].id], first.id);
    }

    #[test]
    fn parity_openclaw_string_model_connects_and_restores() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = dir.path().join("openclaw.json");
        let original = json!({"agents":{"defaults":{"model":"synthetic/native","modelPolicy":{"allow":["synthetic/*"]}}}}).to_string();
        std::fs::write(&path, &original).map_err(|_| FAILED)?;
        let model = Model {
            api: "openai-completions",
            native_model: None,
            output_tokens: None,
            cost: None,
            id: "vary-provider/test".into(),
            name: "Synthetic".into(),
            context_window: None,
            compact_at: None,
            group: "Synthetic".into(),
            reasoning_levels: vec![],
            reasoning: false,
            default_reasoning_level: None,
            images: false,
            icon: None,
        };
        let file = edit(
            "openclaw",
            path.clone(),
            &[model],
            "http://127.0.0.1:1/connections/openclaw",
            "synthetic-local",
            "vary-provider/test",
        )?;
        file.write()?;
        let installed: Value =
            json5::from_str(&std::fs::read_to_string(&path).map_err(|_| FAILED)?)
                .map_err(|_| FAILED)?;
        assert_eq!(
            installed["agents"]["defaults"]["model"],
            "varyai/vary-provider/test"
        );
        assert!(installed
            .pointer("/agents/defaults/modelPolicy/allow")
            .and_then(Value::as_array)
            .is_some_and(|allow| allow.contains(&json!("varyai/*"))));
        file.restore()?;
        assert_eq!(std::fs::read_to_string(path).map_err(|_| FAILED)?, original);
        Ok(())
    }

    #[test]
    fn deepseek_desktop_projection_is_readable_reversible_and_keeps_foreign_edits(
    ) -> Result<(), String> {
        let root = tempfile::tempdir().map_err(|_| FAILED)?;
        let path = root.path().join("cordis.patch.yml");
        let original =
            "# Desktop customization\n- id: another-plugin\n  config:\n    enabled: true\n";
        std::fs::write(&path, original).map_err(|_| FAILED)?;
        let catalog = vec![Model {
            api: "openai-completions",
            native_model: None,
            output_tokens: None,
            cost: None,
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
            json!({"off":"none","high":"high","max":"ultra"})
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
                api: "openai-completions",
                native_model: None,
                output_tokens: None,
                cost: None,
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
                api: "openai-completions",
                native_model: None,
                output_tokens: None,
                cost: None,
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
                        json!(128_000)
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
            api: "openai-completions",
            native_model: None,
            output_tokens: None,
            cost: None,
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
        let key = catalog[0].name.clone();
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
            api: "openai-completions",
            native_model: None,
            output_tokens: None,
            cost: None,
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
                    installed["model"][&catalog[0].name]["reasoning_efforts"],
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
                        .all(|row| row["max_context_size"] == json!(128_000))));
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
