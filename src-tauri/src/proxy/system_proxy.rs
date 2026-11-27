//! Dynamic proxy resolution with short-TTL OS caching and dead loopback fallback.
//!
//! Resolution order matches desktop expectations:
//! 1. Explicit proxy configured in Vary AI settings (never silently bypassed).
//! 2. Process environment variables (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY`),
//!    skipping entries pointing to Vary AI's own local proxy port.
//! 3. OS system proxy (macOS `scutil --proxy`, Windows `Internet Settings` registry),
//!    falling back to direct connection when a loopback system proxy port is not listening.
//! 4. Direct connection.

use std::collections::HashMap;
use std::env;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const SYSTEM_KEEP: Duration = Duration::from_secs(15);
const NONE_KEEP: Duration = Duration::from_secs(2);
const READ_WAIT: Duration = Duration::from_secs(2);
#[cfg(target_os = "macos")]
const TOOL_WAIT: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_millis(300);
const PORT_UP_KEEP: Duration = Duration::from_secs(15);
const PORT_DOWN_KEEP: Duration = Duration::from_secs(2);
const MAX_PORT_CACHE_ENTRIES: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SystemProxyConfig {
    pub https: Option<String>,
    pub http: Option<String>,
    pub socks: Option<String>,
    pub bypass: Vec<String>,
}

