//! Device-local quota history and explicit, identity-bound Codex controls.
pub(crate) mod core;
pub(crate) mod network;
#[cfg(test)]
mod tests;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::services::intent_routing::catalog::ExecutionCredentials;
use crate::services::subscription::SubscriptionQuota;
pub(crate) use core::{CreditsMode, Point, Policy, ResetCard};
use core::{Entry, Store};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, RwLock, Weak,
    },
};
use tokio::sync::Mutex;

const FAILED: &str = "quotaPanel.failed";
const STORAGE: &str = "quotaPanel.storage";
#[derive(Clone)]
struct Runtime {
    manager: Weak<Manager>,
    account: String,
    policy: Policy,
    observed: Option<SubscriptionQuota>,
}
static RUNTIME: LazyLock<RwLock<BTreeMap<String, Runtime>>> =
    LazyLock::new(|| RwLock::new(BTreeMap::new()));
static POLICY_READY: AtomicBool = AtomicBool::new(true);

struct FileStore {
    path: PathBuf,
    store: Mutex<Option<Store>>,
}
impl FileStore {
    fn load(path: &std::path::Path) -> Result<Store, String> {
        use std::io::Read;
        let store = match std::fs::symlink_metadata(path) {
            Ok(meta) if !meta.is_symlink() && meta.is_file() && meta.len() <= 4 * 1024 * 1024 => {
                let mut bytes = Vec::new();
                std::fs::File::open(path)
                    .map_err(|_| STORAGE)?
                    .take(4 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| STORAGE)?;
                if bytes.len() > 4 * 1024 * 1024 {
                    return Err(STORAGE.into());
                }
                serde_json::from_slice::<Store>(&bytes).map_err(|_| STORAGE)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Store::default(),
            _ => return Err(STORAGE.into()),
        };
        if !store.valid() {
            return Err(STORAGE.into());
        }
        Ok(store)
    }
    async fn read(&self) -> Result<Store, String> {
        let mut cached = self.store.lock().await;
        if let Some(store) = cached.as_ref() {
            return Ok(store.clone());
        }
        let path = self.path.clone();
        let store = tokio::task::spawn_blocking(move || Self::load(&path))
            .await
            .map_err(|_| STORAGE)??;
        *cached = Some(store.clone());
        Ok(store)
    }
    async fn update<T>(
        &self,
        change: impl FnOnce(&mut Store) -> Result<T, String>,
    ) -> Result<T, String> {
        self.read().await?;
        let mut guard = self.store.lock().await;
        let mut next = guard.as_ref().cloned().ok_or(STORAGE)?;
        let result = change(&mut next)?;
        if !next.valid() {
            return Err(STORAGE.into());
        }
        let bytes = serde_json::to_vec(&next).map_err(|_| STORAGE)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(STORAGE.into());
        }
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || crate::config::atomic_write_private(&path, &bytes))
            .await
            .map_err(|_| STORAGE)?
            .map_err(|_| STORAGE)?;
        *guard = Some(next);
        Ok(result)
    }
}
pub(crate) struct Manager {
    data: FileStore,
    history: FileStore,
    auth: Arc<CodexOAuthManager>,
    spending: Mutex<()>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Controls {
    pub identity: String,
    pub policy: Policy,
    pub resets: Vec<ResetCard>,
    pub pending: bool,
    pub pending_credit: Option<String>,
}
impl Manager {
    pub fn new(path: PathBuf, auth: Arc<CodexOAuthManager>) -> Arc<Self> {
        #[cfg(not(test))]
        let initial = {
            let result = FileStore::load(&path);
            POLICY_READY.store(result.is_ok(), Ordering::Release);
            result.ok()
        };
        #[cfg(test)]
        let initial = None;
        let entries = initial
            .as_ref()
            .map(|s: &Store| s.entries.clone())
            .unwrap_or_default();
        let manager = Arc::new(Self {
            history: FileStore {
                path: path.with_file_name("quota-history.json"),
                store: Mutex::new(None),
            },
            data: FileStore {
                path,
                store: Mutex::new(initial),
            },
            auth,
            spending: Mutex::new(()),
        });
        for (identity, entry) in entries {
            manager.publish(&identity, &entry, None);
        }
        #[cfg(not(test))]
        {
            let weak = Arc::downgrade(&manager);
            tauri::async_runtime::spawn(async move {
                loop {
                    if let Some(manager) = weak.upgrade() {
                        if let Ok(store) = manager.read().await {
                            for (identity, entry) in store.entries {
                                manager.publish(&identity, &entry, None);
                                if entry.policy.credits != CreditsMode::On {
                                    if let Ok(context) = manager.context(&entry.account).await {
                                        if context.quota_identity().as_deref()
                                            == Some(identity.as_str())
                                        {
                                            if let Ok(quota) =
                                                manager.read_quota(&entry.account, &context).await
                                            {
                                                manager.publish(&identity, &entry, Some(quota));
                                            }
                                        }
                                    }
                                }
                                if entry.policy.expiring && entry.pending.is_none() {
                                    let _ = manager.expiring(&identity, &entry).await;
                                }
                            }
                        }
                    } else {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }
            });
        }
        manager
    }
    async fn read(&self) -> Result<Store, String> {
        self.data.read().await
    }
    async fn update<T>(
        &self,
        change: impl FnOnce(&mut Store) -> Result<T, String>,
    ) -> Result<T, String> {
        self.data.update(change).await
    }
    fn publish(
        self: &Arc<Self>,
        identity: &str,
        entry: &Entry,
        observation: Option<SubscriptionQuota>,
    ) {
        let Ok(store) = self.data.store.try_lock() else {
            return;
        };
        let entry = store
            .as_ref()
            .and_then(|s| s.entries.get(identity))
            .unwrap_or(entry);
        if let Ok(mut runtime) = RUNTIME.write() {
            runtime.retain(|_, entry| entry.manager.strong_count() > 0);
            let previous = runtime.get(identity).and_then(|r| r.observed.clone());
            runtime.insert(
                identity.to_owned(),
                Runtime {
                    manager: Arc::downgrade(self),
                    account: entry.account.clone(),
                    policy: entry.policy.clone(),
                    observed: observation.filter(|q| q.success).or(previous),
                },
            );
        }
    }
    async fn context(&self, account: &str) -> Result<ExecutionCredentials, String> {
        if !crate::services::routing_upstreams::valid_identity(account) {
            return Err(FAILED.into());
        }
        let mut provider = crate::provider::Provider::with_id(
            "quota-control".into(),
            "Codex".into(),
            serde_json::json!({}),
            None,
        );
        crate::services::codex_provider::bind(&mut provider, account);
        let context = crate::services::intent_routing::catalog::freeze_execution(
            &provider,
            self.auth.clone(),
        )
        .await
        .ok_or("quotaPanel.loginRequired")?;
        if account == "native" {
            let selected =
                tokio::task::spawn_blocking(crate::services::subscription::read_codex_credentials)
                    .await
                    .map_err(|_| FAILED)?
                    .map_err(|_| FAILED)?;
            if selected.0.as_deref() != context.token().await.as_deref()
                || selected.1.as_deref() != Some(context.workspace.as_str())
            {
                return Err("quotaPanel.accountChanged".into());
            }
        }
        Ok(context)
    }
    pub async fn account_identity(&self, account: &str) -> Option<String> {
        self.context(account).await.ok()?.quota_identity()
    }
    pub async fn observe(
        self: &Arc<Self>,
        account: &str,
        expected: Option<&str>,
        quota: &SubscriptionQuota,
    ) -> Option<String> {
        if !quota.success {
            return None;
        }
        let context = self.context(account).await.ok()?;
        let identity = context.quota_identity()?;
        if expected != Some(identity.as_str()) {
            return None;
        }
        let store = self.read().await.ok()?;
        let entry = store
            .entries
            .get(&identity)
            .cloned()
            .unwrap_or_else(|| Entry {
                account: account.to_owned(),
                ..Default::default()
            });
        self.publish(&identity, &entry, Some(quota.clone()));
        if let Some(point) = quota.tiers.first().and_then(|tier| {
            let reset = tier
                .resets_at
                .as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())?
                .timestamp();
            Some(Point {
                at: quota.queried_at? / 1000,
                remaining: 100.0 - tier.utilization,
                resets_at: reset,
                span: core::window_seconds(&tier.name)?,
            })
        }) {
            let manager = self.clone();
            let key = format!("codex:{identity}:{}", quota.tiers[0].name);
            tokio::spawn(async move {
                let _ = manager.note(&key, point).await;
            });
        }
        Some(identity)
    }
    pub async fn controls(self: &Arc<Self>, account: &str) -> Result<Controls, String> {
        let context = self.context(account).await?;
        let identity = context.quota_identity().ok_or(FAILED)?;
        let resets = network::cards(&self.auth, account, &context).await?;
        let store = self.read().await?;
        let mut entry = store
            .entries
            .get(&identity)
            .cloned()
            .unwrap_or_else(|| Entry {
                account: account.to_owned(),
                ..Default::default()
            });
        if entry
            .pending
            .as_ref()
            .is_some_and(|pending| !resets.iter().any(|card| card.id == pending.credit))
        {
            self.update(|store| {
                if let Some(entry) = store.entries.get_mut(&identity) {
                    entry.pending = None;
                }
                Ok(())
            })
            .await?;
            entry.pending = None;
        }
        self.publish(&identity, &entry, None);
        Ok(Controls {
            identity,
            policy: entry.policy,
            resets,
            pending: entry.pending.is_some(),
            pending_credit: entry.pending.map(|pending| pending.credit),
        })
    }
    pub async fn policy(
        self: &Arc<Self>,
        account: &str,
        identity: &str,
        policy: Policy,
    ) -> Result<Policy, String> {
        let context = self.context(account).await?;
        if context.quota_identity().as_deref() != Some(identity) {
            return Err("quotaPanel.accountChanged".into());
        }
        let entry = self
            .update(|store| {
                let entry = store.entries.entry(identity.to_owned()).or_default();
                entry.account = account.to_owned();
                entry.policy = policy.clone();
                Ok(entry.clone())
            })
            .await?;
        self.publish(identity, &entry, None);
        if entry.policy.credits == CreditsMode::Off {
            if let Ok(quota) = self.read_quota(account, &context).await {
                self.publish(identity, &entry, Some(quota));
            }
        }
        Ok(policy)
    }
    pub async fn history(&self, key: &str) -> Result<Vec<Point>, String> {
        if key.len() > 2048 || key.is_empty() {
            return Err(FAILED.into());
        }
        Ok(self
            .history
            .read()
            .await?
            .histories
            .get(&hash(key))
            .cloned()
            .unwrap_or_default())
    }
    pub async fn note(&self, key: &str, point: Point) -> Result<Vec<Point>, String> {
        if key.len() > 2048 || key.is_empty() {
            return Err(FAILED.into());
        }
        let now = chrono::Utc::now().timestamp();
        if !point.valid(now) {
            return Err(FAILED.into());
        }
        let key = hash(key);
        self.history
            .update(|store| {
                store.note(key.clone(), point, now);
                Ok(store.histories.get(&key).cloned().unwrap_or_default())
            })
            .await
    }
    pub async fn consume(
        self: &Arc<Self>,
        account: &str,
        identity: &str,
        credit: &str,
        automatic: bool,
        week: bool,
    ) -> Result<network::Outcome, String> {
        let _lock = self.spending.lock().await;
        let context = self.context(account).await?;
        if context.quota_identity().as_deref() != Some(identity) {
            return Err("quotaPanel.accountChanged".into());
        }
        let cards = network::cards(&self.auth, account, &context).await?;
        let first = cards.first().ok_or("quotaPanel.noReset")?;
        let credit = if credit.is_empty() { &first.id } else { credit };
        if !cards.iter().any(|c| c.id == credit) {
            return Err("quotaPanel.noReset".into());
        }
        let now = chrono::Utc::now().timestamp();
        let pending = self
            .update(|store| {
                store.reserve(
                    identity,
                    account,
                    core::Pending {
                        credit: credit.to_owned(),
                        request: uuid::Uuid::new_v4().to_string(),
                    },
                    automatic,
                    week,
                    now,
                )
            })
            .await?;
        let result = network::consume(&self.auth, account, &context, &pending).await?;
        // A server-confirmed spend stays successful even when its local receipt cannot be updated.
        let _ = self
            .update(|store| {
                if let Some(entry) = store.entries.get_mut(identity) {
                    entry.pending = None;
                    if result.code == "reset" {
                        entry.last_spent = now;
                    }
                }
                Ok(())
            })
            .await;
        if result.code == "reset" {
            if let Ok(mut runtime) = RUNTIME.write() {
                if let Some(entry) = runtime.get_mut(identity) {
                    entry.observed = None;
                }
            }
            if let Ok(quota) = self.read_quota(account, &context).await {
                let _ = self.observe(account, Some(identity), &quota).await;
            }
        }
        Ok(result)
    }
    async fn read_quota(
        &self,
        account: &str,
        context: &ExecutionCredentials,
    ) -> Result<SubscriptionQuota, String> {
        let token = context.token().await.ok_or(FAILED)?;
        let client = network::client(&self.auth, account).await?;
        crate::services::subscription::query_codex_quota_with_client(
            &client,
            &token,
            Some(&context.workspace),
            "codex",
            "",
        )
        .await
        .map_err(|_| FAILED.into())
    }
    async fn expiring(self: &Arc<Self>, identity: &str, entry: &Entry) -> Result<(), String> {
        let context = self.context(&entry.account).await?;
        if context.quota_identity().as_deref() != Some(identity) {
            return Err(FAILED.into());
        }
        let now = chrono::Utc::now().timestamp();
        if now.saturating_sub(entry.last_spent) < 1800 {
            return Ok(());
        }
        let cards = network::cards(&self.auth, &entry.account, &context).await?;
        if !core::expiry_due(&entry.policy, &cards, true, now) {
            return Ok(());
        }
        let quota = self.read_quota(&entry.account, &context).await?;
        let used = quota.success
            && quota
                .tiers
                .iter()
                .any(|t| t.utilization.is_finite() && t.utilization > 0.0);
        if core::expiry_due(&entry.policy, &cards, used, now) {
            self.consume(&entry.account, identity, "", true, false)
                .await?;
        }
        Ok(())
    }
}
fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(crate) fn credits(identity: &str, exhausted: bool) -> CreditsMode {
    match RUNTIME.read() {
        Ok(runtime) => runtime
            .get(identity)
            .filter(|r| r.manager.strong_count() > 0)
            .map_or(
                if POLICY_READY.load(Ordering::Acquire) {
                    CreditsMode::On
                } else {
                    CreditsMode::Off
                },
                |r| {
                    if r.policy.credits == CreditsMode::Off
                        && !r.observed.as_ref().is_some_and(|q| {
                            q.success
                                && q.tiers.iter().any(|t| {
                                    t.utilization.is_finite()
                                        && t.resets_at
                                            .as_deref()
                                            .and_then(|s| {
                                                chrono::DateTime::parse_from_rfc3339(s).ok()
                                            })
                                            .is_some_and(|at| {
                                                at.timestamp() > chrono::Utc::now().timestamp()
                                            })
                                })
                        })
                    {
                        return CreditsMode::Off;
                    }
                    if exhausted || observed_exhausted(r, false) {
                        r.policy.credits
                    } else {
                        CreditsMode::On
                    }
                },
            ),
        Err(_) => CreditsMode::Off,
    }
}
fn observed_exhausted(runtime: &Runtime, weekly: bool) -> bool {
    let now = chrono::Utc::now().timestamp();
    runtime.observed.as_ref().is_some_and(|q| {
        q.success
            && q.tiers.iter().any(|t| {
                (!weekly || matches!(t.name.as_str(), "seven_day" | "weekly"))
                    && t.utilization.is_finite()
                    && t.utilization >= 100.0
                    && t.resets_at
                        .as_deref()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .is_some_and(|at| at.timestamp() > now)
            })
    })
}
pub(crate) fn week_enabled(identity: &str) -> bool {
    RUNTIME
        .read()
        .ok()
        .and_then(|r| r.get(identity).map(|r| r.policy.week))
        .unwrap_or(false)
}
pub(crate) fn observed_week_full(identity: &str) -> bool {
    RUNTIME
        .read()
        .ok()
        .and_then(|r| r.get(identity).map(|r| observed_exhausted(r, true)))
        .unwrap_or(false)
}
pub(crate) async fn auto_week(identity: &str) -> bool {
    let runtime = RUNTIME.read().ok().and_then(|r| r.get(identity).cloned());
    let Some(runtime) = runtime else {
        return false;
    };
    if !runtime.policy.week {
        return false;
    }
    let Some(manager) = runtime.manager.upgrade() else {
        return false;
    };
    let Ok(context) = manager.context(&runtime.account).await else {
        return false;
    };
    if context.quota_identity().as_deref() != Some(identity) {
        return false;
    }
    let Ok(quota) = manager.read_quota(&runtime.account, &context).await else {
        return false;
    };
    let now = chrono::Utc::now().timestamp();
    let full_week = quota.success
        && quota.tiers.iter().any(|t| {
            matches!(t.name.as_str(), "seven_day" | "weekly")
                && t.utilization >= 100.0
                && t.resets_at
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .is_some_and(|at| at.timestamp() > now)
        });
    full_week
        && manager
            .consume(&runtime.account, identity, "", true, true)
            .await
            .is_ok_and(|outcome| outcome.code == "reset")
}
