//! Codex service-tier capabilities shared by discovery and client catalogs.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Tier {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

impl Tier {
    fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 64
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            && !self.name.is_empty()
            && self.name.len() <= 160
            && !self.name.chars().any(char::is_control)
            && self.description.len() <= 512
            && !self.description.chars().any(char::is_control)
    }
}

fn named(id: &str) -> Tier {
    let (name, description) = match id {
        "priority" => ("Fast", "Lower latency, increased usage"),
        "ultrafast" => ("Ultrafast", "Lower latency, increased usage"),
        _ => (id, ""),
    };
    Tier {
        id: id.into(),
        name: name.into(),
        description: description.into(),
    }
}

/// Missing capabilities allow the model-family fallback; explicit empty lists do not.
pub(crate) fn parse(entry: &Value) -> Option<Vec<Tier>> {
    let raw = entry
        .get("service_tiers")
        .or_else(|| entry.get("serviceTiers"));
    let (raw, legacy) = match raw {
        Some(raw) => (raw, false),
        None => (entry.get("additional_speed_tiers")?, true),
    };
    let mut tiers = Vec::new();
    for value in raw.as_array().into_iter().flatten().take(16) {
        let tier = if let Some(id) = value.as_str() {
            let id = if legacy && id == "fast" {
                "priority"
            } else {
                id
            };
            named(id)
        } else {
            let Some(id) = value.get("id").and_then(Value::as_str) else {
                continue;
            };
            let mut tier = named(id);
            if let Some(name) = value.get("name").and_then(Value::as_str) {
                tier.name = name.into();
            }
            if let Some(description) = value.get("description").and_then(Value::as_str) {
                tier.description = description.into();
            }
            tier
        };
        if tier.valid() && !tiers.iter().any(|previous: &Tier| previous.id == tier.id) {
            tiers.push(tier);
        }
    }
    Some(tiers)
}

pub(crate) fn valid(tiers: Option<&[Tier]>) -> bool {
    tiers.is_none_or(|tiers| {
        tiers.len() <= 16
            && tiers.iter().all(Tier::valid)
            && tiers
                .iter()
                .enumerate()
                .all(|(i, tier)| !tiers[..i].iter().any(|other| other.id == tier.id))
    })
}

/// Follow Magpie's GPT fallback using the real upstream ID, never a local alias.
pub(crate) fn for_model(declared: Option<&[Tier]>, model: &str) -> Vec<Tier> {
    if let Some(tiers) = declared {
        return tiers.to_vec();
    }
    let slug = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();
    if slug.starts_with("gpt-")
        || (slug.starts_with('o') && slug.as_bytes().get(1).is_some_and(u8::is_ascii_digit))
    {
        vec![named("priority")]
    } else {
        Vec::new()
    }
}

pub(crate) fn advertise(entry: &mut Value, tiers: &[Tier]) {
    entry["service_tiers"] = json!(tiers);
    entry["additional_speed_tiers"] = json!(tiers
        .iter()
        .filter_map(|tier| match tier.id.as_str() {
            "priority" => Some("fast"),
            "ultrafast" => Some("ultrafast"),
            _ => None,
        })
        .collect::<Vec<_>>());
}

#[cfg(test)]
mod tests {
    use crate::services::intent_routing::effort::Capability;
    use serde_json::json;

    #[test]
    fn speed_tiers_survive_discovery_and_saved_declarations() {
        let tiers = json!([
            {"id":"priority","name":"Fast","description":"Higher usage"},
            {"id":"ultrafast","name":"Ultrafast","description":"Highest usage"}
        ]);
        let capability = Capability::parse(&json!({
            "supported_reasoning_levels":["low","high"],
            "service_tiers":tiers,
            "private_extension":"must not be retained"
        }));
        let saved = capability.declaration();
        assert_eq!(saved["service_tiers"], tiers);
        assert_eq!(Capability::parse(&saved), capability);
        assert!(saved.get("private_extension").is_none());
    }

    #[test]
    fn speed_tiers_preserve_an_explicit_empty_declaration() {
        let saved = Capability::parse(&json!({"service_tiers":[]})).declaration();
        assert_eq!(saved["service_tiers"], json!([]));
        assert!(Capability::parse(&json!({}))
            .declaration()
            .get("service_tiers")
            .is_none());
    }
    #[test]
    fn speed_tiers_survive_the_model_discovery_transport() {
        let tiers = json!([{"id":"priority","name":"Fast","description":"Higher usage"}]);
        let model = crate::services::model_fetch::FetchedModel::from_entry(
            "synthetic-model".into(),
            None,
            &json!({"service_tiers":tiers}),
        );
        assert_eq!(model.capability_entry()["service_tiers"], tiers);
    }

