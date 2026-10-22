use super::{error, AppError, AppState, AppType, Provider};
use serde_json::{json, Value};
use std::path::PathBuf;

pub fn configuration_paths(app: &AppType) -> Result<Vec<PathBuf>, AppError> {
    Ok(match app {
        AppType::Claude => vec![
            crate::config::get_claude_settings_path(),
            crate::config::get_default_claude_mcp_path(),
        ],
        AppType::ClaudeDesktop => crate::claude_desktop_config::profile_configuration_paths()?,
        AppType::Codex => vec![
            crate::codex_config::get_codex_auth_path(),
            crate::codex_config::get_codex_config_path(),
            crate::codex_config::get_codex_model_catalog_path(),
            crate::codex_config::get_codex_managed_oauth_live_auth_marker_path(),
        ],
        AppType::Gemini => vec![
            crate::gemini_config::get_gemini_env_path(),
            crate::gemini_config::get_gemini_settings_path(),
        ],
        AppType::GrokBuild => vec![crate::grok_config::get_grok_config_path()],
        AppType::OpenCode => vec![
            crate::opencode_config::get_opencode_config_path(),
            crate::opencode_config::get_opencode_env_path(),
        ],
        AppType::OpenClaw => vec![crate::openclaw_config::get_openclaw_config_path()],
        AppType::Hermes => vec![crate::hermes_config::get_hermes_config_path()],
        AppType::Pi => vec![
            crate::pi_config::get_pi_models_path()?,
            crate::pi_config::get_pi_settings_path()?,
        ],
        AppType::WorkBuddy => vec![crate::workbuddy_config::get_workbuddy_config_path()],
        AppType::Qoder => vec![crate::qoder_config::path()],
        AppType::Kimi | AppType::Qwen | AppType::ZCode => {
            vec![crate::services::agent_clients::config_path(app.as_str())?]
        }
        // TRAE has no file adapter. Profiles save local selection separately
        // from best-effort delivery to an already connected bridge.
        AppType::Trae => vec![],
        AppType::DoubaoWork => return Err(error("nativeLoginOnly")),
    })
}

pub fn enabled(state: &AppState, app: &AppType) -> Result<Vec<String>, AppError> {
    match app {
        AppType::OpenCode => Ok(crate::opencode_config::get_providers()?
            .keys()
            .cloned()
            .collect()),
        AppType::OpenClaw => Ok(crate::openclaw_config::get_providers()?
            .keys()
            .cloned()
            .collect()),
        AppType::Hermes => Ok(crate::hermes_config::get_providers()?
            .keys()
            .cloned()
            .collect()),
        AppType::Pi => Ok(crate::pi_config::read_pi_native_providers()?
            .keys()
            .cloned()
            .collect()),
        AppType::Qoder => crate::services::provider::get_qoder_live_provider_ids(state),
        AppType::WorkBuddy => crate::services::provider::get_workbuddy_live_provider_ids(state),
        _ => Ok(state
            .db
            .get_all_providers(app.as_str())?
            .values()
            .filter(|p| {
                p.meta.as_ref().and_then(|m| m.live_config_managed) == Some(true)
                    && p.category.as_deref() != Some("official")
            })
            .map(|p| p.id.clone())
            .collect()),
    }
}

