use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn env(pairs: &[(&str, &str)]) -> Environment {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
fn system(http: &str, socks: &str, bypass: &[&str]) -> SystemProxy {
    SystemProxy {
        url: if http.is_empty() { socks } else { http }.into(),
        http: http.into(),
        socks: socks.into(),
        bypass: bypass.iter().map(|v| v.to_string()).collect(),
    }
}

#[test]
fn codex_proxy_diagnostic_numeric_no_proxy_matches_magpie() {
    for list in [
        "localhost",
        "127.*",
        "*",
        "example.com",
        "127.0.0.1:15722",
        "128.0.0.0/8",
    ] {
        assert!(
            !resolver::no_proxy("127.0.0.1", list),
            "unexpected numeric bypass"
        );
    }
    for list in [
        "127.0.0.1",
        "127.0.0.0/8",
        "localhost, 127.0.0.1",
        "0.0.0.0/0",
    ] {
        assert!(
            resolver::no_proxy("127.0.0.1", list),
            "missing numeric bypass"
        );
    }
    assert!(resolver::no_proxy("[::1]", "::1"));
    assert!(resolver::no_proxy("::1", "::1/128"));
    assert!(resolver::no_proxy("child.example.com", ".example.com"));
    assert!(resolver::no_proxy("localhost", "*"));
    assert!(!resolver::no_proxy("notexample.com", "example.com"));
}

#[test]
fn codex_proxy_diagnostic_resolver_os_and_env_fixtures() {
    let http = "http://127.0.0.1:7890";
    let other = "http://127.0.0.1:7892";
    let socks = "socks5://127.0.0.1:7891";
    let empty = SystemProxy::default();
    let win = system(http, "", &["localhost", "127.*", "<local>"]);
    let mac = system(http, "", &["127.0.0.1", "localhost"]);
    let https_only = SystemProxy {
        url: other.into(),
        ..Default::default()
    };
    let mac_socks = system("", socks, &["*.local"]);
    let win_per_protocol = SystemProxy {
        url: http.into(),
        socks: socks.into(),
        ..Default::default()
    };
    let fixtures = [
        (Platform::Mac, false, env(&[]), &empty, None),
        (
            Platform::Mac,
            false,
            env(&[("HTTP_PROXY", http)]),
            &empty,
            Some((http, Source::Environment)),
        ),
        (
            Platform::Other,
            false,
            env(&[("http_proxy", http)]),
            &empty,
            Some((http, Source::Environment)),
        ),
        (
            Platform::Mac,
            false,
            env(&[("HTTP_PROXY", http), ("http_proxy", other)]),
            &empty,
            Some((http, Source::Environment)),
        ),
        (
            Platform::Mac,
            false,
            env(&[("HTTPS_PROXY", http)]),
            &empty,
            None,
        ),
        (
            Platform::Other,
            false,
            env(&[("ALL_PROXY", socks)]),
            &empty,
            Some((socks, Source::Environment)),
        ),
        (
            Platform::Mac,
            false,
            env(&[("HTTP_PROXY", http), ("NO_PROXY", "127.0.0.1")]),
            &empty,
            None,
        ),
        (
            Platform::Mac,
            false,
            env(&[
                ("HTTP_PROXY", http),
                ("NO_PROXY", "*"),
                ("no_proxy", "127.0.0.1"),
            ]),
            &empty,
            Some((http, Source::Environment)),
        ),
        (
            Platform::Windows,
            false,
            env(&[]),
            &win,
            Some((http, Source::System)),
        ),
        (Platform::Windows, false, env(&[]), &win_per_protocol, None),
        (Platform::Windows, false, env(&[]), &mac, None),
        (
            Platform::Windows,
            false,
            env(&[("NO_PROXY", "example.com")]),
            &mac,
            Some((http, Source::System)),
        ),
        (
            Platform::Mac,
            false,
            env(&[]),
            &mac,
            Some((http, Source::System)),
        ),
        (Platform::Mac, false, env(&[]), &https_only, None),
        (Platform::Mac, false, env(&[]), &mac_socks, None),
        (
            Platform::Mac,
            false,
            env(&[("ALL_PROXY", socks)]),
            &mac_socks,
            Some((socks, Source::Environment)),
        ),
        (
            Platform::Mac,
            false,
            env(&[("HTTP_PROXY", other)]),
            &mac,
            Some((other, Source::Environment)),
        ),
        (
            Platform::Mac,
            false,
            env(&[("ALL_PROXY", other)]),
            &mac,
            Some((http, Source::System)),
        ),
        (Platform::Windows, true, env(&[]), &win, None),
        (
            Platform::Windows,
            true,
            env(&[]),
            &win_per_protocol,
            Some((http, Source::System)),
        ),
        (Platform::Mac, true, env(&[]), &mac, None),
        (
            Platform::Mac,
            true,
            env(&[("HTTP_PROXY", http)]),
            &https_only,
            None,
        ),
        (
            Platform::Mac,
            true,
            env(&[("HTTP_PROXY", http)]),
            &empty,
            None,
        ),
        (
            Platform::Mac,
            true,
            env(&[("HTTP_PROXY", http)]),
            &mac_socks,
            Some((http, Source::Environment)),
        ),
        (
            Platform::Mac,
            true,
            env(&[("all_proxy", socks), ("no_proxy", "127.0.0.1")]),
            &mac_socks,
            None,
        ),
    ];
    for (index, (platform, respect, env, sys, want)) in fixtures.iter().enumerate() {
        let got = resolver::resolve("127.0.0.1", *respect, *platform, env, sys);
        assert_eq!(
            got.as_ref().map(|r| (r.url.as_str(), r.source)),
            *want,
            "resolver fixture {index}"
        );
    }
}

#[test]
fn codex_proxy_diagnostic_system_wildcards_differ_from_reqwest() {
    for pattern in ["127.*", "*", "http://127.*", "127.0.0.1"] {
        assert!(resolver::system_bypassed("127.0.0.1", &[pattern.into()]));
    }
    assert!(!resolver::system_bypassed("127.0.0.1", &["<local>".into()]));
    assert!(resolver::system_bypassed("localhost", &["<local>".into()]));
    assert!(resolver::system_bypassed(
        "child.example.com",
        &[".example.com".into()]
    ));
}

#[test]
fn codex_proxy_diagnostic_native_parsers() {
    let mac = resolver::parse_mac("<dictionary> {\n HTTPEnable : 1\n HTTPProxy : 127.0.0.1\n HTTPPort : 7890\n HTTPSEnable : 1\n HTTPSProxy : ::1\n HTTPSPort : 7892\n SOCKSEnable : 1\n SOCKSProxy : 127.0.0.1\n SOCKSPort : 7891\n ExceptionsList : <array> {\n 0 : localhost\n 1 : 127.*\n }\n}\n");
    assert_eq!(mac.url, "http://[::1]:7892");
    assert_eq!(mac.http, "http://127.0.0.1:7890");
    assert_eq!(mac.socks, "socks5://127.0.0.1:7891");
    assert_eq!(mac.bypass, vec!["localhost", "127.*"]);
    assert_eq!(
        resolver::parse_mac("HTTPEnable : 0\nHTTPProxy : 127.0.0.1\nHTTPPort : 7890"),
        SystemProxy::default()
    );
    let win = resolver::parse_windows("127.0.0.1:7890", "localhost;127.*;<local>");
    assert_eq!(win.http, "http://127.0.0.1:7890");
    assert_eq!(win.bypass, vec!["localhost", "127.*", "<local>"]);
    let win = resolver::parse_windows(
        "http=127.0.0.1:7890; https=127.0.0.1:7892;socks=127.0.0.1:7891",
        "",
    );
    assert!(win.http.is_empty());
    assert_eq!(win.url, "http://127.0.0.1:7892");
    assert_eq!(win.socks, "socks5://127.0.0.1:7891");
}

#[test]
fn codex_proxy_diagnostic_origin_sanitization() {
    assert_eq!(
        origin(
            "http://alice:synthetic@127.0.0.1:15722/intent/v1?private=value#secret",
            true
        )
        .as_deref(),
        Some("http://127.0.0.1:15722")
    );
    assert_eq!(
        origin("alice:synthetic@127.0.0.1:7890/path?secret=x", false).as_deref(),
        Some("http://127.0.0.1:7890")
    );
    assert_eq!(
        origin("socks5h://alice:synthetic@[::1]:7891/path", false).as_deref(),
        Some("socks5h://[::1]:7891")
    );
    for bad in [
        "https://127.0.0.1:15722",
        "http://example.com",
        "http://0.0.0.0:15722",
        "file:///tmp/test",
        "invalid",
    ] {
        assert!(origin(bad, true).is_none());
    }
    for bad in ["file:///tmp/test", "http://", "ftp://127.0.0.1:7890"] {
        assert!(origin(bad, false).is_none());
    }
}

#[test]
fn codex_proxy_diagnostic_login_values_are_only_exact_exported_keys() {
    // The NUL terminator before the address is explicit: `\0127.0.0.1` would read as an
    // octal escape and hide which byte the fixture actually intends.
    let mut bytes =
        b"startup noise\0VARY_PROXY_ENV_V1\0HTTP_PROXY\0http://127.0.0.1:7890\n\0NO_PROXY\0"
            .to_vec();
    bytes.extend_from_slice(b"127.0.0.1\n\0");
    let parsed = discovery::parse_login(&bytes).expect("synthetic login fixture");
    let mut process = env(&[
        ("HTTP_PROXY", "http://127.0.0.1:7892"),
        ("ALL_PROXY", "http://127.0.0.1:7891"),
    ]);
    discovery::merge_login(&mut process, parsed);
    assert_eq!(
        process.get("HTTP_PROXY").map(String::as_str),
        Some("http://127.0.0.1:7890")
    );
    assert_eq!(
        process.get("ALL_PROXY").map(String::as_str),
        Some("http://127.0.0.1:7891")
    );
    assert!(discovery::parse_login(b"HTTP_PROXY=http://127.0.0.1:7890").is_none());
    assert!(discovery::parse_login(b"\0VARY_PROXY_ENV_V1\0PRIVATE_TOKEN\0synthetic\0").is_none());
    assert!(discovery::KEYS
        .iter()
        .all(|key| key.ends_with("PROXY") || key.ends_with("proxy")));
}

struct Server {
    origin: String,
    task: tokio::task::JoinHandle<()>,
    requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    proxy: bool,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
        if !std::thread::panicking() {
            let requests = self.requests.lock().expect("fixture requests");
            for request in requests.iter() {
                assert!(request.starts_with(if self.proxy {
                    "GET http://127.0.0.1:"
                } else {
                    "GET /health "
                }));
                assert!(!request
                    .lines()
                    .any(|line| line.to_ascii_lowercase().starts_with("authorization:")));
                assert!(!request.contains("synthetic-model"));
            }
        }
    }
}
async fn server(responses: Vec<String>, proxy: bool) -> Result<Server, Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = requests.clone();
    let task = tokio::spawn(async move {
        let mut index = 0;
        while let Ok((mut stream, _)) = listener.accept().await {
            let response = responses[index.min(responses.len() - 1)].clone();
            index += 1;
            let mut bytes = vec![0; 8192];
            let Ok(n) = stream.read(&mut bytes).await else {
                continue;
            };
            if let Ok(mut requests) = recorded.lock() {
                requests.push(String::from_utf8_lossy(&bytes[..n]).into_owned());
            }
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    Ok(Server {
        origin,
        task,
        requests,
        proxy,
    })
}
fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
fn healthy() -> String {
    response("200 OK", r#"{"status":"healthy"}"#)
}
fn route(server: &Server) -> Route {
    Route {
        url: server.origin.clone(),
        source: Source::Environment,
    }
}

#[tokio::test]
async fn codex_proxy_diagnostic_only_proxy_failure_is_an_issue() -> TestResult {
    let target = server(vec![healthy()], false).await?;
    let proxy = server(vec![response("502 Bad Gateway", "synthetic failure")], true).await?;
    let issue = diagnose(&target.origin, Some(&route(&proxy)))
        .await
        .ok_or("missing diagnostic")?;
    assert_eq!(
        issue,
        ProxyIssue {
            target: target.origin.clone(),
            proxy: proxy.origin.clone(),
            source: Source::Environment
        }
    );
    let pass = server(vec![healthy()], true).await?;
    assert!(diagnose(&target.origin, Some(&route(&pass)))
        .await
        .is_none());
    assert!(diagnose(&target.origin, None).await.is_none());
    let down = server(vec![response("503 Service Unavailable", "offline")], false).await?;
    assert!(diagnose(&down.origin, Some(&route(&proxy))).await.is_none());
    Ok(())
}

#[tokio::test]
async fn codex_proxy_diagnostic_direct_recheck_prevents_shutdown_warning() -> TestResult {
    let target = server(
        vec![healthy(), response("503 Service Unavailable", "stopped")],
        false,
    )
    .await?;
    let proxy = server(vec![response("502 Bad Gateway", "")], true).await?;
    assert!(diagnose(&target.origin, Some(&route(&proxy)))
        .await
        .is_none());
    assert_eq!(
        target
            .requests
            .lock()
            .map_err(|_| "fixture recorder")?
            .len(),
        2
    );
    assert_eq!(
        proxy.requests.lock().map_err(|_| "fixture recorder")?.len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn codex_proxy_diagnostic_health_rejects_wrong_identity_redirect_and_oversize() -> TestResult
{
    let good = server(vec![healthy()], false).await?;
    assert!(health(&good.origin, None).await);
    let cases=vec![response("200 OK",r#"{"status":"other"}"#), response("200 OK","healthy"), response("200 OK",&format!(r#"{{"status":"healthy","padding":"{}"}}"#,"x".repeat(4096))), format!("HTTP/1.1 302 Found\r\nLocation: {}/health\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",good.origin)];
    for fixture in cases {
        let bad = server(vec![fixture], false).await?;
        assert!(!health(&bad.origin, None).await);
    }
    Ok(())
}

#[tokio::test]
async fn codex_proxy_diagnostic_cache_ttl_force_and_config_changes() -> TestResult {
    use std::sync::atomic::AtomicUsize;
    let cache = Cache::default();
    let count = AtomicUsize::new(0);
    let key = Snapshot {
        target: "http://127.0.0.1:15722".into(),
        respect_system_proxy: false,
        fingerprint: [1; 32],
    };
    let probe = || async {
        count.fetch_add(1, Ordering::SeqCst);
        None
    };
    cache.run(key.clone(), false, probe).await;
    cache.run(key.clone(), false, probe).await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
    cache.run(key.clone(), true, probe).await;
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let mut changed = key.clone();
    changed.fingerprint = [2; 32];
    cache.run(changed, false, probe).await;
    assert_eq!(count.load(Ordering::SeqCst), 3);
    cache.run(key.clone(), false, probe).await;
    cache
        .entry
        .lock()
        .await
        .as_mut()
        .ok_or("missing cache")?
        .created = Instant::now() - CACHE_TTL;
    cache.run(key, false, probe).await;
    assert_eq!(count.load(Ordering::SeqCst), 5);
    Ok(())
}

#[tokio::test]
async fn codex_proxy_diagnostic_single_flight_including_force() {
    use std::sync::{atomic::AtomicUsize, Arc};
    let cache = Arc::new(Cache::default());
    let count = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let key = Snapshot {
        target: "http://127.0.0.1:15722".into(),
        respect_system_proxy: false,
        fingerprint: [1; 32],
    };
    let first = {
        let cache = cache.clone();
        let count = count.clone();
        let started = started.clone();
        let release = release.clone();
        let key = key.clone();
        tokio::spawn(async move {
            cache
                .run(key, true, || async {
                    count.fetch_add(1, Ordering::SeqCst);
                    started.notify_one();
                    release.notified().await;
                    None
                })
                .await
        })
    };
    started.notified().await;
    let second_started = Arc::new(tokio::sync::Notify::new());
    let second = {
        let cache = cache.clone();
        let count = count.clone();
        let started = second_started.clone();
        tokio::spawn(async move {
            started.notify_one();
            cache
                .run(key, true, || async {
                    count.fetch_add(1, Ordering::SeqCst);
                    None
                })
                .await
        })
    };
    second_started.notified().await;
    release.notify_one();
    assert!(first.await.expect("first probe task").is_none());
    assert!(second.await.expect("second probe task").is_none());
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn codex_proxy_diagnostic_proxy_auth_stays_on_proxy_transport() -> TestResult {
    let target = server(vec![healthy()], false).await?;
    let proxy = server(vec![healthy()], true).await?;
    let url = proxy
        .origin
        .replacen("http://", "http://fixture-user:fixture-password@", 1);
    let route = Route {
        url,
        source: Source::Environment,
    };
    assert!(health(&target.origin, Some(&route)).await);
    let requests = proxy.requests.lock().map_err(|_| "fixture recorder")?;
    assert!(requests
        .iter()
        .any(|request| request.lines().any(|line| line
            .to_ascii_lowercase()
            .starts_with("proxy-authorization:"))));
    Ok(())
}

#[tokio::test]
async fn codex_proxy_diagnostic_socks_uses_codex_http_transport() -> TestResult {
    let target = server(vec![healthy()], false).await?;
    let proxy = server(vec![healthy()], true).await?;
    let route = Route {
        url: proxy.origin.replacen("http://", "socks5://", 1),
        source: Source::Environment,
    };
    assert!(health(&target.origin, Some(&route)).await);
    Ok(())
}
