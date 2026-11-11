//! Desktop profile patches. Account credentials remain owned by Harness.
use serde_json::{json, Value};
use std::path::PathBuf;

#[cfg(test)]
const START: &str = "# BEGIN VaryAI model providers\n";
#[cfg(test)]
const END: &str = "# END VaryAI model providers\n";
const PLUGIN: &str = "@deepseek-ai/dsh-llm-pi-ai";
const FAILED: &str = "agentConnections.failed";

pub(crate) fn config_path() -> PathBuf {
    // A test/debug home must not inherit a running Harness instance's home.
    let isolated = std::env::var("CC_SWITCH_TEST_HOME").is_ok_and(|home| !home.trim().is_empty());
    std::env::var_os("DSH_HOME")
        .filter(|value| !isolated && !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::get_home_dir().join(".dsh"))
        .join("profiles/desktop/cordis.patch.yml")
}

fn array(text: &str) -> Result<Vec<Value>, String> {
    let value: Value = serde_yaml::from_str(text).map_err(|_| FAILED)?;
    if value.is_null() {
        return Ok(Vec::new());
    }
    value.as_array().cloned().ok_or_else(|| FAILED.into())
}

fn foreign_conflict(value: &Value, id: &str) -> bool {
    match value {
        Value::Object(values) => {
            value["id"] == key(id) || values.values().any(|value| foreign_conflict(value, id))
        }
        Value::Array(values) => values.iter().any(|value| foreign_conflict(value, id)),
        _ => false,
    }
}

