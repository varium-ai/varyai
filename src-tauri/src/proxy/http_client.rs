//! 全局 HTTP 客户端模块
//!
//! 提供支持全局代理配置与动态系统代理切换的 HTTP 客户端。
//! 所有需要发送 HTTP 请求的模块都应使用此模块提供的客户端。

use once_cell::sync::OnceCell;
use reqwest::Client;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::RwLock;
use std::time::Duration;

use super::system_proxy::{self, EffectiveProxy};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ClientProfile {
    Default,
    NoRedirects,
    ForwardingNoRedirects,
}

impl ClientProfile {
    fn redirects(self) -> reqwest::redirect::Policy {
        match self {
            Self::Default => reqwest::redirect::Policy::default(),
            Self::NoRedirects | Self::ForwardingNoRedirects => reqwest::redirect::Policy::none(),
        }
    }

    fn total_timeout(self) -> Option<Duration> {
        match self {
            Self::Default | Self::NoRedirects => Some(Duration::from_secs(600)),
            Self::ForwardingNoRedirects => None,
        }
    }
}

/// 缓存按 `(ClientProfile, EffectiveProxy)` 隔离的客户端，代理配置变化时自动重建以淘汰旧连接池。
static CLIENT_CACHE: OnceCell<RwLock<HashMap<ClientProfile, (EffectiveProxy, Client)>>> =
    OnceCell::new();

/// 当前显式配置代理 URL（用于日志和状态查询）
static CURRENT_PROXY_URL: OnceCell<RwLock<Option<String>>> = OnceCell::new();

static CC_SWITCH_PROXY_PORT: OnceCell<RwLock<u16>> = OnceCell::new();

fn client_cache() -> &'static RwLock<HashMap<ClientProfile, (EffectiveProxy, Client)>> {
    CLIENT_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// 应在代理服务器启动时调用，以便系统代理检测能正确识别自己的端口
pub fn set_proxy_port(port: u16) {
    if let Some(lock) = CC_SWITCH_PROXY_PORT.get() {
        if let Ok(mut current_port) = lock.write() {
            *current_port = port;
            log::debug!("[GlobalProxy] Updated Vary AI proxy port to {port}");
        }
    } else {
        let _ = CC_SWITCH_PROXY_PORT.set(RwLock::new(port));
        log::debug!("[GlobalProxy] Initialized Vary AI proxy port to {port}");
    }
}

fn get_proxy_port() -> u16 {
    CC_SWITCH_PROXY_PORT
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|port| *port)
        .unwrap_or(super::types::DEFAULT_PROXY_PORT)
}

/// 初始化全局 HTTP 客户端
///
/// 应在应用启动时调用一次。
///
/// # Arguments
/// * `proxy_url` - 代理 URL，如 `http://127.0.0.1:7890` 或 `socks5://127.0.0.1:1080`
///   传入 None 或空字符串表示直连/跟随系统代理
pub fn init(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.map(str::trim).filter(|s| !s.is_empty());
    if CURRENT_PROXY_URL.get().is_some() {
        log::warn!(
            "[GlobalProxy] [GP-003] Already initialized, updating instead: {}",
            effective_url
                .map(mask_url)
                .unwrap_or_else(|| "direct connection".to_string())
        );
        return apply_proxy(proxy_url);
    }

    if let Some(url) = effective_url {
        system_proxy::validate_explicit_proxy_url(url)?;
    }
    let resolved = system_proxy::resolve_effective_proxy_warm(effective_url, get_proxy_port());
    let client = build_client_for_effective_proxy(ClientProfile::Default, &resolved)?;

    let _ = CURRENT_PROXY_URL.set(RwLock::new(effective_url.map(str::to_string)));
    if let Ok(mut guard) = client_cache().write() {
        guard.clear();
        guard.insert(ClientProfile::Default, (resolved, client));
    }

    log::info!(
        "[GlobalProxy] Initialized: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

/// 验证代理配置（不应用）
pub fn validate_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    let effective_url = proxy_url.map(str::trim).filter(|s| !s.is_empty());
    build_client(effective_url)?;
    Ok(())
}

/// 应用代理配置（假设已验证）
pub fn apply_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    update_proxy_inner(proxy_url, "Applied")
}

