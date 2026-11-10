//! Optional vendor rewards, isolated from routing and synchronized configuration.
#[cfg(target_os = "macos")]
mod qoder_native;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::PathBuf, process::Stdio, sync::Arc, time::Duration};
use tokio::{io::AsyncWriteExt, sync::Mutex};

const APPS: [&str; 3] = ["trae", "qoder", "zcode"];
const ERROR: &str = "agentRewards.operationFailed";
const STORAGE: &str = "agentRewards.storageError";
const SCRIPT: &str = include_str!("../../resources/agent-rewards.cjs");

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
    ConnectionRequired,
    SwitchRequired,
    Unsupported,
    Failed,
}
impl Outcome {
    fn confirmed(self) -> bool {
        matches!(self, Self::Claimed | Self::AlreadyChecked)
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    browser_account_id: Option<String>,
    source: Option<AccountSource>,
    #[serde(default)]
    native_available: bool,
    account_id: Option<String>,
    label: Option<String>,
    outcome: Outcome,
    credit: Option<f64>,
    until: Option<i64>,
    window_key: Option<String>,
}
impl Observation {
    #[cfg(test)]
    fn outcome(outcome: Outcome) -> Self {
        Self {
            browser_account_id: None,
            source: None,
            native_available: false,
            account_id: None,
            label: None,
            outcome,
            credit: None,
            until: None,
            window_key: None,
        }
    }
    fn validate(mut self) -> Result<Self, String> {
        if self
            .account_id
            .as_ref()
            .is_some_and(|id| id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()))
            || self
                .browser_account_id
                .as_ref()
                .is_some_and(|id| id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()))
            || self
                .window_key
                .as_ref()
                .is_some_and(|key| key.len() > 128 || key.chars().any(char::is_control))
            || self
                .credit
                .is_some_and(|n| !n.is_finite() || n <= 0.0 || n > 1e15)
            || (self.outcome.confirmed() && self.account_id.is_none())
        {
            return Err(ERROR.into());
        }
        self.label = self.label.map(|label| {
            label
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect()
        });
        Ok(self)
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct Record {
    outcome: Outcome,
    credit: Option<f64>,
    checked_at: i64,
    until: i64,
    window: Option<String>,
}
#[derive(Default, Deserialize, Serialize)]
struct Store {
    enabled: BTreeMap<String, bool>,
    records: BTreeMap<String, Record>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    browser_account_id: Option<String>,
    source: Option<AccountSource>,
    native_available: bool,
    app: String,
    account_id: Option<String>,
    label: Option<String>,
    auto_enabled: bool,
    auto_configured: bool,
    outcome: Outcome,
    credit: Option<f64>,
    checked_at: Option<i64>,
    next_check_at: Option<i64>,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum AccountSource {
    Native,
    Browser,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginStart {
    login_id: String,
    url: String,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct LoginStatus {
    complete: bool,
}
pub(crate) struct Manager {
    path: PathBuf,
    db: Arc<crate::database::Database>,
    lock: Mutex<()>,
    #[cfg(target_os = "macos")]
    native_cache: Arc<std::sync::Mutex<qoder_native::Cache>>,
}
fn valid_app(app: &str) -> Result<(), String> {
    APPS.contains(&app)
        .then_some(())
        .ok_or_else(|| ERROR.into())
}
fn key(app: &str, account: &str) -> String {
    format!("{app}|{account}")
}
fn cached(record: &Record, native: &Observation, now: i64) -> bool {
    record.outcome.confirmed()
        && now < record.until
        && (record.window == native.window_key || native.outcome == Outcome::Inactive)
        && matches!(
            native.outcome,
            Outcome::NotChecked | Outcome::AlreadyChecked | Outcome::Inactive
        )
}
impl Manager {
    pub(crate) fn new(path: PathBuf, db: Arc<crate::database::Database>) -> Self {
        Self {
            path,
            db,
            lock: Mutex::new(()),
            #[cfg(target_os = "macos")]
            native_cache: Arc::new(std::sync::Mutex::new(qoder_native::Cache::default())),
        }
    }
    async fn read(&self) -> Result<Store, String> {
        match tokio::fs::read(&self.path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| STORAGE.into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
            Err(_) => Err(STORAGE.into()),
        }
    }
    async fn save(&self, store: &Store) -> Result<(), String> {
        let bytes = serde_json::to_vec(store).map_err(|_| STORAGE.to_owned())?;
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            crate::config::atomic_write_private(&path, &bytes).map_err(|_| STORAGE.to_owned())
        })
        .await
        .map_err(|_| STORAGE.to_owned())?
    }
    async fn helper(&self, app: &str, mut input: Value) -> Result<Value, String> {
        valid_app(app)?;
        if app == "trae" {
            if !self
                .db
                .get_bool_flag(super::trae_bridge::CONSENT_KEY)
                .unwrap_or(false)
            {
                return Ok(json!({"outcome":"connection_required"}));
            }
            return tokio::task::spawn_blocking(move || super::trae_bridge::rewards(input))
                .await
                .map_err(|_| ERROR.to_owned())?
                .or_else(|_| Ok(json!({"outcome":"connection_required"})));
        }
        let directory = self.path.parent().ok_or(ERROR)?.join("agent-rewards");
        #[cfg(target_os = "macos")]
        if app == "qoder" && matches!(input["mode"].as_str(), Some("status" | "claim")) {
            let cache = self.native_cache.clone();
            let home = crate::config::get_home_dir();
            let interactive = input["nativeInteractive"] == true;
            if let Some(native) = tokio::task::spawn_blocking(move || {
                let mut cache = cache.lock().ok()?;
                qoder_native::input(&home, &mut cache, interactive)
            })
            .await
            .map_err(|_| ERROR.to_owned())?
            {
                input["native"] = native;
            }
        }
        input["app"] = json!(app);
        input["directory"] = json!(directory);
        let runtime = tokio::task::spawn_blocking(crate::commands::reward_node_runtime)
            .await
            .map_err(|_| ERROR.to_owned())?;
        let Some(runtime) = runtime else {
            return Ok(json!({"outcome":"unsupported"}));
        };
        let mut command = tokio::process::Command::new(runtime);
        command
            .args(["-e", SCRIPT])
            .env_clear()
            .env("VARYAI_AGENT_REWARD_HELPER", "1");
        // An explicit native credential secret remains in the child process only.
        if let Some(secret) = std::env::var_os("ZCODE_CREDENTIAL_SECRET") {
            command.env("ZCODE_CREDENTIAL_SECRET", secret);
        }
        #[cfg(target_os = "windows")]
        for name in ["SYSTEMROOT", "USERPROFILE", "LOCALAPPDATA"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| ERROR.to_owned())?;
        let bytes = serde_json::to_vec(&input).map_err(|_| ERROR.to_owned())?;
        let mut stdin = child.stdin.take().ok_or(ERROR)?;
        stdin
            .write_all(&bytes)
            .await
            .map_err(|_| ERROR.to_owned())?;
        drop(stdin);
        let output = tokio::time::timeout(Duration::from_secs(45), child.wait_with_output())
            .await
            .map_err(|_| ERROR.to_owned())?
            .map_err(|_| ERROR.to_owned())?;
        if !output.status.success() || output.stdout.len() > 1024 * 1024 {
            return Err(ERROR.into());
        }
        let value: Value = serde_json::from_slice(&output.stdout).map_err(|_| ERROR.to_owned())?;
        #[cfg(target_os = "macos")]
        if app == "qoder"
            && input["native"]["password"].is_string()
            && value["source"] == "native"
            && value["outcome"] == "credentials_protected"
        {
            // A rejected cached key may have rotated; the next explicit read can reauthorize.
            if let Ok(mut cache) = self.native_cache.lock() {
                cache.clear();
            }
        }
        if value["outcome"] == "storage_error" {
            return Err(STORAGE.into());
        }
        Ok(value)
    }
    async fn observe(&self, app: &str, claim: Option<&str>) -> Result<Observation, String> {
        let value = self.helper(app, json!({"mode":if claim.is_some() {"claim"} else {"status"}, "expectedAccountId":claim})).await?;
        serde_json::from_value::<Observation>(value)
            .map_err(|_| ERROR.to_owned())?
            .validate()
    }
    fn snapshot(app: &str, store: &Store, native: &Observation) -> Status {
        let now = chrono::Utc::now().timestamp_millis();
        let id = native.account_id.as_ref().map(|id| key(app, id));
        let record = id.as_ref().and_then(|id| store.records.get(id));
        let confirmed = record.filter(|record| cached(record, native, now));
        Status {
            browser_account_id: native.browser_account_id.clone(),
            source: native.source,
            native_available: native.native_available,
            app: app.into(),
            account_id: native.account_id.clone(),
            label: native.label.clone(),
            auto_enabled: id
                .as_ref()
                .and_then(|id| store.enabled.get(id))
                .copied()
                .unwrap_or(false),
            auto_configured: store
                .enabled
                .iter()
                .any(|(id, enabled)| *enabled && id.starts_with(&format!("{app}|"))),
            outcome: confirmed.map_or(native.outcome, |r| r.outcome),
            credit: native.credit.or_else(|| confirmed.and_then(|r| r.credit)),
            checked_at: record.map(|r| r.checked_at),
            next_check_at: native.until.or_else(|| record.map(|r| r.until)),
        }
    }
    pub(crate) async fn status(&self, app: &str) -> Result<Status, String> {
        valid_app(app)?;
        let _guard = self.lock.lock().await;
        let store = self.read().await?;
        let native = self.observe(app, None).await?;
        Ok(Self::snapshot(app, &store, &native))
    }
    pub(crate) async fn connect_qoder_native(&self) -> Result<Status, String> {
        let _guard = self.lock.lock().await;
        let store = self.read().await?;
        let value = self
            .helper("qoder", json!({"mode":"status","nativeInteractive":true}))
            .await?;
        let native = serde_json::from_value::<Observation>(value)
            .map_err(|_| ERROR.to_owned())?
            .validate()?;
        Ok(Self::snapshot("qoder", &store, &native))
    }
    pub(crate) async fn set_auto(
        &self,
        app: &str,
        id: &str,
        enabled: bool,
    ) -> Result<Status, String> {
        valid_app(app)?;
        let _guard = self.lock.lock().await;
        let mut store = self.read().await?;
        let native = self.observe(app, None).await?;
        if native.account_id.as_deref() != Some(id) {
            return Err("agentRewards.accountChanged".into());
        }
        store.enabled.insert(key(app, id), enabled);
        self.save(&store).await?;
        Ok(Self::snapshot(app, &store, &native))
    }
    async fn claim_locked(
        &self,
        app: &str,
        id: &str,
        store: &mut Store,
        native: Observation,
    ) -> Result<Status, String> {
        if native.account_id.as_deref() != Some(id) {
            return Err("agentRewards.accountChanged".into());
        }
        let now = chrono::Utc::now().timestamp_millis();
        if store
            .records
            .get(&key(app, id))
            .is_some_and(|r| cached(r, &native, now))
        {
            return Ok(Self::snapshot(app, store, &native));
        }
        let result = if native.outcome == Outcome::NotChecked {
            self.observe(app, Some(id)).await?
        } else {
            native
        };
        if result.account_id.as_deref() == Some(id) {
            store.records.insert(
                key(app, id),
                Record {
                    outcome: result.outcome,
                    credit: result.credit,
                    checked_at: now,
                    until: result
                        .until
                        .filter(|until| *until > now)
                        .unwrap_or(now + 30 * 60 * 1000),
                    window: result.window_key.clone(),
                },
            );
            self.save(store).await?;
        }
        Ok(Self::snapshot(app, store, &result))
    }
    pub(crate) async fn claim(&self, app: &str, id: &str) -> Result<Status, String> {
        valid_app(app)?;
        let _guard = self.lock.lock().await;
        let mut store = self.read().await?;
        let native = self.observe(app, None).await?;
        self.claim_locked(app, id, &mut store, native).await
    }
    pub(crate) async fn start_login(&self, app: &str) -> Result<LoginStart, String> {
        if app != "qoder" {
            return Err(ERROR.into());
        }
        let _guard = self.lock.lock().await;
        let result = self.helper(app, json!({"mode":"login_start"})).await?;
        let login: LoginStart = serde_json::from_value(result).map_err(|_| ERROR.to_owned())?;
        let url = url::Url::parse(&login.url).map_err(|_| ERROR.to_owned())?;
        if url.scheme() != "https"
            || url.host_str() != Some("qoder.cn")
            || url.path() != "/device/selectAccounts"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
        {
            return Err(ERROR.into());
        }
        Ok(login)
    }
    pub(crate) async fn poll_login(
        &self,
        app: &str,
        login_id: &str,
    ) -> Result<LoginStatus, String> {
        if app != "qoder" {
            return Err(ERROR.into());
        }
        let _guard = self.lock.lock().await;
        serde_json::from_value(
            self.helper(app, json!({"mode":"login_poll","loginId":login_id}))
                .await?,
        )
        .map_err(|_| ERROR.into())
    }
    pub(crate) async fn cancel_login(&self, app: &str, login_id: &str) -> Result<(), String> {
        if app != "qoder" {
            return Err(ERROR.into());
        }
        let _guard = self.lock.lock().await;
        let value = self
            .helper(app, json!({"mode":"login_cancel","loginId":login_id}))
            .await?;
        if value["complete"] != true {
            return Err(ERROR.into());
        }
        Ok(())
    }
    pub(crate) async fn forget(&self, app: &str, id: &str) -> Result<(), String> {
        if app != "qoder" {
            return Err(ERROR.into());
        }
        let _guard = self.lock.lock().await;
        let mut store = self.read().await?;
        let value = self
            .helper(app, json!({"mode":"forget","expectedAccountId":id}))
            .await?;
        if value["complete"] != true {
            return Err(ERROR.into());
        }
        store.enabled.remove(&key(app, id));
        self.save(&store).await
    }
    async fn run_auto(&self) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        let mut store = self.read().await?;
        let now = chrono::Utc::now().timestamp_millis();
        for app in APPS {
            if !store.enabled.iter().any(|(id, on)| {
                *on && id.starts_with(&format!("{app}|"))
                    && store.records.get(id).is_none_or(|r| now >= r.until)
            }) {
                continue;
            }
            let Ok(native) = self.observe(app, None).await else {
                continue;
            };
            let Some(id) = native
                .account_id
                .clone()
                .filter(|id| store.enabled.get(&key(app, id)) == Some(&true))
            else {
                continue;
            };
            let _ = self.claim_locked(app, &id, &mut store, native).await;
        }
        Ok(())
    }
    pub(crate) fn start_worker(self: Arc<Self>) {
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let _ = self.run_auto().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn saved_auto_mode_survives_reopening_without_a_native_account() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rewards.json");
        let db = Arc::new(crate::database::Database::memory().unwrap());
        let manager = Manager::new(path.clone(), db.clone());
        let mut store = Store::default();
        store
            .enabled
            .insert(key("qoder", "synthetic-account"), true);
        manager.save(&store).await.unwrap();
        let reopened = Manager::new(path, db);
        let store = reopened.read().await.unwrap();
        for outcome in [Outcome::CredentialsProtected, Outcome::ConnectionRequired] {
            let native = Observation::outcome(outcome);
            let value = serde_json::to_value(Manager::snapshot("qoder", &store, &native)).unwrap();
            assert_eq!(value["autoConfigured"], true);
            assert_eq!(value["autoEnabled"], false);
            let other = serde_json::to_value(Manager::snapshot("trae", &store, &native)).unwrap();
            assert_eq!(other["autoConfigured"], false);
        }
        let mut native = Observation::outcome(Outcome::NotChecked);
        native.account_id = Some("another-account".into());
        assert!(!Manager::snapshot("qoder", &store, &native).auto_enabled);
    }
    #[test]
    fn confirmed_rewards_expire_at_the_activity_boundary_and_never_mask_a_new_offer() {
        let record = Record {
            outcome: Outcome::Claimed,
            credit: Some(100.0),
            checked_at: 1,
            until: 20,
            window: Some("one".into()),
        };
        let mut native = Observation::outcome(Outcome::NotChecked);
        native.window_key = Some("one".into());
        assert!(cached(&record, &native, 19));
        assert!(!cached(&record, &native, 20));
        native.window_key = Some("two".into());
        assert!(!cached(&record, &native, 19));
        native.outcome = Outcome::SignInRequired;
        assert!(!cached(&record, &native, 19));
    }
    #[tokio::test]
    async fn corrupt_records_are_preserved_and_invalid_agents_cannot_start_helpers() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("records.json");
        std::fs::write(&file, b"synthetic-corrupt").unwrap();
        let manager = Manager::new(
            file.clone(),
            Arc::new(crate::database::Database::memory().unwrap()),
        );
        assert!(manager.read().await.is_err());
        assert!(manager.status("unrecognized").await.is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"synthetic-corrupt");
    }
}