/// Read Cordis entries as Harness saves them; comments are not configuration.
fn parse_entries(entries: Vec<Value>) -> Result<Value, String> {
    let mut patches = Vec::new();
    let mut providers = serde_json::Map::new();
    let mut selection = Vec::new();
    let mut legacy = false;
    for mut entry in entries {
        if entry["id"] == "agent-default-model" && entry["config"].is_object() {
            selection = vec![json!({"id":"agent-default-model","config":entry["config"]})];
            entry.as_object_mut().ok_or(FAILED)?.remove("config");
            if entry.as_object().is_some_and(|row| row.len() == 1) {
                continue;
            }
        }
        if entry["id"] == "llm-pi-ai" {
            if entry.get("name").is_some_and(|name| name != PLUGIN)
                && entry
                    .pointer("/config/providers")
                    .and_then(Value::as_object)
                    .is_some_and(|providers| providers.keys().any(|key| key.starts_with("varyai-")))
            {
                return Err(FAILED.into());
            }
            if let Some(routes) = entry
                .pointer_mut("/config/providers")
                .and_then(Value::as_object_mut)
            {
                let ids = routes
                    .keys()
                    .filter(|id| id.starts_with("varyai-"))
                    .cloned()
                    .collect::<Vec<_>>();
                let managed = !ids.is_empty();
                for route in ids {
                    let profile = routes.remove(&route).ok_or(FAILED)?;
                    let id = key(route.trim_start_matches("varyai-"));
                    if providers
                        .insert(
                            id.clone(),
                            json!({"id":id,"name":PLUGIN,"config":{"providers":{route:profile}}}),
                        )
                        .is_some()
                    {
                        return Err(FAILED.into());
                    }
                }
                if managed {
                    if routes.is_empty() {
                        entry["config"]
                            .as_object_mut()
                            .ok_or(FAILED)?
                            .remove("providers");
                    }
                    if entry["config"]
                        .as_object()
                        .is_some_and(|config| config.is_empty())
                    {
                        entry.as_object_mut().ok_or(FAILED)?.remove("config");
                    }
                    // Older VaryAI versions appended a copy of the native adapter.
                    // Keep its original row when the remaining config is identical.
                    if patches.iter().rev().any(|row: &Value| {
                        row["id"] == "llm-pi-ai" && row["config"] == entry["config"]
                    }) || (entry.get("config").is_none()
                        && entry.as_object().is_some_and(|row| {
                            row.keys().all(|key| matches!(key.as_str(), "id" | "name"))
                        }))
                    {
                        continue;
                    }
                }
            }
        }
        if let Some(children) = entry.get("insert").and_then(Value::as_array) {
            let mut kept = Vec::new();
            for child in children {
                if child["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("varyai-provider-"))
                    && child["name"] == PLUGIN
                {
                    let id = child["id"].as_str().ok_or(FAILED)?;
                    if providers.insert(id.into(), child.clone()).is_some() {
                        return Err(FAILED.into());
                    }
                    legacy = true;
                } else {
                    kept.push(child.clone());
                }
            }
            if kept.len() != children.len() {
                if kept.is_empty() {
                    continue;
                }
                entry["insert"] = json!(kept);
            }
        }
        if entry["id"] == "llm-pi-ai" && entry.get("name").is_none_or(|name| name == PLUGIN) {
            if entry
                .pointer("/config/providers")
                .and_then(Value::as_object)
                .is_some_and(|routes| routes.is_empty())
            {
                entry["config"]
                    .as_object_mut()
                    .ok_or(FAILED)?
                    .remove("providers");
            }
            if entry["config"]
                .as_object()
                .is_some_and(|config| config.is_empty())
            {
                entry.as_object_mut().ok_or(FAILED)?.remove("config");
            }
            if entry
                .as_object()
                .is_some_and(|row| row.keys().all(|key| matches!(key.as_str(), "id" | "name")))
            {
                continue;
            }
        }
        patches.push(entry);
    }
    let patches = json!(patches);
    if providers
        .keys()
        .any(|id| foreign_conflict(&patches, id.trim_start_matches("varyai-provider-")))
    {
        return Err(FAILED.into());
    }
    Ok(
        json!({"patches":patches,"providers":providers,"selection":selection,"legacyProviders":legacy}),
    )
}

pub(crate) fn parse(text: &str) -> Result<Value, String> {
    parse_entries(array(text)?)
}

/// Normalize receipts written before structural Cordis editing.
pub(crate) fn normalize(value: &Value) -> Result<Value, String> {
    let mut entries = value["patches"].as_array().cloned().ok_or(FAILED)?;
    for provider in value["providers"].as_object().ok_or(FAILED)?.values() {
        if value["legacyProviders"] == true {
            entries.push(json!({"insert":[provider]}));
        } else {
            entries.push(json!({"id":"llm-pi-ai","name":PLUGIN,"config":provider["config"]}));
        }
    }
    entries.extend(value["selection"].as_array().cloned().unwrap_or_default());
    parse_entries(entries)
}

fn entries(next: &Value) -> Result<Vec<Value>, String> {
    let mut entries = next["patches"].as_array().cloned().ok_or(FAILED)?;
    let providers = next["providers"].as_object().ok_or(FAILED)?;
    if next["legacyProviders"] == true {
        entries.extend(
            providers
                .values()
                .map(|provider| json!({"insert":[provider]})),
        );
    } else if !providers.is_empty() {
        let index = entries
            .iter()
            .rposition(|row| row["id"] == "llm-pi-ai" && row["config"].is_object())
            .or_else(|| entries.iter().rposition(|row| row["id"] == "llm-pi-ai"));
        let index = index.unwrap_or_else(|| {
            entries.push(json!({"id":"llm-pi-ai","name":PLUGIN}));
            entries.len() - 1
        });
        let row = &mut entries[index];
        if row.get("config").is_none() {
            row["config"] = json!({});
        }
        if row["config"].get("providers").is_none() {
            row["config"]["providers"] = json!({});
        }
        let routes = row["config"]["providers"].as_object_mut().ok_or(FAILED)?;
        for provider in providers.values() {
            for (id, profile) in provider["config"]["providers"].as_object().ok_or(FAILED)? {
                if routes.insert(id.clone(), profile.clone()).is_some() {
                    return Err(FAILED.into());
                }
            }
        }
    }
    if let Some(selection) = next["selection"].as_array().and_then(|rows| rows.last()) {
        if let Some(row) = entries
            .iter_mut()
            .rev()
            .find(|row| row["id"] == "agent-default-model")
        {
            row["config"] = selection["config"].clone();
        } else {
            entries.push(selection.clone());
        }
    }
    Ok(entries)
}

/// Retain unchanged foreign rows and comments while editing native entry configs.
fn render(source: &str, entries: &[Value]) -> Result<String, String> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        if line.starts_with("- ") {
            starts.push(offset);
        }
        offset += line.len();
    }
    let old = array(source)?;
    if old.len() != starts.len() {
        return serde_yaml::to_string(entries).map_err(|_| FAILED.into());
    }
    let head = &source[..starts.first().copied().unwrap_or(source.len())];
    let clean_comments = |text: &str| -> String {
        text.split_inclusive('\n')
            .filter(|line| {
                !matches!(
                    line.trim_end(),
                    "# BEGIN VaryAI model providers" | "# END VaryAI model providers"
                )
            })
            .collect()
    };
    let mut text = if old.is_empty() {
        head.lines()
            .filter(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
            .map(|line| format!("{line}\n"))
            .collect()
    } else {
        clean_comments(head)
    };
    let mut used = vec![false; old.len()];
    for entry in entries {
        if let Some(index) = old
            .iter()
            .enumerate()
            .position(|(index, row)| !used[index] && row == entry)
        {
            used[index] = true;
            text.push_str(&clean_comments(
                &source[starts[index]..starts.get(index + 1).copied().unwrap_or(source.len())],
            ));
            if !text.ends_with('\n') {
                text.push('\n');
            }
        } else {
            text.push_str(&serde_yaml::to_string(&vec![entry]).map_err(|_| FAILED)?);
        }
    }
    if entries.is_empty() {
        text.push_str("[]\n");
    }
    Ok(text)
}

