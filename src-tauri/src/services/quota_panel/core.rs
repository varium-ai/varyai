//! Bounded observations and explicit account spending preferences.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CreditsMode {
    #[default]
    On,
    Off,
    First,
    Last,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Policy {
    pub week: bool,
    pub expiring: bool,
    pub credits: CreditsMode,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Point {
    pub at: i64,
    pub remaining: f64,
    pub resets_at: i64,
    pub span: i64,
}
impl Point {
    pub fn valid(&self, now: i64) -> bool {
        self.remaining.is_finite()
            && (0.0..=100.0).contains(&self.remaining)
            && (60..=366 * 86400).contains(&self.span)
            && self.at <= now + 60
            && self.at >= now - 366 * 86400
            && self.resets_at > self.at
            && self.resets_at <= self.at + self.span + 86400
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResetCard {
    pub id: String,
    pub until: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pending {
    pub credit: String,
    pub request: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    pub account: String,
    pub policy: Policy,
    pub pending: Option<Pending>,
    pub week_used: Option<i64>,
    #[serde(default)]
    pub last_spent: i64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Store {
    pub entries: BTreeMap<String, Entry>,
    pub histories: BTreeMap<String, Vec<Point>>,
}
impl Store {
    pub fn reserve(
        &mut self,
        identity: &str,
        account: &str,
        requested: Pending,
        automatic: bool,
        week: bool,
        now: i64,
    ) -> Result<Pending, String> {
        let Pending { credit, request } = requested;
        let entry = self.entries.entry(identity.to_owned()).or_default();
        entry.account = account.to_owned();
        if let Some(pending) = entry.pending.as_ref() {
            return if !automatic && pending.credit == credit {
                Ok(pending.clone())
            } else {
                Err("quotaPanel.pending".into())
            };
        }
        if automatic && now.saturating_sub(entry.last_spent) < 1800 {
            return Err("quotaPanel.pending".into());
        }
        if automatic && !week && !entry.policy.expiring {
            return Err("quotaPanel.failed".into());
        }
        if week && !week_due(&entry.policy, 100.0, now + 1, now, entry.week_used, now) {
            return Err("quotaPanel.failed".into());
        }
        if week {
            entry.week_used = Some(now);
        }
        let pending = Pending {
            credit: credit.to_owned(),
            request,
        };
        entry.pending = Some(pending.clone());
        Ok(pending)
    }
    pub fn valid(&self) -> bool {
        self.entries.len() <= 64
            && self.histories.len() <= 64
            && self.entries.iter().all(|(key, value)| {
                valid_key(key) && !value.account.is_empty() && value.account.len() <= 256
            })
            && self.histories.iter().all(|(key, values)| {
                valid_key(key)
                    && values.len() <= 512
                    && values.iter().all(|p| {
                        p.remaining.is_finite()
                            && (0.0..=100.0).contains(&p.remaining)
                            && (60..=366 * 86400).contains(&p.span)
                            && p.at >= 0
                            && p.resets_at > p.at
                            && p.resets_at <= p.at.saturating_add(p.span).saturating_add(86400)
                    })
            })
    }
    pub fn note(&mut self, key: String, point: Point, now: i64) {
        if !point.valid(now) {
            return;
        }
        if !self.histories.contains_key(&key) && self.histories.len() >= 64 {
            if let Some(oldest) = self
                .histories
                .iter()
                .min_by_key(|(_, points)| points.last().map_or(0, |p| p.at))
                .map(|(key, _)| key.clone())
            {
                self.histories.remove(&oldest);
            }
        }
        let points = self.histories.entry(key).or_default();
        points.retain(|p| p.valid(now) && now - p.at <= 31 * 86400);
        if points.last().is_some_and(|p| p.at >= point.at) {
            return;
        }
        let bucket = (point.span / 384).max(60);
        if points
            .last()
            .is_some_and(|p| point.at / bucket == p.at / bucket && p.resets_at == point.resets_at)
        {
            points.pop();
        }
        points.push(point);
        if points.len() > 512 {
            points.drain(..points.len() - 512);
        }
    }
}
fn valid_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
}
pub(crate) fn window_seconds(name: &str) -> Option<i64> {
    match name {
        "five_hour" => Some(18000),
        "seven_day" | "weekly" => Some(604800),
        "thirty_day" => Some(2592000),
        value => {
            let (count, unit) = value.split_once('_')?;
            let multiplier = match unit {
                "day" => 86400,
                "hour" => 3600,
                _ => return None,
            };
            count
                .parse::<i64>()
                .ok()?
                .checked_mul(multiplier)
                .filter(|v| (60..=366 * 86400).contains(v))
        }
    }
}

/// A five-hour limit alone never consumes a weekly reset.
pub(crate) fn week_due(
    policy: &Policy,
    used: f64,
    reset: i64,
    sampled: i64,
    last: Option<i64>,
    now: i64,
) -> bool {
    policy.week
        && used.is_finite()
        && used >= 100.0
        && reset > now
        && (0..=300).contains(&now.saturating_sub(sampled))
        && last.is_none_or(|at| now.saturating_sub(at) >= 7 * 86400)
}
pub(crate) fn expiry_due(policy: &Policy, cards: &[ResetCard], used: bool, now: i64) -> bool {
    policy.expiring
        && used
        && cards
            .iter()
            .any(|c| c.until.is_some_and(|at| at > now && at <= now + 1800))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policies_are_opt_in_and_credit_modes_are_exclusive() {
        let p = Policy::default();
        assert!(!p.week && !p.expiring && p.credits == CreditsMode::On);
        assert!(serde_json::from_str::<CreditsMode>("\"unsupported\"").is_err());
    }
    #[test]
    fn history_is_bounded_and_does_not_invent_points_or_merge_reset_cycles() {
        let mut store = Store::default();
        let key = "a".repeat(64);
        for at in 1000..3000 {
            store.note(
                key.clone(),
                Point {
                    at,
                    remaining: 71.0,
                    resets_at: 9000,
                    span: 18000,
                },
                at,
            );
        }
        let count = store.histories[&key].len();
        assert!((30..=36).contains(&count));
        store.note(
            key.clone(),
            Point {
                at: 4000,
                remaining: f64::NAN,
                resets_at: 9000,
                span: 18000,
            },
            4000,
        );
        assert_eq!(store.histories[&key].len(), count);
        store.note(
            key.clone(),
            Point {
                at: 3001,
                remaining: 100.0,
                resets_at: 12000,
                span: 18000,
            },
            3001,
        );
        assert_eq!(store.histories[&key].len(), count + 1);
    }
    #[test]
    fn automatic_spending_requires_fresh_exhaustion_or_used_expiring_allowance() {
        let policy = Policy {
            week: true,
            expiring: true,
            ..Default::default()
        };
        assert!(week_due(&policy, 100.0, 9000, 1000, None, 1001));
        assert!(!week_due(&policy, 100.0, 9000, 1000, Some(9000), 1001));
        assert!(!week_due(&policy, 100.0, 9000, 1000, None, 1500));
        assert!(!week_due(&policy, 99.0, 9000, 1000, None, 1001));
        let cards = vec![ResetCard {
            id: "synthetic".into(),
            until: Some(1800),
        }];
        assert!(expiry_due(&policy, &cards, true, 1000));
        assert!(!expiry_due(&policy, &cards, false, 1000));
        assert!(!expiry_due(&Policy::default(), &cards, true, 1000));
    }
    #[test]
    fn ambiguous_spends_reuse_one_request_and_never_choose_a_different_credit() {
        let mut store = Store::default();
        let identity = "a".repeat(64);
        let first = store
            .reserve(
                &identity,
                "synthetic",
                Pending {
                    credit: "credit-one".into(),
                    request: "request-one".into(),
                },
                false,
                false,
                10000,
            )
            .unwrap();
        let retry = store
            .reserve(
                &identity,
                "synthetic",
                Pending {
                    credit: "credit-one".into(),
                    request: "request-two".into(),
                },
                false,
                false,
                10001,
            )
            .unwrap();
        assert_eq!(first.request, retry.request);
        assert!(store
            .reserve(
                &identity,
                "synthetic",
                Pending {
                    credit: "credit-two".into(),
                    request: "request-three".into()
                },
                false,
                false,
                10002
            )
            .is_err());
        assert!(store
            .reserve(
                &identity,
                "synthetic",
                Pending {
                    credit: "credit-one".into(),
                    request: "request-four".into()
                },
                true,
                false,
                10003
            )
            .is_err());
    }
    #[test]
    fn automatic_reservations_require_the_current_saved_opt_in() {
        let mut store = Store::default();
        let identity = "a".repeat(64);
        assert!(store
            .reserve(
                &identity,
                "synthetic",
                Pending {
                    credit: "credit".into(),
                    request: "request".into()
                },
                true,
                false,
                10000
            )
            .is_err());
        assert!(store.entries[&identity].pending.is_none());
    }
}
