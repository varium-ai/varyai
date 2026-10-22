//! Doubao Work's native Chromium connection, independent of other clients.
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::{watch, Mutex},
    task::JoinHandle,
};
use tokio_tungstenite::tungstenite::Message;

pub(crate) const ENABLED_KEY: &str = "doubaowork_audit_enabled";
pub(crate) const CONSENT_KEY: &str = "doubaowork_local_connection_consent";
const COMPLETION: &str = "https://www.doubao.com/chat/completion*";

#[derive(Default)]
pub(crate) struct Manager {
    worker: Mutex<Option<(watch::Sender<bool>, JoinHandle<()>)>>,
    connected: Arc<AtomicBool>,
}
impl Manager {
    pub fn active(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }
    pub async fn enable(&self) {
        let mut worker = self.worker.lock().await;
        if worker.as_ref().is_some_and(|(_, task)| !task.is_finished()) {
            return;
        }
        let (tx, mut stop) = watch::channel(false);
        let active = self.connected.clone();
        let task = tokio::spawn(async move {
            loop {
                if *stop.borrow() {
                    break;
                }
                if let Some(endpoint) = discover().await {
                    let _ = connect(&endpoint, stop.clone(), &active).await;
                }
                active.store(false, Ordering::Release);
                tokio::select! {
                    _ = stop.changed() => break,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
            active.store(false, Ordering::Release);
        });
        *worker = Some((tx, task));
    }
    pub async fn disable(&self) {
        let mut worker = self.worker.lock().await;
        if let Some((stop, mut task)) = worker.take() {
            let _ = stop.send(true);
            if tokio::time::timeout(Duration::from_secs(4), &mut task)
                .await
                .is_err()
            {
                task.abort();
            }
        }
        self.connected.store(false, Ordering::Release);
    }
}

fn port_file() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(dirs::home_dir()?.join("Library/Application Support/DoubaoWork/DevToolsActivePort"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
fn browser_endpoint(source: &str) -> Option<String> {
    let mut lines = source.lines();
    let port: u16 = lines.next()?.parse().ok()?;
    let path = lines.next()?;
    if port == 0
        || !path.starts_with("/devtools/browser/")
        || path[18..].is_empty()
        || path
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || "/-".contains(c)))
    {
        return None;
    }
    Some(format!("ws://127.0.0.1:{port}{path}"))
}
async fn discover() -> Option<String> {
    let source = tokio::fs::read_to_string(port_file()?).await.ok()?;
    let endpoint = browser_endpoint(&source)?;
    let port = url::Url::parse(&endpoint).ok()?.port()?;
    // Never follow redirects, environment proxies, or a debugger URL supplied by another host.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(1))
        .build()
        .ok()?;
    let version: Value = client
        .get(format!("http://127.0.0.1:{port}/json/version"))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    if version["webSocketDebuggerUrl"].as_str()? != endpoint {
        return None;
    }
    Some(endpoint)
}

/// Explicit user action only. Never stop a running client or alter its signed bundle/settings.
pub(crate) fn launch() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let paths = [
            std::path::PathBuf::from("/Applications/DoubaoWork.app"),
            crate::config::get_home_dir().join("Applications/DoubaoWork.app"),
        ];
        let bundle = paths
            .iter()
            .find(|p| p.is_dir())
            .ok_or("homeAudit.doubaoNotInstalled")?;
        let output = std::process::Command::new("/usr/bin/pgrep")
            .args(["-f", "DoubaoWork.app/Contents/MacOS/DoubaoWork"])
            .output()
            .map_err(|_| "homeAudit.doubaoLaunchFailed")?;
        if output.status.success() {
            return Err("homeAudit.doubaoQuitFirst".into());
        }
        std::process::Command::new(bundle.join("Contents/MacOS/DoubaoWork"))
            .args([
                "--remote-debugging-port=0",
                "--remote-debugging-address=127.0.0.1",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|_| "homeAudit.doubaoLaunchFailed")?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("homeAudit.doubaoPlatform".into())
    }
}

fn text_envelope(native: &Value) -> (Value, Vec<String>) {
    let mut texts = Vec::new();
    let mut paths = Vec::new();
    if let Some(messages) = native["messages"].as_array() {
        for (i, message) in messages.iter().enumerate() {
            if let Some(blocks) = message["content_block"].as_array() {
                for (j, block) in blocks.iter().enumerate() {
                    if let Some(text) = block
                        .pointer("/content/text_block/text")
                        .and_then(Value::as_str)
                    {
                        texts.push(json!({"role":"user", "content":text}));
                        paths.push(format!(
                            "/messages/{i}/content_block/{j}/content/text_block/text"
                        ));
                    }
                }
            }
        }
    }
    let session = ["conversation_id", "local_conversation_id"]
        .iter()
        .filter_map(|key| native["client_meta"][key].as_str())
        .find(|id| {
            !id.is_empty()
                && *id != "0"
                && id.len() < 200
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
        })
        .map(|id| format!("doubaowork_{id}"));
    (
        json!({"messages":texts,"metadata":{"session_id":session}}),
        paths,
    )
}
fn apply_texts(native: &mut Value, envelope: &Value, paths: &[String]) {
    for (i, path) in paths.iter().enumerate() {
        if let (Some(target), Some(text)) = (
            native.pointer_mut(path),
            envelope["messages"][i]["content"].as_str(),
        ) {
            *target = Value::String(text.into());
        }
    }
}
fn audited_body(raw: &str) -> Result<Option<String>, ()> {
    let mut native: Value = serde_json::from_str(raw).map_err(|_| ())?;
    let (mut envelope, paths) = text_envelope(&native);
    if paths.is_empty() {
        return Ok(Some(raw.into()));
    }
    let before = envelope.clone();
    let blocked = crate::proxy::security::inspect_and_intercept_mut(
        &crate::app_config::AppType::DoubaoWork,
        &mut envelope,
    )
    .map_err(|_| ())?;
    if blocked.is_some() {
        return Ok(None);
    }
    if envelope == before {
        return Ok(Some(raw.into()));
    }
    apply_texts(&mut native, &envelope, &paths);
    serde_json::to_string(&native).map(Some).map_err(|_| ())
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn command(
    socket: &mut Socket,
    next: &mut u64,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> Result<u64, ()> {
    *next += 1;
    let mut value = json!({"id":*next,"method":method,"params":params});
    if let Some(session) = session {
        value["sessionId"] = session.into();
    }
    tokio::time::timeout(
        Duration::from_secs(2),
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    Ok(*next)
}

async fn connect(
    endpoint: &str,
    mut stop: watch::Receiver<bool>,
    active: &AtomicBool,
) -> Result<(), ()> {
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(2),
        tokio_tungstenite::connect_async(endpoint),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    let mut next = 0;
    let mut sessions = HashSet::<String>::new();
    let mut ready = HashSet::<String>::new();
    let mut pending = HashMap::<u64, (String, tokio::time::Instant)>::new();
    let setup = command(&mut socket, &mut next, None, "Target.setAutoAttach", json!({"autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true,"filter":[{"type":"page"},{"exclude":true}]})).await?;
    pending.insert(setup, (String::new(), tokio::time::Instant::now()));
    let mut watchdog = tokio::time::interval(Duration::from_secs(1));
    let result = loop {
        let item = tokio::select! {
            _ = stop.changed() => break Ok(()),
            _ = watchdog.tick() => {
                if pending.values().any(|(_, at)| at.elapsed() > Duration::from_secs(5)) { break Err(()); }
                continue;
            }
            value = socket.next() => value,
        };
        let Some(Ok(message)) = item else {
            break Err(());
        };
        if let Message::Ping(data) = message {
            if socket.send(Message::Pong(data)).await.is_err() {
                break Err(());
            }
            continue;
        }
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(id) = value["id"].as_u64() {
            if let Some((session, _)) = pending.remove(&id) {
                if value.get("error").is_some() {
                    break Err(());
                }
                if !session.is_empty() {
                    ready.insert(session);
                    active.store(true, Ordering::Release);
                }
            }
            continue;
        }
        let params = &value["params"];
        match value["method"].as_str() {
            Some("Target.attachedToTarget") => {
                let Some(session) = params["sessionId"].as_str() else {
                    continue;
                };
                sessions.insert(session.into());
                let id = command(
                    &mut socket,
                    &mut next,
                    Some(session),
                    "Fetch.enable",
                    json!({"patterns":[{"urlPattern":COMPLETION,"requestStage":"Request"}]}),
                )
                .await?;
                pending.insert(id, (session.into(), tokio::time::Instant::now()));
                command(
                    &mut socket,
                    &mut next,
                    Some(session),
                    "Runtime.runIfWaitingForDebugger",
                    json!({}),
                )
                .await?;
            }
            Some("Target.detachedFromTarget") => {
                if let Some(session) = params["sessionId"].as_str() {
                    sessions.remove(session);
                    ready.remove(session);
                    active.store(!ready.is_empty(), Ordering::Release);
                }
            }
            Some("Fetch.requestPaused") => {
                let Some(session) = value["sessionId"].as_str() else {
                    continue;
                };
                let Some(request) = params["requestId"].as_str() else {
                    continue;
                };
                let mut continuation = json!({"requestId":request});
                let mut method = "Fetch.continueRequest";
                // The exact endpoint and POST check protect unrelated traffic from rewriting.
                let url = params["request"]["url"]
                    .as_str()
                    .and_then(|s| url::Url::parse(s).ok());
                if params["request"]["method"] == "POST"
                    && url.is_some_and(|u| {
                        u.scheme() == "https"
                            && u.host_str() == Some("www.doubao.com")
                            && u.path() == "/chat/completion"
                    })
                {
                    if let Some(raw) = params["request"]["postData"]
                        .as_str()
                        .filter(|s| s.len() <= 2_000_000)
                    {
                        match audited_body(raw) {
                            Ok(Some(body)) if body != raw => {
                                continuation["postData"] =
                                    base64::engine::general_purpose::STANDARD
                                        .encode(body)
                                        .into();
                            }
                            Ok(None) => {
                                method = "Fetch.failRequest";
                                continuation["errorReason"] = "BlockedByClient".into();
                            }
                            _ => {}
                        }
                    }
                }
                let id =
                    command(&mut socket, &mut next, Some(session), method, continuation).await?;
                pending.insert(id, (String::new(), tokio::time::Instant::now()));
            }
            _ => {}
        }
    };
    active.store(false, Ordering::Release);
    // Confirm release before closing, so shutdown does not race in-flight protocol replies.
    let mut released = HashSet::new();
    for session in sessions {
        if let Ok(id) = command(
            &mut socket,
            &mut next,
            Some(&session),
            "Fetch.disable",
            json!({}),
        )
        .await
        {
            released.insert(id);
        }
    }
    if let Ok(id) = command(
        &mut socket,
        &mut next,
        None,
        "Target.setAutoAttach",
        json!({"autoAttach":false,"waitForDebuggerOnStart":false,"flatten":true}),
    )
    .await
    {
        released.insert(id);
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while !released.is_empty() {
            let Some(Ok(Message::Text(raw))) = socket.next().await else {
                break;
            };
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                if let Some(id) = value["id"].as_u64() {
                    released.remove(&id);
                }
            }
        }
    })
    .await;
    let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
    active.store(false, Ordering::Release);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::security::{types::AuditAction, SecurityAuditEngine, SecurityRuntimeConfig};
    use serde_json::json;

    #[test]
    fn native_text_follows_policy_without_changing_protocol_fields() {
        let native = json!({"client_meta":{"conversation_id":"123","bot_id":"keep"},
            "messages":[{"local_message_id":"m1","content_block":[{"block_type":10000,
                "content":{"text_block":{"text":"手机号 13800138000","icon_url":"keep"}},"block_id":"keep"},
                {"block_type":10003,"content":{"image_block":{"uri":"keep"}}}]}],
            "option":{"model_config":{"model_item_key":"keep"}},"ext":{"opaque":"keep"}});
        for action in [AuditAction::Redact, AuditAction::Audit, AuditAction::Allow] {
            let mut runtime = SecurityRuntimeConfig::default();
            runtime
                .builtin_action_overrides
                .insert("chinese_phone_number".into(), action);
            let (mut envelope, paths) = text_envelope(&native);
            assert_eq!(envelope["metadata"]["session_id"], "doubaowork_123");
            let inspected =
                SecurityAuditEngine::inspect_and_redact_with_runtime(&mut envelope, &runtime);
            assert!(!inspected.is_blocked());
            let mut changed = native.clone();
            apply_texts(&mut changed, &envelope, &paths);
            let pointer = "/messages/0/content_block/0/content/text_block/text";
            assert_eq!(
                changed
                    .pointer(pointer)
                    .and_then(serde_json::Value::as_str)
                    .unwrap()
                    .contains("13800138000"),
                action != AuditAction::Redact
            );
            changed
                .pointer_mut(pointer)
                .map(|v| *v = native.pointer(pointer).unwrap().clone());
            assert_eq!(changed, native);
        }
    }

    #[test]
    fn loopback_discovery_rejects_remote_or_unrelated_websockets() {
        assert!(browser_endpoint("12345\n/devtools/browser/id\n").is_some());
        for invalid in [
            "0\n/devtools/browser/id",
            "12345\nws://evil/",
            "12345\n/devtools/page/id",
            "abc\n/devtools/browser/id",
        ] {
            assert!(browser_endpoint(invalid).is_none());
        }
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    #[tokio::test]
    async fn attaches_new_pages_and_releases_pauses_on_disable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, rx) = watch::channel(false);
        let active = Arc::new(AtomicBool::new(false));
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut methods = Vec::new();
            while let Some(Ok(Message::Text(raw))) = socket.next().await {
                let value: Value = serde_json::from_str(&raw).unwrap();
                methods.push(value["method"].as_str().unwrap().to_string());
                socket
                    .send(Message::Text(
                        json!({"id":value["id"],"result":{}}).to_string().into(),
                    ))
                    .await
                    .unwrap();
                if methods.len() == 1 {
                    assert_eq!(value["params"]["waitForDebuggerOnStart"], true);
                    socket.send(Message::Text(json!({"method":"Target.attachedToTarget","params":{"sessionId":"page1","targetInfo":{"type":"page"}}}).to_string().into())).await.unwrap();
                }
                if value["method"] == "Runtime.runIfWaitingForDebugger" {
                    socket.send(Message::Text(json!({"method":"Fetch.requestPaused","sessionId":"page1","params":{"requestId":"r1","request":{"method":"POST","url":"https://www.doubao.com/chat/completion","postData":"{\"messages\":[]}"}}}).to_string().into())).await.unwrap();
                }
                if value["method"] == "Fetch.continueRequest" {
                    stop.send(true).unwrap();
                }
                if value["method"] == "Target.setAutoAttach"
                    && value["params"]["autoAttach"] == false
                {
                    break;
                }
            }
            methods
        });
        tokio::time::timeout(
            Duration::from_secs(5),
            connect(&format!("ws://{address}"), rx, &active),
        )
        .await
        .unwrap()
        .unwrap();
        let methods = server.await.unwrap();
        assert!(methods.contains(&"Fetch.disable".into()));
        assert!(methods.contains(&"Fetch.continueRequest".into()));
        assert!(!active.load(Ordering::Acquire));
    }

    #[test]
    fn rsa_and_phone_obey_each_configured_action() {
        use crate::proxy::security::{
            types::AuditAction, SecurityAuditEngine, SecurityRuntimeConfig,
        };
        for action in [AuditAction::Redact, AuditAction::Audit, AuditAction::Allow] {
            let mut policy = SecurityRuntimeConfig::default();
            for rule in ["chinese_phone_number", "rsa_ssh_private_key"] {
                policy.builtin_action_overrides.insert(rule.into(), action);
            }
            let mut body = json!({"messages":[{"content":"13800138000\n-----BEGIN RSA PRIVATE KEY-----\n1234556\n-----END RSA PRIVATE KEY-----"}]});
            let before = body.clone();
            let result = SecurityAuditEngine::inspect_and_redact_with_runtime(&mut body, &policy);
            assert!(!result.is_blocked());
            assert!(result
                .findings
                .iter()
                .any(|v| v.rule_id == "rsa_ssh_private_key"));
            assert!(result
                .findings
                .iter()
                .any(|v| v.rule_id == "chinese_phone_number"));
            if action == AuditAction::Redact {
                assert!(!body.to_string().contains("13800138000"));
                assert!(!body.to_string().contains("BEGIN RSA"));
            } else {
                assert_eq!(before, body);
            }
        }
    }
}

#[cfg(test)]
mod desktop_acceptance {
    use super::*;
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "Explicit live Doubao Work acceptance using an isolated database and an already-authorized local debug connection"]
    async fn live_official_messages() {
        use crate::proxy::security::{self, types::AuditAction, SecurityRuntimeConfig};
        let directory = std::path::PathBuf::from(
            std::env::var("VARYAI_DOUBAO_LIVE_DIR").expect("explicit acceptance directory"),
        );
        assert!(directory.is_dir());
        let db = Arc::new(crate::database::Database::memory().unwrap());
        security::init_audit_logger(db.clone());
        let manager = Manager::default();
        manager.enable().await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1200);
        while tokio::time::Instant::now() < deadline && !directory.join("stop").exists() {
            let policy = std::fs::read_to_string(directory.join("policy")).unwrap_or_default();
            let action = match policy.trim() {
                "audit" => AuditAction::Audit,
                "allow" => AuditAction::Allow,
                _ => AuditAction::Redact,
            };
            let mut config = SecurityRuntimeConfig::default();
            for rule in ["chinese_phone_number", "rsa_ssh_private_key"] {
                config.builtin_action_overrides.insert(rule.into(), action);
            }
            security::runtime::set_security_runtime_for_test(config);
            let events = db
                .get_security_audit_logs(
                    &crate::database::AuditLogFilter {
                        app_type: Some("doubaowork".into()),
                        ..Default::default()
                    },
                    1,
                    100,
                )
                .unwrap();
            let proof = json!({"connected":manager.active(),"policy":policy.trim(),"total":events.total,"events":events.items.iter().map(|event|json!({"rule":event.rule_id,"action":event.action_taken,"session":event.session_id})).collect::<Vec<_>>()});
            crate::config::atomic_write_private(
                &directory.join("proof.json"),
                proof.to_string().as_bytes(),
            )
            .unwrap();
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        manager.disable().await;
        assert!(!manager.active());
        security::runtime::set_security_runtime_for_test(Default::default());
    }
}
