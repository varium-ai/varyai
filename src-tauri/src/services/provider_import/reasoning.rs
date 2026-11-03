//! Translate source model metadata without inventing supported effort levels.
use super::*;
use crate::services::intent_routing::{effort::Capability, model_preferences};

fn levels(value: &Value) -> Option<Vec<String>> {
    value.as_array()?;
    Some(Capability::parse(&json!({"reasoningLevels":value})).levels)
}

pub(super) fn normalize(row: &mut Value) {
    let explicit = [
        "reasoningLevels",
        "reasoning_levels",
        "supported_reasoning_levels",
        "reasoning_options",
    ]
    .iter()
    .any(|field| row.get(*field).is_some_and(Value::is_array));
    if !explicit {
        let native = [
            "efforts",
            "reasoning_efforts",
            "support_efforts",
            "reasoningLevels",
        ]
        .iter()
        .find_map(|field| row.get(*field).and_then(levels))
        .or_else(|| row.pointer("/reasoning/supportedEfforts").and_then(levels))
        .or_else(|| {
            row.pointer("/capabilities/thinking/supportedEffortLevels")
                .and_then(levels)
        });
        let variants = row
            .get("variants")
            .and_then(Value::as_object)
            .map(|variants| {
                variants
                    .values()
                    .filter(|variant| variant["disabled"] != true)
                    .filter_map(|variant| variant["reasoningEffort"].as_str())
                    .collect::<Vec<_>>()
            });
        if let Some(native) = native {
            row["reasoningLevels"] = json!(native);
        } else if let Some(variants) = variants.filter(|levels| !levels.is_empty()) {
            row["reasoningLevels"] =
                json!(Capability::parse(&json!({"reasoningLevels":variants})).levels);
        } else if let Some(map) = row.get("thinkingLevelMap").and_then(Value::as_object) {
            let listed = map.values().filter_map(Value::as_str).collect::<Vec<_>>();
            if !listed.is_empty() {
                row["reasoningLevels"] =
                    json!(Capability::parse(&json!({"reasoningLevels":listed})).levels);
            }
        }
    }
    let mut vendor = row.clone();
    if let Some(vendor) = vendor.as_object_mut() {
        vendor.remove("modelPreferences");
    }
    let capability = Capability::parse(&vendor);
    if let Some(declaration) = capability.declaration().as_object() {
        for (key, value) in declaration {
            row[key] = value.clone();
        }
    }
    let default = text(
        row,
        &[
            "defaultReasoningLevel",
            "default_reasoning_level",
            "default_effort",
        ],
    );
    let default = if default.is_empty() {
        row.pointer("/options/reasoningEffort")
            .and_then(Value::as_str)
            .or_else(|| {
                row.pointer("/reasoning/defaultEffort")
                    .and_then(Value::as_str)
            })
            .unwrap_or("")
            .to_owned()
    } else {
        default
    };
    if crate::services::intent_routing::effort::valid_member_level(&default) {
        row["defaultReasoningLevel"] = json!(default);
    }
}

pub(super) fn selected(row: &mut Value, effort: Option<&str>) {
    if let Some(effort) =
        effort.filter(|effort| crate::services::intent_routing::effort::valid_member_level(effort))
    {
        row["defaultReasoningLevel"] = json!(effort);
    }
}