impl SystemProxyConfig {
    pub fn has_any(&self) -> bool {
        self.https.is_some() || self.http.is_some() || self.socks.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EnvProxyConfig {
    pub https: Option<String>,
    pub http: Option<String>,
    pub all: Option<String>,
    pub no_proxy: Vec<String>,
}

impl EnvProxyConfig {
    pub fn has_any(&self) -> bool {
        self.https.is_some() || self.http.is_some() || self.all.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EffectiveProxy {
    Explicit(String),
    Env(EnvProxyConfig),
    System(SystemProxyConfig),
    Direct,
}

impl EffectiveProxy {
    pub fn mode_label(&self) -> &'static str {
        match self {
            Self::Explicit(_) => "explicit",
            Self::Env(_) => "env",
            Self::System(_) => "system",
            Self::Direct => "direct",
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Explicit(url) => format!("explicit({})", super::http_client::mask_url(url)),
            Self::Env(cfg) => {
                let representative = cfg
                    .https
                    .as_deref()
                    .or(cfg.http.as_deref())
                    .or(cfg.all.as_deref())
                    .map(super::http_client::mask_url)
                    .unwrap_or_else(|| "none".to_string());
                format!("env({representative})")
            }
            Self::System(cfg) => {
                let representative = cfg
                    .https
                    .as_deref()
                    .or(cfg.http.as_deref())
                    .or(cfg.socks.as_deref())
                    .map(super::http_client::mask_url)
                    .unwrap_or_else(|| "none".to_string());
                format!("system({representative})")
            }
            Self::Direct => "direct".to_string(),
        }
    }

    pub fn proxy_for_url(&self, target: &url::Url) -> Option<url::Url> {
        let host = target.host_str()?;
        if is_loopback_host(host) {
            return None;
        }
        let target_port = target.port_or_known_default();
        let raw = match self {
            Self::Direct => return None,
            Self::Explicit(url) => Some(url.as_str()),
            Self::Env(cfg) => {
                if host_matches_bypass(host, target_port, &cfg.no_proxy) {
                    return None;
                }
                match target.scheme() {
                    "https" | "wss" => cfg
                        .https
                        .as_deref()
                        .or(cfg.http.as_deref())
                        .or(cfg.all.as_deref()),
                    _ => cfg.http.as_deref().or(cfg.all.as_deref()),
                }
            }
            Self::System(cfg) => {
                if is_lan_host(host) || host_matches_bypass(host, target_port, &cfg.bypass) {
                    return None;
                }
                match target.scheme() {
                    "https" | "wss" => cfg
                        .https
                        .as_deref()
                        .or(cfg.http.as_deref())
                        .or(cfg.socks.as_deref()),
                    _ => cfg.http.as_deref().or(cfg.socks.as_deref()),
                }
            }
        }?;
        url::Url::parse(raw).ok()
    }
}

pub(crate) fn validate_explicit_proxy_url(url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(url).map_err(|e| {
        format!(
            "Invalid proxy URL '{}': {}",
            super::http_client::mask_url(url),
            e
        )
    })?;
    let scheme = parsed.scheme();
    if !["http", "https", "socks5", "socks5h"].contains(&scheme) {
        return Err(format!(
            "Invalid proxy scheme '{}' in URL '{}'. Supported: http, https, socks5, socks5h",
            scheme,
            super::http_client::mask_url(url)
        ));
    }
    Ok(url.to_string())
}

pub(crate) fn resolve_effective_proxy(
    explicit_proxy: Option<&str>,
    self_proxy_port: u16,
) -> EffectiveProxy {
    resolve_with_reader(explicit_proxy, self_proxy_port, false, false)
}

pub(crate) fn resolve_effective_proxy_warm(
    explicit_proxy: Option<&str>,
    self_proxy_port: u16,
) -> EffectiveProxy {
    resolve_with_reader(explicit_proxy, self_proxy_port, false, true)
}

pub(crate) fn force_refresh_effective_proxy(
    explicit_proxy: Option<&str>,
    self_proxy_port: u16,
) -> EffectiveProxy {
    resolve_with_reader(explicit_proxy, self_proxy_port, true, false)
}

fn resolve_with_reader(
    explicit_proxy: Option<&str>,
    self_proxy_port: u16,
    force_refresh: bool,
    wait_if_uninitialized: bool,
) -> EffectiveProxy {
    if let Some(explicit) = explicit_proxy.map(str::trim).filter(|s| !s.is_empty()) {
        return EffectiveProxy::Explicit(explicit.to_string());
    }

    let env_cfg = read_env_proxy(self_proxy_port);
    if env_cfg.has_any() {
        return EffectiveProxy::Env(env_cfg);
    }

    let live_system = if force_refresh {
        clear_port_liveness_cache();
        system_cache().force_refresh(self_proxy_port)
    } else {
        system_cache().get(self_proxy_port, wait_if_uninitialized)
    };

    if live_system.has_any() {
        EffectiveProxy::System(live_system)
    } else {
        EffectiveProxy::Direct
    }
}

fn read_env_proxy(self_proxy_port: u16) -> EnvProxyConfig {
    let pick = |keys: &[&str]| -> Option<String> {
        for key in keys {
            if let Ok(raw) = env::var(key) {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Some(normalized) = normalize_proxy_url(trimmed, "http") {
                    if !points_to_self_proxy(&normalized, self_proxy_port) {
                        return Some(normalized);
                    }
                }
            }
        }
        None
    };

    let no_proxy = ["NO_PROXY", "no_proxy"]
        .into_iter()
        .find_map(|key| env::var(key).ok())
        .map(|val| {
            val.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    EnvProxyConfig {
        https: pick(&["HTTPS_PROXY", "https_proxy"]),
        http: pick(&["HTTP_PROXY", "http_proxy"]),
        all: pick(&["ALL_PROXY", "all_proxy"]),
        no_proxy,
    }
}

fn filter_live_system_proxy(raw: SystemProxyConfig, self_proxy_port: u16) -> SystemProxyConfig {
    let keep = |entry: Option<String>| -> Option<String> {
        let raw_url = entry?;
        let parsed = url::Url::parse(&raw_url).ok()?;
        let scheme = parsed.scheme();
        if !["http", "https", "socks5", "socks5h"].contains(&scheme) {
            return None;
        }
        if points_to_self_proxy(&raw_url, self_proxy_port) {
            return None;
        }
        if let Some(host) = parsed.host_str() {
            if is_loopback_host(host) {
                let port = parsed.port_or_known_default()?;
                if !is_loopback_port_listening(host, port) {
                    return None;
                }
            }
        }
        Some(raw_url)
    };

    SystemProxyConfig {
        https: keep(raw.https),
        http: keep(raw.http),
        socks: keep(raw.socks),
        bypass: raw.bypass,
    }
}

fn normalize_proxy_url(raw: &str, default_scheme: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("{default_scheme}://{trimmed}")
    };
    let parsed = url::Url::parse(&candidate).ok()?;
    let scheme = parsed.scheme();
    if !["http", "https", "socks5", "socks5h"].contains(&scheme) || parsed.host_str().is_none() {
        return None;
    }
    Some(candidate)
}

pub(crate) fn points_to_self_proxy(value: &str, self_proxy_port: u16) -> bool {
    if let Ok(parsed) = url::Url::parse(value) {
        if let Some(host) = parsed.host_str() {
            return is_loopback_host(host) && parsed.port() == Some(self_proxy_port);
        }
        return false;
    }
    let with_scheme = format!("http://{value}");
    if let Ok(parsed) = url::Url::parse(&with_scheme) {
        if let Some(host) = parsed.host_str() {
            return is_loopback_host(host) && parsed.port() == Some(self_proxy_port);
        }
    }
    false
}

fn is_loopback_host(host: &str) -> bool {
    let bare = host.trim_matches(|c| c == '[' || c == ']');
    if bare.eq_ignore_ascii_case("localhost")
        || bare
            .to_ascii_lowercase()
            .strip_suffix(".localhost")
            .is_some()
    {
        return true;
    }
    bare.parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

fn is_lan_host(host: &str) -> bool {
    let bare = host.trim_matches(|c| c == '[' || c == ']');
    if bare
        .trim_end_matches('.')
        .to_ascii_lowercase()
        .ends_with(".local")
    {
        return true;
    }
    let Ok(ip) = bare.parse::<IpAddr>() else {
        return false;
    };
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
    }
}

fn host_matches_bypass(host: &str, port: Option<u16>, bypass: &[String]) -> bool {
    if bypass.is_empty() {
        return false;
    }
    let host_bare = host
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let host_ip = host_bare.parse::<IpAddr>().ok();

    for raw_entry in bypass {
        let entry = raw_entry.trim().to_ascii_lowercase();
        if entry.is_empty() {
            continue;
        }
        if entry == "*" {
            return true;
        }
        if entry == "<local>" {
            if !host_bare.contains('.') {
                return true;
            }
            continue;
        }
        if let Some(ip) = host_ip {
            if ip_matches_cidr_entry(ip, &entry) {
                return true;
            }
        }
        let (pattern_host, pattern_port) = split_host_and_optional_port(&entry);
        if let Some(expected_port) = pattern_port {
            if port != Some(expected_port) {
                continue;
            }
        }
        let pattern_host = pattern_host.trim_matches(|c| c == '[' || c == ']');
        if pattern_host.is_empty() {
            continue;
        }
        if let Some(suffix) = pattern_host.strip_prefix("*.") {
            if host_ip.is_none()
                && (host_bare == suffix || host_bare.ends_with(&format!(".{suffix}")))
            {
                return true;
            }
        } else if let Some(suffix) = pattern_host.strip_prefix('.') {
            if host_ip.is_none()
                && (host_bare == suffix || host_bare.ends_with(&format!(".{suffix}")))
            {
                return true;
            }
        } else if host_bare == pattern_host
            || (host_ip.is_none() && host_bare.ends_with(&format!(".{pattern_host}")))
        {
            return true;
        }
    }
    false
}

fn split_host_and_optional_port(entry: &str) -> (&str, Option<u16>) {
    if let Some(rest) = entry.strip_prefix('[') {
        if let Some((host, after)) = rest.split_once(']') {
            if let Some(port_str) = after.strip_prefix(':') {
                if let Ok(port) = port_str.parse::<u16>() {
                    return (host, Some(port));
                }
            }
            return (host, None);
        }
    }
    if entry.matches(':').count() == 1 {
        if let Some((host, port_str)) = entry.split_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                return (host, Some(port));
            }
        }
    }
    (entry, None)
}

fn ip_matches_cidr_entry(ip: IpAddr, entry: &str) -> bool {
    let Some((net_str, prefix_str)) = entry.split_once('/') else {
        return false;
    };
    let Ok(prefix) = prefix_str.parse::<u8>() else {
        return false;
    };
    match ip {
        IpAddr::V4(addr) => {
            let Some(net) = parse_ipv4_prefix_addr(net_str) else {
                return false;
            };
            if prefix > 32 {
                return false;
            }
            if prefix == 0 {
                return true;
            }
            let mask = u32::MAX << (32 - prefix);
            (u32::from(addr) & mask) == (u32::from(net) & mask)
        }
        IpAddr::V6(addr) => {
            let Ok(net) = net_str
                .trim_matches(|c| c == '[' || c == ']')
                .parse::<Ipv6Addr>()
            else {
                return false;
            };
            if prefix > 128 {
                return false;
            }
            if prefix == 0 {
                return true;
            }
            let mask = u128::MAX << (128 - prefix);
            (u128::from(addr) & mask) == (u128::from(net) & mask)
        }
    }
}

fn parse_ipv4_prefix_addr(input: &str) -> Option<Ipv4Addr> {
    if let Ok(addr) = input.parse::<Ipv4Addr>() {
        return Some(addr);
    }
    // Support shortened macOS/BSD CIDR forms such as `169.254/16` or `10/8`.
    let parts: Vec<&str> = input.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut octets = [0u8; 4];
    for (idx, part) in parts.iter().enumerate() {
        octets[idx] = part.parse::<u8>().ok()?;
    }
    Some(Ipv4Addr::from(octets))
}

#[derive(Clone, Copy)]
struct PortCacheEntry {
    checked_at: Instant,
    listening: bool,
}

fn port_cache() -> &'static Mutex<HashMap<(String, u16), PortCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, u16), PortCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn clear_port_liveness_cache() {
    if let Ok(mut guard) = port_cache().lock() {
        guard.clear();
    }
}

fn is_loopback_port_listening(host: &str, port: u16) -> bool {
    let key_host = host
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    let key = (key_host.clone(), port);
    if let Ok(guard) = port_cache().lock() {
        if let Some(entry) = guard.get(&key) {
            let ttl = if entry.listening {
                PORT_UP_KEEP
            } else {
                PORT_DOWN_KEEP
            };
            if entry.checked_at.elapsed() <= ttl {
                return entry.listening;
            }
        }
    }

    let listening = probe_loopback_tcp(&key_host, port);
    if let Ok(mut guard) = port_cache().lock() {
        if guard.len() >= MAX_PORT_CACHE_ENTRIES {
            guard.clear();
        }
        guard.insert(
            key,
            PortCacheEntry {
                checked_at: Instant::now(),
                listening,
            },
        );
    }
    listening
}

fn probe_loopback_tcp(host: &str, port: u16) -> bool {
    #[cfg(test)]
    if let Some(result) = test_support::probe_override(host, port) {
        return result;
    }
    let addr_candidates: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
        ]
    };
    addr_candidates
        .into_iter()
        .any(|addr| TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok())
}

