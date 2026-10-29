//! Native configuration for the next batch of local agents.

use crate::{config::atomic_write_private, error::AppError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(crate) const APPS: &[&str] = &["kimi", "qwen", "zcode"];
pub(crate) fn supported(app: &str) -> bool {
    APPS.contains(&app)
}
fn invalid() -> AppError {
    AppError::localized(
        "agents.invalid_config",
        "客户端配置无效或版本尚不支持，原配置未修改",
        "Invalid or unsupported agent configuration; original configuration was not changed",
    )
}
#[cfg(test)]
thread_local! {static TEST_CONFIG_PATH: std::cell::RefCell<Option<PathBuf>> = const {std::cell::RefCell::new(None)};}
#[cfg(test)]
pub(crate) fn with_test_config<T>(path: &Path, run: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_CONFIG_PATH.with(|p| *p.borrow_mut() = None);
        }
    }
    TEST_CONFIG_PATH.with(|p| *p.borrow_mut() = Some(path.to_owned()));
    let _reset = Reset;
    run()
}
pub(crate) fn config_path(app: &str) -> Result<PathBuf, AppError> {
    #[cfg(test)]
    if let Some(path) = TEST_CONFIG_PATH.with(|p| p.borrow().clone()) {
        return Ok(path);
    }
    let home = crate::config::get_home_dir();
    match app {
        "kimi" => Ok(std::env::var_os("KIMI_CODE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".kimi-code"))
            .join("config.toml")),
        "qwen" => Ok(std::env::var_os("QWEN_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".qwen"))
            .join("settings.json")),
        "zcode" => Ok(home.join(".zcode/v2/provider_config.json")),
        _ => Err(invalid()),
    }
}
pub(crate) fn validate(config: &Value) -> Result<(), AppError> {
    let url =
        url::Url::parse(config["baseUrl"].as_str().ok_or_else(invalid)?).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    if !matches!(
        config["protocol"].as_str(),
        Some("openai" | "openai-responses" | "anthropic")
    ) || config["apiKey"]
        .as_str()
        .is_none_or(|key| key.trim().is_empty())
    {
        return Err(invalid());
    }
    let models = config["models"].as_array().ok_or_else(invalid)?;
    let mut seen = std::collections::HashSet::new();
    if models.is_empty()
        || models.iter().any(|m| {
            m["model"]
                .as_str()
                .is_none_or(|id| id.trim().is_empty() || !seen.insert(id))
        })
    {
        return Err(invalid());
    }
    Ok(())
}
fn object(value: &mut Value, key: &str) -> Result<(), AppError> {
    if value.get(key).is_none() {
        value[key] = json!({});
    }
    if !value[key].is_object() {
        return Err(invalid());
    }
    Ok(())
}
fn pointer(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| format!("/{}", part.replace('~', "~0").replace('/', "~1")))
        .collect()
}
fn local_url(origin: &str, app: &str, id: &str, upstream: &url::Url) -> Result<String, AppError> {
    let mut url = url::Url::parse(origin).map_err(|_| invalid())?;
    url.set_path(&format!("/agents/{app}/providers/{id}{}", upstream.path()));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.into())
}

