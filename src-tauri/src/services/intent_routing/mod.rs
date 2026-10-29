//! Codex intent-routing configuration and deterministic planning.

pub(crate) mod catalog;
pub(crate) mod classifier;
pub(crate) mod client;
mod discovery;
pub(crate) mod effort;
pub(crate) mod execution;
pub(crate) mod menu;

use crate::{app_config::AppType, provider::Provider};
pub(crate) use discovery::{discover_groups, legacy_ids};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(crate) const MAX_GROUPS: usize = 32;
const INVALID: &str = "intentRouting.invalid";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RoutingStrategy {
    Smart,
    #[default]
    Ordered,
    RoundRobin,
    LeastUsed,
    Pace,
    FreeFirst,
    Fastest,
    Weighted,
    Manual,
}

fn default_weight() -> u16 {
    1
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Target {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Member {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    #[serde(default = "default_weight")]
    pub weight: u16,
    /// Explicit override; absence uses the provider's model catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Rule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub target_member_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RoutingGroup {
    pub id: String,
    pub name: String,
    pub revision: u32,
    pub client: String,
    pub enabled: bool,
    pub members: Vec<Member>,
    pub default_member_id: String,
    pub fallback_member_ids: Vec<String>,
    pub intent_recognition_enabled: bool,
    pub classifier_target: Option<Target>,
    pub classification_budget_ms: u32,
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub auto_generated: bool,
    #[serde(default)]
    pub routing: RoutingStrategy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_member_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoutingCatalog {
    pub groups: Vec<RoutingGroup>,
    pub auto_create: bool,
    pub omitted_groups: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanPreview {
    pub group_id: String,
    pub revision: u32,
    pub candidates: Vec<Member>,
    pub reason: &'static str,
    pub live: bool,
}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= max
        && !value.chars().any(|c| c.is_control() && c != '\n')
}

fn valid_target(provider_id: &str, model_id: &str) -> bool {
    valid_text(provider_id, 128)
        && valid_text(model_id, 200)
        && model_id == model_id.trim()
        && !model_id.chars().any(char::is_control)
        && !model_id.starts_with("vary-auto/")
        && !model_id.starts_with("vary-provider/")
}

impl RoutingGroup {
    pub(crate) fn uses_classifier(&self) -> bool {
        self.intent_recognition_enabled && self.routing != RoutingStrategy::Manual
    }

    pub(crate) fn includes_in_menu(&self, member: &Member) -> bool {
        self.routing != RoutingStrategy::Manual
            || &member.id
                == self
                    .manual_member_id
                    .as_ref()
                    .unwrap_or(&self.default_member_id)
    }

    pub(crate) fn alias(&self) -> String {
        self.model_alias
            .clone()
            .unwrap_or_else(|| format!("vary-auto/{}", self.id))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_id(&self.id)
            || !valid_text(&self.name, 80)
            || self.client != "codex"
            || self.members.is_empty()
            || self.members.len() > 16
            || self.rules.len() > 16
            || self.fallback_member_ids.len() > 16
            || !(500..=5000).contains(&self.classification_budget_ms)
            || self
                .model_alias
                .as_ref()
                .is_some_and(|alias| !discovery::valid_alias(alias))
        {
            return Err(INVALID);
        }
        let mut ids = HashSet::new();
        let mut targets = HashSet::new();
        for member in &self.members {
            if !valid_id(&member.id)
                || !valid_target(&member.provider_id, &member.model_id)
                || !ids.insert(member.id.as_str())
                || !targets.insert((&member.provider_id, &member.model_id))
                || !(1..=1000).contains(&member.weight)
            {
                return Err(INVALID);
            }
        }
        if !ids.contains(self.default_member_id.as_str())
            || self
                .manual_member_id
                .as_ref()
                .is_some_and(|id| !ids.contains(id.as_str()))
            || self
                .fallback_member_ids
                .iter()
                .any(|id| !ids.contains(id.as_str()))
            || self
                .fallback_member_ids
                .iter()
                .collect::<HashSet<_>>()
                .len()
                != self.fallback_member_ids.len()
        {
            return Err(INVALID);
        }
        let mut rule_ids = HashSet::new();
        for rule in &self.rules {
            if !valid_id(&rule.id)
                || !rule_ids.insert(&rule.id)
                || !valid_text(&rule.name, 80)
                || !valid_text(&rule.description, 500)
                || !ids.contains(rule.target_member_id.as_str())
            {
                return Err(INVALID);
            }
        }
        if self
            .classifier_target
            .as_ref()
            .is_some_and(|t| !valid_target(&t.provider_id, &t.model_id))
            || (self.uses_classifier() && self.classifier_target.is_none())
        {
            return Err(INVALID);
        }
        Ok(())
    }

    /// Strict output validation is also used by the offline plan inspector.
    pub fn parse_classification(&self, output: &str) -> Result<Option<String>, &'static str> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Classification {
            // Value requires the field to be present; Option would accept omission.
            intent_id: serde_json::Value,
        }
        if output.len() > 4096 {
            return Err("invalid_output");
        }
        let parsed: Classification = serde_json::from_str(output).map_err(|_| "invalid_output")?;
        match parsed.intent_id {
            serde_json::Value::Null => Ok(None),
            serde_json::Value::String(id) if self.rules.iter().any(|r| r.enabled && r.id == id) => {
                Ok(Some(id))
            }
            _ => Err("invalid_output"),
        }
    }

    /// No network calls, configuration writes or production turn-state changes.
    pub fn preview(&self, intent_id: Option<&str>) -> Result<PlanPreview, &'static str> {
        self.validate()?;
        if self.routing == RoutingStrategy::Manual {
            let selected = self
                .manual_member_id
                .as_ref()
                .unwrap_or(&self.default_member_id);
            let member = self
                .members
                .iter()
                .find(|m| &m.id == selected)
                .ok_or(INVALID)?;
            return Ok(PlanPreview {
                group_id: self.id.clone(),
                revision: self.revision,
                candidates: vec![member.clone()],
                reason: "manual",
                live: false,
            });
        }
        let preferred = match intent_id {
            Some(id) => {
                &self
                    .rules
                    .iter()
                    .find(|r| r.enabled && r.id == id)
                    .ok_or("invalid_output")?
                    .target_member_id
            }
            None => &self.default_member_id,
        };
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        for id in std::iter::once(preferred)
            .chain(self.fallback_member_ids.iter())
            .chain(std::iter::once(&self.default_member_id))
        {
            if seen.insert(id) {
                let member = self.members.iter().find(|m| &m.id == id).ok_or(INVALID)?;
                candidates.push(member.clone());
            }
        }
        Ok(PlanPreview {
            group_id: self.id.clone(),
            revision: self.revision,
            candidates,
            reason: if intent_id.is_some() {
                "rule"
            } else {
                "default_target"
            },
            live: false,
        })
    }
}

/// A configuration eligibility check, not an inference or capability probe.
pub(crate) fn eligible_provider(provider: &Provider, classifier: bool) -> bool {
    if provider.category.as_deref() == Some("official")
        || provider.uses_managed_account_auth()
        || provider
            .meta
            .as_ref()
            .and_then(|m| m.auth_binding.as_ref())
            .is_some_and(|a| a.source == crate::provider::AuthBindingSource::ManagedAccount)
    {
        return false;
    }
    let format = provider
        .meta
        .as_ref()
        .and_then(|m| m.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("api_format")
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("apiFormat")
                .and_then(|v| v.as_str())
        });
    // Require an explicit format until the live transport adapter is integrated.
    if format
        != Some(if classifier {
            "openai_chat"
        } else {
            "openai_responses"
        })
    {
        return false;
    }
    let (endpoint, key) = provider.resolve_usage_credentials(&AppType::Codex);
    let Ok(url) = reqwest::Url::parse(&endpoint) else {
        return false;
    };
    !key.trim().is_empty()
        && !key.chars().any(char::is_control)
        && matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && !matches!(url.host_str(), Some("chatgpt.com" | "chat.openai.com"))
}

#[cfg(test)]
mod tests;
