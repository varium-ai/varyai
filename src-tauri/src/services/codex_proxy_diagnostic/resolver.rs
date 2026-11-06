use super::*;
use std::net::IpAddr;

fn value<'a>(env: &'a Environment, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| env.get(*key).map(|v| v.trim()).filter(|v| !v.is_empty()))
}

/// Match Codex's HTTP loopback route, including its native opt-in resolver.
pub(super) fn resolve(
    host: &str,
    respect: bool,
    platform: Platform,
    env: &Environment,
    system: &SystemProxy,
) -> Option<Route> {
    let environment = || {
        value(env, &["HTTP_PROXY", "http_proxy"])
            .or_else(|| value(env, &["ALL_PROXY", "all_proxy"]))
            .map(|url| Route {
                url: url.into(),
                source: Source::Environment,
            })
    };
    let no = value(env, &["NO_PROXY", "no_proxy"]).unwrap_or_default();
    if respect {
        if system.url.is_empty() || system_bypassed(host, &system.bypass) {
            return None;
        }
        if platform != Platform::Mac {
            return Some(Route {
                url: system.url.clone(),
                source: Source::System,
            });
        }
        if !system.http.is_empty() {
            return Some(Route {
                url: system.http.clone(),
                source: Source::System,
            });
        }
        // Codex cannot use a native macOS SOCKS entry and falls back to env.
        return if !system.socks.is_empty() && !no_proxy(host, no) {
            environment()
        } else {
            None
        };
    }
    let route = value(env, &["HTTP_PROXY", "http_proxy"])
        .map(|url| Route {
            url: url.into(),
            source: Source::Environment,
        })
        .or_else(|| {
            (!system.http.is_empty()).then(|| Route {
                url: system.http.clone(),
                source: Source::System,
            })
        })
        .or_else(environment);
    let windows_no;
    let no = if no.is_empty() && platform == Platform::Windows && !system.url.is_empty() {
        windows_no = system.bypass.join(",").replace("*.", "");
        windows_no.as_str()
    } else {
        no
    };
    if no_proxy(host, no) {
        None
    } else {
        route
    }
}

/// Numeric hosts match only literal IPs or CIDRs. '*' applies to names only.
pub(super) fn no_proxy(host: &str, list: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    let ip = host.parse::<IpAddr>().ok();
    list.split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .any(|entry| {
            if let Some(ip) = ip {
                if let Ok(other) = entry.parse::<IpAddr>() {
                    return ip == other;
                }
                let Some((network, prefix)) = entry.split_once('/') else {
                    return false;
                };
                let (Ok(network), Ok(prefix)) = (network.parse::<IpAddr>(), prefix.parse::<u32>())
                else {
                    return false;
                };
                match (ip, network) {
                    (IpAddr::V4(ip), IpAddr::V4(network)) if prefix <= 32 => {
                        let mask = u32::MAX.checked_shl(32 - prefix).unwrap_or(0);
                        u32::from(ip) & mask == u32::from(network) & mask
                    }
                    (IpAddr::V6(ip), IpAddr::V6(network)) if prefix <= 128 => {
                        let mask = u128::MAX.checked_shl(128 - prefix).unwrap_or(0);
                        u128::from(ip) & mask == u128::from(network) & mask
                    }
                    _ => false,
                }
            } else {
                let domain = entry
                    .strip_prefix('.')
                    .unwrap_or(entry)
                    .to_ascii_lowercase();
                let host = host.to_ascii_lowercase();
                entry == "*" || host == domain || host.ends_with(&format!(".{domain}"))
            }
        })
}

pub(super) fn system_bypassed(host: &str, list: &[String]) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    list.iter().any(|entry| {
        let entry = entry.trim().to_ascii_lowercase();
        let entry = entry
            .strip_prefix("http://")
            .or_else(|| entry.strip_prefix("https://"))
            .unwrap_or(&entry);
        if entry.is_empty() {
            return false;
        }
        if entry == "<local>" {
            return !host.contains('.');
        }
        if let Some(domain) = entry.strip_prefix('.') {
            return host == domain || host.ends_with(entry);
        }
        if entry.contains('*') {
            // IP and domain bypass patterns need only '*' (no shell expansion).
            let mut remaining = host.as_str();
            let parts: Vec<_> = entry.split('*').collect();
            for (index, part) in parts.iter().enumerate() {
                if index == 0 {
                    let Some(rest) = remaining.strip_prefix(part) else {
                        return false;
                    };
                    remaining = rest;
                } else if index == parts.len() - 1 {
                    return remaining.ends_with(part);
                } else if let Some(offset) = remaining.find(part) {
                    remaining = &remaining[offset + part.len()..];
                } else {
                    return false;
                }
            }
            return remaining.is_empty();
        }
        host == entry
    })
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn parse_mac(text: &str) -> SystemProxy {
    let mut values = BTreeMap::new();
    let mut bypass = Vec::new();
    let mut in_list = false;
    for line in text.lines().map(str::trim) {
        if in_list {
            if line == "}" {
                in_list = false;
            } else if let Some((_, v)) = line.split_once(" : ") {
                bypass.push(v.to_owned());
            }
        } else if let Some((key, value)) = line.split_once(" : ") {
            if key == "ExceptionsList" {
                in_list = true;
            } else {
                values.insert(key, value);
            }
        }
    }
    let entry = |key: &str, scheme: &str| {
        if values.get(format!("{key}Enable").as_str()) != Some(&"1") {
            return String::new();
        }
        let Some(host) = values
            .get(format!("{key}Proxy").as_str())
            .filter(|v| !v.is_empty())
        else {
            return String::new();
        };
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let port = values
            .get(format!("{key}Port").as_str())
            .map(|port| format!(":{port}"))
            .unwrap_or_default();
        format!("{scheme}://{host}{port}")
    };
    let http = entry("HTTP", "http");
    let socks = entry("SOCKS", "socks5");
    let url = [entry("HTTPS", "http"), http.clone(), socks.clone()]
        .into_iter()
        .find(|url| !url.is_empty())
        .unwrap_or_default();
    if url.is_empty() {
        SystemProxy::default()
    } else {
        SystemProxy {
            url,
            http,
            socks,
            bypass,
        }
    }
}

#[cfg(any(windows, test))]
pub(super) fn parse_windows(server: &str, overrides: &str) -> SystemProxy {
    let server = server.trim();
    if server.is_empty() {
        return SystemProxy::default();
    }
    let bypass = overrides.split(';').map(str::to_owned).collect();
    let entry = |value: &str, scheme: &str| {
        if value.is_empty() || value.contains("://") {
            value.to_string()
        } else {
            format!("{scheme}://{value}")
        }
    };
    if !server.contains('=') {
        let url = entry(server, "http");
        return SystemProxy {
            http: url.clone(),
            url,
            socks: String::new(),
            bypass,
        };
    }
    let by: BTreeMap<_, _> = server
        .split(';')
        .filter_map(|part| part.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim()))
        .collect();
    let socks = entry(by.get("socks").copied().unwrap_or_default(), "socks5");
    let url = [
        entry(by.get("https").copied().unwrap_or_default(), "http"),
        entry(by.get("http").copied().unwrap_or_default(), "http"),
        socks.clone(),
    ]
    .into_iter()
    .find(|url| !url.is_empty())
    .unwrap_or_default();
    if url.is_empty() {
        SystemProxy::default()
    } else {
        SystemProxy {
            url,
            http: String::new(),
            socks,
            bypass,
        }
    }
}
