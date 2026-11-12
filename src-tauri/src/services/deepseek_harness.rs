//! Desktop profile patches. Account credentials remain owned by Harness.
use serde_json::{json, Value};
use std::path::PathBuf;

const START: &str = "# BEGIN VaryAI model providers\n";
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

fn sections(text: &str) -> Result<(&str, &str, &str), String> {
    match (text.find(START), text.find(END)) {
        (None, None) => Ok((text, "", "")),
        (Some(start), Some(end))
            if start < end
                && (start == 0 || text.as_bytes()[start - 1] == b'\n')
                && text.matches(START).count() == 1
                && text.matches(END).count() == 1 =>
        {
            Ok((
                &text[..start],
                &text[start + START.len()..end],
                &text[end + END.len()..],
            ))
        }
        _ => Err(FAILED.into()),
    }
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
            value["id"] == key(id)
                || value
                    .pointer("/config/providers")
                    .and_then(Value::as_object)
                    .is_some_and(|providers| providers.contains_key(&format!("varyai-{id}")))
                || values.values().any(|value| foreign_conflict(value, id))
        }
        Value::Array(values) => values.iter().any(|value| foreign_conflict(value, id)),
        _ => false,
    }
}

fn provider_config(patches: &Value) -> Value {
    if patches["id"] == "llm-pi-ai" && patches["config"].is_object() {
        return patches["config"].clone();
    }
    match patches {
        Value::Array(items) => items
            .iter()
            .rev()
            .map(provider_config)
            .find(|v| !v.is_null()),
        Value::Object(items) => items
            .values()
            .rev()
            .map(provider_config)
            .find(|v| !v.is_null()),
        _ => None,
    }
    .unwrap_or(Value::Null)
}

pub(crate) fn parse(text: &str) -> Result<Value, String> {
    let (before, managed, after) = sections(text)?;
    let patches = array(&format!("{before}{after}"))?;
    let mut providers = serde_json::Map::new();
    let mut selection = Vec::new();
    let mut legacy = false;
    let mut mounted = false;
    let base_config = provider_config(&json!(patches));
    for entry in array(managed)? {
        if entry["id"] == "agent-default-model" {
            if !selection.is_empty() || !valid_selection(&entry["config"]) {
                return Err(FAILED.into());
            }
            selection.push(entry);
            continue;
        }
        if entry["id"] == "llm-pi-ai" {
            if !providers.is_empty() || entry["name"] != PLUGIN {
                return Err(FAILED.into());
            }
            mounted = true;
            let mut config = entry["config"].clone();
            let routes = config["providers"].as_object_mut().ok_or(FAILED)?;
            let ids = routes
                .keys()
                .filter(|id| id.starts_with("varyai-"))
                .cloned()
                .collect::<Vec<_>>();
            if ids.is_empty() {
                return Err(FAILED.into());
            }
            for route in ids {
                let profile = routes.remove(&route).ok_or(FAILED)?;
                let id = key(route.trim_start_matches("varyai-"));
                providers.insert(
                    id.clone(),
                    json!({"id":id,"name":PLUGIN,"config":{"providers":{route:profile}}}),
                );
            }
            let mut base = if base_config.is_null() {
                json!({})
            } else {
                base_config.clone()
            };
            if base.get("providers").is_none() {
                base["providers"] = json!({});
            }
            if config != base {
                return Err(FAILED.into());
            }
            continue;
        }
        if mounted {
            return Err(FAILED.into());
        }
        legacy = true;
        let children = entry["insert"].as_array().ok_or(FAILED)?;
        if children.len() != 1 || entry.as_object().is_none_or(|entry| entry.len() != 1) {
            return Err(FAILED.into());
        }
        let child = &children[0];
        let id = child["id"]
            .as_str()
            .filter(|id| id.starts_with("varyai-provider-"))
            .ok_or(FAILED)?;
        if child["name"] != PLUGIN || providers.insert(id.into(), child.clone()).is_some() {
            return Err(FAILED.into());
        }
    }
    let patches = json!(patches);
    if providers
        .keys()
        .any(|id| foreign_conflict(&patches, id.trim_start_matches("varyai-provider-")))
    {
        return Err(FAILED.into());
    }
    if !selection.is_empty() && !foreign_selection(&json!(array(after)?)).is_null() {
        return Err(FAILED.into());
    }
    Ok(
        json!({"patches":patches,"providers":providers,"selection":selection,"legacyProviders":legacy}),
    )
}