pub(crate) struct Document {
    pub value: Value,
    toml: Option<toml_edit::DocumentMut>,
    original: Value,
    source: String,
    existed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Route {
    pub id: String,
    pub pointer: String,
    pub upstream: String,
    pub local: String,
    pub models: Vec<String>,
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_provider: Option<Value>,
}
impl Document {
    pub fn parse(app: &str, text: &str) -> Result<Self, AppError> {
        if !supported(app) {
            return Err(invalid());
        }
        let (value, toml) = if app == "kimi" {
            let doc = text
                .parse::<toml_edit::DocumentMut>()
                .map_err(|_| invalid())?;
            let value = text.parse::<toml::Value>().map_err(|_| invalid())?;
            (
                serde_json::to_value(value).map_err(|_| invalid())?,
                Some(doc),
            )
        } else {
            (
                serde_json::from_str::<Value>(text).map_err(|_| invalid())?,
                None,
            )
        };
        if !value.is_object() {
            return Err(invalid());
        }
        Ok(Self {
            original: value.clone(),
            value,
            toml,
            source: text.into(),
            existed: true,
        })
    }
    pub fn read(app: &str, path: &Path) -> Result<Self, AppError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(app, &text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut doc = Self::parse(app, if app == "kimi" { "" } else { "{}" })?;
                doc.existed = false;
                Ok(doc)
            }
            Err(_) => Err(invalid()),
        }
    }
    pub fn encode(&self) -> Result<String, AppError> {
        if let Some(mut toml) = self.toml.clone() {
            patch_toml(toml.as_item_mut(), &self.original, &self.value)?;
            Ok(toml.to_string())
        } else {
            serde_json::to_string_pretty(&self.value).map_err(|_| invalid())
        }
    }
    pub fn write(&self, path: &Path) -> Result<(), AppError> {
        let live = std::fs::read_to_string(path);
        if live.is_ok() != self.existed {
            return Err(invalid());
        }
        if live.as_deref().is_ok_and(|text| text != self.source) {
            return Err(invalid());
        }
        if live
            .as_ref()
            .is_err_and(|e| e.kind() != std::io::ErrorKind::NotFound)
        {
            return Err(invalid());
        }
        atomic_write_private(path, self.encode()?.as_bytes())
    }
    pub fn selection(&self, app: &str) -> Value {
        match app {
            "kimi" => self.value["default_model"].clone(),
            "qwen" => {
                json!({"model":self.value["model"]["name"],"baseUrl":self.value["model"]["baseUrl"],"auth":self.value["security"]["auth"]["selectedType"]})
            }
            "zcode" => self.value["config"]["defaultModelSelection"].clone(),
            _ => Value::Null,
        }
    }
    pub fn selection_owned(&self, app: &str, id: Option<&str>) -> bool {
        let prefix = id
            .map(|id| format!("varyai-{id}"))
            .unwrap_or_else(|| "varyai-".into());
        match app {
            "kimi" => self.value["default_model"].as_str().is_some_and(|s| {
                s.starts_with(&format!("{prefix}{}", if id.is_some() { "-" } else { "" }))
            }),
            "zcode" => self.value["config"]["defaultModelSelection"]["providerId"]
                .as_str()
                .is_some_and(|s| {
                    if id.is_some() {
                        s == prefix
                    } else {
                        s.starts_with(&prefix)
                    }
                }),
            "qwen" => self.value["modelProviders"]
                .as_object()
                .is_some_and(|groups| {
                    groups.iter().any(|(protocol, models)| {
                        models.as_array().is_some_and(|models| {
                            models.iter().any(|model| {
                                self.qwen_selection_matches(
                                    protocol,
                                    model,
                                    &self.selection("qwen"),
                                ) && model["envKey"].as_str().is_some_and(|env| {
                                    id.map(|id| {
                                        env == format!("VARYAI_{}_API_KEY", id.replace('-', "_"))
                                    })
                                    .unwrap_or_else(|| env.starts_with("VARYAI_"))
                                })
                            })
                        })
                    })
                }),
            _ => false,
        }
    }
    pub fn set_selection(&mut self, app: &str, selection: &Value) -> Result<(), AppError> {
        fn set(value: &mut Value, key: &str, selection: &Value) -> Result<(), AppError> {
            let map = value.as_object_mut().ok_or_else(invalid)?;
            if selection.is_null() {
                map.remove(key);
            } else {
                map.insert(key.into(), selection.clone());
            }
            Ok(())
        }
        match app {
            "kimi" => set(&mut self.value, "default_model", selection),
            "qwen" => {
                object(&mut self.value, "model")?;
                object(&mut self.value, "security")?;
                object(&mut self.value["security"], "auth")?;
                set(&mut self.value["model"], "name", &selection["model"])?;
                set(&mut self.value["model"], "baseUrl", &selection["baseUrl"])?;
                set(
                    &mut self.value["security"]["auth"],
                    "selectedType",
                    &selection["auth"],
                )
            }
            "zcode" => {
                object(&mut self.value, "config")?;
                set(
                    &mut self.value["config"],
                    "defaultModelSelection",
                    selection,
                )
            }
            _ => Err(invalid()),
        }
    }
    pub fn selection_available(&self, app: &str, selection: &Value) -> bool {
        match app {
            "kimi" => selection
                .as_str()
                .is_some_and(|id| self.value["models"].get(id).is_some()),
            "qwen" => self.value["modelProviders"]
                .as_object()
                .is_some_and(|groups| {
                    groups.iter().any(|(protocol, models)| {
                        models.as_array().is_some_and(|models| {
                            models.iter().any(|model| {
                                self.qwen_selection_matches(protocol, model, selection)
                            })
                        })
                    })
                }),
            "zcode" => self
                .value
                .pointer("/config/providerConfigRules/providerRules")
                .and_then(Value::as_array)
                .is_some_and(|rules| {
                    rules.iter().any(|rule| {
                        rule["providerId"] == selection["providerId"]
                            && rule["config"]["personalModelIds"]
                                .as_array()
                                .is_some_and(|models| models.contains(&selection["modelId"]))
                    })
                }),
            _ => false,
        }
    }
    pub fn qwen_model_protocol<'a>(&'a self, provider: &'a str, model: &Value) -> &'a str {
        let protocol = self.value["providerProtocol"][provider]
            .as_str()
            .unwrap_or(provider);
        if protocol == "openai" && model["wireApi"] == "responses" {
            "openai-responses"
        } else {
            protocol
        }
    }
    fn qwen_selection_matches(&self, provider: &str, model: &Value, selection: &Value) -> bool {
        selection["auth"] == self.qwen_model_protocol(provider, model)
            && model["id"] == selection["model"]
            && (selection["baseUrl"].is_null() || model["baseUrl"] == selection["baseUrl"])
    }

    pub fn qwen_provider_conflicts(&self, id: &str, target: &Value) -> bool {
        let env = format!("VARYAI_{}_API_KEY", id.replace('-', "_"));
        self.value["modelProviders"]
            .as_object()
            .is_some_and(|groups| {
                groups.iter().any(|(protocol, entries)| {
                    entries.as_array().is_some_and(|entries| {
                        entries.iter().any(|model| {
                            model["envKey"] == env
                                && model["baseUrl"] == target["baseUrl"]
                                && Some(self.qwen_model_protocol(protocol, model))
                                    == target["protocol"].as_str()
                                && target["models"].as_array().is_some_and(|models| {
                                    models.iter().any(|wanted| wanted["model"] == model["id"])
                                })
                        })
                    })
                })
            })
    }

    pub fn apply(
        &mut self,
        app: &str,
        id: &str,
        name: &str,
        config: &Value,
    ) -> Result<(), AppError> {
        validate(config)?;
        let key = format!("varyai-{id}");
        let models = config["models"].as_array().ok_or_else(invalid)?;
        let first = models
            .first()
            .and_then(|m| m["model"].as_str())
            .ok_or_else(invalid)?;
        match app {
            "kimi" => {
                object(&mut self.value, "providers")?;
                object(&mut self.value, "models")?;
                let previous_key = self.kimi_provider_key(id);
                let provider_name = name.trim();
                if provider_name.is_empty()
                    || (provider_name != previous_key
                        && self.value["providers"].get(provider_name).is_some())
                {
                    return Err(invalid());
                }
                if previous_key != provider_name {
                    if let Some(previous) = self.value["providers"]
                        .as_object_mut()
                        .ok_or_else(invalid)?
                        .remove(&previous_key)
                    {
                        self.value["providers"][provider_name] = previous;
                    }
                    for model in self.value["models"]
                        .as_object_mut()
                        .ok_or_else(invalid)?
                        .values_mut()
                    {
                        if model["provider"] == previous_key {
                            model["provider"] = json!(provider_name);
                        }
                    }
                }
                let protocol = match config["protocol"].as_str() {
                    Some("anthropic") => "anthropic",
                    Some("openai-responses") => "openai_responses",
                    _ => "openai",
                };
                object(&mut self.value["providers"], provider_name)?;
                self.value["providers"][provider_name]["type"] = json!(protocol);
                self.value["providers"][provider_name]["base_url"] = config["baseUrl"].clone();
                self.value["providers"][provider_name]["api_key"] = config["apiKey"].clone();
                self.value["models"]
                    .as_object_mut()
                    .ok_or_else(invalid)?
                    .retain(|_, existing| {
                        existing["provider"] != provider_name
                            || models
                                .iter()
                                .any(|model| model["model"] == existing["model"])
                    });
                for model in models {
                    let wire = model["model"].as_str().ok_or_else(invalid)?;
                    let alias = format!("{key}-{wire}");
                    object(&mut self.value["models"], &alias)?;
                    let entry = &mut self.value["models"][&alias];
                    entry["provider"] = json!(provider_name);
                    entry["model"] = json!(wire);
                    if model["contextWindow"].is_u64() || entry.get("max_context_size").is_none() {
                        entry["max_context_size"] =
                            json!(model["contextWindow"].as_u64().unwrap_or(200000));
                    }
                }
                self.value["default_model"] = json!(format!("{key}-{first}"));
            }
            "qwen" => {
                for section in ["modelProviders", "env", "model", "security"] {
                    object(&mut self.value, section)?;
                }
                object(&mut self.value["security"], "auth")?;
                let env = format!("VARYAI_{}_API_KEY", id.replace('-', "_"));
                self.value["env"][&env] = config["apiKey"].clone();
                for entries in self.value["modelProviders"]
                    .as_object_mut()
                    .ok_or_else(invalid)?
                    .values_mut()
                {
                    if let Some(entries) = entries.as_array_mut() {
                        entries.retain(|model| model["envKey"] != env);
                    }
                }
                let protocol = if config["protocol"] == "anthropic" {
                    "anthropic"
                } else {
                    "openai"
                };
                let mut entries = self.value["modelProviders"]
                    .get(protocol)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                entries.retain(|model| model["envKey"] != env);
                if entries.iter().any(|existing| {
                    let responses = config["protocol"] == "openai-responses";
                    models.iter().any(|model| model["model"] == existing["id"])
                        && existing["baseUrl"] == config["baseUrl"]
                        && (existing["wireApi"] == "responses") == responses
                }) {
                    return Err(AppError::localized(
                        "agents.duplicate_model",
                        "该 API 协议和接口地址已有相同模型 ID，请编辑已有配置",
                        "This API protocol and endpoint already have that model ID; edit the existing configuration",
                    ));
                }
                entries.extend(models.iter().map(|m| {
                    let mut model = json!({"id":m["model"],"name":m["model"],"baseUrl":config["baseUrl"],"envKey":env});
                    if config["protocol"] == "openai-responses" { model["wireApi"] = json!("responses"); }
                    model
                }));
                self.value["modelProviders"][protocol] = Value::Array(entries);
                self.value["model"]["name"] = json!(first);
                self.value["model"]["baseUrl"] = config["baseUrl"].clone();
                self.value["security"]["auth"]["selectedType"] =
                    if config["protocol"] == "openai-responses" {
                        json!("openai-responses")
                    } else {
                        json!(protocol)
                    };
            }
            "zcode" => {
                self.ensure_zcode_config()?;
                let protocol = match config["protocol"].as_str() {
                    Some("anthropic") => "anthropic-messages",
                    Some("openai-responses") => "openai-responses",
                    _ => "openai-chat-completions",
                };
                let ids: Vec<_> = models.iter().map(|m| m["model"].clone()).collect();
                let rules = self
                    .value
                    .pointer_mut("/config/providerConfigRules/providerRules")
                    .and_then(Value::as_array_mut)
                    .ok_or_else(invalid)?;
                let existing = rules.iter().position(|r| r["providerId"] == key);
                let mut rule = existing
                    .map(|i| rules[i].clone())
                    .unwrap_or_else(|| json!({"providerId":key,"config":{}}));
                rule["providerName"] = json!(name);
                rule["config"]["group"] = json!("standard-personal");
                rule["config"]["access"] = json!({"type":"api-key","apiKey":config["apiKey"]});
                rule["config"]["api"] = json!({"type":protocol,"baseUrl":config["baseUrl"]});
                rule["config"]["personalModelIds"] = json!(ids);
                rule["config"]["modelOrder"] = json!(ids);
                if let Some(index) = existing {
                    rules[index] = rule;
                } else {
                    rules.push(rule);
                }
                let rules = self
                    .value
                    .pointer_mut("/config/modelConfigRules/providerModelRules")
                    .and_then(Value::as_array_mut)
                    .ok_or_else(invalid)?;
                let previous: Vec<_> = rules
                    .iter()
                    .filter(|r| r["providerId"] == key)
                    .cloned()
                    .collect();
                rules.retain(|r| r["providerId"] != key);
                for model in models {
                    let mut rule = previous
                        .iter()
                        .find(|r| r["modelId"] == model["model"])
                        .cloned()
                        .unwrap_or_else(
                            || json!({"providerId":key,"modelId":model["model"],"config":{}}),
                        );
                    object(&mut rule["config"], "properties")?;
                    rule["config"]["enabled"] = json!(true);
                    if model["contextWindow"].is_u64()
                        || rule["config"]["properties"].get("contextWindow").is_none()
                    {
                        rule["config"]["properties"]["contextWindow"] =
                            json!(model["contextWindow"].as_u64().unwrap_or(200000));
                    }
                    rules.push(rule);
                }
                self.value["config"]["defaultModelSelection"] =
                    json!({"providerId":key,"modelId":first});
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }
    fn ensure_zcode_config(&mut self) -> Result<(), AppError> {
        if self.value.as_object().is_some_and(|v| v.is_empty()) {
            self.value = json!({"schemaVersion":1,"config":{"providerConfigRules":{"providerRules":[]},"modelConfigRules":{"providerModelRules":[],"manualProviderModelRules":[]}}});
        }
        if self.value["schemaVersion"] != 1
            || !self
                .value
                .pointer("/config/providerConfigRules/providerRules")
                .is_some_and(Value::is_array)
            || !self
                .value
                .pointer("/config/modelConfigRules/providerModelRules")
                .is_some_and(Value::is_array)
        {
            return Err(invalid());
        }
        Ok(())
    }
    pub(crate) fn prepare_zcode_official(
        &mut self,
        builtin: &Value,
    ) -> Result<Vec<Value>, AppError> {
        self.ensure_zcode_config()?;
        let defaults = builtin
            .pointer("/config/providerConfigRules/providerRules")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        let rules = self
            .value
            .pointer_mut("/config/providerConfigRules/providerRules")
            .and_then(Value::as_array_mut)
            .ok_or_else(invalid)?;
        let mut added = Vec::new();
        for default in defaults.iter().filter(|r| {
            r["providerId"]
                .as_str()
                .is_some_and(|id| id.starts_with("account:"))
        }) {
            let Some(api) = default
                .pointer("/config/api")
                .filter(|api| api["baseUrl"].is_string())
            else {
                continue;
            };
            if let Some(rule) = rules
                .iter_mut()
                .find(|r| r["providerId"] == default["providerId"])
            {
                object(&mut rule["config"], "api")?;
                if rule["config"]["api"].get("baseUrl").is_none() {
                    return Err(invalid());
                }
            } else {
                let rule = json!({"providerId":default["providerId"],"config":{"api":api}});
                rules.push(rule.clone());
                added.push(rule);
            }
        }
        Ok(added)
    }
    pub fn route(&mut self, app: &str, origin: &str) -> Result<Vec<Route>, AppError> {
        let mut candidates: Vec<(String, String, Vec<String>)> = Vec::new();
        if app == "kimi" {
            if let Some(providers) = self.value["providers"].as_object() {
                for (id, p) in providers {
                    if !matches!(
                        p["type"].as_str(),
                        Some(
                            "kimi" | "openai" | "openai_legacy" | "openai_responses" | "anthropic"
                        )
                    ) {
                        return Err(invalid());
                    }
                    if let Some(base) = p["base_url"].as_str() {
                        let models = self.value["models"]
                            .as_object()
                            .into_iter()
                            .flat_map(|ms| ms.values())
                            .filter(|m| m["provider"] == *id && m.get("base_url").is_none())
                            .filter_map(|m| m["model"].as_str().map(String::from))
                            .collect();
                        candidates.push((
                            pointer(&["providers", id, "base_url"]),
                            base.into(),
                            models,
                        ));
                    }
                }
            }
            if let Some(models) = self.value["models"].as_object() {
                for (alias, model) in models {
                    if let (Some(base), Some(wire)) =
                        (model["base_url"].as_str(), model["model"].as_str())
                    {
                        candidates.push((
                            pointer(&["models", alias, "base_url"]),
                            base.into(),
                            vec![wire.into()],
                        ));
                    }
                }
            }
        } else if app == "qwen" {
            if let Some(providers) = self.value["modelProviders"].as_object() {
                for (protocol, models) in providers {
                    let kind = self.value["providerProtocol"][protocol]
                        .as_str()
                        .unwrap_or(protocol);
                    if !matches!(kind, "openai" | "anthropic") {
                        return Err(invalid());
                    }
                    for (index, m) in models.as_array().ok_or_else(invalid)?.iter().enumerate() {
                        if let (Some(base), Some(model)) = (m["baseUrl"].as_str(), m["id"].as_str())
                        {
                            candidates.push((
                                pointer(&[
                                    "modelProviders",
                                    protocol,
                                    &index.to_string(),
                                    "baseUrl",
                                ]),
                                base.into(),
                                vec![model.into()],
                            ));
                        }
                    }
                }
            }
        } else if app == "zcode" {
            self.ensure_zcode_config()?;
            let rules = self
                .value
                .pointer("/config/providerConfigRules/providerRules")
                .and_then(Value::as_array)
                .ok_or_else(invalid)?;
            for (index, rule) in rules.iter().enumerate() {
                if rule["enabled"] == false {
                    continue;
                }
                if let Some(base) = rule.pointer("/config/api/baseUrl").and_then(Value::as_str) {
                    if !matches!(
                        rule.pointer("/config/api/type").and_then(Value::as_str),
                        Some("anthropic-messages" | "openai-chat-completions" | "openai-responses")
                    ) {
                        return Err(invalid());
                    }
                    let models = rule["config"]["personalModelIds"]
                        .as_array()
                        .map(|ms| {
                            ms.iter()
                                .filter_map(Value::as_str)
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default();
                    candidates.push((
                        pointer(&[
                            "config",
                            "providerConfigRules",
                            "providerRules",
                            &index.to_string(),
                            "config",
                            "api",
                            "baseUrl",
                        ]),
                        base.into(),
                        models,
                    ));
                }
            }
        } else {
            return Err(invalid());
        }
        let mut routes = Vec::new();
        for (path, upstream, models) in candidates {
            let url = url::Url::parse(&upstream).map_err(|_| invalid())?;
            if !matches!(url.scheme(), "https" | "http")
                || url.query().is_some()
                || url.fragment().is_some()
                || url.password().is_some()
                || !url.username().is_empty()
                || upstream.contains("/agents/")
            {
                return Err(invalid());
            }
            let qwen_model = if app == "qwen" {
                path.rsplit_once('/')
                    .and_then(|(parent, _)| self.value.pointer(parent))
            } else {
                None
            };
            let qwen_protocol = path.split('/').nth(2).unwrap_or_default();
            let qwen_selected = self.value["model"]["baseUrl"].is_string()
                && qwen_model.is_some_and(|model| {
                    self.qwen_selection_matches(qwen_protocol, model, &self.selection("qwen"))
                });
            let identity = if let Some(model) = qwen_model {
                format!(
                    "qwen:{qwen_protocol}:{}:{}:{}:{upstream}",
                    model["envKey"].as_str().unwrap_or_default(),
                    model["id"].as_str().unwrap_or_default(),
                    model["wireApi"].as_str().unwrap_or("chat")
                )
            } else if app == "zcode" {
                let provider = path
                    .strip_suffix("/config/api/baseUrl")
                    .and_then(|p| self.value.pointer(p))
                    .and_then(|rule| rule["providerId"].as_str())
                    .ok_or_else(invalid)?;
                format!("zcode:{provider}:{upstream}")
            } else {
                path.clone()
            };
            let id = format!("{:x}", sha2::Sha256::digest(identity.as_bytes()));
            let local = local_url(origin, app, &id, &url)?;
            let provider_id = if app == "qwen" {
                path.rsplit_once('/')
                    .and_then(|(model_path, _)| self.value.pointer(model_path))
                    .and_then(|model| model["envKey"].as_str())
                    .and_then(|env| env.strip_prefix("VARYAI_")?.strip_suffix("_API_KEY"))
                    .map(|id| id.replace('_', "-"))
            } else if app == "zcode" {
                path.split('/')
                    .nth(4)
                    .and_then(|index| index.parse::<usize>().ok())
                    .and_then(|index| {
                        self.value
                            .pointer("/config/providerConfigRules/providerRules")?
                            .get(index)
                    })
                    .and_then(|rule| rule["providerId"].as_str())
                    .and_then(|id| id.strip_prefix("varyai-"))
                    .map(String::from)
            } else if app == "kimi" {
                self.value["models"].as_object().and_then(|models| {
                    models.iter().find_map(|(alias, model)| {
                        let provider = model["provider"].as_str()?;
                        if path != pointer(&["providers", provider, "base_url"])
                            && path != pointer(&["models", alias, "base_url"])
                        {
                            return None;
                        }
                        alias
                            .strip_prefix("varyai-")?
                            .strip_suffix(&format!("-{}", model["model"].as_str()?))
                            .map(String::from)
                    })
                })
            } else {
                path.split('/')
                    .find_map(|part| part.strip_prefix("varyai-"))
                    .map(String::from)
            }
            .unwrap_or_else(|| format!("native-{id}"));
            let slot = self.value.pointer_mut(&path).ok_or_else(invalid)?;
            *slot = json!(local);
            if qwen_selected {
                self.value["model"]["baseUrl"] = json!(local);
            }
            routes.push(Route {
                provider_id,
                created_provider: None,
                id,
                pointer: path,
                upstream,
                local,
                models,
            });
        }
        if routes.is_empty() {
            return Err(AppError::localized(
                "agents.no_models",
                "请先在客户端登录或应用一个 API Key 模型配置",
                "Sign in in the client or apply an API-key model configuration first",
            ));
        }
        Ok(routes)
    }
    pub fn restore(&mut self, routes: &[Route]) -> Result<(), AppError> {
        fn restore_owned(value: &mut Value, routes: &[Route]) {
            match value {
                Value::String(text) => {
                    if let Some(route) = routes.iter().find(|route| route.local == *text) {
                        *text = route.upstream.clone();
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        restore_owned(value, routes);
                    }
                }
                Value::Object(values) => {
                    for value in values.values_mut() {
                        restore_owned(value, routes);
                    }
                }
                _ => {}
            }
        }
        restore_owned(&mut self.value, routes);
        if let Some(rules) = self
            .value
            .pointer_mut("/config/providerConfigRules/providerRules")
            .and_then(Value::as_array_mut)
        {
            rules.retain(|rule| {
                !routes
                    .iter()
                    .any(|route| route.created_provider.as_ref() == Some(rule))
            });
        }
        Ok(())
    }
    fn kimi_provider_key(&self, id: &str) -> String {
        self.value["models"]
            .as_object()
            .and_then(|models| {
                models.iter().find_map(|(alias, model)| {
                    let wire = model["model"].as_str()?;
                    (alias == &format!("varyai-{id}-{wire}"))
                        .then(|| model["provider"].as_str().map(String::from))?
                })
            })
            .unwrap_or_else(|| format!("varyai-{id}"))
    }

    pub fn remove(&mut self, app: &str, id: &str, expected: &Value) -> Result<(), AppError> {
        let key = if app == "kimi" {
            self.kimi_provider_key(id)
        } else {
            format!("varyai-{id}")
        };
        if app == "qwen" {
            let env_key = format!("VARYAI_{}_API_KEY", id.replace('-', "_"));
            if self.value["env"]
                .get(&env_key)
                .is_some_and(|key| Some(key) != expected.get("apiKey"))
            {
                return Err(invalid());
            }
            if let Some(groups) = self.value["modelProviders"].as_object() {
                for models in groups.values().filter_map(Value::as_array) {
                    if models.iter().any(|m| {
                        m["envKey"] == env_key && m.get("baseUrl") != expected.get("baseUrl")
                    }) {
                        return Err(invalid());
                    }
                }
            }
            if let Some(groups) = self.value["modelProviders"].as_object_mut() {
                for models in groups.values_mut().filter_map(Value::as_array_mut) {
                    models.retain(|m| m["envKey"] != env_key);
                }
            }
            if let Some(env) = self.value["env"].as_object_mut() {
                env.remove(&env_key);
            }
            return Ok(());
        }
        if app == "zcode" {
            self.ensure_zcode_config()?;
            let rules = self
                .value
                .pointer_mut("/config/providerConfigRules/providerRules")
                .and_then(Value::as_array_mut)
                .ok_or_else(invalid)?;
            if rules.iter().any(|r| {
                r["providerId"] == key
                    && (r.pointer("/config/api/baseUrl") != expected.get("baseUrl")
                        || r.pointer("/config/access/apiKey") != expected.get("apiKey"))
            }) {
                return Err(invalid());
            }
            rules.retain(|r| r["providerId"] != key);
            for section in ["providerModelRules", "manualProviderModelRules"] {
                if let Some(rules) =
                    self.value["config"]["modelConfigRules"][section].as_array_mut()
                {
                    rules.retain(|r| r["providerId"] != key);
                }
            }
            return Ok(());
        }
        let (section, url_pointer) = match app {
            "kimi" => ("providers", pointer(&["providers", &key, "base_url"])),
            "qwen" => (
                "modelProviders",
                pointer(&["modelProviders", &key, "0", "baseUrl"]),
            ),
            "zcode" => (
                "provider",
                pointer(&["provider", &key, "options", "baseURL"]),
            ),
            _ => return Err(invalid()),
        };
        if self.value.get(section).and_then(|v| v.get(&key)).is_none() {
            return Ok(());
        }
        if self.value.pointer(&url_pointer) != expected.get("baseUrl")
            || self.value[section][&key].get("api_key") != expected.get("apiKey")
        {
            return Err(invalid());
        }
        self.value[section]
            .as_object_mut()
            .ok_or_else(invalid)?
            .remove(&key);
        if app == "kimi" {
            if let Some(models) = self.value["models"].as_object_mut() {
                models.retain(|_, m| m["provider"] != key);
            }
        }
        Ok(())
    }
}
use sha2::Digest;
fn patch_toml(item: &mut toml_edit::Item, before: &Value, after: &Value) -> Result<(), AppError> {
    if before == after {
        return Ok(());
    }
    if let Some(values) = after.as_object() {
        if !item.is_table_like() {
            *item = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let table = item.as_table_like_mut().ok_or_else(invalid)?;
        if let Some(old) = before.as_object() {
            for key in old.keys().filter(|k| !values.contains_key(*k)) {
                table.remove(key);
            }
        }
        for (key, value) in values {
            if before.get(key) == Some(value) {
                continue;
            }
            if table.get(key).is_none() {
                table.insert(key, toml_edit::Item::Table(toml_edit::Table::new()));
            }
            patch_toml(table.get_mut(key).ok_or_else(invalid)?, &before[key], value)?;
        }
    } else {
        let serialized = toml::to_string(&json!({"value":after})).map_err(|_| invalid())?;
        let mut doc = serialized
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| invalid())?;
        *item = doc.as_table_mut().remove("value").ok_or_else(invalid)?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Backup {
    pub path: PathBuf,
    pub routes: Vec<Route>,
}
pub(crate) fn enabled_key(app: &str) -> String {
    format!("{app}_proxy_enabled")
}
pub(crate) async fn restore(db: &crate::database::Database, app: &str) -> Result<(), AppError> {
    if let Some(saved) = db.get_live_backup(app).await? {
        let backup: Backup = serde_json::from_str(&saved.original_config).map_err(|_| invalid())?;
        if !backup.path.try_exists().map_err(|_| invalid())? {
            db.delete_live_backup(app).await?;
            return Ok(());
        }
        let mut doc = Document::read(app, &backup.path)?;
        doc.restore(&backup.routes)?;
        doc.write(&backup.path)?;
        db.delete_live_backup(app).await?;
    }
    Ok(())
}
pub(crate) fn zcode_builtin() -> Result<Value, AppError> {
    fn collect(root: &Path, depth: u8, files: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_file() && entry.file_name() == "zcode-builtin.json" {
                files.push(entry.path());
            } else if kind.is_dir() && depth > 0 {
                collect(&entry.path(), depth - 1, files);
            }
        }
    }
    let root = dirs::home_dir()
        .ok_or_else(invalid)?
        .join(".zcode/v2/runtime/provider");
    let mut files = Vec::new();
    collect(&root, 4, &mut files);
    files.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    let file = files.last().ok_or_else(invalid)?;
    serde_json::from_slice(&std::fs::read(file).map_err(|_| invalid())?).map_err(|_| invalid())
}
pub(crate) async fn enable(
    db: &crate::database::Database,
    app: &str,
    origin: &str,
) -> Result<(), AppError> {
    if !supported(app) {
        return Err(invalid());
    }
    restore(db, app).await?;
    let path = config_path(app)?;
    let mut doc = Document::read(app, &path)?;
    let added = if app == "zcode" {
        doc.prepare_zcode_official(&zcode_builtin()?)?
    } else {
        Vec::new()
    };
    let mut routes = doc.route(app, origin)?;
    for route in &mut routes {
        if let Some(rule) = route
            .pointer
            .strip_suffix("/config/api/baseUrl")
            .and_then(|path| doc.value.pointer(path))
        {
            route.created_provider = added
                .iter()
                .find(|created| created["providerId"] == rule["providerId"])
                .cloned();
        }
    }
    let backup = Backup {
        path: path.clone(),
        routes,
    };
    db.save_live_backup(app, &serde_json::to_string(&backup).map_err(|_| invalid())?)
        .await?;
    doc.write(&path)?;
    db.set_setting(&enabled_key(app), "true")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider() -> Value {
        json!({"baseUrl":"https://models.example/v1","apiKey":"synthetic-test-key","protocol":"openai","models":[{"model":"alpha"},{"model":"beta"}]})
    }

    #[tokio::test]
    async fn recovery_respects_externally_deleted_configuration() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("removed.json");
        let db = crate::database::Database::memory().expect("db");
        let backup = Backup {
            path: path.clone(),
            routes: vec![],
        };
        db.save_live_backup("qwen", &serde_json::to_string(&backup).expect("encode"))
            .await
            .expect("backup");
        restore(&db, "qwen").await.expect("restore deleted file");
        assert!(!path.exists());
        assert!(db.get_live_backup("qwen").await.expect("backup").is_none());
    }

    #[test]
    fn zcode_model_updates_preserve_native_capabilities_and_context_options() {
        let mut doc = Document::parse("zcode", "{}").expect("doc");
        doc.apply("zcode", "saved", "Gateway", &provider())
            .expect("apply");
        doc.value["config"]["modelConfigRules"]["providerModelRules"][0]["config"]["properties"] =
            json!({"contextWindow":32768,"nativeCapability":true});
        doc.apply("zcode", "saved", "Gateway", &provider())
            .expect("update");
        assert_eq!(
            doc.value["config"]["modelConfigRules"]["providerModelRules"][0]["config"]
                ["properties"],
            json!({"contextWindow":32768,"nativeCapability":true})
        );
    }

    #[test]
    fn removing_a_provider_preserves_credentials_edited_in_the_native_client() {
        for app in ["kimi", "qwen", "zcode"] {
            let mut doc = Document::parse(app, if app == "kimi" { "" } else { "{}" }).expect("doc");
            doc.apply(app, "saved", "Gateway", &provider())
                .expect("apply");
            match app {
                "kimi" => {
                    doc.value["providers"]["Gateway"]["api_key"] = json!("synthetic-external-edit")
                }
                "qwen" => {
                    doc.value["env"]["VARYAI_saved_API_KEY"] = json!("synthetic-external-edit")
                }
                _ => {
                    doc.value["config"]["providerConfigRules"]["providerRules"][0]["config"]
                        ["access"]["apiKey"] = json!("synthetic-external-edit")
                }
            }
            let before = doc.value.clone();
            assert!(
                doc.remove(app, "saved", &provider()).is_err(),
                "{app} must preserve external edits"
            );
            assert_eq!(doc.value, before);
        }
    }

    #[test]
    fn missing_native_endpoints_cannot_claim_successful_takeover() {
        for app in ["kimi", "qwen", "zcode"] {
            let mut doc = Document::parse(app, if app == "kimi" { "" } else { "{}" }).expect("doc");
            assert!(doc.route(app, "http://127.0.0.1:1234").is_err());
        }
    }

    #[test]
    fn concurrent_file_deletion_is_not_undone_by_config_write() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{}").expect("write");
        let doc = Document::read("qwen", &path).expect("read");
        std::fs::remove_file(&path).expect("external delete");
        assert!(doc.write(&path).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn updating_kimi_models_prunes_removed_models_and_preserves_native_options() {
        let mut doc = Document::parse("kimi", "").expect("parse");
        doc.apply("kimi", "saved", "Gateway", &provider())
            .expect("apply");
        doc.value["providers"]["Gateway"]["custom_headers"] = json!({"x-test":"preserve"});
        doc.value["models"]["varyai-saved-alpha"]["capabilities"] = json!(["vision"]);
        let mut next = provider();
        next["models"] = json!([{"model":"alpha"}]);
        doc.apply("kimi", "saved", "Gateway", &next)
            .expect("update");
        assert!(doc.value["models"].get("varyai-saved-beta").is_none());
        assert_eq!(
            doc.value["providers"]["Gateway"]["custom_headers"],
            json!({"x-test":"preserve"})
        );
        assert_eq!(
            doc.value["models"]["varyai-saved-alpha"]["capabilities"],
            json!(["vision"])
        );
    }

    #[test]
    fn qwen_same_model_keeps_distinct_protocols_endpoints_and_selection() {
        let mut doc = Document::parse("qwen", "{}").expect("parse");
        let chat = provider();
        doc.apply("qwen", "chat", "Chat", &chat)
            .expect("apply chat");
        let mut responses = chat.clone();
        responses["protocol"] = json!("openai-responses");
        doc.apply("qwen", "responses", "Responses", &responses)
            .expect("apply responses with same model");
        assert_eq!(
            doc.value["security"]["auth"]["selectedType"],
            "openai-responses"
        );
        assert!(doc.selection_owned("qwen", Some("responses")));
        assert!(!doc.selection_owned("qwen", Some("chat")));
        let selection = doc.selection("qwen");
        assert!(doc.selection_available("qwen", &selection));
        let mut alternate = chat.clone();
        alternate["baseUrl"] = json!("https://alternate.example/v1");
        doc.apply("qwen", "alternate", "Alternate", &alternate)
            .expect("apply alternate endpoint");
        assert!(doc.selection_owned("qwen", Some("alternate")));
        assert!(!doc.selection_owned("qwen", Some("chat")));
        doc.set_selection("qwen", &selection)
            .expect("restore response selection");
        assert_eq!(doc.value["model"]["baseUrl"], responses["baseUrl"]);
        let routes = doc.route("qwen", "http://127.0.0.1:1234").expect("route");
        let selected = routes
            .iter()
            .find(|route| route.provider_id == "responses" && route.models == ["alpha"])
            .expect("selected route");
        assert_eq!(doc.value["model"]["baseUrl"], selected.local);
        assert!(doc.selection_owned("qwen", Some("responses")));
        doc.restore(&routes).expect("restore");
        assert_eq!(doc.value["model"]["baseUrl"], responses["baseUrl"]);
        assert!(
            doc.apply("qwen", "duplicate", "Duplicate", &chat).is_err(),
            "identical native identities remain ambiguous"
        );
    }

    #[test]
    fn qwen_model_routes_stay_stable_when_native_entries_move() {
        let mut doc = Document::parse("qwen", "{}").expect("parse");
        doc.apply("qwen", "saved", "Gateway", &provider())
            .expect("apply");
        let routes = doc.route("qwen", "http://127.0.0.1:1234").expect("route");
        let original = routes
            .iter()
            .find(|r| r.models == ["alpha"])
            .expect("alpha")
            .local
            .clone();
        doc.restore(&routes).expect("restore");
        doc.value["modelProviders"]["openai"]
            .as_array_mut()
            .expect("models")
            .reverse();
        let routes = doc
            .route("qwen", "http://127.0.0.1:1234")
            .expect("route reordered");
        assert_eq!(
            routes
                .iter()
                .find(|r| r.models == ["alpha"])
                .expect("alpha")
                .local,
            original
        );
    }

    #[test]
    fn qwen_protocol_update_removes_previous_owned_protocol_entries() {
        let mut doc = Document::parse("qwen", "{}").expect("parse");
        doc.apply("qwen", "saved", "Gateway", &provider())
            .expect("apply");
        let mut next = provider();
        next["protocol"] = json!("anthropic");
        doc.apply("qwen", "saved", "Gateway", &next)
            .expect("update");
        assert!(doc.value["modelProviders"]["openai"]
            .as_array()
            .expect("array")
            .is_empty());
        assert_eq!(
            doc.value["modelProviders"]["anthropic"]
                .as_array()
                .expect("array")
                .len(),
            2
        );
    }

    #[test]
    fn kimi_projects_multiple_models_without_touching_login_or_unrelated_toml() {
        let original = "# user settings\ndefault_model = 'official'\n[providers.official]\ntype = 'kimi'\nbase_url = 'https://api.kimi.com/coding/v1'\n[providers.official.oauth]\nstorage = 'file'\nkey = 'oauth/kimi-code'\n[models.official]\nprovider = 'official'\nmodel = 'kimi-test'\nmax_context_size = 200000\n";
        let mut doc = Document::parse("kimi", original).expect("parse");
        doc.apply("kimi", "saved", "Gateway", &provider())
            .expect("apply");
        let serialized = doc.encode().expect("encode");
        assert!(serialized.contains("# user settings"));
        let value = Document::parse("kimi", &serialized)
            .expect("roundtrip")
            .value;
        assert_eq!(
            value["providers"]["official"]["oauth"]["key"],
            "oauth/kimi-code"
        );
        assert_eq!(value["models"]["varyai-saved-beta"]["model"], "beta");
        assert_eq!(value["default_model"], "varyai-saved-alpha");
    }

    #[test]
    fn kimi_provider_groups_follow_the_saved_name_and_keep_stable_model_aliases() {
        let mut doc = Document::parse("kimi", "").expect("parse");
        doc.apply("kimi", "saved", "Company", &provider())
            .expect("apply");
        assert!(doc.value["providers"].get("Company").is_some());
        assert_eq!(
            doc.value["models"]["varyai-saved-alpha"]["provider"],
            "Company"
        );
        let selection = doc.selection("kimi");
        doc.apply("kimi", "saved", "Renamed Company", &provider())
            .expect("rename");
        assert!(doc.value["providers"].get("Company").is_none());
        assert_eq!(
            doc.value["models"]["varyai-saved-alpha"]["provider"],
            "Renamed Company"
        );
        assert_eq!(doc.selection("kimi"), selection);
        let routes = doc.route("kimi", "http://127.0.0.1:1234").expect("route");
        assert_eq!(routes[0].provider_id, "saved");
        doc.restore(&routes).expect("restore");
        doc.remove("kimi", "saved", &provider()).expect("remove");
        assert!(doc.value["providers"].get("Renamed Company").is_none());
    }

    #[test]
    fn kimi_provider_name_migrates_legacy_groups_without_overwriting_native_groups() {
        let original = json!({
            "providers": {"varyai-saved": {"type":"openai", "base_url":"https://models.example/v1", "api_key":"synthetic-test-key", "custom_headers":{"x-keep":"yes"}}, "Native": {"type":"kimi", "base_url":"https://native.example/v1", "api_key":"synthetic-native"}},
            "models": {"varyai-saved-alpha":{"provider":"varyai-saved", "model":"alpha", "max_context_size":200000}},
            "default_model":"varyai-saved-alpha"
        });
        let mut doc = Document::parse("kimi", "").expect("parse");
        doc.value = original.clone();
        assert!(doc.apply("kimi", "saved", "Native", &provider()).is_err());
        assert_eq!(doc.value, original);
        doc.apply("kimi", "saved", "Company", &provider())
            .expect("migrate");
        assert_eq!(
            doc.value["providers"]["Company"]["custom_headers"]["x-keep"],
            "yes"
        );
        assert_eq!(
            doc.value["providers"]["Native"],
            original["providers"]["Native"]
        );
        assert!(doc.value["providers"].get("varyai-saved").is_none());
    }

    #[test]
    fn qwen_uses_independent_provider_and_environment_key() {
        let original = json!({"modelProviders":{"openai":[{"id":"existing","baseUrl":"https://user.example/v1","envKey":"USER_KEY"}]},"env":{"USER_KEY":"keep"},"unknown":true});
        let mut doc = Document::parse("qwen", &original.to_string()).expect("parse");
        doc.apply("qwen", "saved", "Gateway", &provider())
            .expect("apply");
        assert_eq!(
            doc.value["modelProviders"]["openai"][0],
            original["modelProviders"]["openai"][0]
        );
        assert_eq!(doc.value["modelProviders"]["openai"][1]["id"], "alpha");
        assert_eq!(doc.value["modelProviders"]["openai"][2]["id"], "beta");
        assert_eq!(
            doc.value["modelProviders"]["openai"][1]["envKey"],
            "VARYAI_saved_API_KEY"
        );
        assert_eq!(doc.value["security"]["auth"]["selectedType"], "openai");
        assert_eq!(doc.value["env"]["USER_KEY"], "keep");
    }

    #[test]
    fn zcode_route_ids_keep_provider_identity_when_other_models_are_removed() {
        let mut doc = Document::parse("zcode", "{}").expect("doc");
        doc.apply("zcode", "first", "First", &provider())
            .expect("first");
        doc.apply("zcode", "second", "Second", &provider())
            .expect("second");
        let original = doc.route("zcode", "http://127.0.0.1:1234").expect("route");
        let first_id = original
            .iter()
            .find(|r| r.provider_id == "first")
            .expect("first route")
            .id
            .clone();
        let second_id = original
            .iter()
            .find(|r| r.provider_id == "second")
            .expect("second route")
            .id
            .clone();
        doc.restore(&original).expect("restore");
        doc.remove("zcode", "first", &provider())
            .expect("withdraw first");
        let remaining = doc
            .route("zcode", "http://127.0.0.1:1234")
            .expect("reroute");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].provider_id, "second");
        assert_eq!(remaining[0].id, second_id);
        assert_ne!(
            remaining[0].id, first_id,
            "a removed model route must not become another provider's route"
        );
    }

    #[test]
    fn zcode_uses_current_desktop_provider_rules_and_preserves_native_login() {
        let original = json!({"schemaVersion":1,"config":{"providerConfigRules":{"providerRules":[{"providerId":"account:bigmodel-start-plan","config":{"api":{"type":"anthropic-messages","baseUrl":"https://zcode.z.ai/api/v1/zcode-plan/anthropic"}}}]},"modelConfigRules":{"providerModelRules":[],"manualProviderModelRules":[]}}});
        let mut doc = Document::parse("zcode", &original.to_string()).expect("parse");
        doc.apply("zcode", "saved", "Gateway", &provider())
            .expect("apply");
        let rules = doc
            .value
            .pointer("/config/providerConfigRules/providerRules")
            .and_then(Value::as_array)
            .expect("native rules");
        assert_eq!(
            rules[0],
            original["config"]["providerConfigRules"]["providerRules"][0]
        );
        assert_eq!(rules[1]["providerId"], "varyai-saved");
        assert_eq!(
            rules[1]["config"]["api"]["baseUrl"],
            "https://models.example/v1"
        );
        assert_eq!(
            rules[1]["config"]["personalModelIds"],
            json!(["alpha", "beta"])
        );
        assert!(doc.value.get("provider").is_none());
        let routes = doc.route("zcode", "http://127.0.0.1:1234").expect("route");
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().any(|r| r.provider_id == "saved"));
        doc.restore(&routes).expect("restore");
        doc.remove("zcode", "saved", &provider()).expect("remove");
        assert_eq!(
            doc.value["config"]["providerConfigRules"]["providerRules"],
            original["config"]["providerConfigRules"]["providerRules"]
        );
    }

    #[test]
    fn takeover_restores_only_its_own_endpoint_and_preserves_external_edits() {
        let mut doc = Document::parse("kimi", "[providers.native]\ntype='kimi'\nbase_url='https://api.kimi.com/coding/v1'\n[models.a]\nprovider='native'\nmodel='a'\n").expect("parse");
        let routes = doc.route("kimi", "http://127.0.0.1:1234").expect("route");
        assert_eq!(routes.len(), 1);
        assert!(doc.value["providers"]["native"]["base_url"]
            .as_str()
            .expect("endpoint")
            .contains("/agents/kimi/"));
        doc.value["theme"] = json!("light");
        doc.restore(&routes).expect("restore");
        assert_eq!(
            doc.value["providers"]["native"]["base_url"],
            "https://api.kimi.com/coding/v1"
        );
        assert_eq!(doc.value["theme"], "light");
        doc.value["providers"]["native"]["base_url"] = json!("https://user-edited.example/v1");
        doc.restore(&routes).expect("preserve edited endpoint");
        assert_eq!(
            doc.value["providers"]["native"]["base_url"],
            "https://user-edited.example/v1"
        );
    }

    #[test]
    fn kimi_routes_model_specific_endpoint_overrides() {
        let mut doc = Document::parse("kimi", "[providers.native]\ntype='openai'\nbase_url='https://default.example/v1'\n[models.a]\nprovider='native'\nmodel='a'\nbase_url='https://override.example/v1'\n").expect("parse");
        let routes = doc.route("kimi", "http://127.0.0.1:1234").expect("route");
        assert!(routes
            .iter()
            .any(|r| r.upstream == "https://override.example/v1"));
        assert!(doc.value["models"]["a"]["base_url"]
            .as_str()
            .expect("base")
            .starts_with("http://127.0.0.1:1234/agents/kimi/"));
        doc.restore(&routes).expect("restore");
        assert_eq!(
            doc.value["models"]["a"]["base_url"],
            "https://override.example/v1"
        );
    }

    #[test]
    fn qwen_removes_only_owned_models_after_array_reordering() {
        let mut doc = Document::parse("qwen", "{}").expect("parse");
        doc.apply("qwen", "saved", "Gateway", &provider())
            .expect("apply");
        let routes = doc.route("qwen", "http://127.0.0.1:1234").expect("route");
        assert!(routes.iter().all(|r| r.provider_id == "saved"));
        doc.value["modelProviders"]["openai"]
            .as_array_mut()
            .expect("models")
            .reverse();
        doc.restore(&routes).expect("restore");
        doc.value["modelProviders"]["openai"]
            .as_array_mut()
            .expect("models")
            .push(json!({"id":"user-model","baseUrl":"https://user.example","envKey":"USER_KEY"}));
        doc.remove("qwen", "saved", &provider()).expect("remove");
        assert_eq!(
            doc.value["modelProviders"]["openai"]
                .as_array()
                .expect("models")
                .len(),
            1
        );
        assert_eq!(doc.value["modelProviders"]["openai"][0]["id"], "user-model");
        assert!(doc.value["env"].get("VARYAI_saved_API_KEY").is_none());
    }

    #[test]
    fn zcode_official_defaults_route_without_copying_account_credentials_and_restore_cleanly() {
        let mut doc=Document::parse("zcode",r#"{"schemaVersion":1,"config":{"providerConfigRules":{"providerRules":[]},"modelConfigRules":{"providerModelRules":[],"manualProviderModelRules":[]}}}"#).expect("parse");
        let original = doc.value.clone();
        let builtin = json!({"config":{"providerConfigRules":{"providerRules":[{"providerId":"account:bigmodel-start-plan","config":{"access":{"type":"zhipu-account","mode":"start-plan"},"api":{"type":"anthropic-messages","baseUrl":"https://zcode.z.ai/api/v1/zcode-plan/anthropic"},"builtinModelIds":["glm-test"]}}]}}});
        let added = doc.prepare_zcode_official(&builtin).expect("defaults");
        let mut routes = doc.route("zcode", "http://127.0.0.1:1234").expect("route");
        for route in &mut routes {
            route.created_provider = added
                .iter()
                .find(|rule| {
                    rule.pointer("/config/api/baseUrl").and_then(Value::as_str)
                        == Some(route.upstream.as_str())
                })
                .cloned();
        }
        assert!(
            routes[0].local.ends_with("/anthropic"),
            "Preserve the suffix the native SDK uses to append /v1"
        );
        assert_eq!(routes.len(), 1);
        assert!(
            doc.value["config"]["providerConfigRules"]["providerRules"][0]["config"]
                .get("access")
                .is_none()
        );
        doc.restore(&routes).expect("restore");
        assert_eq!(doc.value, original);
    }

    #[test]
    fn reject_unsupported_protocols_and_never_invent_trae_storage() {
        assert!(validate(&json!({"baseUrl":"file:///tmp/secret","apiKey":"test","protocol":"openai","models":[{"model":"a"}]})).is_err());
        assert!(Document::parse("trae", "{}").is_err());
    }
}
