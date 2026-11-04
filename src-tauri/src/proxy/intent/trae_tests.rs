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
                if body["stream"] == true {
                    let delta = if followup { json!({"role":"assistant","content":"synthetic completion"}) } else { json!({"role":"assistant","tool_calls":[{"index":0,"id":call,"type":"function","function":{"name":"read_fixture","arguments":"{}"}}]}) };
                    let frames = format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                        json!({"id":"synthetic-stream","object":"chat.completion.chunk","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                        json!({"id":"synthetic-stream","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":if followup {"stop"} else {"tool_calls"}}]}));
                    let stream = futures::stream::iter(frames.into_bytes().chunks(7).map(|part| Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(part))).collect::<Vec<_>>());
                    return ([(header::CONTENT_TYPE,"text/event-stream")], Body::from_stream(stream)).into_response();
                }
                let message = if followup { json!({"role":"assistant","content":"synthetic completion"}) } else { json!({"role":"assistant","content":null,"tool_calls":[{"id":call,"type":"function","function":{"name":"read_fixture","arguments":"{}"}}]}) };
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
        let call = if stream {
            "synthetic-stream-call"
        } else {
            "synthetic-json-call"
        };
        let mut body = payload(stream);
        body["messages"].as_array_mut().ok_or("missing messages")?.extend([
            json!({"role":"assistant","content":null,"reasoning_content":"synthetic reasoning","tool_calls":[{"id":call,"type":"function","function":{"name":"read_fixture","arguments":"{}"}}]}),
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
