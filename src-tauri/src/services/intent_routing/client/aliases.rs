//! Readable Codex wire names with retained mappings for existing conversations.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn wire_name(label: &str) -> String {
    // Codex also serializes the model ID in x-codex-routing-hint. Unicode
    // labels belong in display_name, never in that header's model ID.
    let ascii = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    let name = ascii
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if name.is_empty() {
        "model".into()
    } else {
        name
    }
}

pub(crate) fn project(
    catalog: &mut Value,
    retained: &mut BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    let models = catalog["models"]
        .as_array_mut()
        .ok_or("intentRouting.clientFailed")?;
    let mut entries = models
        .iter()
        .enumerate()
        .map(|(index, model)| {
            let id = model["slug"].as_str().ok_or("intentRouting.clientFailed")?;
            let label = model["display_name"]
                .as_str()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(id);
            Ok((id.to_owned(), label.to_owned(), index))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let reserved = entries
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let labels = entries
        .iter()
        .map(|(_, label, _)| wire_name(label))
        .collect::<BTreeSet<_>>();
    // Allocate duplicates by stable identity, independent of picker ordering.
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut legacy = Vec::new();
    for (id, label, index) in entries {
        let base = wire_name(&label);
        let mut name = base.clone();
        let mut number = 2;
        while (reserved.contains(&name) && name != id)
            || retained.get(&name).is_some_and(|owner| owner != &id)
            || (name != base && labels.contains(&name))
        {
            name = format!("{base}-{number}");
            number += 1;
        }
        if !retained.contains_key(&name) && retained.len() >= 16384 {
            return Err("intentRouting.limit".into());
        }
        retained.insert(name.clone(), id.clone());
        let model = &mut models[index];
        model["slug"] = Value::String(name.clone());
        if number > 2 {
            model["display_name"] = Value::String(format!("{label} ({})", number - 1));
        }
        // Existing sessions keep the same capability metadata without adding
        // duplicate picker rows. Retired names stay reserved in the receipt.
        let mut previous = retained
            .iter()
            .filter(|(_, owner)| **owner == id)
            .map(|(wire, _)| wire.clone())
            .collect::<BTreeSet<_>>();
        previous.insert(id);
        previous.remove(&name);
        for wire in previous {
            let mut hidden = model.clone();
            hidden["slug"] = Value::String(wire);
            hidden["visibility"] = Value::String("hide".into());
            legacy.push(hidden);
        }
    }
    let visible = models
        .iter()
        .filter_map(|model| model["slug"].as_str().map(str::to_owned))
        .collect();
    models.extend(legacy);
    Ok(visible)
}

pub(crate) fn selected<'a>(
    visible: &'a [String],
    retained: &BTreeMap<String, String>,
    model: &str,
) -> Option<&'a str> {
    let canonical = retained.get(model).map(String::as_str).unwrap_or(model);
    visible
        .iter()
        .find(|wire| retained.get(*wire).is_some_and(|id| id == canonical))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog(entries: &[(&str, &str)]) -> Value {
        json!({"models":entries.iter().map(|(id, name)| json!({
            "slug":id,"display_name":name,"visibility":"list",
            "context_window":123456,"supported_reasoning_levels":[{"effort":"high"}],
            "service_tiers":[{"id":"priority"}]
        })).collect::<Vec<_>>()})
    }

    #[test]
    fn readable_wire_names_keep_legacy_metadata_hidden_in_the_picker() {
        let mut catalog = catalog(&[("vary-provider/model-synthetic", "Grok · Synthetic")]);
        let mut retained = BTreeMap::new();
        let visible = project(&mut catalog, &mut retained).unwrap();
        assert_eq!(visible, ["Grok-Synthetic"]);
        assert_eq!(retained["Grok-Synthetic"], "vary-provider/model-synthetic");
        assert_eq!(catalog["models"][0]["slug"], "Grok-Synthetic");
        assert_eq!(
            catalog["models"][1]["slug"],
            "vary-provider/model-synthetic"
        );
        assert_eq!(catalog["models"][1]["visibility"], "hide");
        for key in [
            "context_window",
            "supported_reasoning_levels",
            "service_tiers",
        ] {
            assert_eq!(catalog["models"][0][key], catalog["models"][1][key]);
        }
    }

    #[test]
    fn unicode_labels_never_enter_codex_routing_headers() {
        let mut catalog = catalog(&[("vary-provider/synthetic", "grok-4.5 · 我的Grok")]);
        let mut retained = BTreeMap::from([(
            "grok-4.5 · 我的Grok".into(),
            "vary-provider/synthetic".into(),
        )]);
        let names = project(&mut catalog, &mut retained).unwrap();
        assert!(names
            .iter()
            .all(|name| name.is_ascii() && !name.contains(char::is_whitespace)));
        assert_eq!(names, ["grok-4.5-Grok"]);
        assert_eq!(catalog["models"][0]["display_name"], "grok-4.5 · 我的Grok");
        assert_eq!(retained["grok-4.5 · 我的Grok"], "vary-provider/synthetic");
    }

    #[test]
    fn duplicate_names_are_stable_across_catalog_order_and_restart() {
        let entries = [("vary-provider/a", "Shared"), ("vary-provider/b", "Shared")];
        let mut retained = BTreeMap::new();
        assert_eq!(
            project(&mut catalog(&entries), &mut retained).unwrap(),
            ["Shared", "Shared-2"]
        );
        let mut restored =
            serde_json::from_str(&serde_json::to_string(&retained).unwrap()).unwrap();
        assert_eq!(
            project(&mut catalog(&[entries[1], entries[0]]), &mut restored).unwrap(),
            ["Shared-2", "Shared"]
        );
        assert_eq!(restored, retained);
    }

    #[test]
    fn renamed_and_removed_names_never_move_to_a_different_destination() {
        let mut retained = BTreeMap::new();
        project(
            &mut catalog(&[("vary-provider/a", "Original")]),
            &mut retained,
        )
        .unwrap();
        let mut renamed = catalog(&[("vary-provider/a", "Renamed")]);
        assert_eq!(project(&mut renamed, &mut retained).unwrap(), ["Renamed"]);
        assert_eq!(retained["Original"], "vary-provider/a");
        assert!(renamed["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["slug"] == "Original" && m["visibility"] == "hide"));
        let mut replacement = catalog(&[("vary-provider/b", "Original")]);
        assert_eq!(
            project(&mut replacement, &mut retained).unwrap(),
            ["Original-2"]
        );
        assert_eq!(retained["Original"], "vary-provider/a");
        assert!(!replacement["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["slug"] == "Original"));
    }

    #[test]
    fn readable_names_cannot_shadow_legacy_ids_or_other_requested_names() {
        let mut retained = BTreeMap::new();
        let mut catalog = catalog(&[
            ("vary-provider/a", "Shared"),
            ("vary-provider/b", "Shared"),
            ("vary-provider/c", "Shared (2)"),
            ("vary-provider/d", "vary-provider/a"),
        ]);
        let names = project(&mut catalog, &mut retained).unwrap();
        assert_eq!(
            names,
            ["Shared", "Shared-3", "Shared-2", "vary-provider/a-2"]
        );
        assert!(!retained.contains_key("vary-provider/a"));
    }
}
