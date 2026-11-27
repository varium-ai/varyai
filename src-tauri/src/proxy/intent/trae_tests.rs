use super::*;
use crate::{database::Database, proxy::server::ProxyServer, proxy::ProxyConfig};
use axum::{response::IntoResponse, routing::post, Router};

fn catalog(address: std::net::SocketAddr) -> Result<Menu, Box<dyn std::error::Error>> {
    let mut menu = super::global_tests::menu(&format!("http://{address}/v1/chat/completions"));
    let session = Arc::make_mut(menu.groups.values_mut().next().ok_or("missing fixture")?);
    let mut provider = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic TRAE upstream".into(),
        json!({"env":{"ANTHROPIC_BASE_URL":format!("http://{address}"),"ANTHROPIC_API_KEY":"synthetic-upstream"}}),
        None,
    );
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_chat".into()),
        ..Default::default()
    });
    session.targets[0].upstream = Some(provider);
    let mut second = session.targets[0].clone();
    second.model = "other-exact-model".into();
    session.targets.push(second);
    session
        .group
        .members
        .push(crate::services::intent_routing::Member {
            id: "b".into(),
            provider_id: "fixture".into(),
            model_id: "other-exact-model".into(),
            ..session.group.members[0].clone()
        });
    session.group.fallback_member_ids = vec!["b".into()];
    Ok(menu)
}

fn receipt(app: &str) -> crate::services::agent_connections::Receipt {
    crate::services::agent_connections::Receipt {
        app: app.into(),
        token: format!("synthetic-{app}"),
        model: super::global_tests::group().alias(),
        model_map: Default::default(),
        visible_models: None,
        files: vec![],
        suspended: false,
        applied_at_ms: None,
    }
}

#[tokio::test]
#[serial_test::serial]
async fn trae_stream_opens_before_slow_upstream_headers_and_keeps_wire_chunks(
) -> Result<(), Box<dyn std::error::Error>> {
    struct RestoreSecurity(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for RestoreSecurity {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _security = RestoreSecurity((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let release = Arc::new(tokio::sync::Notify::new());
    let gate = release.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/v1/chat/completions", post(move || {
            let gate = gate.clone();
            async move {
                gate.notified().await;
                ([(header::CONTENT_TYPE, "text/event-stream")], "data: {\"id\":\"synthetic-slow\",\"choices\":[{\"delta\":{\"content\":\"synthetic answer\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
            }
        }))).await
    });
    let runtime = Arc::new(Runtime::default());
    runtime.set_global(catalog(address)?, 0)?;
    let connection = receipt("trae");
    runtime.connections.install(&connection)?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(runtime.clone());
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let request = client.post(format!("http://127.0.0.1:{}/connections/trae/v1/chat/completions", info.port))
        .bearer_auth("synthetic-trae")
        .json(&json!({"model":super::global_tests::group().alias(),"stream":true,"metadata":{"session_id":"synthetic-chat"},"messages":[{"role":"user","content":"synthetic slow fixture"}]}))
        .send();
    let response = tokio::time::timeout(Duration::from_secs(8), request).await;
    if response.is_err() {
        release.notify_one();
        server.stop().await?;
        task.abort();
        return Err("TRAE stream did not open while upstream headers were pending".into());
    }
    let mut response = response??;
    assert_eq!(response.status(), StatusCode::OK);
    let heartbeat = tokio::time::timeout(Duration::from_secs(2), response.chunk())
        .await??
        .ok_or("missing heartbeat")?;
    assert_eq!(heartbeat.as_ref(), b": varyai-waiting\n\n");
    let live = runtime
        .traces
        .live_requests(runtime.traces.snapshot(), |agent| {
            assert_eq!(agent, "trae");
            vec![("synthetic-chat".into(), "Synthetic title".into())]
        });
    assert_eq!(
        serde_json::to_value(live)?[0]["sessionTitle"],
        "Synthetic title"
    );
    release.notify_one();
    let remaining = response.bytes().await?;
    assert_eq!(remaining.as_ref(), b"data: {\"id\":\"synthetic-slow\",\"choices\":[{\"delta\":{\"content\":\"synthetic answer\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n");
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
async fn trae_waiting_stream_preserves_fast_errors_sanitizes_late_errors_and_cancels(
) -> Result<(), Box<dyn std::error::Error>> {
    let immediate = trae::waiting_response(async {
        error(StatusCode::CONFLICT, "routing_unknown_continuation")
    })
    .await;
    assert_eq!(immediate.status(), StatusCode::CONFLICT);
    let (release, blocked) = tokio::sync::oneshot::channel();
    let response = trae::waiting_response(async move {
        let _ = blocked.await;
        (StatusCode::BAD_GATEWAY, "synthetic private upstream detail").into_response()
    })
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    assert_eq!(
        stream.next().await.ok_or("missing heartbeat")??.as_ref(),
        b": varyai-waiting\n\n"
    );
    release
        .send(())
        .map_err(|_| "waiting response was dropped")?;
    let late = stream.next().await.ok_or("missing terminal error")??;
    let late = std::str::from_utf8(&late)?;
    assert!(late.contains("routing_request_failed"));
    assert!(!late.contains("synthetic private upstream detail"));
    assert!(stream.next().await.is_none());

    struct Dropped(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = Dropped(dropped.clone());
    let response = trae::waiting_response(async move {
        let _guard = guard;
        std::future::pending::<Response>().await
    })
    .await;
    assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));
    drop(response);
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
#[ignore = "Manual native TRAE slow-header acceptance with an isolated synthetic proxy"]
async fn trae_native_slow_header_acceptance() -> Result<(), Box<dyn std::error::Error>> {
    let control = std::env::var("VARYAI_TRAE_SLOW_CONTROL")?;
    let control = std::path::Path::new(&control);
    // This manual harness forwards synthetic fixtures only, with no production policy.
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/v1/chat/completions", post(|| async {
            tokio::time::sleep(Duration::from_secs(100)).await;
            ([(header::CONTENT_TYPE, "text/event-stream")], "data: {\"id\":\"synthetic-slow\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"Synthetic slow response completed.\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
        }))).await
    });
    let runtime = Arc::new(Runtime::default());
    runtime.set_global(catalog(address)?, 0)?;
    runtime.connections.install(&receipt("trae"))?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(runtime);
    let info = server.start().await?;
    std::fs::write(control.join("ready.json"), json!({"port":info.port,"model":super::global_tests::group().alias(),"token":receipt("trae").token}).to_string())?;
    for _ in 0..180 {
        if control.join("stop").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    server.stop().await?;
    task.abort();
    Ok(())
}

