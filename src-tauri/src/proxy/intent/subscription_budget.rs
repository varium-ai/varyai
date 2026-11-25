//! Quota eligibility for the expiry template. Observations never block forwarding.

use super::{scheduling::Health, Session, Target};
use crate::services::{
    routing_upstreams::SubscriptionProvider,
    subscription::{
        QuotaTier, TIER_FIVE_HOUR, TIER_SEVEN_DAY, TIER_SEVEN_DAY_FABLE, TIER_SEVEN_DAY_OPUS,
        TIER_SEVEN_DAY_SONNET,
    },
    subscription_provider,
};
use std::time::Duration;
use tokio::sync::Semaphore;

const FIVE_HOURS: i64 = 5 * 3600;
const WEEK: i64 = 7 * 24 * 3600;
static READS: Semaphore = Semaphore::const_new(8);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Budget {
    Unknown,
    Pace(f64),
    Exhausted,
}

pub(super) fn evaluate(
    windows: impl IntoIterator<Item = (bool, f64, Option<i64>)>,
    allow_short_only: bool,
    now: i64,
) -> Budget {
    let mut weekly = None::<f64>;
    let mut short = None::<f64>;
    let mut has_week = false;
    let mut incomplete_week = false;
    for (week, used, reset) in windows {
        has_week |= week;
        if reset.is_some_and(|at| at <= now) {
            continue;
        }
        let Some(remaining_seconds) = reset
            .map(|at| at.saturating_sub(now))
            .filter(|_| used.is_finite() && used >= 0.0)
        else {
            incomplete_week |= week;
            continue;
        };
        if used >= 100.0 {
            return Budget::Exhausted;
        }
        let pace = (100.0 - used) / (remaining_seconds as f64 / 3600.0).max(1.0);
        let slot = if week { &mut weekly } else { &mut short };
        *slot = Some(slot.map_or(pace, |previous| previous.min(pace)));
    }
    let pace = if has_week {
        weekly.filter(|_| !incomplete_week)
    } else if allow_short_only {
        short
    } else {
        None
    };
    pace.map_or(Budget::Unknown, Budget::Pace)
}

pub(super) fn codex_window(seconds: i64) -> Option<bool> {
    match seconds {
        WEEK => Some(true),
        FIVE_HOURS => Some(false),
        _ => None,
    }
}

pub(super) fn claude_budget(
    tiers: &[QuotaTier],
    short_only: bool,
    model: &str,
    now: i64,
) -> Budget {
    let family = |name: &str| {
        model.eq_ignore_ascii_case(name)
            || (model.starts_with("claude-")
                && model.split('-').any(|part| part.eq_ignore_ascii_case(name)))
    };
    evaluate(
        tiers.iter().filter_map(|tier| {
            let week = match tier.name.as_str() {
                TIER_FIVE_HOUR => false,
                TIER_SEVEN_DAY => true,
                TIER_SEVEN_DAY_FABLE if family("fable") => true,
                TIER_SEVEN_DAY_OPUS if family("opus") => true,
                TIER_SEVEN_DAY_SONNET if family("sonnet") => true,
                _ => return None,
            };
            Some((
                week,
                tier.utilization,
                tier.resets_at
                    .as_deref()
                    .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                    .map(|time| time.timestamp()),
            ))
        }),
        short_only,
        now,
    )
}

fn budget(target: &Target, health: &Health, client: &reqwest::Client, now: i64) -> Budget {
    let binding = target
        .upstream
        .as_ref()
        .and_then(subscription_provider::binding);
    if target.official && target.effort.free != Some(true) {
        if let Some(credentials) = target.credentials.as_ref() {
            refresh(target, now, || {
                let credentials = credentials.clone();
                let scheduling = target.scheduling.clone();
                let client = client.clone();
                let mut headers = target.headers.clone();
                async move {
                    if let Some(value) = credentials.token().await.and_then(|token| {
                        http::HeaderValue::from_str(&format!("Bearer {token}")).ok()
                    }) {
                        headers.insert(http::header::AUTHORIZATION, value);
                        scheduling.prime(&client, &headers).await;
                    }
                }
            });
        }
        health.codex_budget(now)
    } else if let Some(binding) = binding
        .as_ref()
        .filter(|b| b.provider == SubscriptionProvider::Claude)
    {
        refresh(target, now, || {
            let scheduling = target.scheduling.clone();
            let account = binding.account_id.clone();
            async move {
                if let Ok(quota) =
                    crate::services::native_subscription::claude_routing_quota(&account).await
                {
                    scheduling.observe_claude_quota(quota, chrono::Utc::now().timestamp());
                }
            }
        });
        health.claude_budget(&target.model, now)
    } else {
        Budget::Unknown
    }
}

fn refresh<F>(target: &Target, now: i64, start: impl FnOnce() -> F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let Ok(permit) = READS.try_acquire() else {
        return;
    };
    if !target.scheduling.claim_budget_refresh(now) {
        return;
    }
    let work = start();
    tokio::spawn(async move {
        let _permit = permit;
        let _ = tokio::time::timeout(Duration::from_secs(30), work).await;
    });
}