#[derive(Default)]
struct CacheState {
    last_finished: Option<Instant>,
    live_config: SystemProxyConfig,
    filtered_for_port: u16,
    last_failed: bool,
    refreshing: bool,
    generation: u64,
}

struct SystemCache {
    state: Mutex<CacheState>,
    notify: Condvar,
}

impl SystemCache {
    fn new() -> Self {
        Self {
            state: Mutex::new(CacheState::default()),
            notify: Condvar::new(),
        }
    }

    fn get(&self, self_proxy_port: u16, wait_if_uninitialized: bool) -> SystemProxyConfig {
        #[cfg(test)]
        if let Some(override_cfg) = test_support::system_reader_override() {
            return filter_live_system_proxy(override_cfg.unwrap_or_default(), self_proxy_port);
        }

        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let keep = if guard.live_config.has_any() {
            SYSTEM_KEEP
        } else {
            NONE_KEEP
        };
        if let Some(at) = guard.last_finished {
            if guard.filtered_for_port == self_proxy_port && at.elapsed() <= keep {
                return guard.live_config.clone();
            }
        }

        let should_wait = wait_if_uninitialized && guard.last_finished.is_none();
        let seen_gen = guard.generation;
        if !guard.refreshing {
            guard.refreshing = true;
            spawn_system_probe(self_proxy_port);
        }

        if should_wait {
            let deadline = Instant::now() + READ_WAIT;
            while guard.refreshing && guard.generation == seen_gen {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    break;
                };
                if remaining.is_zero() {
                    break;
                }
                let wait_result = match self.notify.wait_timeout(guard, remaining) {
                    Ok(res) => res,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard = wait_result.0;
                if wait_result.1.timed_out() {
                    break;
                }
            }
        }

        guard.live_config.clone()
    }

