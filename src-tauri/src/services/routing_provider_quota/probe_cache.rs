//! Device-local interface discovery shared by endpoint origin and credential digest.
use super::{origin, read, Allowance, Endpoint, Status, FAILED};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

const HIT: u64 = 24 * 60 * 60;
const MISS: u64 = 6 * 60 * 60;
const DOWN: u64 = 5 * 60;
const MAX_ENTRIES: usize = 1024;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Interface {
    Found(Endpoint),
    Missing,
    Down,
    Rejected,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
struct Discovery {
    interface: Interface,
    until: u64,
}
type Entries = BTreeMap<String, Discovery>;

pub(super) struct ProbeCache {
    path: PathBuf,
    entries: Mutex<Option<Entries>>,
    gates: Mutex<BTreeMap<String, Weak<Mutex<()>>>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn identity(origin: &reqwest::Url, key: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(origin.as_str().as_bytes());
    hash.update([0]);
    hash.update(key.as_bytes());
    format!("{:x}", hash.finalize())
}

fn absent(interface: Interface, retry_after: u64) -> Allowance {
    let mut quota = Allowance::empty(if matches!(interface, Interface::Rejected) {
        Status::Unauthorized
    } else {
        Status::Unsupported
    });
    quota.retry_after_ms = Some(retry_after.max(1) * 1000);
    quota
}

fn load(path: &Path) -> Option<Entries> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.is_symlink() || !meta.is_file() || meta.len() > 1024 * 1024 {
        return None;
    }
    let mut bytes = vec![];
    std::fs::File::open(path)
        .ok()?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    let entries: Entries = serde_json::from_slice(&bytes).ok()?;
    let at = now();
    if entries.len() > MAX_ENTRIES
        || entries.iter().any(|(key, entry)| {
            key.len() != 64
                || !key.bytes().all(|c| c.is_ascii_hexdigit())
                || entry.until > at + HIT
                || !matches!(entry.interface, Interface::Found(_) | Interface::Missing)
        })
    {
        return None;
    }
    Some(
        entries
            .into_iter()
            .filter(|(_, entry)| entry.until > at)
            .collect(),
    )
}

impl ProbeCache {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: Mutex::new(None),
            gates: Mutex::new(BTreeMap::new()),
        }
    }

    async fn gate(&self, key: &str) -> Arc<Mutex<()>> {
        let mut gates = self.gates.lock().await;
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(key).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(Mutex::new(()));
        gates.insert(key.to_owned(), Arc::downgrade(&gate));
        gate
    }

    async fn cached(&self, key: &str) -> Option<Discovery> {
        let mut entries = self.entries.lock().await;
        if entries.is_none() {
            let path = self.path.clone();
            *entries = Some(
                tokio::task::spawn_blocking(move || load(&path))
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
            );
        }
        entries.as_ref()?.get(key).copied()
    }

    async fn remember(&self, key: String, interface: Interface, ttl: u64) {
        let mut guard = self.entries.lock().await;
        let entries = guard.get_or_insert_with(BTreeMap::new);
        let at = now();
        entries.retain(|_, entry| entry.until > at);
        if entries.len() >= MAX_ENTRIES {
            if let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.until)
                .map(|(k, _)| k.clone())
            {
                entries.remove(&oldest);
            }
        }
        entries.insert(
            key,
            Discovery {
                interface,
                until: at + ttl,
            },
        );
        // Only interface discovery survives a restart; neither balances nor credentials do.
        let durable: Entries = entries
            .iter()
            .filter(|(_, entry)| {
                matches!(entry.interface, Interface::Found(_) | Interface::Missing)
            })
            .map(|(key, entry)| (key.clone(), *entry))
            .collect();
        let Ok(bytes) = serde_json::to_vec(&durable) else {
            return;
        };
        let path = self.path.clone();
        // Cache failures remain auxiliary. Private atomic replacement is shared by macOS and Windows.
        let _ =
            tokio::task::spawn_blocking(move || crate::config::atomic_write_private(&path, &bytes))
                .await;
    }

    pub(super) async fn query(
        &self,
        base: &str,
        key: &str,
        force: bool,
    ) -> Result<Allowance, String> {
        if key.trim().is_empty() {
            return Ok(Allowance::empty(Status::Unauthorized));
        }
        let origin = origin(base)?;
        let id = identity(&origin, key);
        let gate = self.gate(&id).await;
        let _request = gate.lock().await;
        let previous = self.cached(&id).await;
        if let Some(entry) = previous.filter(|entry| !force && entry.until > now()) {
            return match entry.interface {
                Interface::Found(endpoint) => read(&origin, key, endpoint).await,
                interface => Ok(absent(interface, entry.until.saturating_sub(now()))),
            };
        }
        let mut failed = false;
        let mut rejected = false;
        for endpoint in Endpoint::ALL {
            match read(&origin, key, endpoint).await {
                Ok(quota) if quota.status == Status::Ready => {
                    self.remember(id, Interface::Found(endpoint), HIT).await;
                    return Ok(quota);
                }
                Ok(quota) => rejected |= quota.status == Status::Unauthorized,
                Err(_) => failed = true,
            }
        }
        // A previously known interface is retained through temporary failures and rejection.
        if let Some(Discovery {
            interface: Interface::Found(endpoint),
            ..
        }) = previous
        {
            if failed || rejected {
                self.remember(
                    id,
                    Interface::Found(endpoint),
                    if rejected { HIT } else { DOWN },
                )
                .await;
                return if failed {
                    Err(FAILED.into())
                } else {
                    Ok(Allowance::empty(Status::Unauthorized))
                };
            }
        }
        let (interface, ttl) = if rejected {
            (Interface::Rejected, DOWN)
        } else if failed {
            (Interface::Down, DOWN)
        } else {
            (Interface::Missing, MISS)
        };
        self.remember(id, interface, ttl).await;
        // An unsuccessful first discovery has no allowance card, as in Magpie.
        Ok(absent(interface, ttl))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, routing::get, Json, Router};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Relay {
        base: String,
        probes: Arc<AtomicUsize>,
        reads: Arc<AtomicUsize>,
        status: Arc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Relay {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl Relay {
        async fn new(status: StatusCode) -> Result<Self, Box<dyn std::error::Error>> {
            let probes = Arc::new(AtomicUsize::new(0));
            let reads = Arc::new(AtomicUsize::new(0));
            let status = Arc::new(AtomicUsize::new(status.as_u16() as usize));
            let first = probes.clone();
            let second = reads.clone();
            let mode = status.clone();
            let app = Router::new()
                .route("/v1/usage", get(move || { let first = first.clone(); async move {
                    first.fetch_add(1, Ordering::SeqCst);
                    StatusCode::NOT_FOUND
                }}))
                .route("/api/usage/token", get(move || {
                    let second = second.clone(); let mode = mode.clone(); async move {
                        second.fetch_add(1, Ordering::SeqCst);
                        let code = StatusCode::from_u16(mode.load(Ordering::SeqCst) as u16).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                        (code, Json(json!({"data":{"total_available":1500000},"error":"synthetic-private-error"})))
                    }
                }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let base = format!("http://{}/v1", listener.local_addr()?);
            let task = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            Ok(Self {
                base,
                probes,
                reads,
                status,
                task,
            })
        }
        fn set(&self, status: StatusCode) {
            self.status
                .store(status.as_u16() as usize, Ordering::SeqCst);
        }
        fn probes(&self) -> usize {
            self.probes.load(Ordering::SeqCst)
        }
        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    #[tokio::test]
    async fn copies_share_missing_interface_even_after_restart(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("probes.json");
        let relay = Relay::new(StatusCode::NOT_FOUND).await?;
        let cache = ProbeCache::new(path.clone());
        let copy = relay.base.replace("/v1", "/another/v1");
        let (a, b) = tokio::join!(
            cache.query(&relay.base, "synthetic-key", false),
            cache.query(&copy, "synthetic-key", false)
        );
        assert_eq!(a?.status, Status::Unsupported);
        assert_eq!(b?.status, Status::Unsupported);
        assert_eq!(relay.probes(), 1, "copies must share discovery");
        assert_eq!(relay.reads(), 1);
        let saved = std::fs::read_to_string(&path)?;
        assert!(
            !saved.contains("synthetic-key"),
            "cache must not store credentials"
        );
        assert!(
            !saved.contains("127.0.0.1"),
            "cache must not store private endpoints"
        );
        let restarted = ProbeCache::new(path);
        assert_eq!(
            restarted
                .query(&relay.base, "synthetic-key", false)
                .await?
                .status,
            Status::Unsupported
        );
        assert_eq!(relay.probes(), 1, "restart must retain discovery");
        restarted
            .query(&relay.base, "synthetic-other-key", false)
            .await?;
        assert_eq!(relay.probes(), 2, "different keys are independent");
        let other = Relay::new(StatusCode::OK).await?;
        assert_eq!(
            restarted
                .query(&other.base, "synthetic-key", false)
                .await?
                .status,
            Status::Ready
        );
        assert_eq!(other.probes(), 1, "different origins are independent");
        Ok(())
    }

    #[tokio::test]
    async fn copies_read_fresh_balances_at_the_discovered_interface(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("probes.json");
        let relay = Relay::new(StatusCode::OK).await?;
        let cache = ProbeCache::new(path.clone());
        assert_eq!(
            cache
                .query(&relay.base, "synthetic-key", false)
                .await?
                .balance,
            Some(3.0)
        );
        assert_eq!(
            cache
                .query(&relay.base, "synthetic-key", false)
                .await?
                .balance,
            Some(3.0)
        );
        let restarted = ProbeCache::new(path);
        assert_eq!(
            restarted
                .query(&relay.base, "synthetic-key", false)
                .await?
                .balance,
            Some(3.0)
        );
        assert_eq!(
            relay.probes(),
            1,
            "do not rediscover for each copy or revision"
        );
        assert_eq!(relay.reads(), 3, "cache interfaces, not balances");
        Ok(())
    }

    #[tokio::test]
    async fn manual_refresh_and_expiry_can_recover_an_unsupported_relay(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let relay = Relay::new(StatusCode::NOT_FOUND).await?;
        let cache = ProbeCache::new(dir.path().join("probes.json"));
        cache.query(&relay.base, "synthetic-key", false).await?;
        relay.set(StatusCode::OK);
        assert_eq!(
            cache
                .query(&relay.base, "synthetic-key", false)
                .await?
                .status,
            Status::Unsupported
        );
        assert_eq!(
            cache
                .query(&relay.base, "synthetic-key", true)
                .await?
                .balance,
            Some(3.0)
        );
        assert_eq!(relay.probes(), 2);
        let id = identity(&origin(&relay.base)?, "synthetic-key");
        cache.remember(id, Interface::Missing, 0).await;
        assert_eq!(
            cache
                .query(&relay.base, "synthetic-key", false)
                .await?
                .status,
            Status::Ready
        );
        assert_eq!(relay.probes(), 3, "expiry reopens discovery");
        Ok(())
    }

    #[tokio::test]
    async fn initial_failure_is_hidden_and_retried_after_five_minutes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("probes.json");
        let relay = Relay::new(StatusCode::SERVICE_UNAVAILABLE).await?;
        let cache = ProbeCache::new(path.clone());
        let first = cache.query(&relay.base, "synthetic-key", false).await?;
        assert_eq!(first.status, Status::Unsupported);
        assert_eq!(first.retry_after_ms, Some(DOWN * 1000));
        relay.set(StatusCode::OK);
        let cached = cache.query(&relay.base, "synthetic-key", false).await?;
        assert_eq!(cached.status, Status::Unsupported);
        assert!(cached
            .retry_after_ms
            .is_some_and(|ms| ms > 0 && ms <= DOWN * 1000));
        assert_eq!(relay.probes(), 1);
        let restarted = ProbeCache::new(path);
        assert_eq!(
            restarted
                .query(&relay.base, "synthetic-key", false)
                .await?
                .status,
            Status::Ready,
            "temporary failures are not durable"
        );
        Ok(())
    }

    #[tokio::test]
    async fn known_interfaces_keep_failures_and_rejection_visible(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let relay = Relay::new(StatusCode::OK).await?;
        let cache = ProbeCache::new(dir.path().join("probes.json"));
        cache.query(&relay.base, "synthetic-key", false).await?;
        relay.set(StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            cache
                .query(&relay.base, "synthetic-key", false)
                .await
                .err()
                .as_deref(),
            Some(FAILED)
        );
        relay.set(StatusCode::UNAUTHORIZED);
        let rejected = cache.query(&relay.base, "synthetic-key", false).await?;
        assert_eq!(rejected.status, Status::Unauthorized);
        assert!(rejected.balance.is_none());
        relay.set(StatusCode::OK);
        assert_eq!(
            cache
                .query(&relay.base, "synthetic-key", false)
                .await?
                .balance,
            Some(3.0)
        );
        assert_eq!(
            relay.probes(),
            1,
            "failures must not forget a known interface"
        );
        Ok(())
    }

    #[tokio::test]
    async fn unreadable_or_unwritable_cache_does_not_break_allowances(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let relay = Relay::new(StatusCode::OK).await?;
        let cache = ProbeCache::new(dir.path().to_path_buf());
        assert_eq!(
            cache
                .query(&relay.base, "synthetic-key", false)
                .await?
                .balance,
            Some(3.0)
        );
        assert_eq!(
            cache
                .query(&relay.base, "synthetic-key", false)
                .await?
                .balance,
            Some(3.0)
        );
        assert_eq!(
            relay.probes(),
            1,
            "an unwritable cache still works in memory"
        );
        let path = dir.path().join("corrupt.json");
        std::fs::write(&path, b"invalid JSON")?;
        let corrupted = ProbeCache::new(path);
        assert_eq!(
            corrupted
                .query(&relay.base, "synthetic-key", false)
                .await?
                .balance,
            Some(3.0)
        );
        assert_eq!(relay.probes(), 2);
        Ok(())
    }
}
