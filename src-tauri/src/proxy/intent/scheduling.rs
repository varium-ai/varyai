//! Reference-compatible pool strategies and bounded, account/model-scoped telemetry.
//! Adapted from the reference implementation routes/strategies.go, fastest.go and round.go (MIT;
//! attribution in third_party/routing-reference/LICENSE). Strategies only order approved routes.
use super::subscription_budget::{self, Budget};
use crate::services::{intent_routing::RoutingStrategy, native_subscription::ClaudeRoutingQuota};
use http::HeaderMap;
use serde_json::Value;
use std::sync::{
    atomic::{AtomicU64, Ordering as AtomicOrdering},
    Arc, Mutex,
};

static SINK_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
mod quota_policy_tests {
    use super::*;
    #[test]
    fn codex_short_only_requires_a_fresh_complete_account_read() {
        let state = Scheduling::new(true);
        let now = 1000;
        state.observe_event(&serde_json::json!({
            "type":"codex.rate_limits",
            "rate_limits":{"primary":{"used_percent":50,"window_minutes":300,"reset_at":now + 7200}}
        }), now);
        assert_eq!(state.snapshot_at(now).codex_budget(now), Budget::Unknown);
        state.observe_codex_usage(&serde_json::json!({"rate_limit":{
            "primary_window":{"used_percent":50,"limit_window_seconds":18000,"reset_at":now + 7200},
            "secondary_window":null
        }}), now);
        assert_eq!(state.snapshot_at(now).codex_budget(now), Budget::Pace(25.0));
        assert_eq!(
            state.snapshot_at(now + 301).codex_budget(now + 301),
            Budget::Unknown
        );
        state.observe_codex_usage(&serde_json::json!({"rate_limit":{
            "primary_window":{"used_percent":50,"limit_window_seconds":18000,"reset_at":now + 7200},
            "secondary_window":{"limit_window_seconds":604800}
        }}), now + 1);
        assert_eq!(
            state.snapshot_at(now + 1).codex_budget(now + 1),
            Budget::Unknown
        );
    }

