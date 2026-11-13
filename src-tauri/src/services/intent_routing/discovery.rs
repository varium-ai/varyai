//! Vendor spelling normalization for discovery only. Wire model IDs remain exact.
use super::{valid_target, Member, RoutingGroup, RoutingStrategy};
use crate::provider::Provider;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) fn same_model(id: &str) -> String {
    let lower = id.trim().to_lowercase();
    let leaf = lower.rsplit('/').next().unwrap_or(&lower);
    let mut key = leaf
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let bytes = key.as_bytes();
    let mut normalized = String::with_capacity(key.len());
    for (i, c) in key.char_indices() {
        normalized.push(
            if matches!(c, '.' | 'p')
                && i > 0
                && i + 1 < bytes.len()
                && bytes[i - 1].is_ascii_digit()
                && bytes[i + 1].is_ascii_digit()
            {
                '-'
            } else {
                c
            },
        );
    }
    key = normalized;
    if let Some((prefix, date)) = key.rsplit_once(['-', '@']) {
        if snapshot(date) {
            return prefix.into();
        }
    }
    key
}

fn snapshot(date: &str) -> bool {
    if !date.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if date.len() == 8 && date.starts_with("20") {
        return true;
    }
    date.len() == 6
        && date[..2]
            .parse::<u8>()
            .is_ok_and(|y| (23..=39).contains(&y))
        && date[2..4]
            .parse::<u8>()
            .is_ok_and(|m| (1..=12).contains(&m))
        && date[4..].parse::<u8>().is_ok_and(|d| (1..=31).contains(&d))
}

pub(super) fn valid_alias(alias: &str) -> bool {
    alias.strip_prefix("auto/").is_some_and(|id| {
        !id.is_empty()
            && id.len() <= 200
            && !id.contains("..")
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_' | b':'))
    })
}

pub(crate) fn legacy_ids(group: &RoutingGroup) -> Vec<String> {
    group
        .members
        .iter()
        .map(|m| format!("auto-{:x}", Sha256::digest(m.model_id.as_bytes())))
        .collect()
}

pub(crate) fn discover_groups(providers: impl IntoIterator<Item = Provider>) -> Vec<RoutingGroup> {
    discover(providers, 2)
}

pub(crate) fn discover_stand_ins(
    providers: impl IntoIterator<Item = Provider>,
) -> Vec<RoutingGroup> {
    discover(providers, 1)
}

