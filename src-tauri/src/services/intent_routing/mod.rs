//! Global routing-group configuration and deterministic planning.

pub(crate) mod catalog;
pub(crate) mod classifier;
pub(crate) mod client;
pub(crate) mod context;
mod discovery;
pub(crate) mod effort;
pub(crate) mod execution;
pub(crate) mod folders;
pub(crate) mod membership;
pub(crate) mod menu;
pub(crate) mod model_context;
pub(crate) mod model_preferences;
pub(crate) mod rules;
pub(crate) mod speed;

use crate::{app_config::AppType, provider::Provider};
pub(crate) use discovery::{discover_groups, discover_stand_ins, legacy_ids};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(crate) const MAX_GROUPS: usize = 32;
const INVALID: &str = "intentRouting.invalid";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RoutingStrategy {
    Smart,
    #[default]
    #[serde(alias = "expiring")]
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
fn global_scope() -> String {
    "global".into()
}
fn legacy_patience() -> i16 {
    -1
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Rule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub target_member_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_member_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditions: Option<rules::Conditions>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub use_effort: String,
    #[serde(default)]
    pub pause: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RoutingGroup {
    pub id: String,
    pub name: String,
    pub revision: u32,
    /// Legacy clients remain readable; new definitions have no owning agent.
    #[serde(default = "global_scope")]
    pub client: String,
    pub enabled: bool,
    pub members: Vec<Member>,
    pub default_member_id: String,
    pub fallback_member_ids: Vec<String>,
    pub intent_recognition_enabled: bool,
    pub classifier_target: Option<Target>,
    pub classification_budget_ms: u32,
    pub rules: Vec<Rule>,
    // Read experimental metadata without keeping it in saved or exported groups.
    #[serde(default, rename = "policy", skip_serializing)]
    pub(super) retired_policy: Option<serde_json::Value>,
    #[serde(default)]
    pub auto_generated: bool,
    #[serde(default)]
    pub routing: RoutingStrategy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_member_id: Option<String>,
    /// None/zero follows the largest member; -1 follows the smallest known member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<i64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub affinity: String,
    #[serde(default)]
    pub first_token: u32,
    #[serde(default = "legacy_patience")]
    pub patience: i16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<String>,
    #[serde(default, rename = "match", skip_serializing_if = "Vec::is_empty")]
    pub matches: Vec<String>,
    #[serde(skip)]
    pub matched: Vec<String>,
    #[serde(default)]
    pub unlists: bool,
    #[serde(default)]
    pub sink: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoutingCatalog {
    pub groups: Vec<RoutingGroup>,
    pub removed_groups: Vec<RemovedGroup>,
    #[serde(skip)]
    pub stand_ins: Vec<RoutingGroup>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub matched_members: std::collections::BTreeMap<String, Vec<Member>>,
    pub auto_create: bool,
    pub omitted_groups: usize,
    pub context_settings: context::Settings,
}

#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemovedGroup {
    pub id: String,
    pub name: String,
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

pub(crate) fn valid_text(value: &str, max: usize) -> bool {
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
        && !model_id.starts_with("auto/")
}

fn matched_id(value: &str) -> bool {
    value.strip_prefix("match-").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

impl RoutingGroup {
    pub(crate) fn uses_classifier(&self) -> bool {
        self.intent_recognition_enabled && self.routing != RoutingStrategy::Manual
    }

    pub(crate) fn includes_in_menu(&self, member: &Member) -> bool {
        !self.off.contains(&member.id)
            && (self.routing != RoutingStrategy::Manual
                || &member.id
                    == self
                        .manual_member_id
                        .as_ref()
                        .unwrap_or(&self.default_member_id))
    }

    pub(crate) fn alias(&self) -> String {
        self.model_alias
            .clone()
            .unwrap_or_else(|| format!("vary-auto/{}", self.id))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_id(&self.id)
            || !valid_text(&self.name, 80)
            || !matches!(self.client.as_str(), "codex" | "global")
            || (self.members.is_empty() && self.matches.is_empty())
            || self.members.len() > 256
            || self.matches.len() > 32
            || self
                .matches
                .iter()
                .any(|pattern| membership::compile(pattern).is_err())
            || self.rules.len() > 16
            || self.fallback_member_ids.len() > 256
            || !(500..=5000).contains(&self.classification_budget_ms)
            || !context::Metadata::valid_group_window(self.context_window)
            || !matches!(
                self.affinity.as_str(),
                "" | "session" | "turn" | "off" | "strict"
            )
            || self.first_token > 3600
            || !effort::valid_levels(&self.levels)
            || !(-1..=240).contains(&self.patience)
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
                || !targets.insert((&member.provider_id, &member.model_id, &member.effort))
                || member
                    .effort
                    .as_ref()
                    .is_some_and(|level| !effort::valid_member_level(level))
                || !(1..=1000).contains(&member.weight)
            {
                return Err(INVALID);
            }
        }
        if (!ids.contains(self.default_member_id.as_str())
            && !(self.members.is_empty()
                && !self.matches.is_empty()
                && self.default_member_id.is_empty()))
            || self.off.len() > 256
            || self.off.iter().any(|id| {
                !ids.contains(id.as_str()) && (self.matches.is_empty() || !matched_id(id))
            })
            || self.off.iter().collect::<HashSet<_>>().len() != self.off.len()
            || self.manual_member_id.as_ref().is_some_and(|id| {
                !ids.contains(id.as_str()) && (self.matches.is_empty() || !matched_id(id))
            })
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
                || (!rule.description.is_empty() && !valid_text(&rule.description, 500))
                || (rule.description.is_empty() && rule.conditions.is_none())
                || rule
                    .conditions
                    .as_ref()
                    .is_some_and(|conditions| !conditions.valid())
                || (!rule.use_effort.is_empty()
                    && !matches!(
                        rule.use_effort.as_str(),
                        "low" | "medium" | "high" | "xhigh" | "max"
                    ))
                || !ids.contains(rule.target_member_id.as_str())
                || (rule.pause
                    && (!rule.description.is_empty()
                        || !rule.use_effort.is_empty()
                        || !rule.fallback_member_ids.is_empty()
                        || rule.conditions.as_ref().is_none_or(|conditions| {
                            conditions.tokens > 0
                                || conditions.images
                                || !conditions.effort.is_empty()
                                || conditions.compact
                        })))
                || rule.fallback_member_ids.len() > 256
                || rule
                    .fallback_member_ids
                    .iter()
                    .any(|id| !ids.contains(id.as_str()) || id == &rule.target_member_id)
                || rule
                    .fallback_member_ids
                    .iter()
                    .collect::<HashSet<_>>()
                    .len()
                    != rule.fallback_member_ids.len()
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
        self.plan_prepared(intent_id)
    }

    /// Execution snapshots are validated before publication, not on each request.
    pub(crate) fn plan_prepared(
        &self,
        intent_id: Option<&str>,
    ) -> Result<PlanPreview, &'static str> {
        if self.routing == RoutingStrategy::Manual {
            let selected = self
                .manual_member_id
                .as_ref()
                .unwrap_or(&self.default_member_id);
            let member = self
                .members
                .iter()
                .find(|m| &m.id == selected && !self.off.contains(&m.id))
                .ok_or(INVALID)?;
            return Ok(PlanPreview {
                group_id: self.id.clone(),
                revision: self.revision,
                candidates: vec![member.clone()],
                reason: "manual",
                live: false,
            });
        }
        let rule = match intent_id {
            Some(id) => Some(
                self.rules
                    .iter()
                    .find(|r| r.enabled && r.id == id)
                    .ok_or("invalid_output")?,
            ),
            None => None,
        };
        let preferred = rule.map_or(&self.default_member_id, |rule| &rule.target_member_id);
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        for id in std::iter::once(preferred)
            .chain(
                rule.into_iter()
                    .flat_map(|rule| rule.fallback_member_ids.iter()),
            )
            .chain(self.fallback_member_ids.iter())
            .chain(std::iter::once(&self.default_member_id))
            .chain(self.members.iter().map(|member| &member.id))
        {
            if !self.off.contains(id) && seen.insert(id) {
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
    if crate::services::subscription_provider::binding(provider).is_some() {
        return !classifier;
    }
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
    if if classifier {
        format != Some("openai_chat")
    } else {
        !matches!(
            format,
            Some("openai_chat" | "openai_responses" | "anthropic" | "gemini_native")
        )
    } {
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
