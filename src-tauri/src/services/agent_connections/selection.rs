//! Native model choices do not change ownership of gateway endpoints or credentials.
use super::{at, put, Receipt, FAILED};
use serde_json::Value;

fn paths(app: &str) -> &'static [&'static [&'static str]] {
    match app {
        "claude" => &[&["env", "ANTHROPIC_MODEL"], &["model"]],
        "pi" => &[&["defaultProvider"], &["defaultModel"]],
        "opencode" => &[&["model"]],
        "gemini" => &[&["GEMINI_MODEL"], &["model", "name"]],
        "grokbuild" => &[&["models", "default"]],
        "openclaw" => &[
            &["agents", "defaults", "model"],
            &["agents", "defaults", "model", "primary"],
        ],
        "hermes" => &[&["model", "provider"], &["model", "default"]],
        "qoder" => &[&["providers", "varyai", "model"]],
        "kimi" => &[&["default_model"]],
        "qwen" => &[&["model", "name"]],
        "zcode" => &[&["config", "defaultModelSelection"]],
        "deepseek-harness" => &[&["selection"]],
        "t3code" => &[
            &["defaultModelSelection", "instanceId"],
            &["defaultModelSelection", "model"],
        ],
        _ => &[],
    }
}

fn valid(value: &Value) -> bool {
    match value {
        Value::String(value) => {
            !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
        }
        Value::Object(value) => !value.is_empty() && value.values().all(valid),
        Value::Array(value) => value.len() == 1 && value.iter().all(valid),
        _ => false,
    }
}

fn managed(value: &Value, receipt: &Receipt) -> bool {
    match value {
        Value::String(value) => {
            receipt.model_map.contains_key(value)
                || value == "varyai"
                || [
                    "varyai-",
                    "varyai/",
                    "vary-provider/",
                    "vary-auto/",
                    "vary-",
                ]
                .iter()
                .any(|prefix| value.starts_with(prefix))
        }
        Value::Object(value) => value.values().any(|value| managed(value, receipt)),
        Value::Array(value) => value.iter().any(|value| managed(value, receipt)),
        _ => false,
    }
}

pub(super) fn adopt(receipt: &mut Receipt) -> Result<(), String> {
    if receipt.suspended || paths(&receipt.app).is_empty() {
        return Ok(());
    }
    for index in 0..receipt.files.len() {
        let file = &receipt.files[index];
        let live = file
            .format
            .parse(file.format.read(&file.path)?.as_deref())?;
        let mut baseline = file.format.parse(file.original.as_deref())?;
        let mut installed = file.installed.clone();
        let mut changed = false;
        for path in paths(&receipt.app) {
            let path = path
                .iter()
                .map(|key| (*key).into())
                .collect::<Vec<String>>();
            if receipt.app == "openclaw"
                && path.len() == 3
                && at(&installed, &path).is_some_and(Value::is_object)
            {
                continue;
            }
            if at(&file.installed, &path).is_none() {
                continue;
            }
            let Some(choice) = at(&live, &path).filter(|choice| valid(choice)) else {
                continue;
            };
            if at(&installed, &path) == Some(choice) {
                continue;
            }
            put(&mut installed, &path, Some(choice.clone()))?;
            if !managed(choice, receipt) {
                put(&mut baseline, &path, Some(choice.clone()))?;
                changed = true;
            }
        }
        let original = if changed {
            Some(file.format.encode(file.original.as_deref(), &baseline)?)
        } else {
            file.original.clone()
        };
        receipt.files[index].installed = installed;
        receipt.files[index].original = original;
    }
    Ok(())
}

fn contains(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(value) => value == needle,
        Value::Object(value) => {
            value.contains_key(needle) || value.values().any(|v| contains(v, needle))
        }
        Value::Array(value) => value.iter().any(|v| contains(v, needle)),
        _ => false,
    }
}

fn renamed_model(
    old: &Receipt,
    names: &std::collections::BTreeMap<String, String>,
    file: &super::FileEdit,
    wire: &str,
) -> Option<String> {
    let base = if old.app == "claude" {
        crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(wire)
    } else {
        wire
    };
    let alias = old
        .model_map
        .get(base)
        .map(String::as_str)
        .or_else(|| {
            (old.app == "grokbuild")
                .then(|| {
                    file.installed
                        .get("model")?
                        .get(wire)?
                        .get("model")?
                        .as_str()
                })
                .flatten()
        })
        .unwrap_or(base);
    names
        .iter()
        .find(|(_, alias_id)| alias_id.as_str() == alias)
        .map(|(name, _)| format!("{name}{}", &wire[base.len()..]))
}

