//! Daily rewards through the connected official client's SDK and native accounts.
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

const SCRIPT: &str = include_str!("../../resources/doubao-checkin.js");
const STORAGE_ERROR: &str = "doubaowork.checkin.storageError";
const ACCOUNT_ERROR: &str = "doubaowork.checkin.accountUnavailable";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    NotChecked,
    Claimed,
    AlreadyChecked,
    Inactive,
    SwitchRequired,
    SignInRequired,
    ConnectionRequired,
    Unsupported,
    Failed,
}

#[derive(Deserialize)]
struct NativeAccount {
    id: String,
    label: String,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeSnapshot {
    #[serde(default)]
    accounts: Vec<NativeAccount>,
    current_account_id: Option<String>,
    outcome: Option<Outcome>,
    credit: Option<f64>,
    streak: Option<u32>,
    total_days: Option<u32>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    day: String,
    checked_at: i64,
    outcome: Outcome,
    credit: Option<f64>,
    streak: Option<u32>,
    total_days: Option<u32>,
}
#[derive(Default, Deserialize, Serialize)]
struct Store {
    enabled: BTreeMap<String, bool>,
    records: BTreeMap<String, Record>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountStatus {
    id: String,
    label: String,
    current: bool,
    auto_enabled: bool,
    outcome: Outcome,
    checked_at: Option<i64>,
    credit: Option<f64>,
    streak: Option<u32>,
    total_days: Option<u32>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    connected: bool,
    day: String,
    outcome: Outcome,
    accounts: Vec<AccountStatus>,
}

pub(crate) struct Manager {
    path: PathBuf,
    db: Arc<crate::database::Database>,
    lock: Mutex<()>,
    #[cfg(test)]
    test_endpoint: Option<String>,
}

fn key(id: &str) -> String {
    format!("{:x}", Sha256::digest(format!("doubaowork|{id}")))
}
fn day(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp.saturating_add(8 * 3600), 0)
        .map(|time| time.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

impl Manager {
    pub(crate) fn new(path: PathBuf, db: Arc<crate::database::Database>) -> Self {
        Self {
            path,
            db,
            lock: Mutex::new(()),
            #[cfg(test)]
            test_endpoint: None,
        }
    }

    async fn read_store(&self) -> Result<Store, String> {
        match tokio::fs::read(&self.path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| STORAGE_ERROR.into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
            Err(_) => Err(STORAGE_ERROR.into()),
        }
    }
    async fn write_store(&self, store: &Store) -> Result<(), String> {
        let path = self.path.clone();
        let bytes = serde_json::to_vec(store).map_err(|_| STORAGE_ERROR.to_owned())?;
        tokio::task::spawn_blocking(move || {
            crate::config::atomic_write_private(&path, &bytes).map_err(|_| STORAGE_ERROR.to_owned())
        })
        .await
        .map_err(|_| STORAGE_ERROR.to_owned())?
    }

    async fn bridge(&self, input: Value) -> Result<Value, ()> {
        if !self
            .db
            .get_bool_flag(super::doubao_work::CONSENT_KEY)
            .unwrap_or(false)
        {
            return Err(());
        }
        #[cfg(test)]
        let endpoint = self.test_endpoint.clone().ok_or(())?;
        #[cfg(not(test))]
        let endpoint = super::doubao_work::discover().await.ok_or(())?;
        tokio::time::timeout(Duration::from_secs(25), evaluate(&endpoint, input))
            .await
            .map_err(|_| ())?
    }

    async fn native(&self) -> Result<NativeSnapshot, ()> {
        let value = self.bridge(json!({"mode":"accounts"})).await?;
        let native: NativeSnapshot = serde_json::from_value(value).map_err(|_| ())?;
        if native.accounts.len() > 64 || native.accounts.iter().any(|account| account.id.is_empty())
        {
            return Err(());
        }
        Ok(native)
    }

    async fn native_with_status(&self) -> Result<NativeSnapshot, ()> {
        let mut native = self.native().await?;
        if let Some(current) = native.current_account_id.as_ref() {
            if let Ok(value) = self
                .bridge(json!({"mode":"status","expectedAccountId":current}))
                .await
            {
                if let Ok(status) = serde_json::from_value::<NativeSnapshot>(value) {
                    if matches!(
                        status.outcome,
                        Some(Outcome::NotChecked | Outcome::AlreadyChecked | Outcome::Inactive)
                    ) {
                        native.outcome = status.outcome;
                        native.credit = status.credit;
                        native.streak = status.streak;
                        native.total_days = status.total_days;
                    }
                }
            }
        }
        Ok(native)
    }

    fn snapshot(store: &Store, native: Option<&NativeSnapshot>) -> Snapshot {
        let today = day(chrono::Utc::now().timestamp());
        let accounts = native
            .map(|native| {
                native
                    .accounts
                    .iter()
                    .map(|account| {
                        let id = key(&account.id);
                        let record = store.records.get(&id).filter(|r| r.day == today);
                        let current = native.current_account_id.as_deref() == Some(&account.id);
                        let observed = current.then_some(native);
                        let confirmed = record
                            .filter(|r| {
                                matches!(r.outcome, Outcome::Claimed | Outcome::AlreadyChecked)
                            })
                            .map(|r| r.outcome);
                        AccountStatus {
                            auto_enabled: store.enabled.get(&id).copied().unwrap_or(false),
                            outcome: confirmed
                                .or_else(|| observed.and_then(|s| s.outcome))
                                .or_else(|| record.map(|r| r.outcome))
                                .unwrap_or(Outcome::NotChecked),
                            checked_at: record.map(|r| r.checked_at),
                            credit: observed
                                .and_then(|s| s.credit)
                                .or_else(|| record.and_then(|r| r.credit)),
                            streak: observed
                                .and_then(|s| s.streak)
                                .or_else(|| record.and_then(|r| r.streak)),
                            total_days: observed
                                .and_then(|s| s.total_days)
                                .or_else(|| record.and_then(|r| r.total_days)),
                            id,
                            label: account.label.chars().take(256).collect(),
                            current,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Snapshot {
            connected: native.is_some(),
            day: today,
            outcome: native
                .map(|n| {
                    n.outcome.unwrap_or(if n.current_account_id.is_some() {
                        Outcome::NotChecked
                    } else {
                        Outcome::SignInRequired
                    })
                })
                .unwrap_or(Outcome::ConnectionRequired),
            accounts,
        }
    }

    pub(crate) async fn status(&self) -> Result<Snapshot, String> {
        let _guard = self.lock.lock().await;
        let store = self.read_store().await?;
        Ok(Self::snapshot(
            &store,
            self.native_with_status().await.ok().as_ref(),
        ))
    }

    pub(crate) async fn set_auto(&self, id: &str, enabled: bool) -> Result<Snapshot, String> {
        let _guard = self.lock.lock().await;
        let mut store = self.read_store().await?;
        let native = self
            .native_with_status()
            .await
            .map_err(|_| ACCOUNT_ERROR.to_owned())?;
        if !native.accounts.iter().any(|account| key(&account.id) == id) {
            return Err(ACCOUNT_ERROR.into());
        }
        store.enabled.insert(id.into(), enabled);
        self.write_store(&store).await?;
        Ok(Self::snapshot(&store, Some(&native)))
    }

    fn due(store: &Store, id: &str, manual: bool) -> bool {
        let now = chrono::Utc::now().timestamp();
        store.records.get(id).is_none_or(|record| {
            record.day != day(now)
                || (!matches!(record.outcome, Outcome::Claimed | Outcome::AlreadyChecked)
                    && now >= record.checked_at
                    && (manual || now - record.checked_at >= 30 * 60))
        })
    }

    async fn select_account(
        &self,
        native: &NativeSnapshot,
        account: &NativeAccount,
    ) -> Result<(), Outcome> {
        let current = native
            .current_account_id
            .as_ref()
            .ok_or(Outcome::SignInRequired)?;
        if current == &account.id {
            return Ok(());
        }
        let result = self
            .bridge(
                json!({"mode":"switch","expectedAccountId":current,"targetAccountId":account.id}),
            )
            .await;
        if let Ok(value) = result {
            match value.get("outcome").and_then(Value::as_str) {
                Some("switched") => {}
                Some("sign_in_required") => return Err(Outcome::SignInRequired),
                Some("switch_required") => return Err(Outcome::SwitchRequired),
                Some("unsupported") => return Err(Outcome::Unsupported),
                _ => return Err(Outcome::Failed),
            }
        }
        // The official switch may reload its page and destroy the evaluation context.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(after) = self.native().await {
                if after.current_account_id.as_ref() == Some(&account.id) {
                    return Ok(());
                }
                if after
                    .current_account_id
                    .as_ref()
                    .is_some_and(|id| id != current)
                {
                    return Err(Outcome::SwitchRequired);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Outcome::Failed);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn save_result(
        &self,
        store: &mut Store,
        id: &str,
        result: NativeSnapshot,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();
        store.records.insert(
            id.into(),
            Record {
                day: day(now),
                checked_at: now,
                outcome: result.outcome.unwrap_or(Outcome::Failed),
                credit: result.credit,
                streak: result.streak,
                total_days: result.total_days,
            },
        );
        self.write_store(store).await
    }

    async fn claim(&self, store: &mut Store, account: &NativeAccount) -> Result<Outcome, String> {
        let result = self
            .bridge(json!({"mode":"checkin","expectedAccountId":account.id}))
            .await
            .ok()
            .and_then(|v| serde_json::from_value::<NativeSnapshot>(v).ok())
            .unwrap_or_default();
        let outcome = result.outcome.unwrap_or(Outcome::Failed);
        self.save_result(store, &key(&account.id), result).await?;
        Ok(outcome)
    }

    async fn process_account(
        &self,
        store: &mut Store,
        native: &NativeSnapshot,
        account: &NativeAccount,
    ) -> Result<Outcome, String> {
        if let Err(outcome) = self.select_account(native, account).await {
            self.save_result(
                store,
                &key(&account.id),
                NativeSnapshot {
                    outcome: Some(outcome),
                    ..NativeSnapshot::default()
                },
            )
            .await?;
            return Ok(outcome);
        }
        self.claim(store, account).await
    }

    pub(crate) async fn check_in(&self, id: &str) -> Result<Snapshot, String> {
        let _guard = self.lock.lock().await;
        let mut store = self.read_store().await?;
        let native = self.native().await.map_err(|_| ACCOUNT_ERROR.to_owned())?;
        let account = native
            .accounts
            .iter()
            .find(|account| key(&account.id) == id)
            .ok_or(ACCOUNT_ERROR)?;
        if Self::due(&store, id, true) {
            self.process_account(&mut store, &native, account).await?;
        }
        Ok(Self::snapshot(&store, self.native().await.ok().as_ref()))
    }

    pub(crate) async fn run_auto(&self) -> Result<(), String> {
        let _guard = self.lock.lock().await;
        let mut store = self.read_store().await?;
        if !store.enabled.values().any(|enabled| *enabled) {
            return Ok(());
        }
        let Ok(native) = self.native().await else {
            return Ok(());
        };
        let ids: Vec<_> = native
            .accounts
            .iter()
            .map(|account| key(&account.id))
            .filter(|id| {
                store.enabled.get(id).copied().unwrap_or(false) && Self::due(&store, id, false)
            })
            .collect();
        for id in ids {
            let Ok(native) = self.native().await else {
                break;
            };
            let Some(account) = native.accounts.iter().find(|a| key(&a.id) == id) else {
                continue;
            };
            let outcome = self.process_account(&mut store, &native, account).await?;
            if !matches!(
                outcome,
                Outcome::Claimed | Outcome::AlreadyChecked | Outcome::Inactive
            ) {
                break;
            }
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

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
async fn rpc(
    socket: &mut Socket,
    next: &mut u64,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value, ()> {
    *next += 1;
    let mut command = json!({"id":next,"method":method,"params":params});
    if let Some(session) = session {
        command["sessionId"] = session.into();
    }
    socket
        .send(Message::Text(command.to_string().into()))
        .await
        .map_err(|_| ())?;
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(raw) = message else {
            continue;
        };
        if raw.len() > 1024 * 1024 {
            return Err(());
        }
        let value: Value = serde_json::from_str(&raw).map_err(|_| ())?;
        if value["id"].as_u64() == Some(*next) {
            return value.get("result").cloned().ok_or(());
        }
    }
    Err(())
}

async fn evaluate(endpoint: &str, input: Value) -> Result<Value, ()> {
    let (mut socket, _) = tokio_tungstenite::connect_async(endpoint)
        .await
        .map_err(|_| ())?;
    let mut next = 0;
    let targets = rpc(&mut socket, &mut next, None, "Target.getTargets", json!({})).await?;
    let target = targets["targetInfos"]
        .as_array()
        .ok_or(())?
        .iter()
        .find(|target| {
            target["type"] == "page"
                && target["url"]
                    .as_str()
                    .and_then(|s| url::Url::parse(s).ok())
                    .is_some_and(|url| {
                        url.scheme() == "doubaowork"
                            && url.host_str() == Some("doubaowork-chat")
                            && url.path() == "/chat"
                    })
        })
        .and_then(|t| t["targetId"].as_str())
        .ok_or(())?;
    let attached = rpc(
        &mut socket,
        &mut next,
        None,
        "Target.attachToTarget",
        json!({"targetId":target,"flatten":true}),
    )
    .await?;
    let session = attached["sessionId"].as_str().ok_or(())?;
    let result = rpc(&mut socket,&mut next,Some(session),"Runtime.evaluate",json!({"expression":format!("({SCRIPT})(/* VARYAI_CHECKIN_INPUT */{input})"),"awaitPromise":true,"returnByValue":true})).await;
    let _ = rpc(
        &mut socket,
        &mut next,
        None,
        "Target.detachFromTarget",
        json!({"sessionId":session}),
    )
    .await;
    let _ = socket.close(None).await;
    result?.pointer("/result/value").cloned().ok_or(())
}

#[cfg(test)]
mod tests;