    fn force_refresh(&self, self_proxy_port: u16) -> SystemProxyConfig {
        #[cfg(test)]
        if let Some(override_cfg) = test_support::system_reader_override() {
            let filtered =
                filter_live_system_proxy(override_cfg.unwrap_or_default(), self_proxy_port);
            if let Ok(mut guard) = self.state.lock() {
                guard.live_config = filtered.clone();
                guard.filtered_for_port = self_proxy_port;
                guard.last_finished = Some(Instant::now());
                guard.last_failed = false;
                guard.generation = guard.generation.wrapping_add(1);
            }
            return filtered;
        }

        let result =
            read_os_system_proxy().map(|cfg| filter_live_system_proxy(cfg, self_proxy_port));
        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        match result {
            Some(cfg) => {
                guard.live_config = cfg;
                guard.filtered_for_port = self_proxy_port;
                guard.last_failed = false;
            }
            None => {
                guard.last_failed = true;
            }
        }
        guard.last_finished = Some(Instant::now());
        guard.refreshing = false;
        guard.generation = guard.generation.wrapping_add(1);
        let out = guard.live_config.clone();
        self.notify.notify_all();
        out
    }

    fn finish_probe(&self, result: Option<SystemProxyConfig>, self_proxy_port: u16) {
        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        match result {
            Some(cfg) => {
                guard.live_config = cfg;
                guard.filtered_for_port = self_proxy_port;
                guard.last_failed = false;
            }
            None => {
                guard.last_failed = true;
            }
        }
        guard.last_finished = Some(Instant::now());
        guard.refreshing = false;
        guard.generation = guard.generation.wrapping_add(1);
        self.notify.notify_all();
    }
}