pub(super) fn preserve(old: &Receipt, next: &mut Receipt) -> Result<(), String> {
    if paths(&old.app).is_empty() {
        return Ok(());
    }
    let catalogs = next
        .files
        .iter()
        .map(|file| file.installed.clone())
        .collect::<Vec<_>>();
    let names = &next.model_map;
    for file in &mut next.files {
        let prior = old
            .files
            .iter()
            .find(|prior| prior.path == file.path)
            .ok_or(FAILED)?;
        if old.app == "pi" {
            if let Some(models) = file
                .installed
                .get_mut("enabledModels")
                .and_then(Value::as_array_mut)
            {
                for model in models {
                    if let Some(name) = model
                        .as_str()
                        .and_then(|wire| wire.strip_prefix("varyai/"))
                        .and_then(|wire| renamed_model(old, names, prior, wire))
                    {
                        *model = Value::String(format!("varyai/{name}"));
                    }
                }
            }
        }
        for path in paths(&old.app) {
            let path = path
                .iter()
                .map(|key| (*key).into())
                .collect::<Vec<String>>();
            let Some(choice) = at(&prior.installed, &path).filter(|choice| valid(choice)) else {
                continue;
            };
            if old.app == "openclaw" && path.len() == 3 && choice.is_object() {
                continue;
            }
            if old.app == "claude"
                && path.first().is_some_and(|key| key == "env")
                && at(&file.installed, &path).is_none()
            {
                continue;
            }
            let renamed = ["claude", "pi", "hermes", "grokbuild"]
                .contains(&old.app.as_str())
                .then(|| {
                    choice
                        .as_str()
                        .and_then(|wire| renamed_model(old, names, prior, wire))
                })
                .flatten()
                .map(Value::String);
            let choice = renamed.as_ref().unwrap_or(choice);
            if old.app == "hermes" && path == ["model", "provider"] && managed(choice, old) {
                continue;
            }
            if old.app == "deepseek-harness"
                && choice
                    .pointer("/0/config/provider")
                    .and_then(Value::as_str)
                    .is_some_and(|provider| {
                        matches!(
                            provider,
                            "varyai-shared-catalog"
                                | "varyai-shared-catalog-responses"
                                | "varyai-shared-catalog-anthropic"
                        )
                    })
            {
                if let Some(model) = choice.pointer("/0/config/model").and_then(Value::as_str) {
                    let route = file.installed["providers"]
                        .as_object()
                        .into_iter()
                        .flat_map(|entries| entries.values())
                        .flat_map(|entry| {
                            entry["config"]["providers"]
                                .as_object()
                                .into_iter()
                                .flat_map(|routes| routes.iter())
                        })
                        .find(|(_, route)| {
                            route["models"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .any(|row| row["id"] == model)
                        })
                        .map(|(id, _)| id.clone());
                    if let Some(route) = route {
                        let mut routed = choice.clone();
                        routed[0]["config"]["provider"] = Value::String(route);
                        put(&mut file.installed, &path, Some(routed))?;
                        continue;
                    }
                }
            }
            // Retain native choices and surviving shared models. Deleted shared models
            // use the newly projected default instead of a dangling model reference.
            let model = choice
                .as_str()
                .or_else(|| choice.get("modelId").and_then(Value::as_str))
                .or_else(|| choice.pointer("/0/config/model").and_then(Value::as_str))
                .map(|model| {
                    if old.app == "claude" {
                        crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(model)
                    } else {
                        model
                    }
                });
            if !managed(choice, old)
                || model.is_some_and(|model| {
                    catalogs.iter().any(|catalog| {
                        contains(catalog, model)
                            || contains(catalog, model.strip_prefix("varyai/").unwrap_or(model))
                    })
                })
            {
                put(&mut file.installed, &path, Some(choice.clone()))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_connections::{FileEdit, Manager, CONFLICT};
    use serde_json::json;
    use std::{collections::BTreeMap, fs};

    #[test]
    fn claude_readable_upgrade_preserves_selected_model_and_context_suffix() -> Result<(), String> {
        let alias = "vary-provider/model-b";
        let name = "Model B · Supplier · VaryAI";
        for (wire, expected) in [
            (alias.to_owned(), name.to_owned()),
            (format!("{alias}[1m]"), format!("{name}[1m]")),
            ("native-choice".into(), "native-choice".into()),
        ] {
            let dir = tempfile::tempdir().map_err(|_| FAILED)?;
            let file = FileEdit::prepare(dir.path().join("settings.json"), |value| {
                value["model"] = json!(wire);
                Ok(())
            })?;
            let old = Receipt {
                app: "claude".into(),
                token: "synthetic-token".into(),
                model: "vary-provider/model-a".into(),
                model_map: BTreeMap::new(),
                visible_models: None,
                files: vec![file],
                suspended: false,
                applied_at_ms: None,
            };
            let mut next = old.clone();
            next.model_map.insert(name.into(), alias.into());
            next.files[0].installed =
                json!({"model":"Model A", "modelPicker":{"options":[{"model":name}]}});
            preserve(&old, &mut next)?;
            assert_eq!(next.files[0].installed["model"], expected);
        }
        Ok(())
    }

    #[test]
    fn readable_catalog_upgrade_preserves_native_selection_and_pi_scope() -> Result<(), String> {
        let alias = "vary-provider/model-b";
        let name = "Model B · Supplier · VaryAI";
        for app in ["pi", "hermes", "grokbuild"] {
            let dir = tempfile::tempdir().map_err(|_| FAILED)?;
            let path = dir.path().join("config");
            let choice_path = paths(app)
                .last()
                .ok_or(FAILED)?
                .iter()
                .map(|key| (*key).into())
                .collect::<Vec<String>>();
            let wire = if app == "grokbuild" {
                "vary-old-hash"
            } else {
                alias
            };
            let file = FileEdit::prepare(path, |value| {
                put(value, &choice_path, Some(json!(wire)))?;
                value["model"][wire] = json!({"model":alias});
                value["enabledModels"] = json!(["native/model", format!("varyai/{alias}")]);
                Ok(())
            })?;
            let old = Receipt {
                app: app.into(),
                token: "synthetic-token".into(),
                model: "vary-provider/model-a".into(),
                model_map: BTreeMap::new(),
                visible_models: None,
                files: vec![file],
                suspended: false,
                applied_at_ms: None,
            };
            let mut next = old.clone();
            next.model_map.insert(name.into(), alias.into());
            next.files[0].installed["catalog"] = json!([name]);
            put(
                &mut next.files[0].installed,
                &choice_path,
                Some(json!("Model A")),
            )?;
            preserve(&old, &mut next)?;
            assert_eq!(
                at(&next.files[0].installed, &choice_path),
                Some(&json!(name)),
                "{app}"
            );
            if app == "pi" {
                assert_eq!(
                    next.files[0].installed["enabledModels"],
                    json!(["native/model", format!("varyai/{name}")])
                );
            }
        }
        Ok(())
    }

    #[test]
    fn native_model_switches_keep_connections_active_without_reapply() -> Result<(), String> {
        for app in [
            "claude",
            "pi",
            "opencode",
            "gemini",
            "grokbuild",
            "openclaw",
            "hermes",
            "qoder",
            "kimi",
            "qwen",
            "zcode",
            "deepseek-harness",
        ] {
            let dir = tempfile::tempdir().map_err(|_| FAILED)?;
            let manager = Manager {
                root: dir.path().join("receipts"),
            };
            let path = dir.path().join("client.json");
            let choice_path = paths(app)
                .last()
                .ok_or(FAILED)?
                .iter()
                .map(|p| (*p).into())
                .collect::<Vec<String>>();
            let choice = if app == "zcode" {
                json!({"providerId":"varyai-shared-catalog","modelId":"Model B"})
            } else if app == "deepseek-harness" {
                json!([{"id":"agent-default-model","config":{"provider":"varyai-shared-catalog","model":"Model B"}}])
            } else {
                json!("Model B")
            };
            if ["kimi", "qwen"].contains(&app) {
                let mut baseline = json!({});
                put(&mut baseline, &choice_path, Some(json!("Model A")))?;
                fs::write(&path, baseline.to_string()).map_err(|_| FAILED)?;
            }
            let file = FileEdit::prepare(path.clone(), |v| {
                v["endpoint"] = json!("http://127.0.0.1:1/v1");
                v["token"] = json!("synthetic-only");
                v["catalog"] = json!(["Model A", "Model B"]);
                put(v, &choice_path, Some(json!("Model A")))
            })?;
            manager.install(Receipt {
                app: app.into(),
                token: "synthetic-only".into(),
                model: "Model A".into(),
                model_map: BTreeMap::from([
                    ("Model A".into(), "vary-provider/a".into()),
                    ("Model B".into(), "vary-provider/b".into()),
                ]),
                visible_models: None,
                files: vec![file],
                suspended: false,
                applied_at_ms: None,
            })?;
            let mut live: Value =
                serde_json::from_str(&fs::read_to_string(&path).map_err(|_| FAILED)?)
                    .map_err(|_| FAILED)?;
            put(&mut live, &choice_path, Some(choice.clone()))?;
            fs::write(&path, live.to_string()).map_err(|_| FAILED)?;
            assert!(
                manager.status(app)?.configured,
                "{app} model selection is not an ownership conflict"
            );
            let old = manager.load(app)?.ok_or(FAILED)?;
            assert_eq!(at(&old.files[0].installed, &choice_path), Some(&choice));
            let mut next = old.clone();
            put(
                &mut next.files[0].installed,
                &choice_path,
                Some(json!("Model A")),
            )?;
            preserve(&old, &mut next)?;
            assert_eq!(at(&next.files[0].installed, &choice_path), Some(&choice));
            live["token"] = json!("synthetic-external-change");
            fs::write(&path, live.to_string()).map_err(|_| FAILED)?;
            assert!(manager.status(app)?.recovery_pending);
            assert!(
                !manager.status(app)?.enabled,
                "external ownership must turn the switch off"
            );
            assert_eq!(
                fs::read_to_string(&path).map_err(|_| FAILED)?,
                live.to_string()
            );
            assert_eq!(manager.disconnect(app), Err(CONFLICT.into()));
        }
        Ok(())
    }
}
