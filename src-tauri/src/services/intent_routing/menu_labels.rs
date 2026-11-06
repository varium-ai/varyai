//! Short source labels for personal accounts in connected agents' model menus.

use crate::provider::Provider;
use std::collections::{BTreeMap, HashMap, HashSet};

fn personal_source(provider: &Provider) -> Option<&'static str> {
    if crate::services::codex_provider::is_codex(provider) {
        return Some("Codex");
    }
    if provider.is_github_copilot() {
        return Some("GitHub Copilot");
    }
    if provider.is_xai_oauth() {
        return Some("Grok");
    }
    let binding = crate::services::subscription_provider::binding(provider)?;
    let service = serde_json::to_value(binding.provider).ok()?;
    Some(match service.as_str()? {
        "github_copilot" => "GitHub Copilot",
        "xai_oauth" => "Grok",
        "claude" => "Claude",
        "gemini_cli" => "Gemini CLI",
        "antigravity" => "Antigravity",
        _ => "ChatGPT API",
    })
}

/// Like Magpie's picker labels, identity stays separate from presentation.
/// Only visible sources participate; stable IDs disambiguate without showing emails.
pub(super) fn provider_names<'a>(
    providers: impl Iterator<Item = &'a Provider>,
) -> HashMap<String, String> {
    let mut groups: BTreeMap<String, Vec<(&Provider, String, bool)>> = BTreeMap::new();
    for provider in providers {
        let (name, abbreviated) = match personal_source(provider) {
            Some(source)
                if provider.name.contains('@')
                    || provider.name == source
                    || provider.name.starts_with(&format!("{source} · ")) =>
            {
                (source.to_owned(), true)
            }
            _ => (provider.name.clone(), false),
        };
        groups
            .entry(name.to_lowercase())
            .or_default()
            .push((provider, name, abbreviated));
    }
    let mut used = groups.keys().cloned().collect::<HashSet<_>>();
    let mut names = HashMap::new();
    for mut group in groups.into_values() {
        group.sort_by_key(|(provider, _, _)| &provider.id);
        let multiple = group.len() > 1;
        let mut number = 0;
        for (provider, name, abbreviated) in group {
            let label = if multiple && abbreviated {
                loop {
                    number += 1;
                    let candidate = format!("{name} {number}");
                    if used.insert(candidate.to_lowercase()) {
                        break candidate;
                    }
                }
            } else {
                name
            };
            names.insert(provider.id.clone(), label);
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn personal(id: &str) -> crate::provider::Provider {
        let mut provider = crate::provider::Provider::with_id(
            id.into(),
            "Codex · synthetic-personal-account@example.test".into(),
            json!({}),
            None,
        );
        provider.category = Some("official".into());
        provider
    }

    #[test]
    fn personal_model_labels_omit_the_account_identity() {
        let provider = personal("fixture");
        let names = provider_names(std::iter::once(&provider));
        assert_eq!(names["fixture"], "Codex");
        assert_eq!(
            provider.name,
            "Codex · synthetic-personal-account@example.test"
        );
        let mut renamed = provider.clone();
        renamed.name = "Personal".into();
        assert_eq!(
            provider_names(std::iter::once(&renamed))["fixture"],
            "Personal"
        );
    }

    #[test]
    fn multiple_personal_sources_have_short_stable_numbers() {
        let a = personal("a");
        let b = personal("b");
        let names = provider_names([&b, &a].into_iter());
        assert_eq!(names["a"], "Codex 1");
        assert_eq!(names["b"], "Codex 2");
        assert_eq!(names, provider_names([&a, &b].into_iter()));
    }

    #[test]
    fn custom_relays_keep_their_user_defined_source_names() {
        let provider =
            crate::provider::Provider::with_id("relay".into(), "sub2api".into(), json!({}), None);
        assert_eq!(
            provider_names(std::iter::once(&provider))["relay"],
            "sub2api"
        );
        let subscription = crate::provider::Provider::with_id(
            "subscription".into(),
            "Gemini CLI · synthetic-personal-account@example.test".into(),
            json!({"routingSubscription":{"provider":"gemini_cli","accountId":"fixture"}}),
            None,
        );
        assert_eq!(
            provider_names(std::iter::once(&subscription))["subscription"],
            "Gemini CLI"
        );
    }
    #[test]
    fn numbered_personal_sources_do_not_collide_with_custom_source_names() {
        let a = personal("a");
        let b = personal("b");
        let relay =
            crate::provider::Provider::with_id("relay".into(), "Codex 1".into(), json!({}), None);
        let names = provider_names([&a, &relay, &b].into_iter());
        assert_eq!(names["relay"], "Codex 1");
        assert_eq!(names.values().collect::<HashSet<_>>().len(), 3);
        assert_eq!(names, provider_names([&b, &a, &relay].into_iter()));
    }
    #[test]
    fn duplicate_custom_relay_names_keep_existing_agent_disambiguation() {
        let first = crate::provider::Provider::with_id(
            "first".into(),
            "Synthetic supplier".into(),
            json!({}),
            None,
        );
        let second = crate::provider::Provider::with_id(
            "second".into(),
            first.name.clone(),
            json!({}),
            None,
        );
        let names = provider_names([&first, &second].into_iter());
        assert_eq!(names["first"], "Synthetic supplier");
        assert_eq!(names["second"], "Synthetic supplier");
    }
}