/// Remove accounts with a known exhausted window in the saved candidate order.
pub(super) fn filter_exhausted(
    session: &Session,
    candidates: &mut Vec<usize>,
    states: &[Health],
    now: i64,
) -> bool {
    let before = candidates.len();
    candidates.retain(|&index| {
        session
            .targets
            .get(index)
            .zip(states.get(index))
            .is_some_and(|(target, health)| {
                budget(target, health, &session.client, now) != Budget::Exhausted
            })
    });
    candidates.len() < before
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_pace_ignores_short_reset_until_a_window_is_full() {
        let now = 1_000;
        let long = (true, 40.0, Some(now + 6 * 3600));
        let other = (true, 80.0, Some(now + 4 * 3600));
        let short = (false, 1.0, Some(now + 60));
        assert_eq!(evaluate([short, long], false, now), Budget::Pace(10.0));
        assert_eq!(evaluate([short, other], false, now), Budget::Pace(5.0));
        let full = (false, 100.0, Some(now + 60));
        assert_eq!(evaluate([full, long], false, now), Budget::Exhausted);
        assert_eq!(
            evaluate([full, long], false, now + 60),
            Budget::Pace(60.0 / (21_540.0 / 3600.0))
        );
    }

    #[test]
    fn short_window_requires_confirmed_absence_of_a_weekly_budget() {
        let now = 1_000;
        let short = (false, 40.0, Some(now + 2 * 3600));
        assert_eq!(evaluate([short], false, now), Budget::Unknown);
        assert_eq!(evaluate([short], true, now), Budget::Pace(30.0));
        assert_eq!(
            evaluate([short, (true, 40.0, None)], true, now),
            Budget::Unknown
        );
        assert_eq!(
            evaluate([(false, -1.0, Some(now + 60))], true, now),
            Budget::Unknown
        );
    }

    #[test]
    fn claude_uses_only_windows_that_apply_to_the_exact_model_family() {
        let now = 1_000;
        let expiry = |hours: i64| {
            chrono::DateTime::from_timestamp(now + hours * 3600, 0).map(|time| time.to_rfc3339())
        };
        let tier = |name: &str, utilization, hours| QuotaTier {
            name: name.into(),
            utilization,
            resets_at: expiry(hours),
            used_value_usd: None,
            max_value_usd: None,
        };
        let tiers = [
            tier("five_hour", 50.0, 1),
            tier("seven_day", 40.0, 10),
            tier("seven_day_opus", 90.0, 5),
            tier("seven_day_fable", 100.0, 1),
        ];
        assert_eq!(
            claude_budget(&tiers, false, "claude-opus-4-7", now),
            Budget::Pace(2.0)
        );
        assert_eq!(
            claude_budget(&tiers, false, "claude-3-sonnet-20240229", now),
            Budget::Pace(6.0)
        );
        assert_eq!(
            claude_budget(&tiers, false, "claude-fable-5", now),
            Budget::Exhausted
        );
    }

    #[tokio::test]
    async fn template_keeps_available_codex_and_claude_in_saved_order(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::services::native_subscription::ClaudeRoutingQuota;
        use serde_json::json;
        use std::sync::Arc;

        let now = chrono::Utc::now().timestamp();
        let mut menu = super::super::global_tests::menu("http://127.0.0.1:1/v1/responses");
        let session = Arc::make_mut(
            menu.groups
                .values_mut()
                .next()
                .ok_or("missing fixture group")?,
        );
        let mut codex = session.targets[0].clone();
        codex.official = true;
        codex.scheduling = Arc::new(super::super::scheduling::Scheduling::new(true));
        codex.scheduling.observe_event(
            &json!({"type":"codex.rate_limits","rate_limits":{
                "primary":{"window_minutes":300,"used_percent":50,"reset_at":now+10},
                "secondary":{"window_minutes":10080,"used_percent":80,"reset_at":now+4*3600}
            }}),
            now,
        );
        let mut claude = session.targets[0].clone();
        claude.model = "claude-opus-4-7".into();
        claude.upstream = Some(crate::provider::Provider::with_id(
            "fixture-claude".into(),
            "Synthetic Claude".into(),
            json!({"routingSubscription":{"provider":"claude","accountId":"fixture-claude-account"}}),
            None,
        ));
        claude.scheduling = Arc::default();
        claude.scheduling.claim_budget_refresh(now);
        claude.scheduling.observe_claude_quota(
            ClaudeRoutingQuota {
                tiers: vec![QuotaTier {
                    name: TIER_SEVEN_DAY.into(),
                    utilization: 40.0,
                    resets_at: chrono::DateTime::from_timestamp(now + 6 * 3600, 0)
                        .map(|t| t.to_rfc3339()),
                    used_value_usd: None,
                    max_value_usd: None,
                }],
                short_only: false,
            },
            now,
        );
        session.targets.extend([codex, claude]);

        let mut order = vec![0, 1, 2];
        let snapshots = || {
            session
                .targets
                .iter()
                .map(|t| t.scheduling.snapshot())
                .collect::<Vec<_>>()
        };
        assert!(!filter_exhausted(session, &mut order, &snapshots(), now));
        assert_eq!(order, [0, 1, 2]);
        session.targets[1].scheduling.observe_event(
            &json!({"type":"codex.rate_limits","rate_limits":{
                "primary":{"window_minutes":300,"used_percent":100,"reset_at":now+10}
            }}),
            now + 1,
        );
        assert!(filter_exhausted(session, &mut order, &snapshots(), now + 1));
        assert_eq!(order, [0, 2]);
        Ok(())
    }
}
