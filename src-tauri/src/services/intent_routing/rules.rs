//! Deterministic request conditions adapted from the reference implementation (MIT).
//! Request facts are ephemeral and are never attached to traces or errors.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TimeWindow {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub days: Vec<String>,
    #[serde(default)]
    pub zone: String,
}

fn minutes(clock: &str) -> Option<u32> {
    let (hour, minute) = clock.trim().split_once(':')?;
    if !(1..=2).contains(&hour.len())
        || minute.len() != 2
        || !hour
            .bytes()
            .chain(minute.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let hour = hour.parse::<u32>().ok()?;
    let minute = minute.parse::<u32>().ok()?;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

impl TimeWindow {
    fn valid(&self) -> bool {
        let from = minutes(&self.from);
        let to = minutes(&self.to);
        let days_only = self.from.is_empty() && self.to.is_empty();
        ((days_only && !self.days.is_empty() && self.days.len() < 7)
            || (from.is_some()
                && to.is_some()
                && (from != to || (!self.days.is_empty() && self.days.len() < 7))))
            && self.days.len() <= 7
            && self.days.iter().all(|day| {
                ["mon", "tue", "wed", "thu", "fri", "sat", "sun"].contains(&day.as_str())
            })
            && self
                .days
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.days.len()
            && (self.zone.is_empty() || self.zone.parse::<chrono_tz::Tz>().is_ok())
    }

    fn holds(&self, at: chrono::DateTime<chrono::Utc>) -> bool {
        use chrono::{Datelike, Timelike};
        if !self.valid() {
            return false;
        }
        let hours = minutes(&self.from).zip(minutes(&self.to));
        let (minute, mut day) = if self.zone.is_empty() {
            let local = at.with_timezone(&chrono::Local);
            (
                local.hour() * 60 + local.minute(),
                local.weekday().num_days_from_monday(),
            )
        } else if let Ok(zone) = self.zone.parse::<chrono_tz::Tz>() {
            let local = at.with_timezone(&zone);
            (
                local.hour() * 60 + local.minute(),
                local.weekday().num_days_from_monday(),
            )
        } else {
            return false;
        };
        if let Some((from, to)) = hours {
            if from < to && !(from..to).contains(&minute) {
                return false;
            }
            if from > to {
                if minute >= to && minute < from {
                    return false;
                }
                if minute < to {
                    day = (day + 6) % 7;
                }
            }
        }
        self.days.is_empty()
            || self.days.iter().any(|value| {
                value == ["mon", "tue", "wed", "thu", "fri", "sat", "sun"][day as usize]
            })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Conditions {
    #[serde(default)]
    pub tokens: u64,
    #[serde(default)]
    pub images: bool,
    #[serde(default)]
    pub effort: String,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub compact: bool,
    #[serde(default)]
    pub time: Option<TimeWindow>,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub except_folders: Vec<String>,
}

pub(crate) struct Facts<'a> {
    pub tokens: Option<u64>,
    pub images: bool,
    pub effort: Option<&'a str>,
    pub agent: &'a str,
    pub compact: bool,
    pub at: chrono::DateTime<chrono::Utc>,
    pub folder: &'a str,
}

pub(crate) fn wire_effort(body: &serde_json::Value, format: &str) -> Option<String> {
    let path = match format {
        "openai_responses" => "/reasoning/effort",
        "openai_chat" => "/reasoning_effort",
        "anthropic" => "/output_config/effort",
        "gemini_native" => "/generationConfig/thinkingConfig/thinkingLevel",
        _ => return None,
    };
    if let Some(level) = body.pointer(path).and_then(serde_json::Value::as_str) {
        let level = level.to_ascii_lowercase();
        return rank(&level).is_some().then_some(level);
    }
    let thinking = match format {
        "anthropic" => matches!(
            body["thinking"]["type"].as_str(),
            Some("enabled" | "adaptive")
        ),
        "gemini_native" => body
            .pointer("/generationConfig/thinkingConfig/thinkingBudget")
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|budget| budget != 0),
        _ => false,
    };
    thinking.then(|| "on".into())
}

fn rank(level: &str) -> Option<usize> {
    [
        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
    ]
    .iter()
    .position(|known| *known == level)
}

impl Conditions {
    pub(crate) fn valid(&self) -> bool {
        (self.tokens > 0
            || self.images
            || !self.effort.is_empty()
            || !self.agents.is_empty()
            || self.compact
            || self.time.is_some()
            || !self.folders.is_empty()
            || !self.except_folders.is_empty())
            && self.time.as_ref().is_none_or(TimeWindow::valid)
            && [&self.folders, &self.except_folders].iter().all(|folders| {
                folders.len() <= 32
                    && folders.iter().all(|folder| super::folders::valid(folder))
                    && folders
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == folders.len()
            })
            && self.tokens <= 100_000_000
            && matches!(
                self.effort.as_str(),
                "" | "on" | "low" | "medium" | "high" | "xhigh" | "max"
            )
            && self.agents.len() <= 32
            && self.agents.iter().all(|agent| {
                [
                    "external",
                    "claude-desktop",
                    "claude",
                    "codex",
                    "pi",
                    "workbuddy",
                    "workbuddy-ai",
                    "opencode",
                    "gemini",
                    "grokbuild",
                    "hermes",
                    "openclaw",
                    "qoder",
                    "trae",
                    "doubaowork",
                    "kimi",
                    "qwen",
                    "zcode",
                    "deepseek-harness",
                    "cursor",
                    "t3code",
                ]
                .contains(&agent.as_str())
            })
            && self
                .agents
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.agents.len()
    }

    pub(crate) fn matches(&self, facts: &Facts<'_>) -> bool {
        (self.tokens == 0 || facts.tokens.is_some_and(|tokens| tokens >= self.tokens))
            && (!self.images || facts.images)
            && (!self.compact || facts.compact)
            && (self.agents.is_empty() || self.agents.iter().any(|agent| agent == facts.agent))
            && self
                .time
                .as_ref()
                .is_none_or(|window| window.holds(facts.at))
            && (self.folders.is_empty()
                || self
                    .folders
                    .iter()
                    .any(|root| super::folders::under(facts.folder, root)))
            && !self
                .except_folders
                .iter()
                .any(|root| super::folders::under(facts.folder, root))
            && match self.effort.as_str() {
                "" => true,
                "on" => facts
                    .effort
                    .is_some_and(|effort| effort != "none" && !effort.is_empty()),
                level => rank(level)
                    .zip(facts.effort.and_then(rank))
                    .is_some_and(|(required, actual)| actual >= required),
            }
    }
}

/// All matching rules contribute their targets in order, before strategy fallbacks.
pub(crate) fn classifier_candidates(group: &super::RoutingGroup, facts: &Facts<'_>) -> Vec<String> {
    group
        .rules
        .iter()
        .filter(|rule| {
            rule.enabled
                && !rule.pause
                && rule
                    .conditions
                    .as_ref()
                    .is_none_or(|conditions| conditions.matches(facts))
        })
        .take_while(|rule| !rule.description.is_empty())
        .map(|rule| rule.id.clone())
        .collect()
}

pub(crate) fn matching(
    group: &super::RoutingGroup,
    facts: &Facts<'_>,
    classified: Option<&str>,
) -> Vec<String> {
    group
        .rules
        .iter()
        .filter(|rule| {
            rule.enabled
                && !rule.pause
                && rule
                    .conditions
                    .as_ref()
                    .is_none_or(|conditions| conditions.matches(facts))
                && (rule.description.is_empty()
                    || (group.uses_classifier() && classified == Some(rule.id.as_str())))
        })
        .map(|rule| rule.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn days_only_windows_match_whole_local_days_without_inventing_hours(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut window = TimeWindow {
            from: String::new(),
            to: String::new(),
            days: vec!["mon".into()],
            zone: "Asia/Shanghai".into(),
        };
        assert!(window.valid());
        for (at, expected) in [
            ("2026-11-01T16:00:00Z", true),
            ("2026-11-02T15:59:59Z", true),
            ("2026-11-02T16:00:00Z", false),
        ] {
            assert_eq!(
                window.holds(chrono::DateTime::parse_from_rfc3339(at)?.with_timezone(&chrono::Utc)),
                expected
            );
        }
        window.to = "18:00".into();
        assert!(!window.valid());
        window.to.clear();
        window.days.clear();
        assert!(!window.valid());
        window.days = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
            .map(String::from)
            .to_vec();
        assert!(!window.valid());
        Ok(())
    }
    #[test]
    fn zoned_overnight_windows_use_the_day_the_window_began(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let conditions: Conditions = serde_json::from_value(serde_json::json!({
            "time":{"from":"22:00", "to":"08:00", "days":["fri"], "zone":"Asia/Shanghai"}
        }))?;
        assert!(conditions.valid());
        let holds = |at: &str| -> Result<bool, Box<dyn std::error::Error>> {
            Ok(conditions.matches(&Facts {
                tokens: None,
                images: false,
                effort: None,
                agent: "codex",
                compact: false,
                at: chrono::DateTime::parse_from_rfc3339(at)?.with_timezone(&chrono::Utc),
                folder: "",
            }))
        };
        assert!(holds("2026-10-30T14:00:00Z")?);
        assert!(holds("2026-10-30T23:59:00Z")?);
        assert!(!holds("2026-10-31T00:00:00Z")?);
        assert!(!holds("2026-10-31T14:00:00Z")?);
        let invalid: Conditions = serde_json::from_value(serde_json::json!({
            "time":{"from":"22:00", "to":"08:00", "zone":"Synthetic/Unknown"}
        }))?;
        assert!(!invalid.valid());
        Ok(())
    }
    #[test]
    fn pause_rules_only_accept_clock_agent_and_folder_conditions(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut group = super::super::tests::fixture();
        group.rules[0] = serde_json::from_value(serde_json::json!({
            "id":"pause", "name":"Reserve", "description":"", "enabled":true,
            "targetMemberId":group.default_member_id, "pause":true,
            "conditions":{"exceptFolders":["/synthetic/protected"]}
        }))?;
        assert!(group.validate().is_ok());
        group.rules[0]
            .conditions
            .as_mut()
            .ok_or("conditions")?
            .images = true;
        assert!(group.validate().is_err());
        Ok(())
    }
    #[test]
    fn workbuddy_editions_have_independent_agent_conditions() {
        let conditions = Conditions {
            agents: vec!["workbuddy-ai".into()],
            ..Default::default()
        };
        assert!(conditions.valid());
        let mut facts = Facts {
            tokens: None,
            images: false,
            effort: None,
            agent: "workbuddy-ai",
            compact: false,
            at: chrono::Utc::now(),
            folder: "",
        };
        assert!(conditions.matches(&facts));
        facts.agent = "workbuddy";
        assert!(!conditions.matches(&facts));
    }

    #[test]
    fn every_condition_must_hold_and_unknown_facts_never_prove_a_threshold() {
        let conditions = Conditions {
            tokens: 100,
            images: true,
            effort: "high".into(),
            agents: vec!["codex".into()],
            compact: true,
            ..Default::default()
        };
        let mut facts = Facts {
            tokens: Some(100),
            images: true,
            effort: Some("max"),
            agent: "codex",
            compact: true,
            at: chrono::Utc::now(),
            folder: "",
        };
        assert!(conditions.valid());
        assert!(conditions.matches(&facts));
        facts.tokens = None;
        assert!(!conditions.matches(&facts));
        facts.tokens = Some(100);
        facts.effort = Some("medium");
        assert!(!conditions.matches(&facts));
        facts.effort = Some("high");
        facts.agent = "claude";
        assert!(!conditions.matches(&facts));
        facts.agent = "codex";
        facts.images = false;
        assert!(!conditions.matches(&facts));
        facts.images = true;
        facts.compact = false;
        assert!(!conditions.matches(&facts));
    }
    #[test]
    fn independent_rules_keep_their_order_and_do_not_implicitly_call_a_classifier() {
        let mut group = super::super::tests::fixture();
        group.rules[0].description.clear();
        group.rules[0].conditions = Some(Conditions {
            agents: vec!["codex".into()],
            ..Default::default()
        });
        let facts = Facts {
            tokens: None,
            images: false,
            effort: None,
            agent: "codex",
            compact: false,
            at: chrono::Utc::now(),
            folder: "",
        };
        assert_eq!(matching(&group, &facts, None), ["debug"]);
        group.rules[0].description = "Synthetic intent".into();
        assert!(matching(&group, &facts, Some("debug")).is_empty());
        group.intent_recognition_enabled = true;
        assert_eq!(matching(&group, &facts, Some("debug")), ["debug"]);
    }
    #[test]
    fn an_unconditional_local_match_short_circuits_later_paid_classification(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut group = super::super::tests::fixture();
        let facts = Facts {
            tokens: Some(1000),
            images: false,
            effort: None,
            agent: "codex",
            compact: false,
            at: chrono::Utc::now(),
            folder: "",
        };
        let mut direct = group.rules[0].clone();
        direct.id = "local".into();
        direct.description.clear();
        direct.conditions = Some(Conditions {
            tokens: 100,
            ..Default::default()
        });
        group.rules.insert(0, direct);
        assert!(classifier_candidates(&group, &facts).is_empty());
        group.rules[0]
            .conditions
            .as_mut()
            .ok_or("conditions")?
            .tokens = 2000;
        assert_eq!(classifier_candidates(&group, &facts), ["debug"]);
        Ok(())
    }
    #[test]
    fn only_explicit_wire_fields_state_effort_while_native_budgets_state_no_specific_rank() {
        use serde_json::json;
        for (format, body, expected) in [
            (
                "openai_responses",
                json!({"reasoning":{"effort":"xhigh"}}),
                Some("xhigh"),
            ),
            (
                "openai_chat",
                json!({"reasoning_effort":"medium"}),
                Some("medium"),
            ),
            (
                "anthropic",
                json!({"output_config":{"effort":"max"}}),
                Some("max"),
            ),
            (
                "anthropic",
                json!({"thinking":{"type":"enabled", "budget_tokens":6000}}),
                Some("on"),
            ),
            (
                "gemini_native",
                json!({"generationConfig":{"thinkingConfig":{"thinkingLevel":"HIGH"}}}),
                Some("high"),
            ),
            (
                "gemini_native",
                json!({"generationConfig":{"thinkingConfig":{"thinkingBudget":-1}}}),
                Some("on"),
            ),
            (
                "openai_responses",
                json!({"reasoning":{"effort":"invented"}}),
                None,
            ),
        ] {
            assert_eq!(wire_effort(&body, format).as_deref(), expected);
        }
    }
}