    #[test]
    fn weekly_reset_never_follows_only_a_five_hour_limit_and_confirmed_resets_clear_health() {
        let state = Scheduling::new(true);
        state.update(
            vec![Window {
                used: 100.0,
                seconds: 18000,
                reset: 90000,
                sampled: 1000,
            }],
            1000,
        );
        assert!(!state.weekly_exhausted(1001));
        state.update(
            vec![Window {
                used: 100.0,
                seconds: 604800,
                reset: 90000,
                sampled: 1000,
            }],
            1000,
        );
        assert!(state.weekly_exhausted(1001));
        state.reset_confirmed();
        assert!(!state.weekly_exhausted(1001));
        assert!(!state.exhausted(1001));
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Window {
    pub used: f64,
    pub seconds: i64,
    pub reset: i64,
    sampled: i64,
}
#[derive(Clone, Default)]
pub(crate) struct Health {
    windows: Vec<Window>,
    codex_window_layout: Option<(i64, bool)>,
    claude_quota: Option<(i64, ClaudeRoutingQuota)>,
    budget_refresh_at: i64,
    rest_until: i64,
    account: bool,
    plan: Option<String>,
    plan_at: i64,
    ttft: f64,
    tps: f64,
    sunk: u64,
    sunk_at: i64,
}
fn fresh(sampled: i64, now: i64) -> bool {
    (0..=300).contains(&now.saturating_sub(sampled))
}

impl Health {
    pub(super) fn cooling(&self, now: i64) -> bool {
        self.rest_until > now
    }

    pub(super) fn codex_budget(&self, now: i64) -> Budget {
        let windows = self.windows.iter().filter(|w| fresh(w.sampled, now));
        let short_only = self.codex_window_layout.is_some_and(|(at, only_short)| {
            only_short && fresh(at, now) && !windows.clone().any(|w| w.seconds >= 86400)
        });
        subscription_budget::evaluate(
            windows.filter_map(|w| {
                subscription_budget::codex_window(w.seconds)
                    .map(|week| (week, w.used, Some(w.reset)))
            }),
            short_only,
            now,
        )
    }

    pub(super) fn claude_budget(&self, model: &str, now: i64) -> Budget {
        self.claude_quota
            .as_ref()
            .filter(|(at, _)| fresh(*at, now))
            .map_or(Budget::Unknown, |(_, quota)| {
                subscription_budget::claude_budget(&quota.tiers, quota.short_only, model, now)
            })
    }
}

#[derive(Default)]
struct Average {
    value: f64,
    weight: f64,
    at: i64,
}
impl Average {
    fn add(&mut self, value: f64, now: i64) {
        if !value.is_finite() || value <= 0.0 {
            return;
        }
        let weight = decay(self.weight, now.saturating_sub(self.at), 1200.0);
        self.value = (self.value * weight + value) / (weight + 1.0);
        self.weight = weight + 1.0;
        self.at = now;
    }
    fn recent(&self, now: i64) -> f64 {
        if now.saturating_sub(self.at) <= 3600 {
            self.value
        } else {
            0.0
        }
    }
}
#[derive(Default)]
struct Speed {
    ttft: Average,
    tps: Average,
    rest_until: i64,
}
#[derive(Default)]
pub(crate) struct Scheduling {
    account: Arc<Mutex<Health>>,
    speed: Mutex<Speed>,
}
fn decay(value: f64, age: i64, half_life: f64) -> f64 {
    value * (-age.max(0) as f64 / half_life).exp2()
}
impl Scheduling {
    pub(super) fn claim_budget_refresh(&self, now: i64) -> bool {
        let Ok(mut state) = self.account.try_lock() else {
            return false;
        };
        if state.budget_refresh_at > now {
            return false;
        }
        state.budget_refresh_at = now.saturating_add(60);
        true
    }
    pub(super) fn observe_claude_quota(&self, quota: ClaudeRoutingQuota, now: i64) {
        if let Ok(mut state) = self.account.lock() {
            state.claude_quota = Some((now, quota));
        }
    }
    pub(super) fn weekly_exhausted(&self, now: i64) -> bool {
        self.account.lock().is_ok_and(|state| {
            state.windows.iter().any(|w| {
                w.seconds == 604800
                    && w.used >= 100.0
                    && w.reset > now
                    && now.saturating_sub(w.sampled) <= 300
            })
        })
    }
    pub(super) fn reset_confirmed(&self) {
        if let Ok(mut state) = self.account.lock() {
            state.windows.clear();
            state.codex_window_layout = None;
            state.budget_refresh_at = 0;
        }
        self.succeeded();
    }
    pub(super) fn exhausted(&self, now: i64) -> bool {
        self.account.try_lock().is_ok_and(|state| {
            state
                .windows
                .iter()
                .any(|w| w.used >= 100.0 && w.reset > now && now.saturating_sub(w.sampled) <= 300)
        })
    }
    pub(crate) fn trace_account(&self) -> Option<super::trace::Telemetry> {
        let now = chrono::Utc::now().timestamp();
        let state = self.account.try_lock().ok()?;
        let speed = self.speed.try_lock().ok()?;
        Some(super::trace::Telemetry {
            plan: state.plan.clone(),
            rest_until: speed.rest_until,
            sunk_at: (state.sunk > 0).then_some(state.sunk_at),
            windows: state
                .windows
                .iter()
                .filter(|w| w.reset > now && now.saturating_sub(w.sampled) <= 300)
                .map(|w| super::trace::Allowance {
                    used_percent: w.used,
                    window_seconds: w.seconds,
                    resets_at: w.reset,
                })
                .collect(),
            first_token_ms: (speed.ttft.recent(now) > 0.0).then_some(speed.ttft.recent(now)),
            output_tokens_per_second: (speed.tps.recent(now) > 0.0)
                .then_some(speed.tps.recent(now)),
        })
    }

    pub fn new(account: bool) -> Self {
        Self {
            account: Arc::new(Mutex::new(Health {
                account,
                ..Default::default()
            })),
            speed: Mutex::default(),
        }
    }
    /// Account quota/use is shared, while latency belongs to an exact model.
    pub fn for_model(&self) -> Self {
        Self {
            account: self.account.clone(),
            speed: Mutex::default(),
        }
    }
    pub(crate) fn merge_quota(&self, newer: &Self) {
        let Some(other) = newer.account.lock().ok().map(|s| s.clone()) else {
            return;
        };
        if let Ok(mut state) = self.account.lock() {
            for window in other.windows {
                if let Some(old) = state
                    .windows
                    .iter_mut()
                    .find(|w| w.seconds == window.seconds)
                {
                    if window.sampled > old.sampled {
                        *old = window;
                    }
                } else {
                    state.windows.push(window);
                }
            }
            state.windows.sort_by_key(|w| std::cmp::Reverse(w.seconds));
            state.windows.truncate(2);
            if other.codex_window_layout.map(|(at, _)| at)
                > state.codex_window_layout.map(|(at, _)| at)
            {
                state.codex_window_layout = other.codex_window_layout;
            }
            if other.claude_quota.as_ref().map(|(at, _)| at)
                > state.claude_quota.as_ref().map(|(at, _)| at)
            {
                state.claude_quota = other.claude_quota;
            }
            if other.plan_at > state.plan_at {
                state.plan = other.plan;
                state.plan_at = other.plan_at;
            }
        }
    }
    pub(crate) fn snapshot(&self) -> Health {
        let now = chrono::Utc::now().timestamp();
        self.snapshot_at(now)
    }
    fn snapshot_at(&self, now: i64) -> Health {
        let mut state = self.account.lock().map(|s| s.clone()).unwrap_or_default();
        if let Ok(speed) = self.speed.lock() {
            state.ttft = speed.ttft.recent(now);
            state.tps = speed.tps.recent(now);
            state.rest_until = speed.rest_until;
        }
        state
    }
    fn update(&self, mut windows: Vec<Window>, now: i64) {
        windows.retain(|w| {
            w.used.is_finite()
                && w.used >= 0.0
                && w.seconds > 0
                && w.seconds <= 366 * 86400
                && w.reset > now
        });
        for window in &mut windows {
            window.used = window.used.min(100.0);
            window.sampled = now;
        }
        windows.sort_by_key(|w| std::cmp::Reverse(w.seconds));
        windows.truncate(2);
        if !windows.is_empty() {
            if let Ok(mut state) = self.account.lock() {
                for previous in &state.windows {
                    if previous.reset > now
                        && now.saturating_sub(previous.sampled) <= 300
                        && !windows.iter().any(|w| w.seconds == previous.seconds)
                    {
                        windows.push(previous.clone());
                    }
                }
                windows.sort_by_key(|w| std::cmp::Reverse(w.seconds));
                windows.truncate(2);
                state.windows = windows;
            }
        }
    }
    pub(crate) fn observe_headers(&self, headers: &HeaderMap, now: i64) {
        let mut windows = Vec::new();
        for prefix in ["x-codex-primary-", "x-codex-secondary-"] {
            let number = |key: &str| {
                headers
                    .get(format!("{prefix}{key}"))?
                    .to_str()
                    .ok()?
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
            };
            if let (Some(used), Some(minutes)) = (number("used-percent"), number("window-minutes"))
            {
                let reset = number("reset-at")
                    .or_else(|| number("reset-after-seconds").map(|s| now as f64 + s));
                if let Some(reset) = reset {
                    windows.push(Window {
                        used,
                        seconds: (minutes * 60.0) as i64,
                        reset: reset as i64,
                        sampled: now,
                    });
                }
            }
        }
        self.update(windows, now);
    }
    pub(crate) fn observe_event(&self, value: &Value, now: i64) {
        if value["type"] != "codex.rate_limits"
            || value["metered_limit_name"]
                .as_str()
                .is_some_and(|s| !s.is_empty() && s != "codex")
        {
            return;
        }
        self.observe_plan(value.get("plan_type"), now);
        self.observe_windows(
            &value["rate_limits"],
            ["primary", "secondary"],
            "window_minutes",
            60,
            now,
        );
    }
    // Publish only recognized subscription names, never arbitrary upstream strings.
    fn observe_plan(&self, value: Option<&Value>, now: i64) {
        let plan = match value.and_then(Value::as_str) {
            Some("free") => "ChatGPT Free",
            Some("plus") => "ChatGPT Plus",
            Some("pro") => "ChatGPT Pro",
            Some("team" | "business") => "ChatGPT Business",
            Some("enterprise") => "ChatGPT Enterprise",
            Some("edu") => "ChatGPT Edu",
            _ => return,
        };
        if let Ok(mut state) = self.account.try_lock() {
            if state.account && now >= state.plan_at {
                state.plan = Some(plan.into());
                state.plan_at = now;
            }
        }
    }
    fn observe_windows(&self, value: &Value, fields: [&str; 2], span: &str, scale: i64, now: i64) {
        let mut windows = Vec::new();
        for field in fields {
            let v = &value[field];
            if let (Some(used), Some(seconds)) = (
                v["used_percent"].as_f64(),
                v[span].as_i64().and_then(|s| s.checked_mul(scale)),
            ) {
                let reset = v["reset_at"].as_i64().or_else(|| {
                    v["reset_after_seconds"]
                        .as_i64()
                        .and_then(|s| now.checked_add(s))
                });
                if let Some(reset) = reset {
                    windows.push(Window {
                        used,
                        seconds,
                        reset,
                        sampled: now,
                    });
                }
            }
        }
        self.update(windows, now);
    }
    pub(crate) fn failed(&self, headers: &HeaderMap, now: i64) {
        let seconds = headers
            .get(http::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                s.parse::<i64>().ok().or_else(|| {
                    chrono::DateTime::parse_from_rfc2822(s)
                        .ok()
                        .map(|d| d.timestamp().saturating_sub(now))
                })
            })
            .unwrap_or(30)
            .clamp(1, 86400);
        if let Ok(mut state) = self.speed.lock() {
            state.rest_until = now.saturating_add(seconds);
        }
    }
    pub(crate) fn succeeded(&self) {
        if let Ok(mut state) = self.speed.lock() {
            state.rest_until = 0;
        }
    }
    pub(crate) fn rate_limited(&self, now: i64) {
        if let Ok(mut state) = self.account.try_lock() {
            let exhausted = state.windows.iter().any(|window| {
                window.used >= 100.0
                    && window.reset > now
                    && now.saturating_sub(window.sampled) <= 300
            });
            if !exhausted {
                state.sunk = SINK_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
                state.sunk_at = now;
            }
        }
    }
    pub(super) fn rest_until(&self) -> i64 {
        self.snapshot().rest_until
    }
    pub(crate) fn served(
        &self,
        _tokens: u64,
        ttft_ms: f64,
        decode_ms: f64,
        output_tokens: u32,
        now: i64,
    ) {
        if let Ok(mut speed) = self.speed.lock() {
            speed.ttft.add(ttft_ms, now);
            if ttft_ms > 0.0 && decode_ms > 0.0 && output_tokens > 0 {
                speed
                    .tps
                    .add(output_tokens as f64 * 1000.0 / decode_ms, now);
            }
        }
    }
    pub(crate) async fn prime(&self, client: &reqwest::Client, headers: &HeaderMap) {
        let mut request = client
            .get("https://chatgpt.com/backend-api/wham/usage")
            .timeout(std::time::Duration::from_secs(5));
        for name in ["authorization", "chatgpt-account-id", "user-agent"] {
            if let Some(value) = headers.get(name) {
                request = request.header(name, value);
            }
        }
        let Ok(mut response) = request.send().await else {
            return;
        };
        if !response.status().is_success() {
            return;
        }
        let mut bytes = Vec::new();
        while let Ok(Some(chunk)) = response.chunk().await {
            if bytes.len() + chunk.len() > 64 * 1024 {
                return;
            }
            bytes.extend_from_slice(&chunk);
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            let now = chrono::Utc::now().timestamp();
            self.observe_codex_usage(&value, now);
        }
    }

    fn observe_codex_usage(&self, value: &Value, now: i64) {
        let rate = &value["rate_limit"];
        self.observe_plan(value.get("plan_type"), now);
        self.observe_windows(
            rate,
            ["primary_window", "secondary_window"],
            "limit_window_seconds",
            1,
            now,
        );
        if let Ok(mut state) = self.account.lock() {
            let only_short = rate.get("secondary_window").is_some_and(Value::is_null)
                && rate["primary_window"]["limit_window_seconds"]
                    .as_i64()
                    .and_then(subscription_budget::codex_window)
                    == Some(false);
            state.codex_window_layout = Some((now, only_short));
            state.budget_refresh_at = state.budget_refresh_at.max(now.saturating_add(60));
        }
    }
}

#[derive(Default)]
pub(crate) struct Pool {
    probes: usize,
    probe_turn: usize,
}
struct Seat {
    used: f64,
    resting: bool,
    ttft: f64,
    tps: f64,
}
impl Seat {
    fn of(state: Option<&Health>, now: i64) -> Self {
        let empty = Health::default();
        let s = state.unwrap_or(&empty);
        let used = s
            .windows
            .iter()
            .filter(|w| w.reset > now && now.saturating_sub(w.sampled) <= 300)
            .map(|w| w.used)
            .fold(0.0, f64::max);
        Self {
            used,
            resting: s.rest_until > now,
            ttft: s.ttft,
            tps: s.tps,
        }
    }
}
fn tier(used: f64) -> u8 {
    if used >= 98.0 {
        2
    } else if used >= 90.0 {
        1
    } else {
        0
    }
}

/// Sink persists past cooldown and success; already-resting seats remain last.
pub(crate) fn sink_last(candidates: &mut [usize], states: &[Health], now: i64) {
    candidates.sort_by_key(|index| {
        states
            .get(*index)
            .map(|state| (state.rest_until > now, state.sunk))
            .unwrap_or_default()
    });
}
/// New selections use this pool. Bound continuations retain their exact destination.
pub(crate) fn arrange(
    strategy: RoutingStrategy,
    candidates: &[usize],
    states: &[Health],
    pool: &mut Pool,
    now: i64,
) -> Vec<usize> {
    let seats: Vec<_> = candidates
        .iter()
        .map(|i| Seat::of(states.get(*i), now))
        .collect();
    let mut order: Vec<_> = (0..candidates.len()).collect();
    match strategy {
        RoutingStrategy::Ordered => {}
        RoutingStrategy::Fastest => {
            let mut fast: Vec<_> = order
                .iter()
                .copied()
                .filter(|i| !seats[*i].resting && tier(seats[*i].used) != 2 && seats[*i].ttft > 0.0)
                .collect();
            let mut unknown: Vec<_> = order
                .iter()
                .copied()
                .filter(|i| {
                    !seats[*i].resting && tier(seats[*i].used) != 2 && seats[*i].ttft <= 0.0
                })
                .collect();
            let mut rates: Vec<_> = fast
                .iter()
                .map(|i| seats[*i].tps)
                .filter(|v| *v > 0.0)
                .collect();
            rates.sort_by(f64::total_cmp);
            let mid = rates.get(rates.len() / 2).copied().unwrap_or(0.0);
            let estimate = |i: usize| {
                let s = &seats[i];
                let tps = if s.tps > 0.0 { s.tps } else { mid };
                s.ttft + if tps > 0.0 { 200000.0 / tps } else { 0.0 }
            };
            fast.sort_by(|a, b| estimate(*a).total_cmp(&estimate(*b)));
            let mut bands = vec![0; seats.len()];
            let (mut band, mut top) = (0, 0.0);
            for (k, i) in fast.iter().enumerate() {
                if k == 0 || estimate(*i) > 1.1 * top {
                    top = estimate(*i);
                    if k > 0 {
                        band += 1;
                    }
                }
                bands[*i] = band;
            }
            fast.sort_by_key(|i| (bands[*i], *i));
            if !fast.is_empty() && !unknown.is_empty() {
                pool.probes += 1;
                if pool.probes >= 5 {
                    let at = pool.probe_turn % unknown.len();
                    let probe = unknown.remove(at);
                    pool.probes = 0;
                    pool.probe_turn = pool.probe_turn.wrapping_add(1);
                    fast.insert(0, probe);
                }
            }
            order = fast;
            order.extend(unknown);
            order.extend((0..seats.len()).filter(|i| seats[*i].resting));
            order.extend(
                (0..seats.len()).filter(|i| !seats[*i].resting && tier(seats[*i].used) == 2),
            );
        }
    }
    // Gateway cooldown applies outside the strategy, as in the reference implementation's ready/rest partition.
    order.sort_by_key(|i| seats[*i].resting);
    order
        .into_iter()
        .filter_map(|i| candidates.get(i).copied())
        .collect()
}
#[cfg(test)]
fn order(
    strategy: RoutingStrategy,
    candidates: &[usize],
    states: &[Health],
    now: i64,
) -> Vec<usize> {
    arrange(strategy, candidates, states, &mut Pool::default(), now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::intent_routing::RoutingStrategy;

    #[test]
    fn subscription_plan_observation_uses_only_known_account_metadata() {
        let state = Scheduling::new(true);
        state.observe_event(
            &serde_json::json!({
                "type":"codex.rate_limits", "plan_type":"plus", "rate_limits":{}
            }),
            100,
        );
        let observed = serde_json::to_value(state.trace_account()).unwrap();
        assert_eq!(observed["plan"], "ChatGPT Plus");
        state.observe_event(&serde_json::json!({
            "type":"codex.rate_limits", "plan_type":"synthetic-private-unknown", "rate_limits":{}
        }), 200);
        assert_eq!(
            serde_json::to_value(state.trace_account()).unwrap()["plan"],
            "ChatGPT Plus"
        );
        let key = Scheduling::new(false);
        key.observe_event(
            &serde_json::json!({
                "type":"codex.rate_limits", "plan_type":"pro", "rate_limits":{}
            }),
            100,
        );
        assert!(serde_json::to_value(key.trace_account())
            .unwrap()
            .get("plan")
            .is_none());
    }

    #[test]
    fn trace_observation_never_waits_for_scheduler_locks() -> Result<(), Box<dyn std::error::Error>>
    {
        let scheduling = Scheduling::new(false);
        let account = scheduling
            .account
            .lock()
            .map_err(|_| "synthetic account lock")?;
        assert!(scheduling.trace_account().is_none());
        drop(account);
        let speed = scheduling
            .speed
            .lock()
            .map_err(|_| "synthetic speed lock")?;
        assert!(scheduling.trace_account().is_none());
        drop(speed);
        assert!(scheduling.trace_account().is_some());
        Ok(())
    }

    #[test]
    fn sinking_survives_success_and_rest_but_does_not_apply_to_exhausted_quota() {
        let a = Scheduling::new(false);
        let b = Scheduling::new(false);
        a.rate_limited(100);
        a.succeeded();
        let mut candidates = vec![0, 1];
        sink_last(
            &mut candidates,
            &[a.snapshot_at(200), b.snapshot_at(200)],
            200,
        );
        assert_eq!(candidates, [1, 0]);
        b.rate_limited(200);
        sink_last(
            &mut candidates,
            &[a.snapshot_at(200), b.snapshot_at(200)],
            200,
        );
        assert_eq!(candidates, [0, 1]);
        assert_eq!(a.snapshot_at(200).sunk, a.for_model().snapshot_at(200).sunk);
        let exhausted = Scheduling::new(true);
        exhausted.update(
            vec![Window {
                used: 100.0,
                seconds: 3600,
                reset: 1000,
                sampled: 100,
            }],
            100,
        );
        exhausted.rate_limited(100);
        assert_eq!(exhausted.snapshot_at(100).sunk, 0);
        let states = [a.snapshot_at(200), Health::default()];
        let mut resting = Health {
            rest_until: 300,
            ..Default::default()
        };
        candidates = vec![0, 1];
        sink_last(&mut candidates, &[states[0].clone(), resting.clone()], 200);
        assert_eq!(candidates, [0, 1]);
        resting.rest_until = 0;
        sink_last(&mut candidates, &[states[0].clone(), resting], 200);
        assert_eq!(candidates, [1, 0]);
    }

    #[test]
    fn refreshed_quota_updates_without_erasing_speed_or_cooldown() {
        let old = Scheduling::new(true);
        let new = Scheduling::new(true);
        old.observe_event(&serde_json::json!({"type":"codex.rate_limits","rate_limits":{"primary":{"used_percent":20,"window_minutes":10080,"reset_at":10000}}}),100);
        new.observe_event(&serde_json::json!({"type":"codex.rate_limits","rate_limits":{"primary":{"used_percent":95,"window_minutes":10080,"reset_at":10000}}}),200);
        old.served(100, 200.0, 1000.0, 20, 200);
        old.failed(&HeaderMap::new(), 200);
        old.merge_quota(&new);
        let kept = old.snapshot_at(200);
        assert_eq!(kept.windows[0].used, 95.0);
        assert_eq!(kept.ttft, 200.0);
        assert_eq!(kept.rest_until, 230);
        new.merge_quota(&old);
        assert_eq!(new.snapshot_at(200).windows[0].used, 95.0);
    }

    #[test]
    fn speed_telemetry_preserves_model_isolation_and_expires_after_one_hour() {
        let a = Scheduling::new(false);
        let other_model = a.for_model();
        a.served(100, 200.0, 1000.0, 40, 100);
        assert_eq!(a.snapshot_at(100).ttft, 200.0);
        assert_eq!(other_model.snapshot_at(100).ttft, 0.0);
        assert_eq!(a.snapshot_at(3701).ttft, 0.0);
    }

    #[test]
    fn fastest_uses_first_token_and_decode_speed_and_rotates_unmeasured_probes() {
        let states = [
            Health {
                ttft: 100.0,
                tps: 10.0,
                ..Default::default()
            },
            Health {
                ttft: 200.0,
                tps: 100.0,
                ..Default::default()
            },
            Health::default(),
            Health::default(),
        ];
        let mut pool = Pool::default();
        for _ in 0..4 {
            assert_eq!(
                arrange(
                    RoutingStrategy::Fastest,
                    &[0, 1, 2, 3],
                    &states,
                    &mut pool,
                    100
                )[0],
                1
            );
        }
        assert_eq!(
            arrange(
                RoutingStrategy::Fastest,
                &[0, 1, 2, 3],
                &states,
                &mut pool,
                100
            )[0],
            2
        );
        for _ in 0..4 {
            arrange(
                RoutingStrategy::Fastest,
                &[0, 1, 2, 3],
                &states,
                &mut pool,
                100,
            );
        }
        assert_eq!(
            arrange(
                RoutingStrategy::Fastest,
                &[0, 1, 2, 3],
                &states,
                &mut pool,
                100
            )[0],
            3
        );
        let close = [
            Health {
                ttft: 105.0,
                ..Default::default()
            },
            Health {
                ttft: 100.0,
                ..Default::default()
            },
        ];
        assert_eq!(
            order(RoutingStrategy::Fastest, &[0, 1], &close, 100),
            [0, 1]
        );
    }

    #[test]
    fn ordered_keeps_configured_order_and_puts_cooling_routes_last() {
        let now = 1_000_000;
        let mut states = vec![Health::default(); 3];
        assert_eq!(
            order(RoutingStrategy::Ordered, &[2, 0, 1], &states, now),
            vec![2, 0, 1]
        );
        states[2].rest_until = now + 30;
        assert_eq!(
            order(RoutingStrategy::Ordered, &[2, 0, 1], &states, now),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn quota_observation_is_bounded_and_ignores_other_metered_limits() {
        let now = 1_000_000;
        let state = Scheduling::default();
        state.observe_event(&serde_json::json!({
            "type":"codex.rate_limits",
            "rate_limits":{"primary":{"used_percent":35,"window_minutes":300,"reset_at":now+500}}
        }), now);
        assert_eq!(state.snapshot().windows[0].used, 35.0);
        state.observe_event(&serde_json::json!({
            "type":"codex.rate_limits", "metered_limit_name":"other-model",
            "rate_limits":{"primary":{"used_percent":100,"window_minutes":300,"reset_at":now+500}}
        }), now);
        assert_eq!(state.snapshot().windows[0].used, 35.0);
        state.failed(
            &http::HeaderMap::from_iter([(
                http::header::RETRY_AFTER,
                http::HeaderValue::from_static("120"),
            )]),
            now,
        );
        assert_eq!(state.snapshot().rest_until, now + 120);
        assert!(state.snapshot().windows.iter().all(|w| w.used.is_finite()));
    }

    #[test]
    fn partial_usage_updates_preserve_but_do_not_freshen_other_windows() {
        let state = Scheduling::default();
        state.observe_event(
            &serde_json::json!({
                "type":"codex.rate_limits", "rate_limits":{
                    "primary":{"used_percent":10,"window_minutes":300,"reset_at":10000},
                    "secondary":{"used_percent":98,"window_minutes":10080,"reset_at":20000}
                }
            }),
            100,
        );
        state.observe_event(
            &serde_json::json!({
                "type":"codex.rate_limits", "rate_limits":{
                    "primary":{"used_percent":20,"window_minutes":300,"reset_at":10000}
                }
            }),
            200,
        );
        let known = state.snapshot();
        assert_eq!(known.windows.len(), 2);
        assert_eq!(known.windows[0].sampled, 100);
    }
}
