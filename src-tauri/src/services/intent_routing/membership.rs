//! Resolve catalog patterns only while preparing immutable execution snapshots.
use super::{Member, RoutingGroup};
use crate::provider::Provider;
use regex::{Regex, RegexBuilder};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub(super) fn compile(pattern: &str) -> Result<Regex, &'static str> {
    if pattern.is_empty()
        || (!pattern.contains('*') && !pattern.starts_with("re:"))
        || pattern.len() > 1024
        || pattern.chars().any(char::is_control)
    {
        return Err("intentRouting.invalid");
    }
    let expression = match pattern.strip_prefix("re:") {
        Some(expression) if !expression.trim().is_empty() => format!("^(?:{expression})$"),
        Some(_) => return Err("intentRouting.invalid"),
        None => format!(
            "^{}$",
            pattern
                .split('*')
                .map(regex::escape)
                .collect::<Vec<_>>()
                .join(".*")
        ),
    };
    RegexBuilder::new(&expression)
        .case_insensitive(true)
        .build()
        .map_err(|_| "intentRouting.invalid")
}

pub(crate) fn resolve(
    mut group: RoutingGroup,
    providers: &[Provider],
) -> Result<RoutingGroup, String> {
    group = definition(group);
    group.validate()?;
    let patterns = group
        .matches
        .iter()
        .map(|pattern| compile(pattern))
        .collect::<Result<Vec<_>, _>>()?;
    if patterns.is_empty() {
        return Ok(group);
    }
    let mut seen = group
        .members
        .iter()
        .map(|member| (member.provider_id.clone(), member.model_id.clone()))
        .collect::<HashSet<_>>();
    let mut available = Vec::new();
    for provider in providers {
        if let Some(rows) = provider
            .settings_config
            .pointer("/modelCatalog/models")
            .and_then(serde_json::Value::as_array)
        {
            for row in rows {
                if let Some(model) = row
                    .as_str()
                    .or_else(|| row["model"].as_str())
                    .or_else(|| row["id"].as_str())
                {
                    available.push((provider.id.clone(), model.to_owned()));
                }
            }
        }
    }
    for (provider, model) in available {
        if !super::valid_target(&provider, &model)
            || !patterns
                .iter()
                .any(|pattern| pattern.is_match(&format!("{provider}/{model}")))
            || !seen.insert((provider.clone(), model.clone()))
        {
            continue;
        }
        if group.members.len() >= 256 {
            return Err("intentRouting.limit".into());
        }
        let id = format!(
            "match-{:x}",
            Sha256::digest(format!("{}:{provider}{model}", provider.len()).as_bytes())
        );
        if group.members.iter().any(|member| member.id == id) {
            return Err("intentRouting.invalid".into());
        }
        group.members.push(Member {
            effort: None,
            id: id.clone(),
            provider_id: provider,
            model_id: model,
            weight: 1,
            free: None,
        });
        group.matched.push(id);
    }
    if group.default_member_id.is_empty() {
        group.default_member_id = group
            .members
            .first()
            .map(|member| member.id.clone())
            .unwrap_or_default();
    }
    group.validate()?;
    Ok(group)
}