    fn session() -> crate::proxy::intent::Session {
        use crate::proxy::intent::{Session, Target};
        let group = super::super::tests::fixture();
        let targets = group
            .members
            .iter()
            .map(|member| Target {
                provider_id: member.provider_id.clone(),
                model: member.model_id.clone(),
                fixed_effort: member.effort.clone(),
                upstream: None,
                effort: Capability::parse(&json!({"service_tiers":[
                    {"id":"priority","name":"Fast","description":"Higher usage"}
                ]})),
                url: "http://127.0.0.1:1/responses"
                    .parse()
                    .expect("synthetic URL"),
                headers: Default::default(),
                official: false,
                credentials: None,
                scheduling: std::sync::Arc::default(),
            })
            .collect();
        Session {
            group,
            targets,
            classifier: None,
            client: reqwest::Client::new(),
            created: std::time::Instant::now(),
            bindings: std::sync::Arc::default(),
        }
    }

    #[test]
    fn speed_tiers_are_shared_by_all_selectable_group_members() {
        let mut group = session();
        assert_eq!(
            group.model_entry("synthetic-group")["service_tiers"][0]["id"],
            "priority"
        );
        group.targets[1].effort = Capability::parse(&json!({"service_tiers":[]}));
        assert_eq!(
            group.model_entry("synthetic-group")["service_tiers"],
            json!([])
        );
        // Hidden/off members must not suppress the active member's capabilities.
        group.group.off = vec![group.group.members[1].id.clone()];
        assert_eq!(
            group.model_entry("synthetic-group")["service_tiers"][0]["id"],
            "priority"
        );
    }

    #[test]
    fn speed_tiers_follow_real_gpt_models_not_the_published_alias() {
        let mut group = session();
        group.group.members.truncate(1);
        group.targets.truncate(1);
        group.group.members[0].model_id = "relay/gpt-6.1-sol-2026-11-01".into();
        group.targets[0].model = group.group.members[0].model_id.clone();
        group.targets[0].effort = Capability::parse(&json!({}));
        assert_eq!(
            group.model_entry("vary-provider/opaque-id")["service_tiers"][0]["id"],
            "priority"
        );
        group.targets[0].effort = Capability::parse(&json!({"service_tiers":[]}));
        assert_eq!(
            group.model_entry("vary-provider/opaque-id")["service_tiers"],
            json!([])
        );
        group.targets[0].model = "synthetic-non-gpt".into();
        group.group.members[0].model_id = group.targets[0].model.clone();
        group.targets[0].effort = Capability::parse(&json!({}));
        assert_eq!(
            group.model_entry("gpt-looking-alias")["service_tiers"],
            json!([])
        );
    }

    #[test]
    fn speed_tiers_are_not_blocked_by_a_reasoning_override() {
        let provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Fixture".into(),
            json!({
                "modelCatalog":{"models":[{"model":"synthetic-model","reasoningLevels":["high"]}]},
                "routingModelCapabilities":{"synthetic-model":{"service_tiers":[
                    {"id":"priority","name":"Fast","description":"Higher usage"}
                ]}}
            }),
            None,
        );
        let saved = Capability::for_provider(&provider, "synthetic-model").declaration();
        assert_eq!(saved["service_tiers"][0]["id"], "priority");
        assert_eq!(saved["reasoningLevels"], json!(["high"]));
    }

    #[test]
    fn speed_tiers_do_not_retain_unknown_fields_or_malformed_ids() {
        let saved = Capability::parse(&json!({"service_tiers":[
            {"id":"priority","name":"Fast","description":"Higher usage","private_extension":"not-persisted"},
            {"id":"synthetic\ninvalid","name":"Invalid"},
            {"id":""},
            {"id":"priority","name":"Duplicate"}
        ]})).declaration();
        assert_eq!(
            saved["service_tiers"],
            json!([
                {"id":"priority","name":"Fast","description":"Higher usage"}
            ])
        );
    }
    #[test]
    fn speed_tiers_accept_legacy_speed_metadata_without_overriding_explicit_tiers() {
        let saved = Capability::parse(&json!({"additional_speed_tiers":["fast"]})).declaration();
        assert_eq!(saved["service_tiers"][0]["id"], "priority");
        let provider = crate::provider::Provider::with_id(
            "fixture".into(),
            "Fixture".into(),
            json!({
                "modelCatalog":{"models":[{"model":"gpt-explicitly-disabled","serviceTiers":[]}]},
                "routingPublicCapabilities":{"gpt-explicitly-disabled":{"service_tiers":["priority"]}}
            }),
            None,
        );
        assert_eq!(
            Capability::for_provider(&provider, "gpt-explicitly-disabled").declaration()
                ["service_tiers"],
            json!([])
        );
    }
}
