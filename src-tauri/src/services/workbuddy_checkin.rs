//! Optional daily credits for independently authorized WorkBuddy China accounts.
//! Kept separate from forwarding, client configuration and synchronized settings.
use chrono::{DateTime, FixedOffset, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

const ENDPOINT: &str = "https://copilot.tencent.com";
const STATUS_PATH: &str = "/v2/billing/meter/checkin-activity-status";
const CLAIM_PATH: &str = "/v2/billing/meter/daily-checkin";
const SUMMARY_PATH: &str = "/billing/meter/get-user-resource-summary";
const STORAGE_ERROR: &str = "workbuddy.checkin.storageError";
const RETRY_SECONDS: i64 = 30 * 60;
const NATIVE_AUTH_SCRIPT: &str = include_str!("../../resources/workbuddy-checkin-auth.cjs");
const NATIVE_AUTH_MARKER: &[u8] = b"\nVARYAI_WORKBUDDY_AUTH:";

fn native_node_mode_enabled(runtime: &Path) -> bool {
    let Some(contents) = runtime.parent().and_then(Path::parent) else {
        return false;
    };
    let framework =
        contents.join("Frameworks/Electron Framework.framework/Versions/A/Electron Framework");
    let Ok(mut file) = std::fs::File::open(framework) else {
        return false;
    };
    let Ok(marker) = aho_corasick::AhoCorasick::new(["dL7pKGdnNz796PbbjQWNKmHXBZaB9tsX"]) else {
        return false;
    };
    let offset = match marker.stream_find_iter(&mut file).next() {
        Some(Ok(found)) => found.end() as u64,
        _ => return false,
    };
    let mut flags = [0; 3];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.read_exact(&mut flags))
        .is_ok()
        && flags[0] == 1
        && flags[1] > 0
        && flags[2] == b'1'
}

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
    CredentialsProtected,
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
    #[serde(default)]
    account_auto: BTreeMap<String, bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    auto_enabled: bool,
    auto_configured: bool,
    day: String,
    outcome: Outcome,
    saved_outcome: Option<Outcome>,
    checked_at: Option<i64>,
    credit: Option<f64>,
    streak: Option<u32>,
    credit_delayed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    uid: String,
    #[serde(rename = "accessToken")]
    access: String,
    domain: String,
    #[serde(default)]
    client_version: Option<String>,
    #[serde(default)]
    device_token: Option<String>,
    #[serde(default)]
    enterprise_id: String,
    #[serde(default)]
    label: String,
    #[serde(skip)]
    native: bool,
}

impl Account {
    fn key(&self) -> String {
        let identity = if self.enterprise_id.is_empty() {
            format!("workbuddy|{}", self.uid)
        } else {
            format!("workbuddy|{}|{}", self.uid, self.enterprise_id)
        };
        format!("{:x}", Sha256::digest(identity))
    }
}

