//! Codex picker source labels and stable routing identities.

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

// Source identities are routing keys, not picker labels. Allocate readable source
// labels once per visible source so every model from it has the same suffix.
fn source_labels(
    models: &[Value],
    menu: Option<&crate::proxy::intent::Menu>,
) -> BTreeMap<String, String> {
    let mut sources = BTreeMap::new();
    for model in models.iter().filter(|model| model["visibility"] != "hide") {
        let Some(target) = model["slug"]
            .as_str()
            .filter(|slug| slug.starts_with("vary-provider/"))
            .and_then(|slug| menu?.groups.get(slug))
            .and_then(|session| session.targets.first())
        else {
            continue;
        };
        let name = target
            .upstream
            .as_ref()
            .map(|provider| provider.name.as_str())
            .or_else(|| {
                target
                    .credentials
                    .as_ref()
                    .map(|credentials| credentials.provider_name())
            })
            .filter(|name| !name.trim().is_empty());
        let name = match name {
            // Automatically generated account labels belong in account management.
            Some(name) if target.official && name.starts_with("Codex · ") => "Codex",
            Some(name) => name,
            None if target.official => "Codex",
            None => "Source",
        };
        sources.insert(target.provider_id.clone(), name.to_owned());
    }
    let mut counts = BTreeMap::new();
    for name in sources.values() {
        *counts.entry(name.to_lowercase()).or_insert(0) += 1;
    }
    let mut reserved = sources
        .values()
        .map(|name| name.to_lowercase())
        .collect::<BTreeSet<_>>();
    for name in sources.values_mut() {
        if counts[&name.to_lowercase()] < 2 {
            continue;
        }
        let mut number = 1;
        loop {
            let candidate = format!("{name} ({number})");
            if reserved.insert(candidate.to_lowercase()) {
                *name = candidate;
                break;
            }
            number += 1;
        }
    }
    sources
}