#[test]
fn trae_rewritten_ids_require_complete_observed_payloads_and_keep_known_owners(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut observer = Observer::default();
    observer.trae = true;
    let mut bindings = Bindings::default();
    let frames = [
        json!({"choices":[{"index":0,"delta":{"tool_calls":[
            {"index":0,"id":"original-a","function":{"name":"read_","arguments":"{\"b\":"}},
            {"index":1,"id":"original-b","function":{"name":"list_fixture","arguments":"{"}}
        ]}}]}),
        json!({"choices":[{"index":0,"delta":{"tool_calls":[
            {"index":0,"function":{"name":"fixture","arguments":"2,\"a\":1}"}},
            {"index":1,"function":{"arguments":"\"path\":\"synthetic\"}"}}
        ]}}]}),
        json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    ];
    let body = |name, arguments| {
        json!({"input":[
            {"type":"function_call","call_id":"rewritten","name":name,"arguments":arguments},
            {"type":"function_call_output","call_id":"rewritten","output":"synthetic"}
        ]})
    };
    let request = body("read_fixture", r#"{ "a":1,"b":2 }"#);
    for frame in &frames[..2] {
        observer.observe_at(frame, &mut bindings, 0, Instant::now());
    }
    assert!(bindings
        .resolve(
            &bindings.continuation_projection("agent:trae", &request),
            None
        )
        .is_err());
    observer.observe_at(&frames[2], &mut bindings, 0, Instant::now());
    assert_eq!(
        bindings.resolve(
            &bindings.continuation_projection("agent:trae", &request),
            None
        ),
        Ok(Some(0))
    );
    for request in [
        body("read_fixture", r#"{"a":1,"b":3}"#),
        body("different_fixture", r#"{"a":1,"b":2}"#),
        json!({"input":[{"type":"function_call_output","call_id":"rewritten","output":"synthetic"}]}),
    ] {
        assert!(bindings
            .resolve(
                &bindings.continuation_projection("agent:trae", &request),
                None
            )
            .is_err());
    }
    assert!(bindings
        .resolve(
            &bindings.continuation_projection("agent:claude", &request),
            None
        )
        .is_err());
    remember(&mut bindings.calls, "call", "rewritten", 1);
    assert_eq!(
        bindings.resolve(
            &bindings.continuation_projection("agent:trae", &request),
            None
        ),
        Ok(Some(1)),
        "a payload fingerprint must never replace a known ID's owner"
    );
    Ok(())
}

#[test]
fn both_trae_editions_normalize_rewritten_calls_with_complete_observed_payloads() {
    for consumer in ["agent:trae", "agent:trae-intl"] {
        let mut bindings = Bindings::default();
        let mut observer = Observer::default();
        observer.trae = true;
        observer.observe_at(&json!({"choices":[{"message":{"tool_calls":[{"id":"synthetic-original","function":{"name":"read_fixture","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}), &mut bindings, 0, Instant::now());
        let body = json!({"input":[{"type":"function_call","call_id":"synthetic-rewritten","name":"read_fixture","arguments":"{}"},{"type":"function_call_output","call_id":"synthetic-rewritten","output":"synthetic result"}]});
        let original = body.clone();
        assert_eq!(
            bindings.resolve(&bindings.continuation_projection(consumer, &body), None),
            Ok(Some(0)),
            "{consumer}"
        );
        assert_eq!(body, original);
        let mut unknown = body.clone();
        unknown["input"][0]["arguments"] = json!("{\"changed\":true}");
        assert!(bindings
            .resolve(&bindings.continuation_projection(consumer, &unknown), None)
            .is_err());
        assert!(bindings
            .resolve(
                &bindings.continuation_projection("agent:claude", &body),
                None
            )
            .is_err());
        remember(&mut bindings.calls, "call", "synthetic-rewritten", 1);
        assert_eq!(
            bindings.resolve(&bindings.continuation_projection(consumer, &body), None),
            Ok(Some(1))
        );
    }
}

#[test]
fn trae_rewritten_payload_collisions_remain_isolated() -> Result<(), Box<dyn std::error::Error>> {
    let conversations = scopes::Conversations::default();
    let identity = Arc::default();
    for (name, target) in [("first", 0), ("second", 1)] {
        let state = conversations.acquire("agent:trae", &identity, Some(name), &json!({}))?;
        let mut observer = Observer::default();
        observer.trae = true;
        let mut bindings = state
            .bindings
            .lock()
            .map_err(|_| "synthetic state poisoned")?;
        observer.observe_at(
            &json!({"choices":[{"message":{"tool_calls":[{"id":format!("original-{name}"),"function":{"name":"read_fixture","arguments":"{}"}}]}}]}),
            &mut bindings,
            target,
            Instant::now(),
        );
    }
    let body = json!({"input":[{"type":"function_call","call_id":"rewritten","name":"read_fixture","arguments":"{}"},{"type":"function_call_output","call_id":"rewritten","output":"synthetic"}]});
    assert!(conversations
        .acquire("agent:trae", &identity, None, &body)
        .is_err());
    assert!(conversations
        .acquire("agent:claude", &identity, None, &body)
        .is_err());
    assert!(conversations
        .acquire("agent:trae", &Arc::default(), None, &body)
        .is_err());
    Ok(())
}

#[test]
fn trae_tool_scopes_reject_unknown_mixed_colliding_and_changed_owners(
) -> Result<(), Box<dyn std::error::Error>> {
    let conversations = scopes::Conversations::default();
    let identity = Arc::default();
    let observe = |name: &str, id: &str, target: usize| -> Result<(), Box<dyn std::error::Error>> {
        let state = conversations.acquire("agent:trae", &identity, Some(name), &json!({}))?;
        let mut bindings = state
            .bindings
            .lock()
            .map_err(|_| "synthetic state poisoned")?;
        bindings.observe(
            &json!({"choices":[{"message":{"tool_calls":[{"id":id}]}}]}),
            target,
        );
        Ok(())
    };
    observe("first", "synthetic-first", 0)?;
    observe("second", "synthetic-second", 1)?;
    observe("first", "synthetic-collision", 0)?;
    observe("second", "synthetic-collision", 1)?;
    let body = |ids: &[&str]| json!({"input":ids.iter().map(|id| json!({"type":"function_call_output","call_id":id,"output":"synthetic"})).collect::<Vec<_>>()});
    for ids in [
        vec!["synthetic-unknown"],
        vec!["synthetic-first", "synthetic-second"],
        vec!["synthetic-collision"],
    ] {
        assert!(conversations
            .acquire("agent:trae", &identity, None, &body(&ids))
            .is_err());
    }
    let first = body(&["synthetic-first"]);
    let state = conversations.acquire("agent:trae", &identity, None, &first)?;
    assert_eq!(
        state
            .bindings
            .lock()
            .map_err(|_| "synthetic state poisoned")?
            .resolve(&first, None),
        Ok(Some(0))
    );
    assert!(conversations
        .acquire("agent:claude", &identity, None, &first)
        .is_err());
    assert!(conversations
        .acquire("agent:trae", &Arc::default(), None, &first)
        .is_err());
    Ok(())
}