fn discover(providers: impl IntoIterator<Item = Provider>, minimum: usize) -> Vec<RoutingGroup> {
    let mut providers: Vec<_> = providers.into_iter().collect();
    providers.sort_by(|a, b| {
        (a.sort_index.unwrap_or(usize::MAX), a.created_at, &a.id).cmp(&(
            b.sort_index.unwrap_or(usize::MAX),
            b.created_at,
            &b.id,
        ))
    });
    let mut found: BTreeMap<String, (String, String, Vec<Member>)> = BTreeMap::new();
    let mut order = Vec::new();
    for provider in providers {
        let mut entries = provider
            .settings_config
            .pointer("/modelCatalog/models")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        // Imported static Codex connections can name their only model in TOML.
        // Account catalogs stay authoritative, including an empty list.
        if !super::catalog::official_source(&provider) {
            if let Some(model) = provider.settings_config["config"]
                .as_str()
                .and_then(|text| text.parse::<toml::Value>().ok())
                .and_then(|doc| doc.get("model")?.as_str().map(str::to_owned))
            {
                if !entries
                    .iter()
                    .any(|entry| model_id(entry) == Some(model.trim()))
                {
                    entries.push(serde_json::json!({"model":model}));
                }
            }
        }
        for entry in &entries {
            let Some(model) = model_id(entry) else {
                continue;
            };
            if !valid_target(&provider.id, model) {
                continue;
            }
            let equivalent = super::model_preferences::same_as(entry).unwrap_or(model);
            let key = same_model(equivalent);
            if key.is_empty() {
                continue;
            }
            let alias_leaf = equivalent
                .rsplit('/')
                .next()
                .unwrap_or(model)
                .to_lowercase()
                .split(|c: char| c.is_whitespace() || c == '_')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-");
            let alias_leaf = match alias_leaf.rsplit_once(['-', '@']) {
                Some((prefix, date)) if snapshot(date) => prefix.to_owned(),
                _ => alias_leaf,
            };
            let alias = format!("auto/{alias_leaf}");
            let mut alias = if valid_alias(&alias) && same_model(&alias_leaf) == key {
                alias
            } else {
                format!("auto/{key}")
            };
            if !valid_alias(&alias) {
                alias = format!("auto/model-{:x}", Sha256::digest(key.as_bytes()));
            }
            let name = ["displayName", "display_name", "name", "label"]
                .iter()
                .find_map(|field| {
                    entry
                        .get(*field)
                        .and_then(|v| v.as_str())
                        .filter(|v| super::valid_text(v, 80) && *v != model)
                })
                .unwrap_or(model)
                .to_owned();
            let group = found.entry(key.clone()).or_insert_with(|| {
                order.push(key);
                (name.clone(), alias.clone(), Vec::new())
            });
            if group.2.iter().any(|m| m.provider_id == provider.id) {
                continue;
            }
            if group.0
                == group
                    .2
                    .first()
                    .map(|m| m.model_id.as_str())
                    .unwrap_or(model)
                && name != model
            {
                group.0 = name;
            }
            // Prefer a vendor's dotted version spelling for the public alias.
            if alias.contains('.') && !group.1.contains('.') {
                group.1 = alias;
            }
            group.2.push(Member {
                effort: None,
                id: format!("m-{:x}", Sha256::digest(provider.id.as_bytes())),
                provider_id: provider.id.clone(),
                model_id: model.into(),
                weight: 1,
                free: None,
            });
        }
    }
    order
        .into_iter()
        .filter_map(|key| {
            let (name, alias, members) = found.remove(&key)?;
            if members.len() < minimum {
                return None;
            }
            Some(RoutingGroup {
                id: format!("auto-{:x}", Sha256::digest(key.as_bytes())),
                name: name.chars().take(80).collect(),
                revision: 0,
                client: "global".into(),
                enabled: true,
                default_member_id: members[0].id.clone(),
                fallback_member_ids: members.iter().skip(1).map(|m| m.id.clone()).collect(),
                members,
                intent_recognition_enabled: false,
                classifier_target: None,
                classification_budget_ms: 2000,
                rules: vec![],
                retired_policy: None,
                auto_generated: true,
                routing: RoutingStrategy::Smart,
                model_alias: Some(alias),
                manual_member_id: None,
                context_window: None,
                affinity: String::new(),
                first_token: 0,
                patience: 0,
                off: vec![],
                matches: vec![],
                matched: vec![],
                unlists: false,
                sink: false,
                levels: vec![],
            })
        })
        .collect()
}

fn model_id(entry: &serde_json::Value) -> Option<&str> {
    entry
        .as_str()
        .or_else(|| entry["model"].as_str())
        .or_else(|| entry["id"].as_str())
        .map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_equivalence_merges_only_distinct_providers_without_rewriting_wire_ids() {
        let make = |id: &str, models| {
            Provider::with_id(
                id.into(),
                id.into(),
                json!({"modelCatalog":{"models":models}}),
                None,
            )
        };
        let mut a = make(
            "a",
            json!([
                {"model":"z-model"}, {"model":"a-model"},
                {"model":"relay-exact","modelPreferences":{"sameAs":"lab/canonical"}},
                {"model":"relay-other","modelPreferences":{"sameAs":"canonical"}},
                {"model":"canonical-thinking"}
            ]),
        );
        a.sort_index = Some(0);
        let b = make(
            "b",
            json!([{"model":"canonical"},{"model":"a-model"},{"model":"z-model"}]),
        );
        let groups = discover_groups(vec![b, a]);
        assert_eq!(
            groups.iter().map(RoutingGroup::alias).collect::<Vec<_>>(),
            ["auto/z-model", "auto/a-model", "auto/canonical"]
        );
        let group = &groups[2];
        assert_eq!(group.members.len(), 2);
        assert_eq!(group.members[0].model_id, "relay-exact");
        assert_eq!(group.members[1].model_id, "canonical");
        assert!(group.enabled);
    }
    #[test]
    fn spelling_rules_keep_real_variants_and_pinned_openai_dates_distinct() {
        for (input, expected) in [
            ("accounts/fireworks/models/deepseek-v4p1", "deepseek-v4-1"),
            (" GPT-6 Sol ", "gpt-6-sol"),
            ("claude-opus-5.5-20260801", "claude-opus-5-5"),
            ("claude_opus_5_5@20260801", "claude-opus-5-5"),
            ("deepseek-v4-1-flash-260910", "deepseek-v4-1-flash"),
            ("gpt-4o-2024-08-06", "gpt-4o-2024-08-06"),
            ("kimi-k2-0905", "kimi-k2-0905"),
            ("model:batch", "model:batch"),
            ("model-thinking", "model-thinking"),
        ] {
            assert_eq!(same_model(input), expected);
        }
    }
}