/// 更新代理配置（热更新）
#[allow(dead_code)]
pub fn update_proxy(proxy_url: Option<&str>) -> Result<(), String> {
    update_proxy_inner(proxy_url, "Updated")
}

fn update_proxy_inner(proxy_url: Option<&str>, action: &str) -> Result<(), String> {
    let effective_url = proxy_url.map(str::trim).filter(|s| !s.is_empty());
    if let Some(url) = effective_url {
        system_proxy::validate_explicit_proxy_url(url)?;
    }

    if CURRENT_PROXY_URL.get().is_none() {
        return init(proxy_url);
    }

    let resolved = system_proxy::resolve_effective_proxy(effective_url, get_proxy_port());
    let new_client = build_client_for_effective_proxy(ClientProfile::Default, &resolved)?;

    if let Some(lock) = CURRENT_PROXY_URL.get() {
        let mut url = lock.write().map_err(|e| {
            log::error!("[GlobalProxy] [GP-002] Failed to acquire URL write lock: {e}");
            "Failed to update proxy URL record: lock poisoned".to_string()
        })?;
        *url = effective_url.map(str::to_string);
    }

    let mut guard = client_cache().write().map_err(|e| {
        log::error!("[GlobalProxy] [GP-001] Failed to acquire client cache write lock: {e}");
        "Failed to update proxy: lock poisoned".to_string()
    })?;
    guard.clear();
    guard.insert(ClientProfile::Default, (resolved, new_client));

    log::info!(
        "[GlobalProxy] {action}: {}",
        effective_url
            .map(mask_url)
            .unwrap_or_else(|| "direct connection".to_string())
    );

    Ok(())
}

/// 获取全局 HTTP 客户端（当系统代理变化时自动重建客户端并丢弃旧连接池）
pub fn get() -> Client {
    get_or_build_client(ClientProfile::Default).unwrap_or_else(|err| {
        log::warn!("[GlobalProxy] [GP-004] Falling back to bare client: {err}");
        Client::new()
    })
}

/// 获取当前显式配置代理 URL
///
/// 返回当前显式配置的代理 URL，None 表示直连/跟随系统代理。
pub fn get_current_proxy_url() -> Option<String> {
    CURRENT_PROXY_URL
        .get()
        .and_then(|lock| lock.read().ok())
        .and_then(|url| url.clone())
}

/// 检查是否正在使用显式全局代理
#[allow(dead_code)]
pub fn is_proxy_enabled() -> bool {
    get_current_proxy_url().is_some()
}

/// Detection and bound forwarding must not inherit authority across redirects.
pub(crate) fn get_without_redirects() -> Result<Client, String> {
    get_or_build_client(ClientProfile::NoRedirects)
}

/// Model streams retain redirect restrictions without a fixed total lifetime.
pub(crate) fn get_forwarding_without_redirects() -> Result<Client, String> {
    get_or_build_client(ClientProfile::ForwardingNoRedirects)
}

pub(crate) fn effective_proxy_for_url(target_url: &str) -> Option<String> {
    let parsed = url::Url::parse(target_url).ok()?;
    let explicit = get_current_proxy_url();
    let effective = system_proxy::resolve_effective_proxy(explicit.as_deref(), get_proxy_port());
    effective.proxy_for_url(&parsed).map(|u| u.to_string())
}

pub(crate) async fn refresh_on_connect_failure() -> bool {
    let explicit = get_current_proxy_url();
    let self_port = get_proxy_port();
    let cached_proxy = client_cache()
        .read()
        .ok()
        .and_then(|guard| guard.values().next().map(|(proxy, _)| proxy.clone()));
    let before = cached_proxy
        .unwrap_or_else(|| system_proxy::resolve_effective_proxy(explicit.as_deref(), self_port));

    let Ok(after) = tokio::task::spawn_blocking(move || {
        system_proxy::force_refresh_effective_proxy(explicit.as_deref(), self_port)
    })
    .await
    else {
        return false;
    };

    if after == before {
        return false;
    }

    log::info!(
        "[GlobalProxy] Refreshed proxy after connect failure: {} -> {}",
        before.summary(),
        after.summary()
    );
    if let Ok(mut guard) = client_cache().write() {
        guard.retain(|_, (cached_proxy, _)| cached_proxy == &after);
    }
    true
}

