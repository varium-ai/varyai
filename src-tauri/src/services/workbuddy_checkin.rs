//! Optional daily credits for the current WorkBuddy China desktop account.
//! Kept separate from forwarding, client configuration and synchronized settings.
use chrono::{DateTime, FixedOffset, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;

const ENDPOINT: &str = "https://copilot.tencent.com";
const STATUS_PATH: &str = "/v2/billing/meter/checkin-activity-status";
const CLAIM_PATH: &str = "/v2/billing/meter/daily-checkin";
const SUMMARY_PATH: &str = "/billing/meter/get-user-resource-summary";
const STORAGE_ERROR: &str = "workbuddy.checkin.storageError";
const RETRY_SECONDS: i64 = 30 * 60;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    NotChecked,
    Claimed,
    AlreadyChecked,
    Inactive,
    Ineligible,
    OwnAppRequired,
    SignInRequired,
    CredentialsUnavailable,
    Failed,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    day: String,
    checked_at: i64,
    outcome: Outcome,
    credit: Option<f64>,
    streak: Option<u32>,
    credit_delayed: bool,
}

impl Record {
    fn should_run(&self, now: i64, manual: bool) -> bool {
        if now < self.checked_at {
            return false;
        }
        if self.day != beijing_day(now) {
            return true;
        }
        if self.outcome == Outcome::Claimed {
            return false;
        }
        if manual {
            return true;
        }
        if now - self.checked_at < RETRY_SECONDS {
            return false;
        }
        match self.outcome {
            Outcome::AlreadyChecked | Outcome::Inactive | Outcome::Ineligible => {
                // Early answers can refer to the vendor's previous campaign day.
                beijing_time(self.checked_at).is_some_and(|time| time.hour() < 12)
            }
            Outcome::OwnAppRequired => false,
            _ => true,
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    auto_enabled: bool,
    records: BTreeMap<String, Record>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    auto_enabled: bool,
    day: String,
    outcome: Outcome,
    checked_at: Option<i64>,
    credit: Option<f64>,
    streak: Option<u32>,
    credit_delayed: bool,
}

struct Account {
    uid: String,
    access: String,
    domain: String,
}

impl Account {
    fn key(&self) -> String {
        format!("{:x}", Sha256::digest(format!("workbuddy|{}", self.uid)))
    }
}

/// One manager serializes manual, automatic and preference updates.
pub(crate) struct Manager {
    path: PathBuf,
    auth_path: PathBuf,
    endpoint: String,
    lock: Mutex<()>,
}

impl Manager {
    pub(crate) fn new(path: PathBuf) -> Self {
        let home = crate::config::get_home_dir();
        #[cfg(target_os = "macos")]
        let base = home.join("Library/Application Support/CodeBuddyExtension");
        #[cfg(target_os = "windows")]
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("CodeBuddyExtension");
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let base = home.join(".local/share/CodeBuddyExtension");
        Self::with_paths(path, base.join("Data/Public/auth/workbuddy-desktop.info"))
    }

    fn with_paths(path: PathBuf, auth_path: PathBuf) -> Self {
        Self {
            path,
            auth_path,
            endpoint: ENDPOINT.into(),
            lock: Mutex::new(()),
        }
    }

    async fn read_store(&self) -> Result<Store, String> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| STORAGE_ERROR.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
            Err(_) => Err(STORAGE_ERROR.into()),
        })
        .await
        .map_err(|_| STORAGE_ERROR.to_string())?
    }

    async fn write_store(&self, store: &Store) -> Result<(), String> {
        let path = self.path.clone();
        let bytes = serde_json::to_vec(store).map_err(|_| STORAGE_ERROR.to_string())?;
        tokio::task::spawn_blocking(move || {
            crate::config::atomic_write_private(&path, &bytes)
                .map_err(|_| STORAGE_ERROR.to_string())
        })
        .await
        .map_err(|_| STORAGE_ERROR.to_string())?
    }

    async fn account(&self) -> Result<Account, Outcome> {
        let path = self.auth_path.clone();
        tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(path).map_err(|_| Outcome::CredentialsUnavailable)?;
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| Outcome::CredentialsUnavailable)?;
            let field = |pointer| {
                value
                    .pointer(pointer)
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_owned)
            };
            let uid = field("/account/uid").ok_or(Outcome::CredentialsUnavailable)?;
            let access = field("/auth/accessToken").ok_or(Outcome::CredentialsUnavailable)?;
            if value
                .pointer("/auth/expiresAt")
                .and_then(Value::as_i64)
                .is_some_and(|expires| expires > 0 && expires <= Utc::now().timestamp_millis())
            {
                return Err(Outcome::SignInRequired);
            }
            Ok(Account {
                uid,
                access,
                domain: field("/auth/domain").unwrap_or_else(|| "copilot.tencent.com".into()),
            })
        })
        .await
        .map_err(|_| Outcome::CredentialsUnavailable)?
    }

    fn snapshot(store: &Store, account: &Result<Account, Outcome>, now: i64) -> Status {
        let day = beijing_day(now);
        let record = account
            .as_ref()
            .ok()
            .and_then(|a| store.records.get(&a.key()))
            .filter(|record| record.day == day);
        Status {
            auto_enabled: store.auto_enabled,
            day,
            outcome: account
                .as_ref()
                .err()
                .copied()
                .unwrap_or_else(|| record.map_or(Outcome::NotChecked, |r| r.outcome)),
            checked_at: record.map(|r| r.checked_at),
            credit: record.and_then(|r| r.credit),
            streak: record.and_then(|r| r.streak),
            credit_delayed: record.is_some_and(|r| r.credit_delayed),
        }
    }

    pub(crate) async fn status(&self) -> Result<Status, String> {
        let _guard = self.lock.lock().await;
        let store = self.read_store().await?;
        Ok(Self::snapshot(
            &store,
            &self.account().await,
            Utc::now().timestamp(),
        ))
    }

    pub(crate) async fn set_auto_enabled(&self, enabled: bool) -> Result<Status, String> {
        let _guard = self.lock.lock().await;
        let mut store = self.read_store().await?;
        store.auto_enabled = enabled;
        self.write_store(&store).await?;
        Ok(Self::snapshot(
            &store,
            &self.account().await,
            Utc::now().timestamp(),
        ))
    }

    pub(crate) async fn check_in(&self, manual: bool) -> Result<Status, String> {
        let _guard = self.lock.lock().await;
        let mut store = self.read_store().await?;
        let account = self.account().await;
        let now = Utc::now().timestamp();
        let Ok(ref current) = account else {
            return Ok(Self::snapshot(&store, &account, now));
        };
        if (!manual && !store.auto_enabled)
            || store
                .records
                .get(&current.key())
                .is_some_and(|r| !r.should_run(now, manual))
        {
            return Ok(Self::snapshot(&store, &account, now));
        }
        let record = self.claim(current).await;
        store.records.insert(current.key(), record);
        self.write_store(&store).await?;
        // The native client may switch accounts while the network request is running.
        Ok(Self::snapshot(
            &store,
            &self.account().await,
            Utc::now().timestamp(),
        ))
    }

    async fn claim(&self, account: &Account) -> Record {
        let result = self.request_claim(account).await;
        let now = Utc::now().timestamp();
        let (outcome, credit, streak, credit_delayed) = match result {
            Ok(result) => result,
            Err(outcome) => (outcome, None, None, false),
        };
        Record {
            day: beijing_day(now),
            checked_at: now,
            outcome,
            credit,
            streak,
            credit_delayed,
        }
    }

    async fn request_claim(
        &self,
        account: &Account,
    ) -> Result<(Outcome, Option<f64>, Option<u32>, bool), Outcome> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| Outcome::Failed)?;
        let result = async {
            let status = self.call(&client, account, STATUS_PATH).await?;
            let active = status
                .get("active")
                .and_then(Value::as_bool)
                .ok_or(CallError::Invalid)?;
            if !active {
                return Ok((Outcome::Inactive, None, None, false));
            }
            let checked = status
                .get("today_checked_in")
                .or_else(|| status.get("todayCheckedIn"))
                .and_then(Value::as_bool)
                .ok_or(CallError::Invalid)?;
            if checked {
                return Ok((
                    Outcome::AlreadyChecked,
                    number(
                        &status,
                        &["today_credit", "todayCredit", "daily_credit", "dailyCredit"],
                    )?,
                    streak(&status)?,
                    false,
                ));
            }
            let claimed = self.call(&client, account, CLAIM_PATH).await?;
            Ok((
                Outcome::Claimed,
                number(&claimed, &["credit"])?,
                streak(&claimed)?,
                claimed
                    .get("credit_delayed")
                    .or_else(|| claimed.get("creditDelayed"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ))
        }
        .await;
        match result {
            Ok(reward) => Ok(reward),
            Err(CallError::Code(1001)) => Err(Outcome::AlreadyChecked),
            Err(CallError::Code(1002)) => Err(Outcome::Ineligible),
            Err(CallError::Code(1003)) => Err(Outcome::Inactive),
            Err(CallError::Unauthorized) => {
                // A working login with a refused check-in needs the desktop device token.
                match self.call(&client, account, SUMMARY_PATH).await {
                    Ok(_) => Err(Outcome::OwnAppRequired),
                    Err(CallError::Unauthorized) => Err(Outcome::SignInRequired),
                    Err(_) => Err(Outcome::Failed),
                }
            }
            Err(_) => Err(Outcome::Failed),
        }
    }

    async fn call(
        &self,
        client: &reqwest::Client,
        account: &Account,
        path: &str,
    ) -> Result<Value, CallError> {
        let mut authorization =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", account.access))
                .map_err(|_| CallError::Invalid)?;
        authorization.set_sensitive(true);
        let response = client
            .post(format!("{}{path}", self.endpoint))
            .header(reqwest::header::AUTHORIZATION, authorization)
            .header("X-User-Id", &account.uid)
            .header("X-Domain", &account.domain)
            .header("X-Product", "SaaS")
            .header("X-IDE-Type", "WorkBuddy")
            .header(reqwest::header::USER_AGENT, "WorkBuddy/5.9.3")
            .header(reqwest::header::ACCEPT, "application/json")
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|_| CallError::Invalid)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CallError::Unauthorized);
        }
        if response.status().is_redirection() {
            return Err(CallError::Invalid);
        }
        let success = response.status().is_success();
        // Do not retain vendor messages, response bodies, paths or credentials in errors.
        let mut bytes = Vec::new();
        let mut response = response;
        while let Some(chunk) = response.chunk().await.map_err(|_| CallError::Invalid)? {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err(CallError::Invalid);
            }
            bytes.extend_from_slice(&chunk);
        }
        let envelope: Value = serde_json::from_slice(&bytes).map_err(|_| CallError::Invalid)?;
        let code = envelope
            .get("code")
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
            .ok_or(CallError::Invalid)?;
        if code == 401 {
            return Err(CallError::Unauthorized);
        }
        if code != 0 {
            return Err(CallError::Code(code));
        }
        if !success {
            return Err(CallError::Invalid);
        }
        envelope
            .get("data")
            .filter(|v| v.is_object())
            .cloned()
            .ok_or(CallError::Invalid)
    }

    pub(crate) fn start_worker(self: Arc<Self>) {
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                // Failure is local to this auxiliary task, never to request forwarding.
                let _ = self.check_in(false).await;
            }
        });
    }
}

enum CallError {
    Invalid,
    Unauthorized,
    Code(i64),
}

fn number(value: &Value, keys: &[&str]) -> Result<Option<f64>, CallError> {
    let Some(value) = keys
        .iter()
        .find_map(|key| value.get(key))
        .filter(|v| !v.is_null())
    else {
        return Ok(None);
    };
    let result = value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .ok_or(CallError::Invalid)?;
    if !result.is_finite() || result < 0.0 {
        return Err(CallError::Invalid);
    }
    Ok(Some(result))
}

fn streak(value: &Value) -> Result<Option<u32>, CallError> {
    number(value, &["streak_days", "streakDays"])?
        .map(|n| {
            if n.fract() == 0.0 && n <= f64::from(u32::MAX) {
                Ok(n as u32)
            } else {
                Err(CallError::Invalid)
            }
        })
        .transpose()
}

fn beijing_time(timestamp: i64) -> Option<DateTime<FixedOffset>> {
    Some(DateTime::from_timestamp(timestamp, 0)?.with_timezone(&FixedOffset::east_opt(8 * 3600)?))
}

fn beijing_day(timestamp: i64) -> String {
    beijing_time(timestamp)
        .map(|time| time.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
