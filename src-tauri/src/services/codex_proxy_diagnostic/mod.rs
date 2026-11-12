//! Read-only evidence that Codex's proxy route cannot reach the owned local gateway.
use serde::Serialize;
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        LazyLock,
    },
    time::{Duration, Instant},
};

mod discovery;
mod resolver;
#[cfg(test)]
mod tests;

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const BODY_LIMIT: usize = 4096;
const CACHE_TTL: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Source {
    Environment,
    System,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ProxyIssue {
    pub target: String,
    pub proxy: String,
    pub source: Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Mac,
    Windows,
    Other,
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
struct SystemProxy {
    url: String,
    http: String,
    socks: String,
    bypass: Vec<String>,
}

type Environment = BTreeMap<String, String>;
#[derive(Clone, Debug, PartialEq, Eq)]
struct Route {
    url: String,
    source: Source,
}

/// Only a verified owner can construct a snapshot; its key contains no plaintext config.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) target: String,
    pub(crate) respect_system_proxy: bool,
    pub(crate) fingerprint: [u8; 32],
}
impl Snapshot {
    pub(crate) fn matches_listener(&self, address: &str, port: u16) -> bool {
        address
            .parse::<std::net::IpAddr>()
            .ok()
            .filter(|ip| ip.is_loopback())
            .is_some_and(|ip| {
                self.target == format!("http://{}", std::net::SocketAddr::new(ip, port))
            })
    }
}

fn proxy_url(raw: &str) -> Option<url::Url> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 4096 {
        return None;
    }
    let url = if raw.contains("://") {
        url::Url::parse(raw)
    } else {
        url::Url::parse(&format!("http://{raw}"))
    }
    .ok()?;
    if !matches!(
        url.scheme(),
        "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h"
    ) || url.host_str().is_none()
    {
        return None;
    }
    Some(url)
}

pub(crate) fn origin(raw: &str, loopback: bool) -> Option<String> {
    let url = proxy_url(raw)?;
    let host = url.host_str()?;
    if loopback
        && (url.scheme() != "http"
            || !host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()))
    {
        return None;
    }
    let port = url
        .port_or_known_default()
        .or_else(|| url.scheme().starts_with("socks").then_some(1080))?;
    if port == 0 {
        return None;
    }
    Some(format!("{}://{}:{port}", url.scheme(), host))
}

async fn health(target: &str, route: Option<&Route>) -> bool {
    let Some(target) = origin(target, true) else {
        return false;
    };
    let operation = async {
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(PROBE_TIMEOUT);
        if let Some(route) = route {
            let Some(mut proxy) = proxy_url(&route.url) else {
                return false;
            };
            // Codex's reqwest build speaks HTTP even when ALL_PROXY names SOCKS.
            if proxy.scheme().starts_with("socks") {
                let port = proxy.port().unwrap_or(1080);
                let http = proxy
                    .as_str()
                    .replacen(&format!("{}://", proxy.scheme()), "http://", 1);
                let Ok(parsed) = url::Url::parse(&http) else {
                    return false;
                };
                proxy = parsed;
                if proxy.set_port(Some(port)).is_err() {
                    return false;
                }
            }
            proxy.set_path("");
            proxy.set_query(None);
            proxy.set_fragment(None);
            let Ok(proxy) = reqwest::Proxy::all(proxy) else {
                return false;
            };
            // Explicit selection must not accidentally inherit this app's NO_PROXY.
            builder = builder.proxy(proxy);
        }
        let Ok(client) = builder.build() else {
            return false;
        };
        let Ok(mut response) = client.get(format!("{target}/health")).send().await else {
            return false;
        };
        if response.status() != reqwest::StatusCode::OK
            || response
                .content_length()
                .is_some_and(|n| n > BODY_LIMIT as u64)
        {
            return false;
        }
        let mut bytes = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if chunk.len() <= BODY_LIMIT - bytes.len() => {
                    bytes.extend_from_slice(&chunk)
                }
                Ok(Some(_)) | Err(_) => return false,
                Ok(None) => break,
            }
        }
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .is_some_and(|value| value.get("status").and_then(|v| v.as_str()) == Some("healthy"))
    };
    tokio::time::timeout(PROBE_TIMEOUT, operation)
        .await
        .unwrap_or(false)
}

async fn diagnose(target: &str, route: Option<&Route>) -> Option<ProxyIssue> {
    let route = route?;
    let target = origin(target, true)?;
    let proxy = origin(&route.url, false)?;
    if !health(&target, None).await
        || health(&target, Some(route)).await
        || !health(&target, None).await
    {
        return None;
    }
    Some(ProxyIssue {
        target,
        proxy,
        source: route.source,
    })
}

struct Entry {
    key: Snapshot,
    value: Option<ProxyIssue>,
    created: Instant,
    generation: u64,
}
#[derive(Default)]
struct Cache {
    entry: tokio::sync::Mutex<Option<Entry>>,
    generation: AtomicU64,
}
impl Cache {
    async fn run<F, Fut>(&self, key: Snapshot, force: bool, probe: F) -> Option<ProxyIssue>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Option<ProxyIssue>>,
    {
        let observed = self.generation.load(Ordering::Acquire);
        // This lock is diagnostic-only; forwarding and configuration never use it.
        let mut entry = self.entry.lock().await;
        if let Some(hit) = entry.as_ref().filter(|entry| {
            entry.key == key
                && entry.created.elapsed() < CACHE_TTL
                && (!force || entry.generation > observed)
        }) {
            return hit.value.clone();
        }
        let value = probe().await;
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        *entry = Some(Entry {
            key,
            value: value.clone(),
            created: Instant::now(),
            generation,
        });
        value
    }
}
static CACHE: LazyLock<Cache> = LazyLock::new(Cache::default);

pub(crate) async fn check(snapshot: Snapshot, force: bool) -> Option<ProxyIssue> {
    CACHE
        .run(snapshot.clone(), force, || async {
            let (platform, env, system) = discovery::discover().await?;
            let target = url::Url::parse(&snapshot.target).ok()?;
            let route = resolver::resolve(
                target.host_str()?,
                snapshot.respect_system_proxy,
                platform,
                &env,
                &system,
            );
            diagnose(&snapshot.target, route.as_ref()).await
        })
        .await
}
