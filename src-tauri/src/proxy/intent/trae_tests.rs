use super::*;
use crate::{database::Database, proxy::server::ProxyServer, proxy::ProxyConfig};
use axum::{response::IntoResponse, routing::post, Json, Router};

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
    .with_intent_runtime(runtime);
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let request = client.post(format!("http://127.0.0.1:{}/connections/trae/v1/chat/completions", info.port))
        .bearer_auth("synthetic-trae")
        .json(&json!({"model":super::global_tests::group().alias(),"stream":true,"messages":[{"role":"user","content":"synthetic slow fixture"}]}))
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
fn trae_rewritten_payload_collisions_remain_isolated() -> Result<(), Box<dyn std::error::Error>> {
    let conversations = scopes::Conversations::default();
    let identity = Arc::default();
    for (name, target) in [("first", 0), ("second", 1)] {
        let state = conversations.acquire("agent:trae", &identity, Some(name), &json!({}))?;
        let mut observer = Observer::default();
        observer.trae = true;
        let mut bindings = state.bindings.try_lock()?;
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
        let mut bindings = state.bindings.try_lock()?;
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
        state.bindings.try_lock()?.resolve(&first, None),
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

#[tokio::test]
#[serial_test::serial]
async fn trae_tools_without_session_resume_exact_observed_owner_after_restart(
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
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/v1/chat/completions", post(move |Json(body): Json<Value>| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(body.clone()).await;
                let followup = body["messages"].as_array().is_some_and(|messages| messages.iter().any(|m| m["role"] == "tool"));
                let call = if body["stream"] == true { "synthetic-stream-call" } else { "synthetic-json-call" };
                let arguments = if body["stream"] == true { r#"{"transport":"stream"}"# } else { r#"{"transport":"json"}"# };
                if body["stream"] == true {
                    let delta = if followup { json!({"role":"assistant","content":"synthetic completion"}) } else { json!({"role":"assistant","tool_calls":[{"index":0,"id":call,"type":"function","function":{"name":"read_fixture","arguments":arguments}}]}) };
                    let frames = format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                        json!({"id":"synthetic-stream","object":"chat.completion.chunk","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                        json!({"id":"synthetic-stream","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":if followup {"stop"} else {"tool_calls"}}]}));
                    let stream = futures::stream::iter(frames.into_bytes().chunks(7).map(|part| Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(part))).collect::<Vec<_>>());
                    return ([(header::CONTENT_TYPE,"text/event-stream")], Body::from_stream(stream)).into_response();
                }
                let message = if followup { json!({"role":"assistant","content":"synthetic completion"}) } else { json!({"role":"assistant","content":null,"tool_calls":[{"id":call,"type":"function","function":{"name":"read_fixture","arguments":arguments}}]}) };
                Json(json!({"id":"synthetic-json","object":"chat.completion","choices":[{"index":0,"message":message,"finish_reason":if followup {"stop"} else {"tool_calls"}}],"usage":{"prompt_tokens":1,"completion_tokens":1}})).into_response()
            }
        }))).await
    });
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("continuations.json");
    let db = Arc::new(Database::memory()?);
    let runtime = Arc::new(Runtime::default());
    runtime.prepare_continuations(path.clone()).await;
    runtime.set_global(catalog(address)?, 0)?;
    runtime.connections.install(&receipt("trae"))?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    )
    .with_intent_runtime(runtime.clone());
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let alias = super::global_tests::group().alias();
    let payload = |stream| json!({"model":alias,"stream":stream,"messages":[{"role":"user","content":"synthetic request"}],"tools":[{"type":"function","function":{"name":"read_fixture","parameters":{"type":"object","properties":{}}}}],"temperature":0.25,"vendor_extension":{"retain":true}});
    for stream in [false, true] {
        let response = client
            .post(format!(
                "http://127.0.0.1:{}/connections/trae/v1/chat/completions",
                info.port
            ))
            .bearer_auth("synthetic-trae")
            .header("sec-fetch-site", "cross-site")
            .json(&payload(stream))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let _ = response.bytes().await?;
        let _ = rx.recv().await.ok_or("missing first request")?;
    }
    runtime.flush_continuations().await;
    server.stop().await?;
    drop(server);
    drop(runtime);

    let runtime = Arc::new(Runtime::default());
    runtime.prepare_continuations(path).await;
    let mut published = catalog(address)?;
    let session = Arc::make_mut(
        published
            .groups
            .values_mut()
            .next()
            .ok_or("missing fixture")?,
    );
    session.targets.swap(0, 1);
    session.group.default_member_id = "b".into();
    session.group.fallback_member_ids = vec!["a".into()];
    runtime.set_global(published, 0)?;
    for app in ["trae", "claude"] {
        runtime.connections.install(&receipt(app))?;
    }
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db,
        None,
    )
    .with_intent_runtime(runtime.clone());
    let info = server.start().await?;
    let root = format!("http://127.0.0.1:{}/connections", info.port);
    for stream in [false, true] {
        // TRAE replaces the upstream ID before replaying the assistant/tool pair.
        let call = if stream {
            "synthetic-trae-stream-rewritten"
        } else {
            "synthetic-trae-json-rewritten"
        };
        let arguments = if stream {
            r#"{"transport":"stream"}"#
        } else {
            r#"{"transport":"json"}"#
        };
        let mut body = payload(stream);
        body["messages"].as_array_mut().ok_or("missing messages")?.extend([
            json!({"role":"assistant","content":null,"reasoning_content":"synthetic reasoning","tool_calls":[{"id":call,"type":"function","function":{"name":"read_fixture","arguments":arguments}}]}),
            json!({"role":"tool","tool_call_id":call,"content":"synthetic result"}),
        ]);
        for later_turn in [false, true] {
            if later_turn {
                body["messages"]
                    .as_array_mut()
                    .ok_or("missing messages")?
                    .extend([
                        json!({"role":"assistant","content":"synthetic completion"}),
                        json!({"role":"user","content":"synthetic next turn"}),
                    ]);
            }
            let response = client
                .post(format!("{root}/trae/v1/chat/completions"))
                .bearer_auth("synthetic-trae")
                .header("sec-fetch-site", "cross-site")
                .json(&body)
                .send()
                .await?;
            let status = response.status();
            let reply = response.text().await?;
            assert_eq!(
                status,
                StatusCode::OK,
                "synthetic tool follow-up: stream={stream}, later={later_turn}"
            );
            assert!(reply.contains("synthetic completion"));
            let sent = rx.recv().await.ok_or("missing continuation request")?;
            assert_eq!(
                sent["model"], "exact-model",
                "continuation moved to a different target"
            );
            for field in ["messages", "tools", "temperature", "vendor_extension"] {
                assert_eq!(sent[field], body[field]);
            }
        }
        for (app, unknown) in [("trae", true), ("claude", false)] {
            let mut rejected = body.clone();
            if unknown {
                rejected["messages"].as_array_mut().ok_or("missing messages")?.push(json!({"role":"tool","tool_call_id":"synthetic-unknown","content":"synthetic"}));
            }
            let response = client
                .post(format!("{root}/{app}/v1/chat/completions"))
                .bearer_auth(format!("synthetic-{app}"))
                .json(&rejected)
                .send()
                .await?;
            assert_eq!(response.status(), StatusCode::CONFLICT);
            assert!(
                rx.try_recv().is_err(),
                "rejected continuation reached upstream"
            );
        }
    }
    server.stop().await?;
    task.abort();
    Ok(())
}