// Persist only model-related fields, never provider credentials or endpoints.
fn safe_models(value: &Value) -> Value {
    match value {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(key, _)| {
                    let k = key.to_ascii_lowercase();
                    ![
                        "key",
                        "auth",
                        "bearer",
                        "secret",
                        "password",
                        "credential",
                        "access_token",
                        "refresh_token",
                        "accesstoken",
                        "refreshtoken",
                        "cookie",
                        "header",
                        "url",
                        "endpoint",
                        "command",
                        "env",
                    ]
                    .iter()
                    .any(|word| k.contains(word))
                })
                .map(|(k, v)| (k.clone(), safe_models(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(safe_models).collect()),
        v => v.clone(),
    }
}
pub fn live_provider(app: &AppType, provider: &Provider) -> Result<Provider, AppError> {
    let value = match app {
        AppType::Pi => crate::pi_config::read_pi_native_provider(&provider.id)?,
        AppType::OpenCode => crate::opencode_config::get_providers()?
            .get(&provider.id)
            .cloned(),
        AppType::OpenClaw => crate::openclaw_config::get_providers()?
            .get(&provider.id)
            .cloned(),
        AppType::Hermes => crate::hermes_config::get_providers()?
            .get(&provider.id)
            .cloned(),
        AppType::Qoder => crate::qoder_config::providers()?
            .get(&crate::qoder_config::native_id(&provider.id))
            .cloned(),
        _ => None,
    };
    let mut p = provider.clone();
    if let Some(value) = value {
        p.settings_config = value;
    }
    Ok(p)
}

pub fn provider_models(provider: &Provider) -> Value {
    let config = &provider.settings_config;
    let mut m = serde_json::Map::new();
    for key in [
        "model",
        "models",
        "modelCatalog",
        "modelName",
        "defaultModel",
        "small_model",
        "reasoning_effort",
    ] {
        m.insert(key.into(), safe_models(&config[key]));
    }
    json!(m)
}
fn merge_models(existing: &Value, saved: &Value) -> Value {
    match (existing, saved) {
        (Value::Object(a), Value::Object(b)) => {
            let mut m = a.clone();
            for (k, v) in b {
                m.insert(k.clone(), merge_models(a.get(k).unwrap_or(&Value::Null), v));
            }
            json!(m)
        }
        (Value::Array(a), Value::Array(b)) => json!(b
            .iter()
            .map(|v| {
                fn identity(v: &Value) -> Option<&Value> {
                    v.get("id")
                        .or_else(|| v.get("model"))
                        .or_else(|| v.get("name"))
                }
                match identity(v).and_then(|id| a.iter().find(|old| identity(old) == Some(id))) {
                    Some(old) => merge_models(old, v),
                    None => v.clone(),
                }
            })
            .collect::<Vec<_>>()),
        (_, v) => v.clone(),
    }
}
pub fn with_provider_models(provider: &Provider, models: &Value) -> Result<Provider, AppError> {
    let mut p = provider.clone();
    let map = p
        .settings_config
        .as_object_mut()
        .ok_or_else(|| error("invalidConfig"))?;
    for key in [
        "model",
        "models",
        "modelCatalog",
        "modelName",
        "defaultModel",
        "small_model",
        "reasoning_effort",
    ] {
        if let Some(v) = models.get(key) {
            if v.is_null() {
                map.remove(key);
            } else {
                let existing = map.get(key).unwrap_or(&Value::Null);
                let saved = safe_models(v);
                let restored = if matches!(key, "models" | "modelCatalog") {
                    match saved.as_object() {
                        Some(models) => Value::Object(
                            models
                                .iter()
                                .map(|(id, model)| (id.clone(), merge_models(&existing[id], model)))
                                .collect(),
                        ),
                        None => merge_models(existing, &saved),
                    }
                } else {
                    merge_models(existing, &saved)
                };
                map.insert(key.into(), restored);
            }
        }
    }
    Ok(p)
}

fn selection_fields(app: &AppType) -> &'static [&'static str] {
    match app {
        AppType::OpenCode => &["model", "small_model"],
        AppType::Pi => &[
            "defaultProvider",
            "defaultModel",
            "enabledModels",
            "thinkingLevel",
        ],
        AppType::Qoder => &["model", "selectedModel", "defaultModel"],
        AppType::WorkBuddy => &["availableModels"],
        _ => &[],
    }
}
pub fn selection(app: &AppType) -> Result<Value, AppError> {
    if crate::services::agent_clients::supported(app.as_str()) {
        return Ok(crate::services::agent_clients::Document::read(
            app.as_str(),
            &crate::services::agent_clients::config_path(app.as_str())?,
        )?
        .selection(app.as_str()));
    }
    if *app == AppType::OpenClaw {
        return Ok(
            json!({"model":crate::openclaw_config::get_default_model()?,"catalog":crate::openclaw_config::get_model_catalog()?}),
        );
    }
    if *app == AppType::Hermes {
        return Ok(safe_models(
            &serde_json::to_value(crate::hermes_config::get_model_config()?)
                .map_err(|_| error("invalidConfig"))?,
        ));
    }
    let fields = selection_fields(app);
    if fields.is_empty() {
        return Ok(Value::Null);
    }
    let paths = configuration_paths(app)?;
    let path = if *app == AppType::Pi {
        &paths[1]
    } else {
        &paths[0]
    };
    let source = match std::fs::read_to_string(path) {
        Ok(s) => json5::from_str::<Value>(&s).map_err(|_| error("invalidConfig"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(_) => return Err(error("readFailed")),
    };
    Ok(json!(fields
        .iter()
        .map(|key| (key.to_string(), source[*key].clone()))
        .collect::<serde_json::Map<_, _>>()))
}
pub fn apply_selection(app: &AppType, saved: &Value) -> Result<(), AppError> {
    if crate::services::agent_clients::supported(app.as_str()) {
        let path = crate::services::agent_clients::config_path(app.as_str())?;
        let mut doc = crate::services::agent_clients::Document::read(app.as_str(), &path)?;
        let native_default = *app == AppType::Qwen
            && (saved["model"].is_null() && saved["auth"].is_null()
                || saved["auth"].as_str().is_some_and(|auth| {
                    !matches!(auth, "openai" | "anthropic" | "openai-responses")
                }))
            || *app == AppType::ZCode
                && saved["providerId"]
                    .as_str()
                    .is_some_and(|id| !id.starts_with("varyai-"));
        if !saved.is_null() && !native_default && !doc.selection_available(app.as_str(), saved) {
            return Err(error("missingResource"));
        }
        doc.set_selection(app.as_str(), saved)?;
        return doc.write(&path);
    }
    if *app == AppType::OpenClaw {
        let mut defaults = crate::openclaw_config::get_agents_defaults()?.unwrap_or_else(|| {
            crate::openclaw_config::OpenClawAgentsDefaults {
                model: None,
                models: None,
                extra: Default::default(),
            }
        });
        defaults.model =
            serde_json::from_value(saved["model"].clone()).map_err(|_| error("invalidConfig"))?;
        defaults.models =
            serde_json::from_value(saved["catalog"].clone()).map_err(|_| error("invalidConfig"))?;
        crate::openclaw_config::set_agents_defaults(&defaults)?;
        return Ok(());
    }
    if *app == AppType::Hermes {
        if !saved.is_null() {
            crate::hermes_config::set_model_config(
                &serde_json::from_value(merge_models(
                    &serde_json::to_value(crate::hermes_config::get_model_config()?)
                        .map_err(|_| error("invalidConfig"))?,
                    saved,
                ))
                .map_err(|_| error("invalidConfig"))?,
            )?;
        } else {
            let mut defaults = crate::hermes_config::get_model_config()?.unwrap_or_default();
            defaults.default = None;
            defaults.provider = None;
            defaults.context_length = None;
            defaults.max_tokens = None;
            crate::hermes_config::set_model_config(&defaults)?;
        }
        return Ok(());
    }
    let fields = selection_fields(app);
    if fields.is_empty() {
        return Ok(());
    }
    let paths = configuration_paths(app)?;
    let path = if *app == AppType::Pi {
        &paths[1]
    } else {
        &paths[0]
    };
    let mut doc = match std::fs::read_to_string(path) {
        Ok(s) => json5::from_str::<Value>(&s).map_err(|_| error("invalidConfig"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(_) => return Err(error("readFailed")),
    };
    if *app == AppType::WorkBuddy && doc.is_array() {
        doc = json!({"models":doc});
    }
    let map = doc.as_object_mut().ok_or_else(|| error("invalidConfig"))?;
    for key in fields {
        if saved[*key].is_null() {
            map.remove(*key);
        } else {
            map.insert((*key).into(), saved[*key].clone());
        }
    }
    crate::config::atomic_write_private(
        path,
        serde_json::to_string_pretty(&doc)
            .map_err(|_| error("invalidConfig"))?
            .as_bytes(),
    )
}

pub fn grok_models(provider: &Provider) -> Result<Value, AppError> {
    let text = provider.settings_config["config"]
        .as_str()
        .unwrap_or_default();
    let config: toml::Value = text.parse().map_err(|_| error("invalidConfig"))?;
    let config = serde_json::to_value(config).map_err(|_| error("invalidConfig"))?;
    Ok(json!({"models":safe_models(&config["models"]),"model":safe_models(&config["model"])}))
}
pub fn with_grok_models(provider: &Provider, saved: &Value) -> Result<Provider, AppError> {
    let mut p = provider.clone();
    let mut doc = provider.settings_config["config"]
        .as_str()
        .unwrap_or_default()
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| error("invalidConfig"))?;
    if let Some(default) = saved["models"]["default"].as_str() {
        doc["models"]["default"] = toml_edit::value(default);
    }
    for (name, fields) in saved["model"].as_object().into_iter().flatten() {
        for key in ["model", "name", "api_backend", "context_window"] {
            if let Some(value) = fields.get(key) {
                if let Some(s) = value.as_str() {
                    doc["model"][name][key] = toml_edit::value(s);
                } else if let Some(i) = value.as_i64() {
                    doc["model"][name][key] = toml_edit::value(i);
                }
            }
        }
    }
    p.settings_config["config"] = json!(doc.to_string());
    Ok(p)
}

pub fn model_labels(value: &Value) -> Vec<String> {
    let mut result = Vec::new();
    match value {
        Value::String(s) => result.push(s.clone()),
        Value::Array(a) => {
            for v in a {
                result.extend(model_labels(v));
            }
        }
        Value::Object(m) => {
            for (key, v) in m {
                if ![
                    "context_window",
                    "contextWindow",
                    "maxTokens",
                    "api_backend",
                    "description",
                    "cost",
                    "input",
                    "output",
                    "temperature",
                    "top_p",
                ]
                .contains(&key.as_str())
                {
                    result.extend(model_labels(v));
                }
            }
        }
        _ => {}
    }
    result.sort();
    result.dedup();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_lists_restore_optional_fields_missing_from_current_provider() {
        let provider = Provider::with_id(
            "pi-company".into(),
            "Company".into(),
            json!({
                "apiKey": "provider-private",
                "models": [{"id": "saved-model", "apiKey": "model-private", "contextWindow": 8192}]
            }),
            None,
        );
        let original = provider.settings_config.clone();
        let restored = with_provider_models(
            &provider,
            &json!({"models": [{
                "id": "saved-model",
                "api": "openai-completions",
                "compat": {"supportsDeveloperRole": true},
                "contextWindow": 16384,
                "apiKey": "must-not-replace-credentials"
            }]}),
        )
        .expect("missing optional model fields must merge without panicking");

        assert_eq!(
            provider.settings_config, original,
            "preview must remain read-only"
        );
        assert_eq!(restored.settings_config["apiKey"], "provider-private");
        let model = &restored.settings_config["models"][0];
        assert_eq!(model["apiKey"], "model-private");
        assert_eq!(model["api"], "openai-completions");
        assert_eq!(model["compat"]["supportsDeveloperRole"], true);
        assert_eq!(model["contextWindow"], 16384);
    }

    #[test]
    fn model_maps_merge_new_nested_compatibility_fields_and_keep_existing_fields() {
        let provider = Provider::with_id(
            "p".into(),
            "P".into(),
            json!({"models": {"saved-model": {
                "apiKey": "private",
                "compat": {"supportsStore": false, "maxTokensField": "max_tokens"}
            }}}),
            None,
        );
        let restored = with_provider_models(
            &provider,
            &json!({"models": {"saved-model": {
                "api": "openai-completions",
                "compat": {"supportsDeveloperRole": true, "supportsStore": true}
            }}}),
        )
        .expect("new nested compatibility fields must merge without panicking");

        let model = &restored.settings_config["models"]["saved-model"];
        assert_eq!(model["apiKey"], "private");
        assert_eq!(model["api"], "openai-completions");
        assert_eq!(model["compat"]["supportsDeveloperRole"], true);
        assert_eq!(model["compat"]["supportsStore"], true);
        assert_eq!(model["compat"]["maxTokensField"], "max_tokens");
    }

    #[test]
    fn explicit_empty_native_selection_is_distinct_from_a_legacy_missing_snapshot() {
        use super::super::ProfileSelection;
        let selection: ProfileSelection = serde_json::from_value(
            json!({"app":"kimi","providerId":"p","models":{},"nativeModels":null}),
        )
        .unwrap();
        assert_eq!(selection.native_models, Some(Value::Null));
        let legacy: ProfileSelection =
            serde_json::from_value(json!({"app":"kimi","providerId":"p","models":{}})).unwrap();
        assert_eq!(legacy.native_models, None);
    }
    #[test]
    fn workbuddy_single_model_id_and_limits_are_part_of_the_snapshot() {
        let mut provider = Provider::with_id(
            "p".into(),
            "P".into(),
            json!({"id":"saved","name":"Saved","maxInputTokens":8192,"url":"https://example.test/v1/chat/completions","apiKey":"private"}),
            None,
        );
        let saved = super::super::model_settings(&AppType::WorkBuddy, &provider).unwrap();
        assert!(!saved.to_string().contains("private"));
        provider.settings_config["id"] = json!("later");
        provider.settings_config["maxInputTokens"] = json!(16384);
        let restored = super::super::with_models(&AppType::WorkBuddy, &provider, &saved).unwrap();
        assert_eq!(restored.settings_config["id"], "saved");
        assert_eq!(restored.settings_config["maxInputTokens"], 8192);
        assert_eq!(restored.settings_config["apiKey"], "private");
    }
    #[test]
    fn applying_a_model_map_removes_later_models_and_preserves_matching_auth() {
        let provider = Provider::with_id(
            "p".into(),
            "P".into(),
            json!({"models":{"saved":{"name":"Saved","apiKey":"secret"},"later":{"name":"Later"}}}),
            None,
        );
        let restored =
            with_provider_models(&provider, &json!({"models":{"saved":{"name":"Snapshot"}}}))
                .unwrap();
        assert!(restored.settings_config["models"].get("later").is_none());
        assert_eq!(
            restored.settings_config["models"]["saved"]["apiKey"],
            "secret"
        );
        assert_eq!(
            restored.settings_config["models"]["saved"]["name"],
            "Snapshot"
        );
    }
    #[test]
    fn model_lists_do_not_capture_credentials_and_restore_keeps_existing_auth() {
        let provider = Provider::with_id(
            "p".into(),
            "P".into(),
            json!({"apiKey":"root-private","models":[{"id":"a","name":"A","apiKey":"model-private","headers":{"Authorization":"bearer-private"},"auth":{"bearer":"private"},"contextWindow":8192,"maxTokens":4096,"access_token":"access-private","refreshToken":"refresh-private"}]}),
            None,
        );
        let saved = provider_models(&provider);
        let encoded = saved.to_string();
        assert!(!encoded.contains("private"));
        let restored = with_provider_models(&provider, &saved).unwrap();
        assert_eq!(restored.settings_config["apiKey"], "root-private");
        assert_eq!(
            restored.settings_config["models"][0]["apiKey"],
            "model-private"
        );
        assert_eq!(restored.settings_config["models"][0]["contextWindow"], 8192);
        assert_eq!(saved["models"][0]["maxTokens"], 4096);
    }
}