pub(crate) fn qualify_menu_labels(menu: &mut crate::proxy::intent::Menu) {
    let entries = menu
        .models
        .values()
        .map(|entry| serde_json::json!({"slug":entry.alias}))
        .collect::<Vec<_>>();
    let sources = source_labels(&entries, Some(menu));
    for entry in menu.models.values_mut() {
        let Some((target, source)) = menu
            .groups
            .get(&entry.alias)
            .filter(|_| entry.alias.starts_with("vary-provider/"))
            .and_then(|session| session.targets.first())
            .and_then(|target| {
                sources
                    .get(&target.provider_id)
                    .map(|source| (target, source))
            })
        else {
            continue;
        };
        if entry.label == target.model
            || !entry.label.to_lowercase().contains(&source.to_lowercase())
        {
            entry.label = format!("{} · {source}", entry.label);
        }
    }
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
    let sources = source_labels(models, menu);
    for (index, (canonical, _, provider, label)) in entries.iter().enumerate() {
        if let Some(source) = sources
            .get(provider)
            .filter(|_| canonical.starts_with("vary-provider/"))
        {
            // Preserve custom names that already identify their source.
            if !label.to_lowercase().contains(&source.to_lowercase()) {
                models[index]["display_name"] = Value::String(format!("{label} · {source}"));
            }
        }
    }
    let mut same: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, model) in models.iter().enumerate() {
        if model["visibility"] != "hide" {
            if let Some(label) = model["display_name"].as_str() {
                same.entry(label.to_lowercase()).or_default().push(index);
            }
        }
    }
    let mut reserved = same.keys().cloned().collect::<BTreeSet<_>>();
    // Equal custom model/group names still need a hint, never an internal ID.
    for indices in same.values_mut().filter(|indices| indices.len() > 1) {
        indices.sort_by(|&a, &b| entries[a].1.cmp(&entries[b].1));
        let mut number = 1;
        for &index in indices.iter() {
            let label = models[index]["display_name"]
                .as_str()
                .unwrap_or(&entries[index].3);
            loop {
                let candidate = format!("{label} ({number})");
                number += 1;
                if reserved.insert(candidate.to_lowercase()) {
                    models[index]["display_name"] = Value::String(candidate);
                    break;
                }
            }
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
    fn group_duplicates_use_readable_numbers_and_hidden_entries_do_not_disambiguate() {
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
            "Shared · routing group (1)"
        );
        assert_eq!(
            value["models"][1]["display_name"],
            "Shared · routing group (2)"
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
    fn duplicate_legacy_labels_use_readable_numbers_without_exposing_identity() {
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
                "GPT · Codex (2)",
                "GPT · Codex (1)",
                "GLM · Relay (1)",
                "glm · relay (2)",
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

    fn source_menu(sources: &[(&str, &str, bool)]) -> crate::proxy::intent::Menu {
        use crate::proxy::intent::{Menu, Session, Target};
        use std::sync::Arc;
        let mut menu = Menu {
            token: "synthetic-label-token".into(),
            models: Default::default(),
            default_model: None,
            omitted_entries: 0,
            context_settings: Default::default(),
            groups: Default::default(),
        };
        let template = Session {
            group: super::super::tests::group(),
            targets: vec![Target {
                fixed_effort: None,
                provider_id: "fixture".into(),
                model: "synthetic-model".into(),
                upstream: None,
                effort: Default::default(),
                url: "http://127.0.0.1:1/responses".parse().unwrap(),
                headers: Default::default(),
                official: false,
                credentials: None,
                scheduling: Arc::default(),
            }],
            classifier: None,
            client: reqwest::Client::new(),
            created: std::time::Instant::now(),
            bindings: Arc::default(),
        };
        for (id, name, official) in sources {
            let mut session = template.clone();
            let target = &mut session.targets[0];
            target.provider_id = (*id).into();
            target.model = "synthetic-model".into();
            target.official = *official;
            target.upstream = (!official).then(|| {
                crate::provider::Provider::with_id((*id).into(), (*name).into(), json!({}), None)
            });
            menu.groups
                .insert(format!("vary-provider/{id}"), Arc::new(session));
        }
        menu
    }

    #[test]
    fn account_and_api_duplicates_show_sources_without_changing_request_identity() {
        let account = "11111111-1111-4111-8111-111111111111";
        let api = "22222222-2222-4222-8222-222222222222";
        let menu = source_menu(&[(account, "Codex", true), (api, "Team Relay", false)]);
        let account_alias = format!("vary-provider/{account}");
        let api_alias = format!("vary-provider/{api}");
        let mut value = catalog(&[
            (&account_alias, "GPT Synthetic"),
            (&api_alias, "GPT Synthetic"),
        ]);
        let mut retained =
            BTreeMap::from([("Previous-account-alias".into(), account_alias.clone())]);
        let visible = project_menu(&mut value, &mut retained, Some(&menu)).unwrap();
        assert_eq!(value["models"][0]["display_name"], "GPT Synthetic · Codex");
        assert_eq!(
            value["models"][1]["display_name"],
            "GPT Synthetic · Team Relay"
        );
        assert_eq!(
            visible,
            [
                format!("{account}/synthetic-model"),
                format!("{api}/synthetic-model")
            ]
        );
        assert_eq!(retained[&visible[0]], account_alias);
        assert_eq!(retained[&visible[1]], api_alias);
        assert_eq!(
            selected(&visible, &retained, "Previous-account-alias"),
            Some(visible[0].as_str())
        );
        assert!(value["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["slug"] == "Previous-account-alias" && m["visibility"] == "hide"));
        for model in value["models"].as_array().unwrap() {
            assert_eq!(model["context_window"], 123456);
            assert_eq!(
                model["supported_reasoning_levels"],
                json!([{"effort":"high"}])
            );
            assert_eq!(model["service_tiers"], json!([{"id":"priority"}]));
        }
    }

    #[test]
    fn every_model_from_a_source_keeps_the_same_suffix_even_without_a_name_collision() {
        use std::sync::Arc;
        let mut menu = source_menu(&[("account", "Codex", true), ("relay", "Team Relay", false)]);
        let mut second = menu.groups["vary-provider/relay"].as_ref().clone();
        second.targets[0].model = "synthetic-other-model".into();
        menu.groups
            .insert("vary-provider/relay-other".into(), Arc::new(second));
        let mut value = catalog(&[
            ("vary-provider/account", "GPT Synthetic Sol"),
            ("vary-provider/relay", "GPT Synthetic Sol"),
            ("vary-provider/relay-other", "GPT Synthetic Terra"),
        ]);
        project_menu(&mut value, &mut BTreeMap::new(), Some(&menu)).unwrap();
        assert_eq!(
            value["models"][0]["display_name"],
            "GPT Synthetic Sol · Codex"
        );
        assert_eq!(
            value["models"][1]["display_name"],
            "GPT Synthetic Sol · Team Relay"
        );
        assert_eq!(
            value["models"][2]["display_name"],
            "GPT Synthetic Terra · Team Relay"
        );
    }

    #[test]
    fn same_source_names_use_stable_readable_numbers_and_ignore_hidden_models() {
        let sources = [("source-b", "Relay", false), ("source-a", "Relay", false)];
        let menu = source_menu(&sources);
        let mut value = catalog(&[
            ("vary-provider/source-b", "Shared"),
            ("vary-provider/source-a", "Shared"),
        ]);
        project_menu(&mut value, &mut BTreeMap::new(), Some(&menu)).unwrap();
        assert_eq!(value["models"][0]["display_name"], "Shared · Relay (2)");
        assert_eq!(value["models"][1]["display_name"], "Shared · Relay (1)");
        let mut reversed = catalog(&[
            ("vary-provider/source-a", "Shared"),
            ("vary-provider/source-b", "Shared"),
        ]);
        project_menu(&mut reversed, &mut BTreeMap::new(), Some(&menu)).unwrap();
        assert_eq!(
            reversed["models"][0]["display_name"],
            value["models"][1]["display_name"]
        );
        let mut hidden = catalog(&[
            ("vary-provider/source-a", "Shared"),
            ("vary-provider/source-b", "Shared"),
        ]);
        hidden["models"][1]["visibility"] = json!("hide");
        project_menu(&mut hidden, &mut BTreeMap::new(), Some(&menu)).unwrap();
        assert_eq!(hidden["models"][0]["display_name"], "Shared · Relay");
    }

    #[test]
    fn custom_names_are_not_suffixed_twice_or_overwritten_by_numbering() {
        let menu = source_menu(&[
            ("a", "Relay", false),
            ("b", "Relay", false),
            ("c", "Other", false),
        ]);
        let mut value = catalog(&[
            ("vary-provider/a", "My RELAY model"),
            ("vary-provider/b", "My RELAY model"),
            ("vary-provider/c", "My RELAY model (1)"),
        ]);
        project_menu(&mut value, &mut BTreeMap::new(), Some(&menu)).unwrap();
        assert_eq!(
            value["models"][0]["display_name"],
            "My RELAY model · Relay (1)"
        );
        assert_eq!(
            value["models"][1]["display_name"],
            "My RELAY model · Relay (2)"
        );
        assert_eq!(
            value["models"][2]["display_name"],
            "My RELAY model (1) · Other"
        );
    }

    #[test]
    fn unique_custom_names_keep_the_source_once_and_source_numbering_avoids_reserved_names() {
        let menu = source_menu(&[
            ("a", "Relay", false),
            ("b", "Relay", false),
            ("c", "Relay (1)", false),
        ]);
        let mut value = catalog(&[
            ("vary-provider/a", "Shared"),
            ("vary-provider/b", "Shared"),
            ("vary-provider/c", "My RELAY (1) model"),
        ]);
        project_menu(&mut value, &mut BTreeMap::new(), Some(&menu)).unwrap();
        assert_eq!(value["models"][0]["display_name"], "Shared · Relay (2)");
        assert_eq!(value["models"][1]["display_name"], "Shared · Relay (3)");
        assert_eq!(value["models"][2]["display_name"], "My RELAY (1) model");
    }

    #[test]
    fn routing_groups_do_not_inherit_their_first_members_source_suffix() {
        use std::sync::Arc;
        let mut menu = source_menu(&[("relay", "Team Relay", false)]);
        let session = menu.groups["vary-provider/relay"].clone();
        menu.groups
            .insert("vary-auto/group".into(), Arc::clone(&session));
        let mut value = catalog(&[
            ("vary-provider/relay", "Direct"),
            ("vary-auto/group", "Development · routing group"),
        ]);
        project_menu(&mut value, &mut BTreeMap::new(), Some(&menu)).unwrap();
        assert_eq!(value["models"][0]["display_name"], "Direct · Team Relay");
        assert_eq!(
            value["models"][1]["display_name"],
            "Development · routing group"
        );
    }

    #[test]
    fn duplicate_model_numbers_do_not_overwrite_literal_custom_labels() {
        let mut value = catalog(&[
            ("source/a", "Shared"),
            ("source/b", "shared"),
            ("source/c", "Shared (1)"),
        ]);
        project(&mut value, &mut BTreeMap::new()).unwrap();
        assert_eq!(value["models"][0]["display_name"], "Shared (2)");
        assert_eq!(value["models"][1]["display_name"], "shared (3)");
        assert_eq!(value["models"][2]["display_name"], "Shared (1)");
    }

    #[test]
    fn generated_account_names_do_not_publish_account_details() {
        let mut menu = source_menu(&[("account", "Codex", true)]);
        let session =
            std::sync::Arc::make_mut(menu.groups.get_mut("vary-provider/account").unwrap());
        session.targets[0].upstream = Some(crate::provider::Provider::with_id(
            "account".into(),
            "Codex · synthetic-person@example.test".into(),
            json!({}),
            None,
        ));
        let mut value = catalog(&[("vary-provider/account", "GPT Synthetic")]);
        project_menu(&mut value, &mut BTreeMap::new(), Some(&menu)).unwrap();
        assert_eq!(value["models"][0]["display_name"], "GPT Synthetic · Codex");
        assert!(!value.to_string().contains("synthetic-person"));
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