pub(super) fn claude(rows: &mut [Value], settings: &Value) {
    let declarations = settings
        .pointer("/env/CLAUDE_CODE_MODEL_CAPABILITIES")
        .and_then(Value::as_str)
        .unwrap_or("");
    let default = settings
        .get("effortLevel")
        .and_then(Value::as_str)
        .or_else(|| {
            settings
                .pointer("/env/CLAUDE_CODE_EFFORT_LEVEL")
                .and_then(Value::as_str)
        });
    let selected_id = settings
        .pointer("/env/ANTHROPIC_MODEL")
        .and_then(Value::as_str)
        .unwrap_or("");
    for row in rows {
        let id = row["model"].as_str().unwrap_or("").to_owned();
        if id == selected_id {
            selected(row, default);
        }
        if Capability::has_reasoning_declaration(row) {
            continue;
        }
        let lower = id.to_ascii_lowercase();
        let caps = declarations
            .split(';')
            .filter_map(|segment| segment.split_once('='))
            .find(|(pattern, _)| {
                lower == *pattern
                    || pattern
                        .strip_suffix('*')
                        .is_some_and(|prefix| lower.starts_with(prefix))
            })
            .map(|(_, caps)| caps.split(',').collect::<Vec<_>>());
        if let Some(caps) = caps.filter(|caps| caps.contains(&"effort")) {
            let mut levels = vec!["low", "medium", "high"];
            if caps.contains(&"xhigh_effort") {
                levels.push("xhigh");
            }
            if caps.contains(&"max_effort") {
                levels.push("max");
            }
            row["reasoningLevels"] = json!(levels);
            row["reasoning"] = json!(true);
        }
    }
}

pub(super) fn preferences(
    provider: &str,
    model: &str,
    embedded: &Value,
    settings: &Value,
) -> Result<Value, String> {
    let reference = format!("{provider}/{model}");
    let mut prefs = serde_json::Map::new();
    for (source, target) in [
        ("modelNames", "name"),
        ("modelSameAs", "sameAs"),
        ("modelImages", "images"),
    ] {
        if let Some(value) = settings.get(source).and_then(|map| map.get(&reference)) {
            prefs.insert(target.into(), value.clone());
        }
    }
    let saved = settings
        .get("modelEfforts")
        .and_then(|map| map.get(&reference));
    for value in [saved, embedded.get("efforts")].into_iter().flatten() {
        let Some(parsed) = levels(value) else {
            return Err(INVALID.into());
        };
        if !value.as_array().is_some_and(|values| {
            values.iter().all(|value| {
                value.as_str().is_some_and(|level| {
                    crate::services::intent_routing::effort::valid_member_level(
                        &level.trim().to_ascii_lowercase(),
                    )
                })
            })
        }) {
            return Err(INVALID.into());
        }
        // The reference format uses an empty effort list to restore vendor defaults.
        if value.as_array().is_some_and(Vec::is_empty) {
            prefs.remove("reasoningLevels");
        } else if parsed.is_empty() {
            return Err(INVALID.into());
        } else {
            prefs.insert("reasoningLevels".into(), json!(parsed));
        }
    }
    for (source, target) in [
        ("name", "name"),
        ("same", "sameAs"),
        ("sameAs", "sameAs"),
        ("images", "images"),
        ("reasoningLevels", "reasoningLevels"),
    ] {
        if let Some(value) = embedded.get(source) {
            if matches!(target, "name" | "sameAs") && value == "" {
                prefs.remove(target);
            } else {
                prefs.insert(target.into(), value.clone());
            }
        }
    }
    if embedded["ownImages"] == true {
        prefs.remove("images");
    }
    let reasonless = embedded
        .get("reasonless")
        .and_then(Value::as_bool)
        .or_else(|| {
            settings
                .get("modelReasonless")
                .and_then(|map| map.get(&reference))
                .and_then(Value::as_bool)
        });
    if embedded
        .get("reasonless")
        .is_some_and(|value| !value.is_boolean())
        || settings
            .get("modelReasonless")
            .and_then(|map| map.get(&reference))
            .is_some_and(|value| !value.is_boolean())
    {
        return Err(INVALID.into());
    }
    if reasonless == Some(true) {
        prefs.insert("reasoningLevels".into(), json!([]));
    }
    let entry = json!({"modelPreferences":prefs});
    if !model_preferences::valid(&entry) {
        return Err(INVALID.into());
    }
    Ok(entry["modelPreferences"].clone())
}
