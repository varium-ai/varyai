use super::*;

#[tokio::test]
async fn unnamed_recovery_does_not_ignore_collision_in_another_conversation(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let store =
        continuation::Store::load(dir.path().join("state.json"), dir.path().join("state.key"))
            .await?;
    let session = two_targets()?;
    let conversations = scopes::Conversations::default();
    for name in ["collision", "unique"] {
        let state = conversations.acquire_durable(
            "local-codex",
            &session,
            Some(name),
            &json!({"input":"synthetic"}),
            Some(store.clone()),
        )?;
        let mut bindings = state.bindings.lock().await;
        bindings.observe(&json!({"id":"shared-response"}), 0);
        if name == "collision" {
            bindings.observe(&json!({"id":"shared-response"}), 1);
        }
    }
    store.flush().await?;
    assert!(scopes::Conversations::default()
        .acquire_durable(
            "local-codex",
            &session,
            None,
            &json!({"previous_response_id":"shared-response"}),
            Some(store),
        )
        .is_err());
    Ok(())
}

#[tokio::test]
async fn overlapping_generations_preserve_all_delivered_owners(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.json");
    let key = dir.path().join("state.key");
    let store = continuation::Store::load(path.clone(), key.clone()).await?;
    let first = two_targets()?;
    let conversations = scopes::Conversations::default();
    let older = conversations.acquire_durable(
        "local-codex",
        &first,
        Some("thread"),
        &json!({"input":"synthetic"}),
        Some(store.clone()),
    )?;
    older
        .bindings
        .lock()
        .await
        .observe(&json!({"id":"initial"}), 1);
    let mut next = two_targets()?;
    next.targets.swap(0, 1);
    let newer = conversations.acquire_durable(
        "local-codex",
        &next,
        Some("thread"),
        &json!({"previous_response_id":"initial"}),
        Some(store.clone()),
    )?;
    newer
        .bindings
        .lock()
        .await
        .observe(&json!({"id":"newer-response"}), 0);
    older
        .bindings
        .lock()
        .await
        .observe(&json!({"id":"older-response"}), 1);
    store.flush().await?;
    drop(newer);
    drop(older);
    drop(conversations);
    drop(store);
    let store = continuation::Store::load(path, key).await?;
    let conversations = scopes::Conversations::default();
    for id in ["initial", "newer-response", "older-response"] {
        let body = json!({"previous_response_id":id});
        let state = conversations.acquire_durable(
            "local-codex",
            &next,
            Some("thread"),
            &body,
            Some(store.clone()),
        )?;
        assert_eq!(
            state.bindings.lock().await.resolve(&body, None),
            Ok(Some(0))
        );
    }
    Ok(())
}

