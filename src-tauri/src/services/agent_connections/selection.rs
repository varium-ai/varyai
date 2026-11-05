//! Native model choices do not change ownership of gateway endpoints or credentials.
use super::{at, put, Receipt, FAILED};
use serde_json::Value;

fn paths(app: &str) -> &'static [&'static [&'static str]] {
    match app {
        "claude" => &[&["env", "ANTHROPIC_MODEL"]],
        "pi" => &[&["defaultProvider"], &["defaultModel"]],
        "opencode" => &[&["model"]],
        "gemini" => &[&["GEMINI_MODEL"], &["model", "name"]],
        "grokbuild" => &[&["models", "default"]],
        "openclaw" => &[&["agents", "defaults", "model", "primary"]],
        "hermes" => &[&["model", "default"]],
        "qoder" => &[&["providers", "varyai", "model"]],
        "kimi" => &[&["default_model"]],
        "qwen" => &[&["model", "name"]],
        "zcode" => &[&["config", "defaultModelSelection"]],
        "deepseek-harness" => &[&["selection"]],
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
        let live = file.format.parse(super::read(&file.path)?.as_deref())?;
        let mut baseline = file.format.parse(file.original.as_deref())?;
        let mut installed = file.installed.clone();
        let mut changed = false;
        for path in paths(&receipt.app) {
            let path = path
                .iter()
                .map(|key| (*key).into())
                .collect::<Vec<String>>();
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

pub(super) fn preserve(old: &Receipt, next: &mut Receipt) -> Result<(), String> {
    let catalogs = next
        .files
        .iter()
        .map(|file| file.installed.clone())
        .collect::<Vec<_>>();
    for file in &mut next.files {
        let prior = old
            .files
            .iter()
            .find(|prior| prior.path == file.path)
            .ok_or(FAILED)?;
        for path in paths(&old.app) {
            let path = path
                .iter()
                .map(|key| (*key).into())
                .collect::<Vec<String>>();
            let Some(choice) = at(&prior.installed, &path).filter(|choice| valid(choice)) else {
                continue;
            };
            // Retain native choices and surviving shared models. Deleted shared models
            // use the newly projected default instead of a dangling model reference.
            let model = choice
                .as_str()
                .or_else(|| choice.get("modelId").and_then(Value::as_str))
                .or_else(|| choice.pointer("/0/config/model").and_then(Value::as_str));
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
            assert_eq!(manager.disconnect(app), Err(CONFLICT.into()));
        }
        Ok(())
    }
}