/// Runtime members never become user-owned members when a snapshot is edited.
pub(crate) fn definition(mut group: RoutingGroup) -> RoutingGroup {
    if group.matched.is_empty() {
        return group;
    }
    group
        .members
        .retain(|member| !group.matched.contains(&member.id));
    if group.matched.contains(&group.default_member_id) {
        group.default_member_id = group
            .members
            .first()
            .map(|member| member.id.clone())
            .unwrap_or_default();
    }
    group
        .fallback_member_ids
        .retain(|id| !group.matched.contains(id));
    group.matched.clear();
    group
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn patterns_match_whole_ids_case_insensitively_unless_flags_override_it() {
        assert!(compile("VENDOR/*").expect("glob").is_match("vendor/model"));
        assert!(!compile("re:model").expect("regex").is_match("vendor/model"));
        assert!(compile("re:VENDOR/.*")
            .expect("case insensitive")
            .is_match("vendor/model"));
        assert!(!compile("re:(?-i)VENDOR/.*")
            .expect("case sensitive")
            .is_match("vendor/model"));
        assert!(compile("re:").is_err());
        assert!(compile("vendor/model").is_err());
    }
    #[test]
    fn patterns_follow_the_catalog_without_persisting_derived_members() {
        let mut value = serde_json::to_value(super::super::tests::fixture()).expect("fixture");
        value["match"] = json!(["vendor/*:free", "re:^vendor/(large|vision)$"]);
        let group: RoutingGroup = serde_json::from_value(value).expect("patterns");
        let provider = Provider::with_id(
            "vendor".into(),
            "Synthetic".into(),
            json!({"modelCatalog":{"models":[
                {"model":"small:free"},{"model":"large"},{"model":"paid"},{"model":"vision"}
            ]}}),
            None,
        );
        let resolved = resolve(group.clone(), std::slice::from_ref(&provider)).expect("resolve");
        assert_eq!(resolved.members.len(), 5);
        assert_eq!(resolved.matched.len(), 3);
        let stored = definition(resolved.clone());
        assert_eq!(stored, group);
        let serialized = serde_json::to_value(&stored).expect("json");
        assert!(serialized.get("matched").is_none());
        assert_eq!(serialized["members"].as_array().expect("members").len(), 2);
        let again = resolve(group.clone(), &[provider]).expect("stable");
        assert_eq!(resolved.matched, again.matched);
        let empty = resolve(group, &[]).expect("empty catalog");
        assert_eq!(empty.members.len(), 2);
        assert!(compile("re:[").is_err());
    }

    #[test]
    fn pattern_only_groups_can_be_saved_before_a_catalog_matches() {
        let mut group = super::super::tests::fixture();
        group.members.clear();
        group.rules.clear();
        group.default_member_id.clear();
        group.fallback_member_ids.clear();
        group.matches = vec!["vendor/*".into()];
        let empty = resolve(group.clone(), &[]).expect("empty catalog");
        assert!(empty.members.is_empty());
        let provider = Provider::with_id(
            "vendor".into(),
            "Synthetic".into(),
            json!({"modelCatalog":{"models":[{"model":"future"}]}}),
            None,
        );
        let resolved =
            resolve(group.clone(), std::slice::from_ref(&provider)).expect("later catalog");
        assert_eq!(resolved.members.len(), 1);
        assert_eq!(definition(resolved.clone()), group);
        assert_eq!(
            resolve(resolved, &[provider])
                .expect("idempotent")
                .members
                .len(),
            1
        );
    }

    #[test]
    fn switched_off_pattern_members_stay_off_after_refresh_and_storage() {
        let provider = Provider::with_id(
            "vendor".into(),
            "Synthetic".into(),
            json!({"modelCatalog":{"models":[{"model":"future"}]}}),
            None,
        );
        let mut group = super::super::tests::fixture();
        group.matches = vec!["vendor/*".into()];
        let mut resolved = resolve(group, std::slice::from_ref(&provider)).expect("catalog");
        let id = resolved.matched[0].clone();
        resolved.off.push(id.clone());
        let stored = definition(resolved);
        assert_eq!(stored.off, vec![id.clone()]);
        let restored: RoutingGroup =
            serde_json::from_value(serde_json::to_value(stored).expect("json")).expect("restore");
        assert!(restored.validate().is_ok());
        let refreshed = resolve(restored.clone(), &[provider]).expect("refresh");
        assert!(!refreshed
            .preview(None)
            .expect("plan")
            .candidates
            .iter()
            .any(|member| member.id == id));
        assert!(resolve(restored, &[]).is_ok());
    }

    #[test]
    fn a_manual_pattern_pick_survives_storage_and_returns_only_when_its_model_returns() {
        let provider = Provider::with_id(
            "vendor".into(),
            "Synthetic".into(),
            json!({"modelCatalog":{"models":["future"]}}),
            None,
        );
        let mut group = super::super::tests::fixture();
        group.matches = vec!["vendor/*".into()];
        let mut resolved = resolve(group, std::slice::from_ref(&provider)).expect("resolve");
        let chosen = resolved.matched[0].clone();
        resolved.routing = super::super::RoutingStrategy::Manual;
        resolved.manual_member_id = Some(chosen.clone());
        let stored = definition(resolved);
        assert_eq!(stored.manual_member_id.as_deref(), Some(chosen.as_str()));
        assert!(stored.validate().is_ok());
        let absent = resolve(stored.clone(), &[]).expect("missing catalog");
        assert!(absent.preview(None).is_err());
        let returned = resolve(stored, &[provider]).expect("catalog returns");
        let plan = returned.preview(None).expect("manual");
        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(plan.candidates[0].id, chosen);
    }
}