fn system_cache() -> &'static SystemCache {
    static CACHE: OnceLock<SystemCache> = OnceLock::new();
    CACHE.get_or_init(SystemCache::new)
}

fn spawn_system_probe(self_proxy_port: u16) {
    let spawned = std::thread::Builder::new()
        .name("system-proxy-probe".to_string())
        .spawn(move || {
            let result =
                read_os_system_proxy().map(|cfg| filter_live_system_proxy(cfg, self_proxy_port));
            system_cache().finish_probe(result, self_proxy_port);
        });
    if spawned.is_err() {
        system_cache().finish_probe(None, self_proxy_port);
    }
}

fn host_discovery_allowed() -> bool {
    !cfg!(test) && env::var_os("CC_SWITCH_TEST_HOME").is_none()
}

fn read_os_system_proxy() -> Option<SystemProxyConfig> {
    if !host_discovery_allowed() {
        return Some(SystemProxyConfig::default());
    }
    read_os_system_proxy_native()
}

#[cfg(target_os = "macos")]
fn read_os_system_proxy_native() -> Option<SystemProxyConfig> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let mut child = Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + TOOL_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut stdout = child.stdout.take()?;
                let mut buf = Vec::new();
                stdout.read_to_end(&mut buf).ok()?;
                let text = std::str::from_utf8(&buf).ok()?;
                return Some(parse_mac_scutil(text));
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(windows)]
fn read_os_system_proxy_native() -> Option<SystemProxyConfig> {
    use winreg::{
        enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE},
        RegKey,
    };

    fn missing(error: &std::io::Error) -> bool {
        error.kind() == std::io::ErrorKind::NotFound
    }

    let key = match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
        KEY_QUERY_VALUE,
    ) {
        Ok(key) => key,
        Err(error) if missing(&error) => return Some(SystemProxyConfig::default()),
        Err(_) => return None,
    };
    match key.get_value::<u32, _>("ProxyEnable") {
        Ok(0) => return Some(SystemProxyConfig::default()),
        Ok(_) => {}
        Err(error) if missing(&error) => return Some(SystemProxyConfig::default()),
        Err(_) => return None,
    }
    let server: String = match key.get_value("ProxyServer") {
        Ok(server) => server,
        Err(error) if missing(&error) => return Some(SystemProxyConfig::default()),
        Err(_) => return None,
    };
    let overrides: String = match key.get_value("ProxyOverride") {
        Ok(value) => value,
        Err(error) if missing(&error) => String::new(),
        Err(_) => return None,
    };
    if server.len() + overrides.len() > 64 * 1024 {
        return None;
    }
    Some(parse_windows_registry(&server, &overrides))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn read_os_system_proxy_native() -> Option<SystemProxyConfig> {
    Some(SystemProxyConfig::default())
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn parse_mac_scutil(text: &str) -> SystemProxyConfig {
    let mut values = std::collections::BTreeMap::new();
    let mut bypass = Vec::new();
    let mut in_list = false;
    for line in text.lines().map(str::trim) {
        if in_list {
            if line == "}" {
                in_list = false;
            } else if let Some((_, value)) = line.split_once(" : ") {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    bypass.push(trimmed.to_string());
                }
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(" : ") {
            if key == "ExceptionsList" {
                in_list = true;
            } else {
                values.insert(key.trim(), value.trim());
            }
        }
    }

    let entry = |key: &str, scheme: &str| -> Option<String> {
        let enable_key = format!("{key}Enable");
        if values.get(enable_key.as_str()).copied() != Some("1") {
            return None;
        }
        let proxy_key = format!("{key}Proxy");
        let host = values
            .get(proxy_key.as_str())
            .copied()
            .filter(|v| !v.is_empty())?;
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let port_key = format!("{key}Port");
        let port = values
            .get(port_key.as_str())
            .copied()
            .filter(|p| !p.is_empty())
            .map(|p| format!(":{p}"))
            .unwrap_or_default();
        normalize_proxy_url(&format!("{scheme}://{host}{port}"), scheme)
    };

    let https = entry("HTTPS", "http");
    let http = entry("HTTP", "http");
    let socks = entry("SOCKS", "socks5h");
    if https.is_none() && http.is_none() && socks.is_none() {
        return SystemProxyConfig::default();
    }
    if values.get("ExcludeSimpleHostnames").copied() == Some("1")
        && !bypass.iter().any(|b| b.eq_ignore_ascii_case("<local>"))
    {
        bypass.push("<local>".to_string());
    }
    SystemProxyConfig {
        https,
        http,
        socks,
        bypass,
    }
}

#[cfg(any(windows, test))]
pub(crate) fn parse_windows_registry(server: &str, overrides: &str) -> SystemProxyConfig {
    let server = server.trim();
    if server.is_empty() {
        return SystemProxyConfig::default();
    }
    let bypass: Vec<String> = overrides
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();

    if !server.contains('=') {
        let Some(url) = normalize_proxy_url(server, "http") else {
            return SystemProxyConfig::default();
        };
        return SystemProxyConfig {
            https: Some(url.clone()),
            http: Some(url),
            socks: None,
            bypass,
        };
    }

    let by: std::collections::BTreeMap<String, &str> = server
        .split(';')
        .filter_map(|part| part.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim()))
        .collect();
    let https = by.get("https").and_then(|v| normalize_proxy_url(v, "http"));
    let http = by.get("http").and_then(|v| normalize_proxy_url(v, "http"));
    let socks = by
        .get("socks")
        .and_then(|v| normalize_proxy_url(v, "socks5h"));
    if https.is_none() && http.is_none() && socks.is_none() {
        return SystemProxyConfig::default();
    }
    SystemProxyConfig {
        https,
        http,
        socks,
        bypass,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::cell::RefCell;

    type ProbeFn = Box<dyn Fn(&str, u16) -> bool + Send + Sync>;

    struct Hooks {
        system_cfg: Option<Option<SystemProxyConfig>>,
        probe: Option<ProbeFn>,
    }

    static TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    static GLOBAL_HOOKS: OnceLock<Mutex<Hooks>> = OnceLock::new();

    thread_local! {
        static DYNAMIC_SESSION_CLIENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static SCOPED_SYSTEM_OVERRIDE: RefCell<Option<SystemProxyConfig>> = const { RefCell::new(None) };
    }

    fn lock_sync() -> tokio::sync::MutexGuard<'static, ()> {
        TEST_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .blocking_lock()
    }

    async fn lock_async() -> tokio::sync::MutexGuard<'static, ()> {
        TEST_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await
    }

    fn hooks() -> &'static Mutex<Hooks> {
        GLOBAL_HOOKS.get_or_init(|| {
            Mutex::new(Hooks {
                system_cfg: None,
                probe: None,
            })
        })
    }

    pub(crate) fn set_system_override(cfg: Option<SystemProxyConfig>) {
        if let Ok(mut guard) = hooks().lock() {
            guard.system_cfg = Some(cfg);
        }
        clear_port_liveness_cache();
    }

    pub(crate) fn set_probe_override<F>(f: F)
    where
        F: Fn(&str, u16) -> bool + Send + Sync + 'static,
    {
        if let Ok(mut guard) = hooks().lock() {
            guard.probe = Some(Box::new(f));
        }
        clear_port_liveness_cache();
    }

    pub(crate) fn clear_overrides() {
        if let Ok(mut guard) = hooks().lock() {
            guard.system_cfg = None;
            guard.probe = None;
        }
        clear_port_liveness_cache();
    }

    pub(super) fn system_reader_override() -> Option<Option<SystemProxyConfig>> {
        if let Some(cfg) = SCOPED_SYSTEM_OVERRIDE.with(|slot| slot.borrow().clone()) {
            return Some(Some(cfg));
        }
        hooks().lock().ok().and_then(|g| g.system_cfg.clone())
    }

    pub(super) fn probe_override(host: &str, port: u16) -> Option<bool> {
        hooks()
            .lock()
            .ok()
            .and_then(|g| g.probe.as_ref().map(|f| f(host, port)))
    }

    pub(crate) fn set_dynamic_session_client(enabled: bool) {
        DYNAMIC_SESSION_CLIENT.with(|cell| cell.set(enabled));
    }

    pub(crate) fn dynamic_session_client_enabled() -> bool {
        DYNAMIC_SESSION_CLIENT.with(|cell| cell.get())
    }

    pub(crate) struct Guard {
        _lock: tokio::sync::MutexGuard<'static, ()>,
    }

    impl Guard {
        pub(crate) fn new() -> Self {
            let lock = lock_sync();
            clear_overrides();
            set_dynamic_session_client(false);
            Self { _lock: lock }
        }

        pub(crate) async fn new_async() -> Self {
            let lock = lock_async().await;
            clear_overrides();
            set_dynamic_session_client(false);
            Self { _lock: lock }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            clear_overrides();
            set_dynamic_session_client(false);
            SCOPED_SYSTEM_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mac_scutil_reads_schemes_ipv6_and_exceptions() {
        let output = r#"
<dictionary> {
  HTTPEnable : 1
  HTTPPort : 7890
  HTTPProxy : 127.0.0.1
  HTTPSEnable : 1
  HTTPSPort : 7891
  HTTPSProxy : ::1
  SOCKSEnable : 1
  SOCKSPort : 1080
  SOCKSProxy : 127.0.0.1
  ExcludeSimpleHostnames : 1
  ExceptionsList : <array> {
    0 : *.local
    1 : 169.254/16
    2 : api.internal.test
  }
}
"#;
        let parsed = parse_mac_scutil(output);
        assert_eq!(parsed.https.as_deref(), Some("http://[::1]:7891"));
        assert_eq!(parsed.http.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(parsed.socks.as_deref(), Some("socks5h://127.0.0.1:1080"));
        assert_eq!(
            parsed.bypass,
            vec!["*.local", "169.254/16", "api.internal.test", "<local>"]
        );
    }

    #[test]
    fn parse_windows_registry_supports_single_and_per_protocol_formats() {
        let single = parse_windows_registry("127.0.0.1:7890", "<local>;*.corp.local");
        assert_eq!(single.https.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(single.http.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(single.socks, None);
        assert_eq!(single.bypass, vec!["<local>", "*.corp.local"]);

        let multi = parse_windows_registry(
            "http=127.0.0.1:8080;https=127.0.0.1:8443;socks=127.0.0.1:1080",
            "10.0.0.0/8",
        );
        assert_eq!(multi.https.as_deref(), Some("http://127.0.0.1:8443"));
        assert_eq!(multi.http.as_deref(), Some("http://127.0.0.1:8080"));
        assert_eq!(multi.socks.as_deref(), Some("socks5h://127.0.0.1:1080"));
        assert_eq!(multi.bypass, vec!["10.0.0.0/8"]);
    }

    #[test]
    fn bypass_and_lan_rules_match_correctly() {
        let sys = EffectiveProxy::System(SystemProxyConfig {
            https: Some("http://127.0.0.1:7890".into()),
            http: Some("http://127.0.0.1:7890".into()),
            socks: None,
            bypass: vec![
                "<local>".into(),
                "*.corp.example".into(),
                ".internal.test".into(),
                "169.254/16".into(),
                "203.0.113.10".into(),
                "api.port-test.example:443".into(),
                "1.1".into(),
            ],
        });

        let direct_urls = [
            "http://127.0.0.1:15722/v1/responses",
            "https://localhost:8443/api",
            "https://192.168.1.20/v1",
            "https://10.1.2.3/v1",
            "https://172.16.0.1/v1",
            "https://printer.local/status",
            "https://intranet/api",
            "https://git.corp.example/api",
            "https://svc.internal.test/v1",
            "https://169.254.10.1/meta",
            "https://203.0.113.10/v1",
            "https://api.port-test.example/v1",
        ];
        for raw in direct_urls {
            let url = url::Url::parse(raw).unwrap();
            assert_eq!(sys.proxy_for_url(&url), None, "expected direct for {raw}");
        }

        // IP literal must not falsely match domain-suffix bypass entry `1.1`,
        // and non-matching port must still be proxied.
        for raw in [
            "https://api.openai.com/v1/responses",
            "https://203.0.1.1/v1/responses",
            "http://api.port-test.example/v1",
        ] {
            let proxied = url::Url::parse(raw).unwrap();
            assert_eq!(
                sys.proxy_for_url(&proxied).map(|u| u.to_string()),
                Some("http://127.0.0.1:7890/".to_string()),
                "expected proxy for {raw}"
            );
        }
    }

    #[test]
    fn dead_loopback_system_proxy_falls_back_to_direct_and_recovers() {
        let _guard = test_support::Guard::new();
        let port_up = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let port_up_probe = std::sync::Arc::clone(&port_up);
        test_support::set_probe_override(move |_host, port| {
            port == 7899 && port_up_probe.load(std::sync::atomic::Ordering::Acquire)
        });
        test_support::set_system_override(Some(SystemProxyConfig {
            https: Some("http://127.0.0.1:7899".into()),
            http: Some("http://127.0.0.1:7899".into()),
            socks: None,
            bypass: vec![],
        }));

        // Dead loopback proxy -> falls back to direct
        assert_eq!(resolve_effective_proxy(None, 15722), EffectiveProxy::Direct);

        // Proxy starts listening + force refresh -> immediately picks it up
        port_up.store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(
            force_refresh_effective_proxy(None, 15722),
            EffectiveProxy::System(SystemProxyConfig {
                https: Some("http://127.0.0.1:7899".into()),
                http: Some("http://127.0.0.1:7899".into()),
                socks: None,
                bypass: vec![],
            })
        );

        // Remote system proxy is never probed and stays active
        test_support::set_probe_override(|_, _| panic!("remote proxy must not be probed"));
        test_support::set_system_override(Some(SystemProxyConfig {
            https: Some("http://10.9.8.7:3128".into()),
            http: None,
            socks: None,
            bypass: vec![],
        }));
        assert!(matches!(
            force_refresh_effective_proxy(None, 15722),
            EffectiveProxy::System(_)
        ));

        // Explicit Vary AI global proxy is never bypassed even if loopback port is down
        test_support::set_probe_override(|_, _| false);
        assert_eq!(
            resolve_effective_proxy(Some("http://127.0.0.1:7899"), 15722),
            EffectiveProxy::Explicit("http://127.0.0.1:7899".into())
        );
    }
}
