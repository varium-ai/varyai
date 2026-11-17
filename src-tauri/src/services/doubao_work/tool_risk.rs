//! Native background lifecycle observation, separate from request DLP interception.
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::watch;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
const BINDING_PREFIX: &str = "__varyaiDoubaoToolRiskV1_";
const SCRIPT: &str = include_str!("../../../resources/doubao-tool-risk.js");

async fn rpc(
    socket: &mut Socket,
    next: &mut u64,
    session: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value, ()> {
    tokio::time::timeout(
        Duration::from_secs(2),
        super::super::doubao_checkin::rpc(socket, next, session, method, params),
    )
    .await
    .map_err(|_| ())?
}

pub(super) async fn run(mut stop: watch::Receiver<bool>) {
    let mut failed = false;
    loop {
        if *stop.borrow() {
            return;
        }
        if crate::services::tool_risk::runtime::collection_enabled() {
            if let Some(endpoint) = super::discover().await {
                let result = connect(&endpoint, stop.clone()).await;
                if result.is_err() && !failed {
                    log::warn!("[ToolRisk] client=doubaowork native_hook=unavailable; native execution unchanged");
                }
                failed = result.is_err();
            }
        }
        tokio::select! {
            _ = stop.changed() => return,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {},
        }
    }
}

async fn connect(endpoint: &str, stop: watch::Receiver<bool>) -> Result<(), ()> {
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(2),
        tokio_tungstenite::connect_async(endpoint),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    let mut next = 0;
    let targets = rpc(&mut socket, &mut next, None, "Target.getTargets", json!({})).await?;
    let candidates: Vec<_> = targets["targetInfos"]
        .as_array()
        .ok_or(())?
        .iter()
        .filter(|target| {
            target["type"] == "page" && target["url"] == "doubaowork://doubaowork-background/"
        })
        .collect();
    if candidates.len() != 1 {
        return Err(());
    }
    let target = candidates[0]["targetId"].as_str().ok_or(())?;
    let attached = rpc(
        &mut socket,
        &mut next,
        None,
        "Target.attachToTarget",
        json!({"targetId":target,"flatten":true}),
    )
    .await?;
    let session = attached["sessionId"].as_str().ok_or(())?;
    let binding = format!("{BINDING_PREFIX}{}", uuid::Uuid::new_v4().simple());
    let result = observe(&mut socket, &mut next, session, &binding, stop).await;
    let _ = rpc(
        &mut socket,
        &mut next,
        Some(session),
        "Runtime.evaluate",
        json!({"expression":format!("(()=>{{const hook=globalThis[Symbol.for('varyai.doubao.toolRisk')];if(hook?.binding==={})hook.dispose();}})()", json!(binding))}),
    )
    .await;
    let _ = rpc(
        &mut socket,
        &mut next,
        Some(session),
        "Runtime.releaseObjectGroup",
        json!({"objectGroup":binding}),
    )
    .await;
    let _ = rpc(
        &mut socket,
        &mut next,
        Some(session),
        "Runtime.removeBinding",
        json!({"name":binding}),
    )
    .await;
    let _ = rpc(
        &mut socket,
        &mut next,
        None,
        "Target.detachFromTarget",
        json!({"sessionId":session}),
    )
    .await;
    let _ = socket.close(None).await;
    result
}

async fn observe(
    socket: &mut Socket,
    next: &mut u64,
    session: &str,
    binding: &str,
    mut stop: watch::Receiver<bool>,
) -> Result<(), ()> {
    rpc(socket, next, Some(session), "Runtime.enable", json!({})).await?;
    rpc(
        socket,
        next,
        Some(session),
        "Runtime.addBinding",
        json!({"name":binding}),
    )
    .await?;
    let result = rpc(socket, next, Some(session), "Runtime.evaluate", json!({"expression":format!("(()=>{{{SCRIPT};return varyaiDoubaoToolLifecycle();}})()"),"objectGroup":binding})).await?;
    let object = result["result"]["objectId"].as_str().ok_or(())?;
    let properties = rpc(
        socket,
        next,
        Some(session),
        "Runtime.getProperties",
        json!({"objectId":object,"ownProperties":true}),
    )
    .await?;
    let properties = properties["result"].as_array().ok_or(())?;
    let entry = properties
        .iter()
        .find(|property| property["name"] == "entry")
        .and_then(|property| property["value"]["objectId"].as_str())
        .ok_or(())?;
    let slot = properties
        .iter()
        .find(|property| property["name"] == "slot")
        .and_then(|property| property["value"]["value"].as_str())
        .ok_or(())?;
    let properties = rpc(
        socket,
        next,
        Some(session),
        "Runtime.getProperties",
        json!({"objectId":entry,"ownProperties":true}),
    )
    .await?;
    let scopes = properties["internalProperties"]
        .as_array()
        .ok_or(())?
        .iter()
        .find(|property| property["name"] == "[[Scopes]]")
        .and_then(|property| property["value"]["objectId"].as_str())
        .ok_or(())?;
    let scopes = rpc(
        socket,
        next,
        Some(session),
        "Runtime.getProperties",
        json!({"objectId":scopes,"ownProperties":true}),
    )
    .await?;
    let mut installed = false;
    for scope in scopes["result"].as_array().ok_or(())?.iter().take(16) {
        let Some(object) = scope["value"]["objectId"].as_str() else {
            continue;
        };
        let variables = rpc(
            socket,
            next,
            Some(session),
            "Runtime.getProperties",
            json!({"objectId":object,"ownProperties":true}),
        )
        .await?;
        let lifecycle = variables["result"]
            .as_array()
            .ok_or(())?
            .iter()
            .find(|variable| variable["name"] == slot)
            .and_then(|variable| variable["value"]["objectId"].as_str());
        let Some(lifecycle) = lifecycle else {
            continue;
        };
        let result = rpc(socket, next, Some(session), "Runtime.callFunctionOn", json!({"objectId":lifecycle,"functionDeclaration":format!("function(binding){{{SCRIPT};return varyaiInstallDoubaoToolObserver.call(this,binding);}}"),"arguments":[{"value":binding}],"returnByValue":true})).await?;
        installed = result["result"]["value"] == true;
        break;
    }
    if !installed {
        return Err(());
    }
    rpc(
        socket,
        next,
        Some(session),
        "Runtime.releaseObjectGroup",
        json!({"objectGroup":binding}),
    )
    .await?;
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    loop {
        if *stop.borrow() || !crate::services::tool_risk::runtime::collection_enabled() {
            return Ok(());
        }
        let item = tokio::select! {
            _ = stop.changed() => return Ok(()),
            _ = ticker.tick() => continue,
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
        if raw.len() > 1024 * 1024 {
            return Err(());
        }
        let value: Value = serde_json::from_str(&raw).map_err(|_| ())?;
        if value["method"] == "Runtime.executionContextsCleared"
            || value["method"] == "Target.detachedFromTarget"
                && value["params"]["sessionId"] == session
        {
            return Err(());
        }
        if value["method"] != "Runtime.bindingCalled"
            || value["sessionId"] != session
            || value["params"]["name"] != binding
        {
            continue;
        }
        let Some(payload) = value["params"]["payload"]
            .as_str()
            .filter(|payload| payload.len() <= 300 * 1024)
        else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(payload) {
            if let Some(observation) =
                crate::proxy::tool_observation::native::doubao_observation(&value)
            {
                crate::proxy::tool_observation::channel::observe_native(observation);
            }
        }
    }
}