pub(crate) fn encode(source: &str, next: &Value) -> Result<String, String> {
    let original = parse(source)?;
    if original == *next {
        return Ok(source.into());
    }
    let (before, _, after) = sections(source)?;
    let foreign = if original["patches"] == next["patches"] {
        format!("{before}{after}")
    } else {
        serde_yaml::to_string(&next["patches"]).map_err(|_| FAILED)?
    };
    let empty = serde_json::Map::new();
    let providers = match next.get("providers") {
        Some(value) => value.as_object().ok_or(FAILED)?,
        None => &empty,
    };
    let selection = next
        .get("selection")
        .map(|value| value.as_array().ok_or(FAILED))
        .transpose()?;
    if providers.is_empty() && selection.is_none_or(Vec::is_empty) {
        return if array(&foreign)?.is_empty() && !foreign.lines().any(|line| line.trim() == "[]") {
            Ok(format!(
                "{foreign}{}[]\n",
                if foreign.is_empty() || foreign.ends_with('\n') {
                    ""
                } else {
                    "\n"
                }
            ))
        } else {
            Ok(foreign)
        };
    }
    // An explicit empty YAML list cannot be followed by block sequence entries.
    let foreign = if array(&foreign)?.is_empty() {
        foreign
            .lines()
            .filter(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
            .map(|line| format!("{line}\n"))
            .collect()
    } else {
        foreign
    };
    let mut entries = Vec::new();
    if next["legacyProviders"] == true {
        entries.extend(
            providers
                .values()
                .map(|provider| json!({"insert":[provider]})),
        );
    } else if !providers.is_empty() {
        let mut config = provider_config(&next["patches"]);
        if config.is_null() {
            config = json!({});
        }
        if config.get("providers").is_none() {
            config["providers"] = json!({});
        }
        let routes = config["providers"].as_object_mut().ok_or(FAILED)?;
        for provider in providers.values() {
            for (id, profile) in provider["config"]["providers"].as_object().ok_or(FAILED)? {
                if routes.insert(id.clone(), profile.clone()).is_some() {
                    return Err(FAILED.into());
                }
            }
        }
        entries.push(json!({"id":"llm-pi-ai","name":PLUGIN,"config":config}));
    }
    entries.extend(selection.into_iter().flatten().cloned());
    let text = format!(
        "{}{}{}{}{}",
        foreign,
        if foreign.is_empty() || foreign.ends_with('\n') {
            ""
        } else {
            "\n"
        },
        START,
        serde_yaml::to_string(&entries).map_err(|_| FAILED)?,
        END
    );
    if parse(&text)? != *next {
        return Err(FAILED.into());
    }
    Ok(text)
}

/// Explicit recovery only: a foreign writer can discard our opening comment.
pub(crate) fn recover_for_reapply(source: &str, installed: &Value) -> Result<String, String> {
    let conflict = super::agent_connections::CONFLICT;
    if source.matches(END).count() != 1 {
        return Err(conflict.into());
    }
    let end = source.find(END).ok_or(conflict)?;
    if end > 0 && source.as_bytes()[end - 1] != b'\n' {
        return Err(conflict.into());
    }
    let expected = installed["providers"]
        .as_object()
        .filter(|providers| !providers.is_empty())
        .ok_or(conflict)?;
    let opening = source.find(START);
    let candidates = if let Some(start) = opening {
        sections(source).map_err(|_| conflict)?;
        vec![start + START.len()]
    } else {
        source[..end]
            .rmatch_indices("\n- ")
            .map(|(index, _)| index + 1)
            .chain(std::iter::once(0))
            .collect()
    };
    for start in candidates {
        let Ok(entries) = array(&source[start..end]) else {
            continue;
        };
        if entries.len() != 2 || entries[0]["id"] != "llm-pi-ai" {
            continue;
        }
        let mut native = entries[0].clone();
        if native["name"] != PLUGIN
            || native.as_object().is_none_or(|entry| entry.len() != 3)
            || entries[1]["id"] != "agent-default-model"
            || !valid_selection(&entries[1]["config"])
        {
            return Err(conflict.into());
        }
        let mut owned_routes = Vec::new();
        let routes = native["config"]["providers"]
            .as_object_mut()
            .ok_or(conflict)?;
        for provider in expected.values() {
            if provider["name"] != PLUGIN {
                return Err(conflict.into());
            }
            for id in provider["config"]["providers"]
                .as_object()
                .filter(|routes| !routes.is_empty())
                .ok_or(conflict)?
                .keys()
            {
                if !id.starts_with("varyai-") || routes.remove(id).is_none() {
                    return Err(conflict.into());
                }
                owned_routes.push(id.as_str());
            }
        }
        if routes.keys().any(|id| id.starts_with("varyai-")) {
            return Err(conflict.into());
        }
        // Keep the effective native config, including another manager's disconnect.
        // Dropping this entire row could expose its stale snapshot earlier in the file.
        let mut retained = vec![native];
        if !owned_routes.contains(&entries[1]["config"]["provider"].as_str().ok_or(conflict)?) {
            retained.push(entries[1].clone());
        }
        let clean = format!(
            "{}{}{}",
            &source[..opening.unwrap_or(start)],
            serde_yaml::to_string(&retained).map_err(|_| conflict)?,
            &source[end + END.len()..],
        );
        let parsed = parse(&clean).map_err(|_| conflict)?;
        if expected.keys().any(|id| {
            foreign_conflict(
                &parsed["patches"],
                id.trim_start_matches("varyai-provider-"),
            )
        }) {
            return Err(conflict.into());
        }
        return Ok(clean);
    }
    Err(conflict.into())
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let source = "# Keep this comment.\n- id: another-plugin\n  config:\n    enabled: true\n";
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
        assert!(parse(START).is_err());
        assert!(parse(&format!("{START}- insert: []\n{END}")).is_err());
        assert_eq!(candidates(&doc).len(), 1);
        Ok(())
    }
}