/// One manager serializes manual, automatic and preference updates.
pub(crate) struct Manager {
    path: PathBuf,
    auth_path: PathBuf,
    endpoint: String,
    native_runtime: Option<PathBuf>,
    lock: Mutex<()>,
    pending_login: Mutex<Option<accounts::PendingLogin>>,
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
        #[cfg(target_os = "macos")]
        let native_runtime = [
            PathBuf::from("/Applications/WorkBuddy.app/Contents/MacOS/Electron"),
            home.join("Applications/WorkBuddy.app/Contents/MacOS/Electron"),
        ]
        .into_iter()
        .find(|path| path.is_file());
        #[cfg(not(target_os = "macos"))]
        let native_runtime = None;
        let mut manager =
            Self::with_paths(path, base.join("Data/Public/auth/workbuddy-desktop.info"));
        manager.native_runtime = native_runtime;
        manager
    }

    fn with_paths(path: PathBuf, auth_path: PathBuf) -> Self {
        Self {
            path,
            auth_path,
            endpoint: ENDPOINT.into(),
            native_runtime: None,
            lock: Mutex::new(()),
            pending_login: Mutex::new(None),
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
        let account = tokio::task::spawn_blocking(move || {
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
            if value
                .pointer("/auth/accessToken/$wbEncrypted")
                .and_then(Value::as_u64)
                == Some(1)
            {
                return Err(Outcome::CredentialsProtected);
            }
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
                client_version: None,
                device_token: None,
                enterprise_id: field("/account/enterpriseId").unwrap_or_default(),
                label: field("/account/nickname").unwrap_or_default(),
                native: true,
            })
        })
        .await
        .map_err(|_| Outcome::CredentialsUnavailable)?;
        match account {
            Err(Outcome::CredentialsProtected) if self.native_runtime.is_some() => {
                self.native_account("session").await
            }
            result => result,
        }
    }

    async fn native_account(&self, mode: &str) -> Result<Account, Outcome> {
        let runtime = self
            .native_runtime
            .as_ref()
            .ok_or(Outcome::CredentialsProtected)?;
        let path = runtime.clone();
        if !tokio::task::spawn_blocking(move || native_node_mode_enabled(&path))
            .await
            .map_err(|_| Outcome::CredentialsProtected)?
        {
            return Err(Outcome::CredentialsProtected);
        }
        let output = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new(runtime)
                .env("ELECTRON_RUN_AS_NODE", "1")
                .args(["--eval", NATIVE_AUTH_SCRIPT, "--", mode])
                .arg(&self.auth_path)
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| Outcome::CredentialsProtected)?
        .map_err(|_| Outcome::CredentialsProtected)?;
        if !output.status.success() || output.stdout.len() > 64 * 1024 {
            return Err(Outcome::CredentialsProtected);
        }
        // The installed SDK can emit diagnostics. Only the framed response is consumed.
        let offset = output
            .stdout
            .windows(NATIVE_AUTH_MARKER.len())
            .rposition(|window| window == NATIVE_AUTH_MARKER)
            .ok_or(Outcome::CredentialsProtected)?
            + NATIVE_AUTH_MARKER.len();
        let value: Value = serde_json::from_slice(&output.stdout[offset..])
            .map_err(|_| Outcome::CredentialsProtected)?;
        if value.get("error").and_then(Value::as_str) == Some("sign_in_required") {
            return Err(Outcome::SignInRequired);
        }
        let mut account: Account =
            serde_json::from_value(value).map_err(|_| Outcome::CredentialsProtected)?;
        if account.uid.trim().is_empty() || account.access.trim().is_empty() {
            return Err(Outcome::CredentialsProtected);
        }
        account.native = true;
        Ok(account)
    }

    fn snapshot(store: &Store, account: &Result<Account, Outcome>, now: i64) -> Status {
        let day = beijing_day(now);
        let record = account
            .as_ref()
            .ok()
            .and_then(|a| store.records.get(&a.key()))
            .filter(|record| record.day == day);
        let saved = account.as_ref().err().and_then(|_| {
            store
                .records
                .values()
                .filter(|r| {
                    r.day == day && matches!(r.outcome, Outcome::Claimed | Outcome::AlreadyChecked)
                })
                .max_by_key(|r| r.checked_at)
        });
        let reward = record.or(saved);
        Status {
            auto_configured: store.auto_enabled
                || store.account_auto.values().any(|enabled| *enabled),
            auto_enabled: account.as_ref().ok().is_some_and(|account| {
                store
                    .account_auto
                    .get(&account.key())
                    .copied()
                    .unwrap_or(store.auto_enabled && account.native)
            }),
            day,
            outcome: account
                .as_ref()
                .err()
                .copied()
                .unwrap_or_else(|| record.map_or(Outcome::NotChecked, |r| r.outcome)),
            saved_outcome: saved.map(|r| r.outcome),
            checked_at: reward.map(|r| r.checked_at),
            credit: reward.and_then(|r| r.credit),
            streak: reward.and_then(|r| r.streak),
            credit_delayed: reward.is_some_and(|r| r.credit_delayed),
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
        let account = self.account().await;
        if let Ok(ref account) = account {
            store.account_auto.insert(account.key(), enabled);
            store.auto_enabled = false;
        } else {
            return Err(STORAGE_ERROR.into());
        }
        self.write_store(&store).await?;
        Ok(Self::snapshot(&store, &account, Utc::now().timestamp()))
    }

    pub(crate) async fn check_in(&self, manual: bool) -> Result<Status, String> {
        let _guard = self.lock.lock().await;
        let mut store = self.read_store().await?;
        let account = self.account().await;
        let now = Utc::now().timestamp();
        let Ok(ref current) = account else {
            return Ok(Self::snapshot(&store, &account, now));
        };
        if (!manual && !Self::snapshot(&store, &account, now).auto_enabled)
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
        let native = if account.native && self.native_runtime.is_some() {
            self.native_account("request").await.ok()
        } else {
            None
        };
        if native
            .as_ref()
            .is_some_and(|current| current.key() != account.key())
        {
            return Err(Outcome::Failed);
        }
        let account = native.as_ref().unwrap_or(account);
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
        let mut request = client
            .post(format!("{}{path}", self.endpoint))
            .header(reqwest::header::AUTHORIZATION, authorization)
            .header("X-User-Id", &account.uid)
            .header("X-Domain", &account.domain)
            .header("X-Product", "SaaS")
            .header("X-IDE-Type", "WorkBuddy")
            .header(
                reqwest::header::USER_AGENT,
                format!(
                    "WorkBuddy/{}",
                    account.client_version.as_deref().unwrap_or("5.9.3")
                ),
            )
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(token) = &account.device_token {
            let mut token =
                reqwest::header::HeaderValue::from_str(token).map_err(|_| CallError::Invalid)?;
            token.set_sensitive(true);
            request = request.header("X-Device-Token", token);
        }
        if !account.enterprise_id.is_empty() {
            request = request
                .header("X-Enterprise-Id", &account.enterprise_id)
                .header("X-Tenant-Id", &account.enterprise_id);
        }
        let response = request
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
                let _ = self.run_auto().await;
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

mod accounts;
#[cfg(test)]
mod tests;
#[cfg(test)]
use accounts::ManagedAccount;
pub(crate) use accounts::{AccountList, LoginStart, LoginStatus};
#[cfg(test)]
mod accounts_tests;