pub(crate) fn forwarding_client_or(fallback: &Client) -> Client {
    #[cfg(test)]
    {
        if super::forwarder::final_send_binding_test_support::is_active()
            || !system_proxy::test_support::dynamic_session_client_enabled()
        {
            return fallback.clone();
        }
    }
    get_forwarding_without_redirects().unwrap_or_else(|_| fallback.clone())
}

fn get_or_build_client(profile: ClientProfile) -> Result<Client, String> {
    #[cfg(test)]
    let isolated_loopback = super::forwarder::final_send_binding_test_support::is_active();
    #[cfg(not(test))]
    let isolated_loopback = false;

    if isolated_loopback {
        return client_builder(profile.redirects(), profile.total_timeout())?
            .no_proxy()
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"));
    }

    let explicit = get_current_proxy_url();
    let desired = system_proxy::resolve_effective_proxy(explicit.as_deref(), get_proxy_port());

    if let Ok(guard) = client_cache().read() {
        if let Some((cached_proxy, client)) = guard.get(&profile) {
            if cached_proxy == &desired {
                return Ok(client.clone());
            }
        }
    }

    let new_client = build_client_for_effective_proxy(profile, &desired)?;
    if let Ok(mut guard) = client_cache().write() {
        if let Some((cached_proxy, client)) = guard.get(&profile) {
            if cached_proxy == &desired {
                return Ok(client.clone());
            }
        }
        let previous = guard.get(&profile).map(|(old, _)| old.clone()).or_else(|| {
            guard
                .values()
                .next()
                .map(|(old, _)| old.clone())
                .filter(|old| old != &desired)
        });
        if let Some(old) = previous {
            log::info!(
                "[GlobalProxy] Effective proxy changed: {} -> {}",
                old.summary(),
                desired.summary()
            );
        }
        guard.retain(|_, (cached_proxy, _)| cached_proxy == &desired);
        guard.insert(profile, (desired, new_client.clone()));
    }
    Ok(new_client)
}

pub(crate) const UPSTREAM_HEADER_WAIT: Duration = Duration::from_secs(600);

fn classify_connect_cause(err: &reqwest::Error) -> &'static str {
    let mut current: Option<&(dyn StdError + 'static)> = Some(err);
    while let Some(source) = current {
        let lower = source.to_string().to_ascii_lowercase();
        if lower.contains("connection refused") || lower.contains("econnrefused") {
            return "tcp_refused";
        }
        if lower.contains("dns")
            || lower.contains("failed to lookup")
            || lower.contains("nodename")
            || lower.contains("no such host")
            || lower.contains("name or service not known")
        {
            return "dns";
        }
        if lower.contains("tls")
            || lower.contains("ssl")
            || lower.contains("certificate")
            || lower.contains("handshake")
        {
            return "tls";
        }
        if lower.contains("reset") || lower.contains("broken pipe") || lower.contains("eof") {
            return "reset";
        }
        current = source.source();
    }
    if err.is_connect() {
        "connect"
    } else {
        "other"
    }
}

fn current_proxy_mode() -> &'static str {
    let explicit = get_current_proxy_url();
    system_proxy::resolve_effective_proxy(explicit.as_deref(), get_proxy_port()).mode_label()
}

/// Dropping the header timer after send completes leaves the body free to stream.
pub(crate) async fn send_with_header_timeout(
    request: reqwest::RequestBuilder,
    wait: Duration,
) -> Result<reqwest::Response, super::ProxyError> {
    tokio::time::timeout(wait, request.send())
        .await
        .map_err(|_| {
            log::warn!(
                "[Proxy] Upstream response headers timed out after {}s",
                wait.as_secs()
            );
            super::ProxyError::Timeout("Upstream response headers timed out".into())
        })?
        .map_err(|err| {
            log::warn!(
                "[Proxy] Upstream send failed: connect={} timeout={} request={} body={} status={:?} cause={} proxy_mode={}",
                err.is_connect(),
                err.is_timeout(),
                err.is_request(),
                err.is_body(),
                err.status(),
                classify_connect_cause(&err),
                current_proxy_mode(),
            );
            super::ProxyError::ForwardFailed(
                if err.is_connect() {
                    "connect_failed"
                } else {
                    "Upstream request failed"
                }
                .into(),
            )
        })
}

