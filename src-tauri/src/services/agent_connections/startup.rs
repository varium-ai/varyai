//! Compare only the configuration a managed Agent needs to load.
use super::{at, Receipt};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Receipt times are durable. Refresh advances them only for startup-relevant changes.
pub(super) fn changed(before: &Receipt, after: &Receipt) -> bool {
    let mut paths: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for file in before.files.iter().chain(&after.files) {
        paths
            .entry(&file.path)
            .or_default()
            .extend(file.fields.iter());
    }
    for (path, fields) in paths {
        if fields.is_empty() {
            continue;
        }
        let old = before.files.iter().find(|file| file.path == *path);
        let new = after.files.iter().find(|file| file.path == *path);
        let (Some(old), Some(new)) = (old, new) else {
            return true;
        };
        let mut old = old.installed.clone();
        let mut new = new.installed.clone();
        normalize(&mut old, &[]);
        normalize(&mut new, &[]);
        if fields
            .iter()
            .any(|field| at(&old, field) != at(&new, field))
        {
            return true;
        }
    }
    false
}

pub(super) fn restoration_changed(receipt: &Receipt) -> Result<bool, String> {
    let mut restored = receipt.clone();
    for file in &mut restored.files {
        file.installed = file.format.parse(super::read(&file.path)?.as_deref())?;
    }
    Ok(changed(receipt, &restored))
}

fn strip_presentation(row: &mut Value, name_is_identity: bool) {
    if let Some(object) = row.as_object_mut() {
        for key in [
            "name",
            "display_name",
            "displayName",
            "label",
            "labelOverride",
            "description",
            "icon",
        ] {
            if key != "name" || !name_is_identity {
                object.remove(key);
            }
        }
    }
}