#[test]
fn destination_fingerprint_ignores_nested_map_insertion_order(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut first = session()?;
    let mut next = session()?;
    first.targets[0].upstream = Some(crate::provider::Provider::with_id(
        "synthetic".into(),
        "Synthetic".into(),
        serde_json::from_str(r#"{"outer":{"a":1,"b":2}}"#)?,
        None,
    ));
    next.targets[0].upstream = Some(crate::provider::Provider::with_id(
        "synthetic".into(),
        "Synthetic".into(),
        serde_json::from_str(r#"{"outer":{"b":2,"a":1}}"#)?,
        None,
    ));
    assert_eq!(
        first.targets[0].continuation_destination(),
        next.targets[0].continuation_destination()
    );
    Ok(())
}

fn two_targets() -> Result<Session, Box<dyn std::error::Error>> {
    let mut session = session()?;
    let mut other = session.targets[0].clone();
    other.provider_id = "other-provider".into();
    other.model = "other-model".into();
    session.targets.push(other);
    Ok(session)
}

async fn saved(
    session: &Session,
    store: Arc<continuation::Store>,
    name: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conversations = scopes::Conversations::default();
    let state = conversations.acquire_durable(
        "local-codex",
        session,
        name,
        &json!({"input":"synthetic"}),
        Some(store.clone()),
    )?;
    let mut bindings = state.bindings.lock().await;
    if let Some(name) = name {
        bindings.bind_session(name, 1);
    }
    bindings.observe(
        &json!({"id":"response-b","output":[
            {"type":"function_call","call_id":"call-b"},
            {"type":"reasoning","id":"item-b","encrypted_content":"opaque-b"}
        ]}),
        1,
    );
    drop(bindings);
    store.flush().await?;
    Ok(())
}

#[tokio::test]
async fn restored_owners_follow_destinations_not_target_positions(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.json");
    let key = dir.path().join("state.key");
    let first = two_targets()?;
    let store = continuation::Store::load(path.clone(), key.clone()).await?;
    saved(&first, store.clone(), Some("thread")).await?;
    drop(store);
    let store = continuation::Store::load(path, key).await?;
    let mut reordered = two_targets()?;
    reordered.targets.swap(0, 1);
    let body = json!({"previous_response_id":"response-b","input":[
        {"type":"function_call_output","call_id":"call-b","output":"synthetic"},
        {"type":"reasoning","id":"item-b","encrypted_content":"opaque-b"}
    ]});
    let conversations = scopes::Conversations::default();
    let restored = conversations.acquire_durable(
        "local-codex",
        &reordered,
        Some("thread"),
        &body,
        Some(store.clone()),
    )?;
    assert_eq!(
        restored
            .bindings
            .lock()
            .await
            .resolve(&body, Some("thread")),
        Ok(Some(0))
    );
    assert!(restored.bindings.lock().await.known_opaque(&body));
    for name in [None, Some("another-thread")] {
        let other = scopes::Conversations::default();
        let result =
            other.acquire_durable("local-codex", &reordered, name, &body, Some(store.clone()));
        if name.is_none() {
            assert_eq!(
                result?.bindings.lock().await.resolve(&body, None),
                Ok(Some(0))
            );
        } else {
            assert!(result?.bindings.lock().await.resolve(&body, name).is_err());
        }
    }
    let mut changed = two_targets()?;
    changed.group.id = "another-alias".into();
    let isolated = scopes::Conversations::default().acquire_durable(
        "local-codex",
        &changed,
        Some("thread"),
        &body,
        Some(store.clone()),
    )?;
    assert!(isolated
        .bindings
        .lock()
        .await
        .resolve(&body, Some("thread"))
        .is_err());
    let mut renamed = two_targets()?;
    renamed.group.model_alias = Some("auto/renamed".into());
    let isolated = scopes::Conversations::default().acquire_durable(
        "local-codex",
        &renamed,
        Some("thread"),
        &body,
        Some(store.clone()),
    )?;
    assert!(isolated
        .bindings
        .lock()
        .await
        .resolve(&body, Some("thread"))
        .is_err());
    for change in 0..4 {
        let mut changed = two_targets()?;
        match change {
            0 => changed.targets[0].model = "changed-model".into(),
            1 => changed.targets[0].url = "https://changed.example/responses".parse()?,
            2 => {
                changed.targets[0].headers.insert(
                    header::AUTHORIZATION,
                    http::HeaderValue::from_static("Bearer synthetic-new-key"),
                );
            }
            _ => changed.targets[0].provider_id = "changed-provider".into(),
        }
        assert!(scopes::Conversations::default()
            .acquire_durable(
                "local-codex",
                &changed,
                Some("thread"),
                &body,
                Some(store.clone()),
            )
            .is_err());
    }
    Ok(())
}

#[tokio::test]
async fn unnamed_recovery_ignores_unrelated_records_and_retains_collisions(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.json");
    let key = dir.path().join("state.key");
    let first = two_targets()?;
    let store = continuation::Store::load(path.clone(), key.clone()).await?;
    saved(&first, store.clone(), None).await?;
    let conversations = scopes::Conversations::default();
    let state = conversations.acquire_durable(
        "local-codex",
        &first,
        Some("collision"),
        &json!({"input":"synthetic"}),
        Some(store.clone()),
    )?;
    {
        let mut bindings = state.bindings.lock().await;
        bindings.observe(&json!({"id":"collision-response"}), 0);
        bindings.observe(&json!({"id":"collision-response"}), 1);
    }
    store.flush().await?;
    drop(state);
    drop(conversations);
    drop(store);
    let store = continuation::Store::load(path, key).await?;
    let current = two_targets()?;
    let body = json!({"previous_response_id":"response-b"});
    let restored = scopes::Conversations::default().acquire_durable(
        "local-codex",
        &current,
        None,
        &body,
        Some(store.clone()),
    )?;
    assert_eq!(
        restored.bindings.lock().await.resolve(&body, None),
        Ok(Some(1))
    );
    assert!(scopes::Conversations::default()
        .acquire_durable(
            "local-codex",
            &current,
            None,
            &json!({"previous_response_id":"collision-response"}),
            Some(store.clone()),
        )
        .is_err());
    assert!(scopes::Conversations::default()
        .acquire_durable(
            "local-codex",
            &current,
            None,
            &json!({"previous_response_id":"never-observed"}),
            Some(store.clone()),
        )
        .is_err());
    let mut changed = two_targets()?;
    changed.targets[0].model = "changed-model".into();
    let fresh = scopes::Conversations::default().acquire_durable(
        "local-codex",
        &changed,
        None,
        &json!({"input":"new text"}),
        Some(store),
    )?;
    assert_eq!(
        fresh
            .bindings
            .lock()
            .await
            .resolve(&json!({"input":"new text"}), None),
        Ok(None)
    );
    Ok(())
}

#[tokio::test]
async fn persistence_failure_does_not_break_live_bindings() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.json");
    let store = continuation::Store::load(path.clone(), dir.path().join("state.key")).await?;
    // A directory cannot be atomically replaced with a snapshot file.
    std::fs::create_dir(&path)?;
    let session = session()?;
    let state = scopes::Conversations::default().acquire_durable(
        "local-codex",
        &session,
        Some("thread"),
        &json!({"input":"synthetic"}),
        Some(store.clone()),
    )?;
    state
        .bindings
        .lock()
        .await
        .observe(&json!({"id":"live-response"}), 0);
    assert!(store.flush().await.is_err());
    assert_eq!(
        state
            .bindings
            .lock()
            .await
            .resolve(&json!({"previous_response_id":"live-response"}), None),
        Ok(Some(0))
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn http_reconnect_restores_json_and_observed_sse_before_eof(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
    };
    use axum::{routing::post, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(move |axum::Json(body): axum::Json<Value>| {
            observed.fetch_add(1, Ordering::SeqCst);
            async move {
                let (content_type, body) = if body["stream"] == true {
                    let events = concat!(
                        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"sse-response\"}}\n\n",
                        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"sse-call\"}}\n\n",
                        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"reasoning\",\"id\":\"sse-item\",\"encrypted_content\":\"sse-opaque\"}}\n\n"
                    );
                    let stream = async_stream::stream! {
                        for chunk in events.as_bytes().chunks(7) {
                            yield Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(chunk));
                        }
                        std::future::pending::<()>().await;
                    };
                    ("text/event-stream", Body::from_stream(stream))
                } else {
                    ("application/json", Body::from(json!({"id":"json-response","output":[
                        {"type":"function_call","call_id":"json-call"},
                        {"type":"reasoning","id":"json-item","encrypted_content":"json-opaque"}
                    ]}).to_string()))
                };
                let mut response = Response::new(body);
                response.headers_mut().insert(header::CONTENT_TYPE, http::HeaderValue::from_static(content_type));
                response
            }
        }))).await;
    });
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("state.json");
    let db = Arc::new(Database::memory()?);
    let make_session = || -> Result<Session, Box<dyn std::error::Error>> {
        let mut session = session()?;
        session.targets[0].url = format!("http://{address}/responses").parse()?;
        Ok(session)
    };
    let runtime = Arc::new(Runtime::default());
    runtime.prepare_continuations(path.clone()).await;
    let first = make_session()?;
    let alias = first.group.alias();
    let make_menu = |token: &str, session: Session| Menu {
        token: token.into(),
        models: Default::default(),
        default_model: None,
        omitted_entries: 0,
        context_settings: Default::default(),
        groups: HashMap::from([(alias.clone(), Arc::new(session))]),
    };
    runtime.set_menu(Arc::new(make_menu("synthetic-first-token", first)))?;
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
    let url = format!("http://127.0.0.1:{}/intent/v1/responses", info.port);
    let client = reqwest::Client::builder().no_proxy().build()?;
    let response = client
        .post(&url)
        .bearer_auth("synthetic-first-token")
        .header("session_id", "json-thread")
        .json(&json!({"model":alias,"input":"synthetic"}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let _: Value = response.json().await?;
    let response = client
        .post(&url)
        .bearer_auth("synthetic-first-token")
        .header("session-id", "sse-thread")
        .json(&json!({"model":alias,"input":"synthetic","stream":true}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream();
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut received = Vec::new();
        while let Some(chunk) = stream.next().await {
            received.extend_from_slice(&chunk?);
            if received.ends_with(b"sse-opaque\"}}\n\n") {
                return Ok::<_, Box<dyn std::error::Error>>(());
            }
        }
        Err("stream ended before observed ownership".into())
    })
    .await??;
    drop(stream);
    runtime.flush_continuations().await;
    server.stop().await?;
    drop(server);
    drop(runtime);
    let runtime = Arc::new(Runtime::default());
    runtime.prepare_continuations(path).await;
    runtime.set_menu(Arc::new(make_menu(
        "synthetic-reconnected-token",
        make_session()?,
    )))?;
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
    let url = format!("http://127.0.0.1:{}/intent/v1/responses", info.port);
    let temporary = runtime.install(make_session()?, None)?;
    let rejected = client
        .post(&url)
        .bearer_auth(&temporary)
        .header("session_id", "json-thread")
        .json(&json!({"model":alias,"previous_response_id":"json-response","input":"synthetic"}))
        .send()
        .await?;
    assert_eq!(rejected.status(), StatusCode::CONFLICT);
    assert_eq!(
        client
            .post(&url)
            .bearer_auth("synthetic-first-token")
            .json(&json!({"model":alias,"input":"synthetic"}))
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for kind in ["json", "sse"] {
        let body = json!({"model":alias,"previous_response_id":format!("{kind}-response"),"input":[
            {"type":"function_call_output","call_id":format!("{kind}-call"),"output":"synthetic"},
            {"type":"reasoning","id":format!("{kind}-item"),"encrypted_content":format!("{kind}-opaque")}
        ]});
        let response = client
            .post(&url)
            .bearer_auth("synthetic-reconnected-token")
            .header("session_id", format!("{kind}-thread"))
            .json(&body)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let _: Value = response.json().await?;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    server.stop().await?;
    task.abort();
    Ok(())
}

fn session() -> Result<Session, Box<dyn std::error::Error>> {
    let group: RoutingGroup = serde_json::from_value(json!({
        "id":"resume-fixture", "name":"Synthetic resume", "revision":0,
        "client":"codex", "enabled":false,
        "members":[{"id":"a","providerId":"synthetic-provider","modelId":"synthetic-model"}],
        "defaultMemberId":"a", "fallbackMemberIds":[], "intentRecognitionEnabled":false,
        "classifierTarget":null, "classificationBudgetMs":2000, "rules":[]
    }))?;
    Ok(Session {
        group,
        targets: vec![Target {
            fixed_effort: None,
            provider_id: "synthetic-provider".into(),
            model: "synthetic-model".into(),
            upstream: None,
            effort: Default::default(),
            url: "https://synthetic.example/responses".parse()?,
            headers: Default::default(),
            official: false,
            credentials: None,
            scheduling: Arc::new(scheduling::Scheduling::new(false)),
        }],
        classifier: None,
        client: reqwest::Client::builder().no_proxy().build()?,
        created: Instant::now(),
        bindings: Arc::default(),
    })
}

#[tokio::test]
async fn restart_restores_exact_response_tool_and_opaque_ownership(
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("continuations.json");
    let key = dir.path().join("continuations.key");
    let store = continuation::Store::load(path.clone(), key.clone()).await?;
    let first = session()?;
    let conversations = scopes::Conversations::default();
    let scope = conversations.acquire_durable(
        "local-codex",
        &first,
        Some("thread"),
        &json!({"input":"synthetic"}),
        Some(store.clone()),
    )?;
    {
        let mut bindings = scope.bindings.lock().await;
        bindings.bind_session("thread", 0);
        bindings.observe(
            &json!({"id":"resp-one", "output":[
                {"type":"function_call","call_id":"call-one"},
                {"type":"reasoning","id":"item-one","encrypted_content":"synthetic-opaque"}
            ]}),
            0,
        );
    }
    store.flush().await?;
    drop(scope);
    drop(conversations);
    drop(store);
    let store = continuation::Store::load(path, key).await?;
    let next = session()?;
    let conversations = scopes::Conversations::default();
    let body = json!({"previous_response_id":"resp-one", "input":[
        {"type":"function_call_output","call_id":"call-one","output":"synthetic"},
        {"type":"reasoning","id":"item-one","encrypted_content":"synthetic-opaque"}
    ]});
    let restored = conversations.acquire_durable(
        "local-codex",
        &next,
        Some("thread"),
        &body,
        Some(store.clone()),
    )?;
    assert_eq!(
        restored
            .bindings
            .lock()
            .await
            .resolve(&body, Some("thread")),
        Ok(Some(0))
    );
    assert!(restored.bindings.lock().await.known_opaque(&body));
    let mut changed = session()?;
    changed.targets[0].model = "different-model".into();
    assert!(scopes::Conversations::default()
        .acquire_durable(
            "local-codex",
            &changed,
            Some("thread"),
            &body,
            Some(store.clone())
        )
        .is_err());
    let foreign = conversations.acquire_durable(
        "other-consumer",
        &next,
        Some("thread"),
        &body,
        Some(store),
    )?;
    assert!(foreign
        .bindings
        .lock()
        .await
        .resolve(&body, Some("thread"))
        .is_err());
    Ok(())
}