fn build_client(proxy_url: Option<&str>) -> Result<Client, String> {
    let effective = match proxy_url.map(str::trim).filter(|s| !s.is_empty()) {
        Some(url) => EffectiveProxy::Explicit(system_proxy::validate_explicit_proxy_url(url)?),
        None => EffectiveProxy::Direct,
    };
    build_client_for_effective_proxy(ClientProfile::Default, &effective)
}

fn client_builder(
    redirects: reqwest::redirect::Policy,
    total_timeout: Option<Duration>,
) -> Result<reqwest::ClientBuilder, String> {
    let builder = Client::builder()
        .use_preconfigured_tls(super::upstream_tls::client_config_h2()?)
        .redirect(redirects)
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(60))
        // 禁用 reqwest 自动解压：防止 reqwest 覆盖客户端原始 accept-encoding header。
        // 响应解压由 response_processor 根据 content-encoding 手动处理。
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd();
    Ok(match total_timeout {
        Some(timeout) => builder.timeout(timeout),
        None => builder,
    })
}

fn build_client_for_effective_proxy(
    profile: ClientProfile,
    effective: &EffectiveProxy,
) -> Result<Client, String> {
    let mut builder = client_builder(profile.redirects(), profile.total_timeout())?.no_proxy();

    if let EffectiveProxy::Explicit(url) = effective {
        system_proxy::validate_explicit_proxy_url(url)?;
        log::debug!("[GlobalProxy] Proxy configured: {}", mask_url(url));
    }

    if !matches!(effective, EffectiveProxy::Direct) {
        let proxy_rules = effective.clone();
        builder = builder.proxy(reqwest::Proxy::custom(move |url| {
            proxy_rules.proxy_for_url(url)
        }));
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

/// 隐藏 URL 中的敏感信息（用于日志）
pub fn mask_url(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        // 隐藏用户名和密码，保留 scheme、host 和端口
        let host = parsed.host_str().unwrap_or("?");
        match parsed.port() {
            Some(port) => format!("{}://{}:{}", parsed.scheme(), host, port),
            None => format!("{}://{}", parsed.scheme(), host),
        }
    } else {
        // URL 解析失败，返回部分内容。截断点回退到最近的字符边界，
        // 避免在多字节 UTF-8 字符中间切割导致 panic。
        if url.len() > 20 {
            let cut = (0..=20)
                .rev()
                .find(|&i| url.is_char_boundary(i))
                .unwrap_or(0);
            format!("{}...", &url[..cut])
        } else {
            url.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn forwarding_header_deadline_does_not_expire_an_open_stream(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, wait) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let read = socket.read(&mut request).await.unwrap();
            assert!(read > 0, "fixture must receive the request line");
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 15\r\n\r\ndata: first\n\n").await.unwrap();
            wait.await.unwrap();
            socket.write_all(b"\n\n").await.unwrap();
        });
        let client = forwarding_client_for_test()?;
        let mut response = send_with_header_timeout(
            client.get(format!("http://{address}")),
            Duration::from_millis(50),
        )
        .await
        .unwrap();
        assert_eq!(response.chunk().await.unwrap().unwrap(), "data: first\n\n");
        tokio::time::sleep(Duration::from_millis(150)).await;
        release.send(()).unwrap();
        assert_eq!(response.bytes().await.unwrap(), "\n\n");
        server.await.unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn forwarding_header_deadline_rejects_a_silent_upstream(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let result = send_with_header_timeout(
            forwarding_client_for_test()?.get(format!("http://{address}")),
            Duration::from_millis(50),
        )
        .await;
        server.abort();
        assert!(matches!(result, Err(super::super::ProxyError::Timeout(_))));
        Ok(())
    }

    #[tokio::test]
    async fn forwarding_header_wait_cancels_when_dropped() -> Result<(), Box<dyn std::error::Error>>
    {
        use tokio::io::AsyncReadExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted, ready) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            assert!(socket.read(&mut bytes).await.unwrap() > 0);
            accepted.send(()).unwrap();
            matches!(socket.read(&mut bytes).await, Ok(0) | Err(_))
        });
        let request = tokio::spawn(send_with_header_timeout(
            forwarding_client_for_test()?.get(format!("http://{address}")),
            Duration::from_secs(600),
        ));
        ready.await.unwrap();
        request.abort();
        assert!(tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap());
        Ok(())
    }

    #[tokio::test]
    async fn forwarding_client_does_not_follow_redirects() -> Result<(), Box<dyn std::error::Error>>
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let destination = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let redirect = destination.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            let read = socket.read(&mut bytes).await.unwrap();
            assert!(read > 0, "fixture must receive the request line");
            socket
                .write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{redirect}/\r\nContent-Length: 0\r\n\r\n").as_bytes())
                .await
                .unwrap();
        });
        let response = send_with_header_timeout(
            forwarding_client_for_test()?.get(format!("http://{address}")),
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), http::StatusCode::TEMPORARY_REDIRECT);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), destination.accept())
                .await
                .is_err()
        );
        server.await.unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn forwarding_client_rebuilds_and_recovers_after_proxy_switch(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        async fn spawn_mock_http_proxy(
            label: &'static str,
        ) -> (
            u16,
            tokio::task::JoinHandle<()>,
            tokio::sync::oneshot::Sender<()>,
        ) {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
            let handle = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = &mut stop_rx => break,
                        accepted = listener.accept() => {
                            let Ok((mut socket, _)) = accepted else { break };
                            tokio::spawn(async move {
                                let mut buf = [0u8; 4096];
                                let _ = socket.read(&mut buf).await;
                                let body = format!("via-{label}");
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                    body.len(),
                                    body
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                            });
                        }
                    }
                }
            });
            (port, handle, stop_tx)
        }

        let _guard = system_proxy::test_support::Guard::new_async().await;
        let _ = apply_proxy(None);

        let (port_a, handle_a, stop_a) = spawn_mock_http_proxy("proxy-a").await;
        let (port_b, handle_b, stop_b) = spawn_mock_http_proxy("proxy-b").await;

        system_proxy::test_support::set_probe_override(move |_host, port| {
            port == port_a || port == port_b
        });
        system_proxy::test_support::set_system_override(Some(system_proxy::SystemProxyConfig {
            https: None,
            http: Some(format!("http://127.0.0.1:{port_a}")),
            socks: None,
            bypass: vec![],
        }));

        let client_a = get_forwarding_without_redirects()?;
        let body_a = send_with_header_timeout(
            client_a.get("http://example.invalid/v1/models"),
            Duration::from_secs(2),
        )
        .await?
        .text()
        .await?;
        assert_eq!(body_a, "via-proxy-a");

        // Switch system proxy from A to B and stop A; refresh_on_connect_failure detects change.
        let _ = stop_a.send(());
        let _ = handle_a.await;
        system_proxy::test_support::set_system_override(Some(system_proxy::SystemProxyConfig {
            https: None,
            http: Some(format!("http://127.0.0.1:{port_b}")),
            socks: None,
            bypass: vec![],
        }));
        assert!(refresh_on_connect_failure().await);

        let client_b = get_forwarding_without_redirects()?;
        let body_b = send_with_header_timeout(
            client_b.get("http://example.invalid/v1/models"),
            Duration::from_secs(2),
        )
        .await?
        .text()
        .await?;
        assert_eq!(body_b, "via-proxy-b");
        assert_eq!(
            effective_proxy_for_url("http://example.invalid/v1/models").as_deref(),
            Some(format!("http://127.0.0.1:{port_b}/").as_str())
        );

        // Stop B without clearing system proxy -> dead loopback probe falls back to direct.
        let _ = stop_b.send(());
        let _ = handle_b.await;
        system_proxy::test_support::set_probe_override(|_, _| false);
        assert!(refresh_on_connect_failure().await);
        assert_eq!(
            effective_proxy_for_url("http://example.invalid/v1/models"),
            None
        );

        Ok(())
    }

    fn forwarding_client_for_test() -> Result<Client, Box<dyn std::error::Error>> {
        Ok(client_builder(reqwest::redirect::Policy::none(), None)?
            .no_proxy()
            .build()?)
    }

    #[test]
    fn test_mask_url() {
        assert_eq!(mask_url("http://127.0.0.1:7890"), "http://127.0.0.1:7890");
        assert_eq!(
            mask_url("http://user:pass@127.0.0.1:7890"),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            mask_url("socks5://admin:secret@proxy.example.com:1080"),
            "socks5://proxy.example.com:1080"
        );
        // 无端口的 URL 不应显示 ":?"
        assert_eq!(
            mask_url("http://proxy.example.com"),
            "http://proxy.example.com"
        );
        assert_eq!(
            mask_url("https://user:pass@proxy.example.com"),
            "https://proxy.example.com"
        );
    }

    #[test]
    fn test_mask_url_does_not_panic_on_multibyte_boundary() {
        // 一个无法被 Url::parse 解析、且在字节 20 处正好切在多字节字符中间的字符串。
        // 回归 https://github.com/farion1231/cc-switch 的 mask_url 越界 panic。
        let bad = "这是一个无效的代理地址不能解析";
        assert!(bad.len() > 20 && !bad.is_char_boundary(20));
        let masked = mask_url(bad);
        assert!(masked.ends_with("..."));
    }

    #[test]
    fn test_build_client_direct() {
        let result = build_client(None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_http_proxy() {
        let result = build_client(Some("http://127.0.0.1:7890"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_with_socks5_proxy() {
        let result = build_client(Some("socks5://127.0.0.1:1080"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_client_invalid_url() {
        // reqwest::Proxy::all 对某些无效 URL 不会立即报错
        // 使用明确无效的 scheme 来触发错误
        let result = build_client(Some("invalid-scheme://127.0.0.1:7890"));
        assert!(result.is_err(), "Should reject invalid proxy scheme");
    }

    #[test]
    fn test_proxy_points_to_loopback() {
        set_proxy_port(15721);

        assert!(system_proxy::points_to_self_proxy(
            "http://127.0.0.1:15721",
            15721
        ));
        assert!(system_proxy::points_to_self_proxy(
            "socks5://localhost:15721",
            15721
        ));
        assert!(system_proxy::points_to_self_proxy("127.0.0.1:15721", 15721));

        // 其他 loopback 端口不应该被跳过（允许使用其他本地代理工具）
        assert!(!system_proxy::points_to_self_proxy(
            "http://127.0.0.1:7890",
            15721
        ));
        assert!(!system_proxy::points_to_self_proxy(
            "socks5://localhost:1080",
            15721
        ));

        // 非 loopback 地址不应该被跳过
        assert!(!system_proxy::points_to_self_proxy(
            "http://192.168.1.10:7890",
            15721
        ));
        assert!(!system_proxy::points_to_self_proxy(
            "http://192.168.1.10:15721",
            15721
        ));
    }

    #[test]
    fn test_system_proxy_points_to_loopback() {
        let _guard = system_proxy::test_support::Guard::new();

        set_proxy_port(15721);

        let keys = [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ];

        for key in &keys {
            env::remove_var(key);
        }

        env::set_var("HTTP_PROXY", "http://127.0.0.1:15721");
        assert_eq!(
            system_proxy::resolve_effective_proxy(None, 15721),
            EffectiveProxy::Direct
        );

        // 指向其他端口的本地代理不应该被跳过
        env::set_var("HTTP_PROXY", "http://127.0.0.1:7890");
        assert!(matches!(
            system_proxy::resolve_effective_proxy(None, 15721),
            EffectiveProxy::Env(_)
        ));

        // 非 loopback 地址不应该被跳过
        env::set_var("HTTP_PROXY", "http://10.0.0.2:7890");
        assert!(matches!(
            system_proxy::resolve_effective_proxy(None, 15721),
            EffectiveProxy::Env(_)
        ));

        for key in &keys {
            env::remove_var(key);
        }
    }
}