pub(crate) fn encode(source: &str, next: &Value) -> Result<String, String> {
    if parse(source)? == *next {
        return Ok(source.into());
    }
    let text = render(source, &entries(next)?)?;
    if parse(&text)? != *next {
        return Err(FAILED.into());
    }
    Ok(text)
}

fn key(id: &str) -> String {
    format!("varyai-provider-{id}")
}

fn valid_selection(config: &Value) -> bool {
    ["provider", "model"].iter().all(|field| {
        config[*field].as_str().is_some_and(|text| {
            !text.is_empty() && text.len() <= 512 && !text.chars().any(char::is_control)
        })
    })
}

fn foreign_selection(value: &Value) -> Value {
    if value["id"] == "agent-default-model" && value["config"].is_object() {
        return value["config"].clone();
    }
    match value {
        Value::Array(values) => values
            .iter()
            .rev()
            .map(foreign_selection)
            .find(|value| !value.is_null())
            .unwrap_or(Value::Null),
        Value::Object(values) => values
            .values()
            .rev()
            .map(foreign_selection)
            .find(|value| !value.is_null())
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

pub(crate) fn selection(value: &Value) -> Value {
    value["selection"]
        .as_array()
        .and_then(|values| values.last())
        .map(|entry| entry["config"].clone())
        .unwrap_or_else(|| foreign_selection(&value["patches"]))
}

pub(crate) fn set_selection(value: &mut Value, config: &Value) -> Result<(), String> {
    if config.is_null() {
        value["selection"] = json!([]);
    } else if valid_selection(config) {
        value["selection"] = json!([{"id":"agent-default-model","config":config}]);
    } else {
        return Err(FAILED.into());
    }
    Ok(())
}
pub(crate) fn apply(value: &mut Value, id: &str, name: &str, config: &Value) -> Result<(), String> {
    if foreign_conflict(&value["patches"], id) {
        return Err(FAILED.into());
    }
    let protocol = match config["protocol"].as_str() {
        Some("openai") => "openai-completions",
        Some("openai-responses") => "openai-responses",
        Some("anthropic") => "anthropic-messages",
        _ => return Err(FAILED.into()),
    };
    let auth = if protocol == "anthropic-messages" {
        json!({"x-api-key":config["apiKey"]})
    } else {
        json!({"Authorization":format!("Bearer {}",config["apiKey"].as_str().ok_or(FAILED)?)})
    };
    let models = config["models"].as_array().ok_or(FAILED)?.iter().map(|model| {
        let mut row = json!({"id":model["model"],"name":model.get("displayName").unwrap_or(&model["model"])});
        if let Some(window) = model["contextWindow"].as_u64().filter(|window| *window > 0) {
            row["contextWindow"] = json!(window);
            row["maxTokens"] = json!((window / 2).clamp(1, 8192));
        }
        for field in ["input", "reasoningEfforts"] {
            if let Some(value) = model.get(field) { row[field] = value.clone(); }
        }
        row
    }).collect::<Vec<_>>();
    let route = format!("varyai-{id}");
    let provider = json!({"id":key(id),"name":PLUGIN,"config":{"providers":{route:{"displayName":name,"api":protocol,"baseURL":config["baseUrl"],"headers":auth,"models":models,"defaultContextWindow":32000,"defaultMaxTokens":8192}}}});
    value["providers"]
        .as_object_mut()
        .ok_or(FAILED)?
        .insert(key(id), provider);
    value["legacyProviders"] = json!(false);
    set_selection(
        value,
        &json!({"provider":format!("varyai-{id}"),"model":config["models"][0]["model"]}),
    )?;
    Ok(())
}

pub(crate) fn remove(value: &mut Value, id: &str, expected: &Value) -> Result<(), String> {
    let providers = value["providers"].as_object_mut().ok_or(FAILED)?;
    if let Some(entry) = providers.get(&key(id)) {
        let route = &entry["config"]["providers"][format!("varyai-{id}")];
        let header = if expected["protocol"] == "anthropic" {
            "x-api-key"
        } else {
            "Authorization"
        };
        let auth = if header == "Authorization" {
            format!("Bearer {}", expected["apiKey"].as_str().ok_or(FAILED)?)
        } else {
            expected["apiKey"].as_str().ok_or(FAILED)?.to_owned()
        };
        if route["baseURL"] != expected["baseUrl"] || route["headers"][header] != auth {
            return Err(FAILED.into());
        }
        providers.remove(&key(id));
    }
    if providers.is_empty() {
        value["legacyProviders"] = json!(false);
    }
    if selection(value)["provider"] == format!("varyai-{id}") {
        set_selection(value, &Value::Null)?;
    }
    Ok(())
}

pub(crate) fn candidates(value: &Value) -> Vec<(String, String, Vec<String>)> {
    fn walk(value: &Value, path: &str, result: &mut Vec<(String, String, Vec<String>)>) {
        if let Some(config) = value
            .get("config")
            .filter(|_| value["id"] == "llm-pi-ai" || value["name"] == PLUGIN)
        {
            if let Some(providers) = config["providers"].as_object() {
                for (id, provider) in providers {
                    if let Some(url) = provider["baseURL"].as_str() {
                        let escaped = id.replace('~', "~0").replace('/', "~1");
                        let models = provider["models"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|model| model["id"].as_str().map(String::from))
                            .collect();
                        result.push((
                            format!("{path}/config/providers/{escaped}/baseURL"),
                            url.into(),
                            models,
                        ));
                    }
                }
            }
        }
        match value {
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    walk(item, &format!("{path}/{index}"), result);
                }
            }
            Value::Object(items) => {
                for (key, item) in items {
                    walk(
                        item,
                        &format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                        result,
                    );
                }
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    walk(value, "", &mut result);
    result
}

/// Native protocol, timeout and compatibility controls do not change route ownership.
pub(crate) fn adopt(installed: &Value, live: &Value) -> Value {
    let mut adopted = installed.clone();
    let Some(providers) = installed["providers"].as_object() else {
        return adopted;
    };
    for (id, entry) in providers {
        let Some(routes) = entry["config"]["providers"].as_object() else {
            continue;
        };
        for (route, old) in routes {
            let Some(current) = live["providers"][id]["config"]["providers"].get(route) else {
                continue;
            };
            let same_gateway = old["baseURL"]
                .as_str()
                .zip(current["baseURL"].as_str())
                .is_some_and(|(old, cur)| {
                    old.trim_end_matches('/').trim_end_matches("/v1")
                        == cur.trim_end_matches('/').trim_end_matches("/v1")
                });
            let known_api = matches!(
                current["api"].as_str(),
                Some("openai-completions" | "openai-responses" | "anthropic-messages")
            );
            let auth_matches = old["headers"].as_object().is_some_and(|headers| {
                headers
                    .iter()
                    .all(|(name, value)| current["headers"].get(name) == Some(value))
            });
            if !same_gateway || !known_api || !auth_matches {
                continue;
            }
            let mut core = current.clone();
            for field in ["api", "baseURL", "headers", "streamIdleTimeoutMs"] {
                match old.get(field) {
                    Some(value) => core[field] = value.clone(),
                    None => {
                        if let Some(map) = core.as_object_mut() {
                            map.remove(field);
                        }
                    }
                }
            }
            if let (Some(models), Some(prior)) =
                (core["models"].as_array_mut(), old["models"].as_array())
            {
                for model in models {
                    if let Some(old_model) = prior.iter().find(|row| row["id"] == model["id"]) {
                        match old_model.get("compat") {
                            Some(compat) => model["compat"] = compat.clone(),
                            None => {
                                if let Some(map) = model.as_object_mut() {
                                    map.remove("compat");
                                }
                            }
                        }
                    }
                }
            }
            // New optional route settings belong to the client, not the catalog.
            if let Some(map) = core.as_object_mut() {
                map.retain(|key, _| old.get(key).is_some());
            }
            if core == *old {
                adopted["providers"][id]["config"]["providers"][route] = current.clone();
            }
        }
    }
    adopted
}

fn adaptive_with_effort(model: &str) -> bool {
    static GLM: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    GLM.get_or_init(|| regex::Regex::new(r"(?i)glm-5\.[23](?:$|[-.:/\[])").ok())
        .as_ref()
        .is_some_and(|pattern| pattern.is_match(model))
}

/// Match Magpie's per-protocol provider routes and preserve native compatibility choices.
pub(crate) fn project(
    value: &mut Value,
    catalog: &[super::agent_connections::Model],
    base: &str,
    token: &str,
    selected: &str,
) -> Result<(), String> {
    let mut selected_route = None;
    for (suffix, native_api, label) in [
        ("", "openai-completions", "Vary AI"),
        ("-responses", "openai-responses", "Vary AI · Responses"),
        ("-anthropic", "anthropic-messages", "Vary AI · Anthropic"),
    ] {
        let id = format!("shared-catalog{suffix}");
        let route = format!("varyai-{id}");
        let old = value["providers"][key(&id)]["config"]["providers"][&route].clone();
        if !old.is_null() && old["headers"]["Authorization"] != format!("Bearer {token}") {
            return Err(FAILED.into());
        }
        let models = catalog
            .iter()
            .filter(|model| model.api == native_api)
            .collect::<Vec<_>>();
        if models.is_empty() {
            value["providers"]
                .as_object_mut()
                .ok_or(FAILED)?
                .remove(&key(&id));
            continue;
        }
        let api = old["api"]
            .as_str()
            .filter(|api| {
                matches!(
                    *api,
                    "openai-completions" | "openai-responses" | "anthropic-messages"
                )
            })
            .unwrap_or(native_api);
        let mut profile = if old.is_object() {
            old.clone()
        } else {
            json!({})
        };
        profile["displayName"] = json!(label);
        profile["api"] = json!(api);
        profile["baseURL"] = json!(if api == "anthropic-messages" {
            base.to_owned()
        } else {
            format!("{base}/v1")
        });
        if profile.get("headers").is_none() {
            profile["headers"] = json!({});
        }
        profile["headers"]["Authorization"] = json!(format!("Bearer {token}"));
        if profile.get("streamIdleTimeoutMs").is_none() {
            profile["streamIdleTimeoutMs"] = json!(600_000);
        }
        profile["models"] = json!(models.iter().map(|model| {
            let mut row = json!({"id":model.id,"name":model.name,"input":if model.images {vec!["text","image"]} else {vec!["text"]}});
            if let Some(window) = model.context_window { row["contextWindow"] = json!(window); }
            if let Some(output) = model.output_tokens {
                let ceiling = model.context_window.map(|window|(window/4).max((window/2).saturating_sub(65536)).max(1));
                row["maxTokens"] = json!(ceiling.map_or(output,|limit|output.min(limit)));
            }
            let levels = model.reasoning_levels.iter().filter_map(|level|match level.as_str() {
                "none"|"off" => Some(("off".to_owned(),json!(level))),
                "ultra" => Some(("max".to_owned(),json!(level))),
                "minimal"|"low"|"medium"|"high"|"xhigh"|"max" => Some((level.clone(),json!(level))),
                _ => None,
            }).collect::<serde_json::Map<_,_>>();
            if levels.keys().any(|level|level!="off") { row["reasoningEfforts"] = json!(levels); }
            if old["api"] == api {
                if let Some(previous) = old["models"].as_array().into_iter().flatten().find(|row|row["id"]==model.id) {
                    if let Some(compat) = previous.get("compat") { row["compat"] = compat.clone(); }
                }
            }
            if api == "openai-responses" { row["compat"]["supportsStrictMode"] = json!(true); }
            if api == "anthropic-messages" {
                let native = model.native_model.as_deref().unwrap_or(&model.id);
                let adaptive_effort = adaptive_with_effort(native);
                if crate::proxy::thinking_optimizer::uses_adaptive_thinking(native) || adaptive_effort {
                    row["compat"]["forceAdaptiveThinking"] = json!(true);
                }
            }
            row
        }).collect::<Vec<_>>());
        value["providers"][key(&id)] =
            json!({"id":key(&id),"name":PLUGIN,"config":{"providers":{route.clone():profile}}});
        if models.iter().any(|model| model.id == selected) {
            selected_route = Some(route);
        }
    }
    value["legacyProviders"] = json!(false);
    set_selection(
        value,
        &json!({"provider":selected_route.ok_or(FAILED)?,"model":selected}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_native_adapter_templates_accept_routes_and_restore() -> Result<(), String> {
        for source in [
            "- id: llm-pi-ai\n  config: {providers: {}}\n",
            "- id: llm-pi-ai\n  name: '@deepseek-ai/dsh-llm-pi-ai'\n  config: {}\n",
            "- id: llm-pi-ai\n  disabled: false\n  config: {providers: {}}\n",
        ] {
            let mut doc = parse(source)?;
            let before = doc.clone();
            let config = json!({"baseUrl":"http://127.0.0.1:1/v1","protocol":"openai","apiKey":"synthetic-only","models":[{"model":"shared-model"}]});
            apply(&mut doc, "shared-catalog", "Vary AI", &config)?;
            let encoded = encode(source, &doc)?;
            assert_eq!(parse(&encoded)?, doc);
            remove(&mut doc, "shared-catalog", &config)?;
            assert_eq!(parse(&encode(&encoded, &doc)?)?, before);
        }
        Ok(())
    }

    #[test]
    fn harness_saves_do_not_depend_on_comment_markers() -> Result<(), String> {
        let source = "# Native profile\n- id: llm-pi-ai\n  config:\n    providers:\n      native:\n        baseURL: https://synthetic.invalid/v1\n        models: [{id: native-model}]\n- id: agent-default-model\n  config: {provider: native, model: native-model}\n";
        let config = json!({"baseUrl":"http://127.0.0.1:1/v1","protocol":"openai","apiKey":"synthetic-only","models":[{"model":"shared-model"}]});
        let mut doc = parse(source)?;
        apply(&mut doc, "shared-catalog", "Vary AI", &config)?;
        let text = encode(source, &doc)?;
        let saved = text.replace(START, "").replace(END, "");
        assert_eq!(parse(&saved)?, doc);
        for saved in [text.replace(START, ""), text.replace(END, "")] {
            assert_eq!(parse(&saved)?, doc);
        }
        let entries = array(&text)?;
        assert_eq!(
            entries
                .iter()
                .filter(|row| row["id"] == "llm-pi-ai")
                .count(),
            1
        );
        assert_eq!(
            entries
                .iter()
                .filter(|row| row["id"] == "agent-default-model")
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn client_saved_foreign_providers_and_native_choices_survive_sync() -> Result<(), String> {
        let source = "- id: llm-pi-ai\n  name: '@deepseek-ai/dsh-llm-pi-ai'\n  config:\n    providers:\n      native:\n        baseURL: https://synthetic.invalid/v1\n        models: [{id: native-model}]\n      varyai-shared-catalog:\n        api: openai-completions\n        baseURL: http://127.0.0.1:1/v1\n        headers: {Authorization: Bearer synthetic-only}\n        models: [{id: shared-model}]\n- id: agent-default-model\n  config: {provider: native, model: native-model, reasoningEffort: high}\n";
        let mut doc = parse(source)?;
        assert_eq!(doc["providers"].as_object().ok_or(FAILED)?.len(), 1);
        let choice = selection(&doc);
        let config = json!({"baseUrl":"http://127.0.0.1:1/v1","protocol":"openai","apiKey":"synthetic-only","models":[{"model":"shared-model"},{"model":"new-model"}]});
        apply(&mut doc, "shared-catalog", "Vary AI", &config)?;
        set_selection(&mut doc, &choice)?;
        let text = encode(source, &doc)?;
        let parsed = parse(&text)?;
        assert_eq!(selection(&parsed), choice);
        assert_eq!(
            parsed["patches"][0]["config"]["providers"]["native"]["models"][0]["id"],
            "native-model"
        );
        remove(&mut doc, "shared-catalog", &config)?;
        let restored = parse(&encode(&text, &doc)?)?;
        assert_eq!(selection(&restored), choice);
        assert!(restored["providers"].as_object().ok_or(FAILED)?.is_empty());
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn isolated_home_takes_priority_over_an_inherited_harness_home() {
        struct Restore {
            isolated: Option<std::ffi::OsString>,
            inherited: Option<std::ffi::OsString>,
        }
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, previous) in [
                    ("CC_SWITCH_TEST_HOME", &self.isolated),
                    ("DSH_HOME", &self.inherited),
                ] {
                    match previous {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
        let _restore = Restore {
            isolated: std::env::var_os("CC_SWITCH_TEST_HOME"),
            inherited: std::env::var_os("DSH_HOME"),
        };
        let isolated = tempfile::tempdir().unwrap();
        let inherited = tempfile::tempdir().unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", isolated.path());
        std::env::set_var("DSH_HOME", inherited.path());
        assert_eq!(
            config_path(),
            isolated
                .path()
                .join(".dsh/profiles/desktop/cordis.patch.yml")
        );
        std::env::set_var("CC_SWITCH_TEST_HOME", "   ");
        assert_eq!(
            config_path(),
            inherited.path().join("profiles/desktop/cordis.patch.yml")
        );
        std::env::remove_var("CC_SWITCH_TEST_HOME");
        assert_eq!(
            config_path(),
            inherited.path().join("profiles/desktop/cordis.patch.yml")
        );
    }

    #[test]
    fn custom_routes_extend_the_existing_adapter_and_keep_native_providers() -> Result<(), String> {
        let source = "# User provider\n- id: llm-pi-ai\n  config:\n    providers:\n      native:\n        baseURL: https://synthetic.invalid/v1\n        models: [{id: native-model}]\n";
        let mut doc = parse(source)?;
        let config = json!({"baseUrl":"http://127.0.0.1:1/v1","protocol":"openai","apiKey":"synthetic-only","models":[{"model":"shared-model"}]});
        apply(&mut doc, "shared-catalog", "Vary AI", &config)?;
        let text = encode(source, &doc)?;
        let patches = array(&text)?;
        assert!(
            !patches.iter().any(|entry| entry.get("insert").is_some()),
            "a second adapter duplicates Harness's provider directory"
        );
        let config = &patches
            .iter()
            .rev()
            .find(|entry| entry["id"] == "llm-pi-ai")
            .ok_or(FAILED)?["config"];
        assert_eq!(
            config["providers"]["native"]["baseURL"],
            "https://synthetic.invalid/v1"
        );
        assert!(config["providers"].get("varyai-shared-catalog").is_some());
        assert_eq!(parse(&text)?, doc);
        Ok(())
    }

    #[test]
    fn legacy_adapter_insertions_migrate_without_changing_route_identity() -> Result<(), String> {
        let config = json!({"baseUrl":"http://127.0.0.1:1/v1","protocol":"openai","apiKey":"synthetic-only","models":[{"model":"shared-model"}]});
        let mut doc = parse("[]")?;
        apply(&mut doc, "shared-catalog", "Vary AI", &config)?;
        doc["legacyProviders"] = json!(true);
        let old = encode("[]", &doc)?;
        let mut migrated = parse(&old)?;
        let old_provider = migrated["providers"].clone();
        apply(&mut migrated, "shared-catalog", "Vary AI", &config)?;
        let new = encode(&old, &migrated)?;
        assert_ne!(new, old);
        assert!(!new.contains("- insert:"));
        assert_eq!(migrated["providers"], old_provider);
        assert_eq!(parse(&new)?, migrated);
        Ok(())
    }

    #[test]
    fn native_provider_protocols_route_and_restore_without_rewriting_foreign_plugins(
    ) -> Result<(), String> {
        for protocol in ["openai", "openai-responses", "anthropic"] {
            let config = json!({"baseUrl":"https://synthetic.invalid/v1","protocol":protocol,"apiKey":"synthetic-only","models":[{"model":"test-model"}]});
            let mut doc = super::super::agent_clients::Document::parse(
                "deepseek-harness",
                "# Customizations\n[]\n",
            )
            .map_err(|_| FAILED)?;
            doc.apply("deepseek-harness", "custom", "Gateway", &config)
                .map_err(|_| FAILED)?;
            let baseline = doc.encode().map_err(|_| FAILED)?;
            let routes = doc
                .route("deepseek-harness", "http://127.0.0.1:1")
                .map_err(|_| FAILED)?;
            assert_eq!(routes.len(), 1);
            assert_eq!(routes[0].provider_id, "custom");
            assert_eq!(routes[0].models, vec!["test-model"]);
            assert!(doc
                .encode()
                .map_err(|_| FAILED)?
                .contains("/agents/deepseek-harness/providers/"));
            doc.restore(&routes).map_err(|_| FAILED)?;
            assert_eq!(doc.encode().map_err(|_| FAILED)?, baseline);
            doc.remove("deepseek-harness", "custom", &config)
                .map_err(|_| FAILED)?;
            assert!(parse(&doc.encode().map_err(|_| FAILED)?)?["providers"]
                .as_object()
                .ok_or(FAILED)?
                .is_empty());
        }
        Ok(())
    }

    #[test]
    fn foreign_providers_are_not_claimed_and_customized_auth_is_not_removed() -> Result<(), String>
    {
        let config = json!({"baseUrl":"https://synthetic.invalid/v1","protocol":"openai","apiKey":"synthetic-only","models":[{"model":"test"}]});
        let mut foreign =
            parse("- insert:\n  - id: varyai-provider-custom\n    name: some-other-plugin\n")?;
        let original = foreign.clone();
        assert!(apply(&mut foreign, "custom", "Gateway", &config).is_err());
        assert_eq!(foreign, original);
        let mut value = parse("[]")?;
        apply(&mut value, "custom", "Gateway", &config)?;
        let encoded = encode("[]", &value)?;
        assert!(parse(&format!(
            "{encoded}- id: varyai-provider-custom\n  config: {{}}\n"
        ))
        .is_err());
        value["providers"]["varyai-provider-custom"]["config"]["providers"]["varyai-custom"]
            ["headers"]["Authorization"] = json!("Bearer synthetic-change");
        let changed = value.clone();
        assert!(remove(&mut value, "custom", &config).is_err());
        assert_eq!(value, changed);
        Ok(())
    }
    #[test]
    fn managed_plugins_preserve_foreign_patches_comments_and_private_account_stores(
    ) -> Result<(), String> {
        let source = "# Keep this comment.\n- id: another-plugin\n  config:\n    enabled: true\n    note: |\n      # BEGIN VaryAI model providers\n      synthetic fixture\n";
        let mut doc = parse(source)?;
        let original = doc.clone();
        apply(
            &mut doc,
            "shared-catalog",
            "VaryAI",
            &json!({"baseUrl":"http://127.0.0.1:1/v1","protocol":"openai","apiKey":"synthetic-only","models":[{"model":"test-model","displayName":"Readable Model · VaryAI"}]}),
        )?;
        let encoded = encode(source, &doc)?;
        assert!(encoded.starts_with(source));
        assert_eq!(parse(&encoded)?, doc);
        let changed = encoded.replacen("enabled: true", "enabled: false", 1);
        let mut restored = parse(&changed)?;
        restored["providers"] = json!({});
        set_selection(&mut restored, &Value::Null)?;
        let restored = encode(&changed, &restored)?;
        assert_eq!(
            restored,
            source.replacen("enabled: true", "enabled: false", 1)
        );
        assert_eq!(original["providers"], json!({}));
        Ok(())
    }
    #[test]
    fn empty_profile_and_unknown_patch_shapes_fail_safely() -> Result<(), String> {
        let mut doc = parse("# Profile\n[]\n")?;
        apply(
            &mut doc,
            "custom",
            "Gateway",
            &json!({"baseUrl":"https://example.invalid/v1","protocol":"openai-responses","apiKey":"synthetic-only","models":[{"model":"test"}]}),
        )?;
        let text = encode("# Profile\n[]\n", &doc)?;
        assert_eq!(parse(&text)?, doc);
        assert!(text.starts_with("# Profile\n"));
        assert!(parse("providers: {}\n").is_err());
        assert!(parse(START).is_ok());
        assert!(parse(&format!("{START}- insert: []\n{END}")).is_ok());
        assert_eq!(candidates(&doc).len(), 1);
        Ok(())
    }
}
