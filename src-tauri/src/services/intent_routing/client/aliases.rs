//! Magpie Codex picker identities and duplicate-label disambiguation.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn project(
    catalog: &mut Value,
    retained: &mut BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    project_menu(catalog, retained, None)
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
}