fn normalize(value: &mut Value, path: &[String]) {
    match value {
        Value::Object(object) => {
            let keyed_models = path == ["provider", "varyai", "models"]
                || (path == ["models"]
                    && object
                        .values()
                        .all(|row| row.get("provider").is_some() && row.get("model").is_some()));
            if keyed_models {
                for row in object.values_mut() {
                    strip_presentation(row, false);
                }
            }
            for (key, value) in object {
                let mut child = path.to_vec();
                child.push(key.clone());
                normalize(value, &child);
            }
        }
        Value::Array(rows) => {
            let inference = path.last().is_some_and(|key| key == "inferenceModels");
            let zcode = path.ends_with(&["modelConfigRules".into(), "providerModelRules".into()]);
            let catalog = inference
                || zcode
                || path.last().is_some_and(|key| {
                    matches!(
                        key.as_str(),
                        "models"
                            | "availableModels"
                            | "customModels"
                            | "personalModelIds"
                            | "modelOrder"
                    )
                })
                || path.ends_with(&["modelPicker".into(), "options".into()])
                || (path.len() == 2 && path[0] == "modelProviders");
            if catalog {
                let identity = |row: &Value| -> Option<String> {
                    if zcode {
                        return serde_json::to_string(&[
                            row.get("providerId")?.as_str()?,
                            row.get("modelId")?.as_str()?,
                        ])
                        .ok();
                    }
                    row.as_str().map(str::to_owned).or_else(|| {
                        if inference {
                            row.get("name")?.as_str().map(str::to_owned)
                        } else {
                            ["id", "slug", "model"]
                                .iter()
                                .find_map(|key| row.get(key)?.as_str().map(str::to_owned))
                        }
                    })
                };
                let keys = rows.iter().map(identity).collect::<Option<Vec<_>>>();
                // Ambiguous/unknown arrays retain their exact values and ordering.
                if let Some(keys) =
                    keys.filter(|keys| keys.iter().collect::<BTreeSet<_>>().len() == keys.len())
                {
                    for row in rows.iter_mut() {
                        strip_presentation(row, inference);
                    }
                    let mut ordered = keys
                        .into_iter()
                        .zip(std::mem::take(rows))
                        .collect::<Vec<_>>();
                    ordered.sort_by(|a, b| a.0.cmp(&b.0));
                    *rows = ordered.into_iter().map(|(_, value)| value).collect();
                }
            }
            for row in rows {
                let mut child = path.to_vec();
                child.push("[]".into());
                normalize(row, &child);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_connections::{FileEdit, Format, APPS, FAILED};
    use serde_json::{json, Value};

    fn receipt(app: &str, value: Value) -> Receipt {
        Receipt {
            app: app.into(),
            token: "synthetic-token".into(),
            model: "alpha".into(),
            model_map: Default::default(),
            visible_models: None,
            suspended: false,
            applied_at_ms: Some(1000),
            files: vec![FileEdit {
                format: Format::Json,
                path: "synthetic-config.json".into(),
                original: None,
                installed: value,
                previous: None,
                fields: vec![vec!["providers".into(), "varyai".into()]],
                desktop: None,
                desktop_carried: false,
            }],
        }
    }
    fn config() -> Value {
        json!({"theme":"light", "providers":{"varyai":{
            "baseUrl":"http://127.0.0.1:1/v1", "apiKey":"synthetic-token",
            "models":[{"id":"alpha","name":"Alpha","contextWindow":128000},
                      {"id":"beta","name":"Beta","contextWindow":64000}]}}})
    }
    #[test]
    fn unrelated_preferences_and_receipt_bookkeeping_do_not_age_any_agent() -> Result<(), String> {
        for app in APPS.iter().filter(|app| **app != "codex") {
            let before = receipt(app, config());
            let mut after = before.clone();
            after.files[0].installed["theme"] = json!("dark");
            after.files[0].previous = Some(json!({"old":"synthetic"}));
            after.files[0].original = Some("{}".into());
            after.applied_at_ms = Some(9000);
            assert!(!changed(&before, &after), "{app}");
        }
        Ok(())
    }
    #[test]
    fn catalog_presentation_and_order_do_not_require_reopening() -> Result<(), String> {
        let before = receipt("qoder", config());
        let mut after = before.clone();
        let rows = after.files[0].installed["providers"]["varyai"]["models"]
            .as_array_mut()
            .ok_or(FAILED)?;
        rows[0]["name"] = json!("Updated label");
        rows[0]["description"] = json!("Updated description");
        rows[0]["icon"] = json!("different-icon");
        rows.reverse();
        assert!(!changed(&before, &after));
        Ok(())
    }
    #[test]
    fn route_identity_catalog_membership_and_behavior_still_require_loading() -> Result<(), String>
    {
        let before = receipt("workbuddy", config());
        for (path, value) in [
            ("/providers/varyai/baseUrl", json!("http://127.0.0.1:2/v1")),
            ("/providers/varyai/apiKey", json!("synthetic-replacement")),
            ("/providers/varyai/models/0/id", json!("new-model")),
            ("/providers/varyai/models/0/contextWindow", json!(256000)),
        ] {
            let mut after = before.clone();
            *after.files[0].installed.pointer_mut(path).ok_or(FAILED)? = value;
            assert!(changed(&before, &after));
        }
        let mut after = before.clone();
        after.files[0].installed["providers"]["varyai"]["models"]
            .as_array_mut()
            .ok_or(FAILED)?
            .pop();
        assert!(changed(&before, &after));
        Ok(())
    }
    #[test]
    fn ordered_non_catalog_arrays_and_provider_names_are_preserved() -> Result<(), String> {
        let mut before = receipt("qoder", config());
        before.files[0].installed["providers"]["varyai"]["name"] = json!("provider-identity");
        before.files[0].installed["providers"]["varyai"]["fallbacks"] = json!(["alpha", "beta"]);
        let mut after = before.clone();
        after.files[0].installed["providers"]["varyai"]["name"] = json!("different-provider");
        assert!(changed(&before, &after));
        after = before.clone();
        after.files[0].installed["providers"]["varyai"]["fallbacks"] = json!(["beta", "alpha"]);
        assert!(changed(&before, &after));
        Ok(())
    }
    #[test]
    fn restoring_only_catalog_presentation_does_not_create_a_restart_requirement(
    ) -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|_| FAILED)?;
        let mut managed = receipt("workbuddy", config());
        managed.files[0].path = dir.path().join("settings.json");
        let mut restored = config();
        restored["providers"]["varyai"]["models"][0]["name"] = json!("Restored label");
        std::fs::write(&managed.files[0].path, restored.to_string()).map_err(|_| FAILED)?;
        assert!(!restoration_changed(&managed)?);
        restored["providers"]["varyai"]["apiKey"] = json!("synthetic-native-token");
        std::fs::write(&managed.files[0].path, restored.to_string()).map_err(|_| FAILED)?;
        assert!(restoration_changed(&managed)?);
        Ok(())
    }

    #[test]
    fn native_catalog_schemas_ignore_only_presentation_and_order() {
        for value in [
            json!({"inferenceModels":[{"name":"alpha","labelOverride":"Alpha","supports1m":true},{"name":"beta","labelOverride":"Beta"}]}),
            json!({"provider":{"varyai":{"models":{"alpha":{"name":"Alpha","limit":{"context":128000}}}}}}),
            json!({"models":{"alpha":{"provider":"varyai","model":"alpha","display_name":"Alpha"}}}),
            json!({"modelProviders":{"openai":[{"id":"alpha","name":"Alpha","baseUrl":"http://127.0.0.1:1"},{"id":"beta","name":"Beta"}]}}),
            json!({"customModels":[{"slug":"alpha","name":"Alpha"},{"slug":"beta","name":"Beta"}]}),
            json!({"config":{"modelConfigRules":{"providerModelRules":[{"providerId":"varyai","modelId":"alpha","config":{"enabled":true}},{"providerId":"varyai","modelId":"beta","config":{"enabled":true}}]}}}),
            json!({"config":{"personalModelIds":["alpha","beta"],"modelOrder":["alpha","beta"]}}),
        ] {
            let mut before = receipt("synthetic-schema", value);
            before.files[0].fields = vec![vec![]];
            let mut after = before.clone();
            fn relabel(value: &mut Value) {
                match value {
                    Value::Array(rows) => {
                        rows.reverse();
                        for row in rows {
                            relabel(row);
                        }
                    }
                    Value::Object(object) => {
                        for (key, value) in object {
                            if ["labelOverride", "display_name"].contains(&key.as_str())
                                || (key == "name" && value == "Alpha")
                            {
                                *value = json!("Updated label");
                            } else {
                                relabel(value);
                            }
                        }
                    }
                    _ => {}
                }
            }
            relabel(&mut after.files[0].installed);
            assert!(!changed(&before, &after));
        }
        let mut before = receipt(
            "claude-desktop",
            json!({"inferenceModels":[{"name":"alpha","labelOverride":"Alpha","supports1m":true}]}),
        );
        before.files[0].fields = vec![vec!["inferenceModels".into()]];
        for (field, value) in [
            ("name", json!("different-model")),
            ("supports1m", json!(false)),
            ("maxEffort", json!("high")),
        ] {
            let mut after = before.clone();
            after.files[0].installed["inferenceModels"][0][field] = value;
            assert!(changed(&before, &after));
        }
    }

    #[test]
    fn changed_leaf_ownership_cannot_hide_a_new_field_or_removed_model() -> Result<(), String> {
        let mut before = receipt(
            "kimi",
            json!({"models":{"alpha":{"model":"alpha","name":"Alpha"}}}),
        );
        before.files[0].fields = vec![vec!["models".into(), "alpha".into(), "model".into()]];
        let mut after = before.clone();
        after.files[0].installed = json!({"models":{"beta":{"model":"beta","name":"Beta"}}});
        after.files[0].fields = vec![vec!["models".into(), "beta".into(), "model".into()]];
        assert!(changed(&before, &after));
        Ok(())
    }
}
