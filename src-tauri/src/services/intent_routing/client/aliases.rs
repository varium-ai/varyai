//! Magpie Codex picker identities and duplicate-label disambiguation.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
pub(super) fn project(
    catalog: &mut Value,
    retained: &mut BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    project_menu(catalog, retained, None)
}

fn wire_name(label: &str) -> String {
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

/// Projects an Agent-facing catalog to readable ASCII model IDs while retaining
/// hidden historical aliases for existing sessions.
pub(crate) fn project_readable(
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
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut legacy = Vec::new();
    for (id, label, index) in entries {
        let previous = crate::services::agent_connections::model_wire(retained, None, &id);
        let base = if previous == id {
            wire_name(&label)
        } else {
            previous.to_owned()
        };
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

pub(super) fn project_menu(
    catalog: &mut Value,
    retained: &mut BTreeMap<String, String>,
    menu: Option<&crate::proxy::intent::Menu>,
) -> Result<Vec<String>, String> {
    let models = catalog["models"]
        .as_array_mut()
        .ok_or("intentRouting.clientFailed")?;
    let entries = models
        .iter()
        .map(|model| {
            let canonical = model["slug"].as_str().ok_or("intentRouting.clientFailed")?;
            let direct = menu
                .and_then(|menu| menu.groups.get(canonical))
                .filter(|_| canonical.starts_with("vary-provider/"))
                .and_then(|session| session.targets.first());
            let id = direct
                .map(|target| format!("{}/{}", target.provider_id, target.model))
                .unwrap_or_else(|| {
                    canonical
                        .strip_prefix("vary-auto/")
                        .map(|id| format!("group/{id}"))
                        .unwrap_or_else(|| canonical.to_owned())
                });
            // Codex reads this identity from an ASCII-only HTTP routing hint.
            let id = if id.is_ascii() {
                id
            } else {
                format!("vary-provider/alias-{:x}", Sha256::digest(id.as_bytes()))
            };
            let provider = menu
                .and_then(|menu| menu.groups.get(canonical))
                .and_then(|session| session.targets.first())
                .map(|target| target.provider_id.clone())
                .unwrap_or_else(|| {
                    if canonical.starts_with("auto/")
                        || canonical.starts_with("vary-auto/")
                        || canonical.starts_with("group/")
                    {
                        String::new()
                    } else {
                        canonical
                            .split_once('/')
                            .map_or("", |(provider, _)| provider)
                            .to_owned()
                    }
                });
            let label = model["display_name"]
                .as_str()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(canonical)
                .to_owned();
            Ok((canonical.to_owned(), id, provider, label))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut same: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, (_, _, _, label)) in entries.iter().enumerate() {
        if models[index]["visibility"] != "hide" {
            same.entry(label.to_lowercase()).or_default().push(index);
        }
    }
    // Magpie codexApart: only full, case-insensitive duplicate labels get
    // a suffix, using a unique provider ID or otherwise the full entry ID.
    for indices in same.values().filter(|indices| indices.len() > 1) {
        for &index in indices {
            let (_, id, provider, label) = &entries[index];
            let group = entries[index].0.starts_with("auto/")
                || entries[index].0.starts_with("vary-auto/")
                || entries[index].0.starts_with("group/");
            let unique = !group
                && !provider.is_empty()
                && indices
                    .iter()
                    .filter(|&&i| entries[i].2.eq_ignore_ascii_case(provider))
                    .count()
                    == 1;
            models[index]["display_name"] =
                Value::String(format!("{label} · {}", if unique { provider } else { id }));
        }
    }
    let mut visible = Vec::new();
    let mut legacy = Vec::new();
    let reserved = entries
        .iter()
        .map(|(_, id, _, _)| id.clone())
        .collect::<BTreeSet<_>>();
    if reserved.len() != entries.len() {
        return Err("intentRouting.clientConflict".into());
    }
    for (index, (canonical, id, _, _)) in entries.into_iter().enumerate() {
        if retained.get(&id).is_some_and(|owner| owner != &canonical) {
            return Err("intentRouting.clientConflict".into());
        }
        if !retained.contains_key(&id) && retained.len() >= 16384 {
            return Err("intentRouting.limit".into());
        }
        retained.insert(id.clone(), canonical.clone());
        models[index]["slug"] = Value::String(id.clone());
        visible.push(id.clone());
        // Preserve historical VaryAI IDs only as hidden compatibility entries.
        let mut previous = retained
            .iter()
            .filter(|(_, owner)| **owner == canonical)
            .map(|(wire, _)| wire.clone())
            .collect::<BTreeSet<_>>();
        previous.insert(canonical);
        for wire in previous {
            if reserved.contains(&wire) {
                continue;
            }
            let mut hidden = models[index].clone();
            hidden["slug"] = Value::String(wire);
            hidden["visibility"] = Value::String("hide".into());
            legacy.push(hidden);
        }
    }
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
    fn unicode_wire_ids_support_codex_headers_and_preserve_legacy_selection() {
        let canonical = "synthetic-供应商/日本語-model";
        let label = "Synthetic model · 测试供应商";
        let mut retained = BTreeMap::from([(canonical.into(), canonical.into())]);
        let mut value = catalog(&[(canonical, label)]);
        let visible = project(&mut value, &mut retained).unwrap();
        let wire = &visible[0];
        let header = http::HeaderValue::from_str(&format!("model={wire}")).unwrap();
        assert!(
            header.to_str().is_ok(),
            "Codex must be able to read its routing hint"
        );
        assert!(wire.is_ascii());
        assert_eq!(retained[wire], canonical);
        assert_eq!(
            selected(&visible, &retained, canonical),
            Some(wire.as_str())
        );
        assert_eq!(value["models"][0]["display_name"], label);
        assert_eq!(value["models"][1]["slug"], canonical);
        assert_eq!(value["models"][1]["visibility"], "hide");
        assert_eq!(
            value["models"][0]["context_window"],
            value["models"][1]["context_window"]
        );
    }

    #[test]
    fn unicode_wire_ids_do_not_collide_with_literal_escapes_or_change_on_reorder() {
        let entries = [
            ("synthetic/测试", "Synthetic Chinese"),
            ("synthetic/%E6%B5%8B%E8%AF%95", "Synthetic literal escape"),
            ("synthetic/model", "Synthetic ASCII"),
            ("vary-auto/测试组", "Synthetic group"),
        ];
        let mut retained = BTreeMap::new();
        let mut value = catalog(&entries);
        let initial = project(&mut value, &mut retained).unwrap();
        assert!(initial.iter().all(|id| id.is_ascii()));
        assert_eq!(initial.iter().collect::<BTreeSet<_>>().len(), entries.len());
        assert_eq!(initial[2], "synthetic/model");
        let mut reordered = catalog(&entries.into_iter().rev().collect::<Vec<_>>());
        let updated = project(&mut reordered, &mut retained).unwrap();
        assert_eq!(updated, initial.into_iter().rev().collect::<Vec<_>>());
    }

    #[test]
    fn magpie_group_duplicates_use_group_ids_and_hidden_entries_do_not_disambiguate() {
        let mut value = catalog(&[
            ("vary-auto/a", "Shared · routing group"),
            ("vary-auto/b", "Shared · routing group"),
            ("relay/a", "Unique · Relay"),
            ("relay/old", "Unique · Relay"),
        ]);
        value["models"][3]["visibility"] = json!("hide");
        project(&mut value, &mut BTreeMap::new()).unwrap();
        assert_eq!(value["models"][0]["slug"], "group/a");
        assert_eq!(
            value["models"][0]["display_name"],
            "Shared · routing group · group/a"
        );
        assert_eq!(
            value["models"][1]["display_name"],
            "Shared · routing group · group/b"
        );
        assert_eq!(value["models"][2]["display_name"], "Unique · Relay");
    }

    #[test]
    fn magpie_identity_is_independent_of_unicode_labels_and_catalog_order() {
        let entries = [
            ("company/glm-5", "GLM-5 · 公司"),
            ("test/glm-5", "GLM-5 · 测试"),
        ];
        let mut retained = BTreeMap::new();
        let mut first = catalog(&entries);
        assert_eq!(
            project(&mut first, &mut retained).unwrap(),
            ["company/glm-5", "test/glm-5"]
        );
        assert_eq!(first["models"][0]["display_name"], "GLM-5 · 公司");
        let mut renamed = catalog(&[(entries[1].0, "New · 测试"), entries[0]]);
        assert_eq!(
            project(&mut renamed, &mut retained).unwrap(),
            ["test/glm-5", "company/glm-5"]
        );
        assert_eq!(retained["company/glm-5"], "company/glm-5");
    }

    #[test]
    fn magpie_duplicate_labels_use_provider_identity_or_full_entry_identity() {
        let mut value = catalog(&[
            ("codex/gpt", "GPT · Codex"),
            ("codex-b/gpt", "GPT · Codex"),
            ("relay/glm", "GLM · Relay"),
            ("relay/glm-old", "glm · relay"),
            ("relay/kimi", "Kimi · Relay"),
        ]);
        project(&mut value, &mut BTreeMap::new()).unwrap();
        let names = value["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["display_name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "GPT · Codex · codex",
                "GPT · Codex · codex-b",
                "GLM · Relay · relay/glm",
                "glm · relay · relay/glm-old",
                "Kimi · Relay"
            ]
        );
    }

    #[test]
    fn previous_display_aliases_remain_hidden_with_capabilities_and_selection() {
        let canonical = "vary-provider/model-synthetic";
        let mut retained = BTreeMap::from([("Grok-Synthetic".into(), canonical.into())]);
        let mut value = catalog(&[(canonical, "Grok · Synthetic")]);
        let names = project(&mut value, &mut retained).unwrap();
        assert_eq!(names, [canonical]);
        assert_eq!(
            selected(&names, &retained, "Grok-Synthetic"),
            Some(canonical)
        );
        assert_eq!(value["models"][1]["slug"], "Grok-Synthetic");
        assert_eq!(value["models"][1]["visibility"], "hide");
        for key in [
            "context_window",
            "supported_reasoning_levels",
            "service_tiers",
        ] {
            assert_eq!(value["models"][0][key], value["models"][1][key]);
        }
    }

    #[test]
    fn readable_agent_aliases_survive_renaming_and_catalog_reordering() {
        let mut retained = BTreeMap::new();
        let mut first = catalog(&[
            ("vary-provider/a", "Readable Model"),
            ("vary-provider/b", "Readable Model"),
            ("vary-provider/c", "Readable-Model-2"),
        ]);
        assert_eq!(
            project_readable(&mut first, &mut retained).unwrap(),
            ["Readable-Model", "Readable-Model-3", "Readable-Model-2"]
        );
        let mut renamed = catalog(&[
            ("vary-provider/c", "Renamed"),
            ("vary-provider/a", "Renamed"),
            ("vary-provider/b", "Renamed"),
        ]);
        let names = project_readable(&mut renamed, &mut retained).unwrap();
        assert_eq!(
            names,
            ["Readable-Model-2", "Readable-Model", "Readable-Model-3"]
        );
        assert_eq!(retained["Readable-Model"], "vary-provider/a");
        let hidden = renamed["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|model| model["slug"] == "vary-provider/a")
            .unwrap();
        assert_eq!(hidden["visibility"], "hide");
        assert_eq!(hidden["context_window"], 123456);
    }
}
