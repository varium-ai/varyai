//! Read-only CDP Network observation; no Fetch interception or native permission changes.
use super::tool_reports::Reports;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tokio_tungstenite::{
    tungstenite::{protocol::WebSocketConfig, Message},
    MaybeTlsStream, WebSocketStream,
};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type Key = (String, String);
const MAX_CHUNK: usize = 1024 * 1024;
struct Stream {
    parser: Reports,
    pending: Option<Vec<u8>>,
    finished: bool,
    started: Instant,
    bytes: usize,
}

fn completion(response: &Value) -> bool {
    response["status"] == 200
        && response["mimeType"] == "text/event-stream"
        && response["url"]
            .as_str()
            .and_then(|url| url::Url::parse(url).ok())
            .is_some_and(|url| {
                url.scheme() == "https"
                    && url.host_str() == Some("www.doubao.com")
                    && (url.path() == "/chat/completion"
                        || url.path().starts_with("/chat/completion/"))
            })
}
fn page(target: &Value) -> bool {
    target["type"] == "page"
        && target["url"].as_str().is_some_and(|url| {
            url == "doubaowork://doubaowork-background/"
                || url.starts_with("doubaowork://doubaowork-chat/chat/")
        })
}
async fn send(
    socket: &mut Socket,
    next: &mut u64,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> Result<u64, ()> {
    *next += 1;
    let mut value = json!({"id":next,"method":method,"params":params});
    if let Some(session) = session {
        value["sessionId"] = json!(session);
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

pub(super) async fn run(mut stop: watch::Receiver<bool>, legacy: Arc<AtomicBool>) {
    loop {
        if *stop.borrow() {
            return;
        }
        if crate::services::tool_risk::runtime::collection_enabled() {
            if let Some(endpoint) = super::discover().await {
                if connect(&endpoint, stop.clone(), &legacy).await.is_err() {
                    log::debug!("[ToolRisk] client=doubaowork report_observer=unavailable; native execution unchanged");
                }
            }
        }
        tokio::select! { _ = stop.changed() => return, _ = tokio::time::sleep(Duration::from_secs(1)) => {} }
    }
}

async fn connect(
    endpoint: &str,
    mut stop: watch::Receiver<bool>,
    legacy: &AtomicBool,
) -> Result<(), ()> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(2 * MAX_CHUNK))
        .max_frame_size(Some(2 * MAX_CHUNK));
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(2),
        tokio_tungstenite::connect_async_with_config(endpoint, Some(config), false),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    let mut next = 0;
    send(&mut socket, &mut next, None, "Target.setAutoAttach", json!({"autoAttach":true,"waitForDebuggerOnStart":false,"flatten":true,"filter":[{"type":"page"},{"exclude":true}]})).await?;
    let mut sessions = HashSet::new();
    let mut streams: HashMap<Key, Stream> = HashMap::new();
    let mut pending: HashMap<u64, Key> = HashMap::new();
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    loop {
        if *stop.borrow() || !crate::services::tool_risk::runtime::collection_enabled() {
            break;
        }
        let item = tokio::select! {
            _ = stop.changed() => break,
            _ = ticker.tick() => {
                streams.retain(|_, s| s.started.elapsed() < Duration::from_secs(if s.pending.is_some() { 5 } else { 600 }));
                pending.retain(|_, key| streams.contains_key(key));
                continue;
            }
            item = socket.next() => item,
        };
        let Some(Ok(message)) = item else {
            return Err(());
        };
        let raw = match message {
            Message::Text(raw) => raw,
            Message::Ping(data) => {
                socket.send(Message::Pong(data)).await.map_err(|_| ())?;
                continue;
            }
            Message::Pong(_) => continue,
            _ => return Err(()),
        };
        let event: Value = serde_json::from_str(&raw).map_err(|_| ())?;
        if let Some(key) = event["id"].as_u64().and_then(|id| pending.remove(&id)) {
            let Some(stream) = streams.get_mut(&key) else {
                continue;
            };
            if event.get("error").is_some() {
                streams.remove(&key);
                continue;
            }
            let Some(bytes) = decode(event["result"]["bufferedData"].as_str().unwrap_or("")) else {
                streams.remove(&key);
                continue;
            };
            ingest(stream, &bytes, legacy);
            if let Some(bytes) = stream.pending.take() {
                ingest(stream, &bytes, legacy);
            }
            if stream.finished {
                streams.remove(&key);
            }
            continue;
        }
        let params = &event["params"];
        match event["method"].as_str() {
            Some("Target.attachedToTarget") => {
                let Some(session) = params["sessionId"].as_str() else {
                    continue;
                };
                if page(&params["targetInfo"]) && sessions.len() < 8 {
                    sessions.insert(session.to_owned());
                    send(&mut socket, &mut next, Some(session), "Network.enable", json!({"maxTotalBufferSize":MAX_CHUNK,"maxResourceBufferSize":MAX_CHUNK,"maxPostDataSize":0})).await?;
                } else {
                    send(
                        &mut socket,
                        &mut next,
                        None,
                        "Target.detachFromTarget",
                        json!({"sessionId":session}),
                    )
                    .await?;
                }
            }
            Some("Target.detachedFromTarget") => {
                if let Some(session) = params["sessionId"].as_str() {
                    sessions.remove(session);
                    streams.retain(|key, _| key.0 != session);
                }
            }
            Some(method) => {
                let (Some(session), Some(request)) =
                    (event["sessionId"].as_str(), params["requestId"].as_str())
                else {
                    continue;
                };
                if !sessions.contains(session) {
                    continue;
                }
                let key = (session.to_owned(), request.to_owned());
                match method {
                    "Network.responseReceived"
                        if completion(&params["response"]) && streams.len() < 8 =>
                    {
                        streams.insert(
                            key.clone(),
                            Stream {
                                parser: Reports::default(),
                                pending: Some(Vec::new()),
                                finished: false,
                                started: Instant::now(),
                                bytes: 0,
                            },
                        );
                        let id = send(
                            &mut socket,
                            &mut next,
                            Some(session),
                            "Network.streamResourceContent",
                            json!({"requestId":request}),
                        )
                        .await?;
                        pending.insert(id, key);
                    }
                    "Network.dataReceived" => {
                        let Some(data) = params["data"].as_str() else {
                            continue;
                        };
                        if let Some(stream) = streams.get_mut(&key) {
                            if let Some(bytes) = decode(data) {
                                if let Some(pending) = &mut stream.pending {
                                    if pending.len() + bytes.len() <= MAX_CHUNK {
                                        pending.extend(bytes);
                                    } else {
                                        streams.remove(&key);
                                    }
                                } else {
                                    ingest(stream, &bytes, legacy);
                                }
                            } else {
                                streams.remove(&key);
                            }
                        }
                    }
                    "Network.loadingFinished" => {
                        if let Some(stream) = streams.get_mut(&key) {
                            if stream.pending.is_some() {
                                stream.finished = true;
                            } else {
                                streams.remove(&key);
                            }
                        }
                    }
                    "Network.loadingFailed" => {
                        streams.remove(&key);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    // Closing this observer's connection removes its Network subscriptions only.
    let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
    Ok(())
}

fn decode(value: &str) -> Option<Vec<u8>> {
    if value.len() > MAX_CHUNK {
        return None;
    }
    base64::engine::general_purpose::STANDARD.decode(value).ok()
}
fn ingest(stream: &mut Stream, bytes: &[u8], legacy: &AtomicBool) {
    stream.bytes = stream.bytes.saturating_add(bytes.len());
    if stream.bytes > 16 * MAX_CHUNK {
        return;
    }
    for observation in stream.parser.feed(bytes) {
        // A working legacy pre-execution hook takes precedence over later reports.
        if !legacy.load(Ordering::Acquire)
            && crate::services::tool_risk::runtime::collection_enabled()
        {
            crate::proxy::tool_observation::channel::observe_native(observation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_native_chat_pages_and_completion_streams_are_observed() {
        assert!(page(
            &json!({"type":"page","url":"doubaowork://doubaowork-chat/chat/synthetic"})
        ));
        assert!(!page(
            &json!({"type":"page","url":"https://www.doubao.com/"})
        ));
        for url in [
            "http://www.doubao.com/chat/completion",
            "https://other.invalid/chat/completion",
            "https://www.doubao.com/chat/completion-unknown",
        ] {
            assert!(!completion(
                &json!({"url":url,"status":200,"mimeType":"text/event-stream"})
            ));
        }
        assert!(completion(
            &json!({"url":"https://www.doubao.com/chat/completion","status":200,"mimeType":"text/event-stream"})
        ));
        assert!(decode("!").is_none());
        assert!(decode(&"a".repeat(MAX_CHUNK + 1)).is_none());
    }
}
