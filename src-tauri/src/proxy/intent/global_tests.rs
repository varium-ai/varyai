use super::*;
use crate::{
    database::Database,
    proxy::{
        gateway::{KeyPolicy, Period},
        server::ProxyServer,
        ProxyConfig,
    },
};
use axum::{routing::post, Json, Router};

pub(super) fn group() -> RoutingGroup {
    serde_json::from_value(json!({"id":"global-fixture","name":"Synthetic global","revision":0,"client":"codex","enabled":true,
        "members":[{"id":"a","providerId":"fixture","modelId":"exact-model"}],"defaultMemberId":"a","fallbackMemberIds":[],
        "intentRecognitionEnabled":false,"classifierTarget":null,"classificationBudgetMs":2000,"rules":[],"levels":["low","max"]})).unwrap()
}
pub(super) fn menu(url: &str) -> Menu {
    let group = group();
    Menu {
        token: "synthetic-local-token".into(),
        models: Default::default(),
        default_model: None,
        omitted_entries: 0,
        context_settings: Default::default(),
        groups: HashMap::from([(
            group.alias(),
            Arc::new(Session {
                group,
                targets: vec![Target {
                    fixed_effort: None,
                    provider_id: "fixture".into(),
                    model: "exact-model".into(),
                    upstream: None,
                    effort: Default::default(),
                    url: url.parse().unwrap(),
                    headers: http::HeaderMap::from_iter([(
                        header::AUTHORIZATION,
                        http::HeaderValue::from_static("Bearer synthetic-upstream"),
                    )]),
                    official: false,
                    credentials: None,
                    scheduling: Arc::default(),
                }],
                classifier: None,
                client: reqwest::Client::builder().no_proxy().build().unwrap(),
                created: Instant::now(),
                bindings: Arc::default(),
            }),
        )]),
    }
}

#[tokio::test]
#[serial_test::serial]
async fn workbuddy_readable_picker_ids_and_legacy_ids_reach_the_same_upstream(
) -> Result<(), Box<dyn std::error::Error>> {
    struct RestoreSecurity(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for RestoreSecurity {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _security = RestoreSecurity((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(
        crate::proxy::security::SecurityRuntimeConfig {
            enabled: true,
            ..Default::default()
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(2);
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/*path", post(move |Json(body): Json<Value>| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(body).await;
                Json(json!({"responseId":"synthetic-response","modelVersion":"gemini-3.6-flash","candidates":[{"content":{"role":"model","parts":[{"text":"Synthetic answer"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}}))
            }
        }))).await
    });
    let runtime = Arc::new(Runtime::default());
    let mut published = menu(&format!("http://{address}/responses"));
    let session = Arc::make_mut(
        published
            .groups
            .values_mut()
            .next()
            .ok_or("missing session")?,
    );
    let mut upstream = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        json!({"base_url":format!("http://{address}"),"api_key":"synthetic-upstream"}),
        None,
    );
    upstream.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("gemini_native".into()),
        ..Default::default()
    });
    session.group.levels.clear();
    session.group.members[0].model_id = "gemini-3.6-flash".into();
    session.targets[0].model = "gemini-3.6-flash".into();
    session.targets[0].effort = crate::services::intent_routing::effort::Capability::parse(
        &json!({"reasoningLevels":["minimal","low","medium","high"]}),
    );
    session.targets[0].upstream = Some(upstream);
    runtime.set_global(published, 0)?;
    let name = "Synthetic model · Synthetic supplier · VaryAI";
    runtime
        .connections
        .install(&crate::services::agent_connections::Receipt {
            app: "workbuddy".into(),
            token: "synthetic-local".into(),
            model: group().alias(),
            model_map: std::collections::BTreeMap::from([(name.into(), group().alias())]),
            visible_models: None,
            files: vec![],
            suspended: false,
            applied_at_ms: None,
        })?;
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
    for (id, stream, assistant_history) in [
        (name.to_owned(), false, None),
        (name.to_owned(), true, None),
        (group().alias(), false, None),
        (group().alias(), true, None),
        (
            name.to_owned(),
            false,
            Some(
                json!({"role":"assistant","content":[{"type":"text","text":"Synthetic prior answer","annotations":[]}],"agent":"synthetic-agent","conversationRequestId":"synthetic-conversation","messageId":"synthetic-message","model":"synthetic-model","rawUsage":{},"requestModelId":"synthetic-model","requestModelName":"Synthetic model","stepSeq":1,"traceId":"synthetic-trace","usage":{}}),
            ),
        ),
    ] {
        let mut payload = json!({"model":id,"stream":stream,"stream_options":{"include_usage":true},"reasoning_effort":"high",
            "messages":[{"role":"system","content":[{"type":"text","text":"Synthetic system prompt"}]},{"role":"user","content":[{"type":"text","text":"Synthetic request"}],"agent":"synthetic-agent","conversationRequestId":"synthetic-conversation","rootRequestId":"synthetic-root","startsNewUserRequest":true}],
            "tools":[{"type":"function","function":{"name":"synthetic_tool","description":"Synthetic tool","parameters":{"type":"object","properties":{"agent":{"type":"string"}}},"strict":false}}]});
        if let Some(history) = assistant_history {
            payload["messages"]
                .as_array_mut()
                .ok_or("missing messages")?
                .insert(1, history);
        }
        let response = client
            .post(format!(
                "http://127.0.0.1:{}/connections/workbuddy/v1/chat/completions",
                info.port
            ))
            .bearer_auth("synthetic-local")
            .header("x-conversation-id", "synthetic-workbuddy-history")
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        assert_eq!(status, StatusCode::OK, "synthetic response: {body}");
        assert!(body.contains("Synthetic answer"));
        if stream {
            assert!(body.contains("[DONE]"));
        }
        let sent = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await?
            .ok_or("missing request")?;
        let last = sent["contents"]
            .as_array()
            .and_then(|items| items.last())
            .ok_or("missing upstream content")?;
        assert_eq!(last["parts"][0]["text"], "Synthetic request");
        assert_eq!(
            sent["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "HIGH"
        );
        assert_eq!(
            sent["tools"][0]["functionDeclarations"][0]["name"],
            "synthetic_tool"
        );
    }
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn desktop_effort_picker_publication_and_old_conversations_reach_exact_upstream(
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
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/v1/messages", post(move |Json(body): Json<Value>| {
            let tx = tx.clone();
            async move {
                let _ = tx.send(body).await;
                Json(json!({"id":"synthetic-message","type":"message","role":"assistant","model":"exact-model","content":[{"type":"text","text":"synthetic answer"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}))
            }
        }))).await
    });
    let mut published = menu(&format!("http://{address}/v1/messages"));
    let alias = group().alias();
    let session = Arc::make_mut(published.groups.get_mut(&alias).ok_or("missing session")?);
    session.group.levels.clear();
    session.targets[0].effort = crate::services::intent_routing::effort::Capability::parse(
        &json!({"reasoningLevels":["low","high"]}),
    );
    let mut provider = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        json!({"env":{"ANTHROPIC_BASE_URL":format!("http://{address}"),"ANTHROPIC_AUTH_TOKEN":concat!("synthetic", "-upstream")}}),
        None,
    );
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("anthropic".into()),
        ..Default::default()
    });
    session.targets[0].upstream = Some(provider);
    published.models.insert(
        alias.clone(),
        MenuModel {
            alias: alias.clone(),
            label: "Synthetic reasoning model".into(),
            description: "VaryAI routing group",
            order: 0,
        },
    );
    published.default_model = Some(alias.clone());
    let runtime = Arc::new(Runtime::default());
    runtime.set_global(published, 0)?;
    let current = crate::claude_desktop_config::shared_reasoning_model_name(&alias);
    let previous = crate::claude_desktop_config::shared_model_name(&alias);
    let oldest = crate::claude_desktop_config::legacy_shared_model_name(&alias);
    runtime
        .connections
        .install(&crate::services::agent_connections::Receipt {
            app: "claude-desktop".into(),
            token: concat!("synthetic", "-local").into(),
            model: alias.clone(),
            model_map: std::collections::BTreeMap::from([(previous.clone(), alias)]),
            visible_models: None,
            files: vec![],
            suspended: false,
            applied_at_ms: None,
        })?;
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
    let root = format!(
        "http://127.0.0.1:{}/connections/claude-desktop/v1",
        info.port
    );
    let client = reqwest::Client::builder().no_proxy().build()?;
    let catalog: Value = client
        .get(format!("{root}/models"))
        .bearer_auth("synthetic-local")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(catalog["data"][0]["id"], current);
    assert_eq!(catalog["data"][0]["max_effort_level"], "high");
    for wire in [current, previous, oldest] {
        for suffix in ["", "[1m]"] {
            for (chosen, expected) in [("low", "low"), ("high", "high"), ("max", "high")] {
                let response = client.post(format!("{root}/messages"))
                    .bearer_auth("synthetic-local")
                    .json(&json!({"model":format!("{wire}{suffix}"),"max_tokens":64,"thinking":{"type":"adaptive"},"output_config":{"effort":chosen},"messages":[{"role":"user","content":"synthetic"}]}))
                    .send().await?;
                let status = response.status();
                let result: Value = response.json().await?;
                assert_eq!(status, StatusCode::OK, "synthetic response: {result}");
                let sent = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                    .await?
                    .ok_or("missing request")?;
                assert_eq!(sent["model"], "exact-model");
                assert_eq!(sent["output_config"]["effort"], expected);
            }
        }
    }
    server.stop().await?;
    task.abort();
    Ok(())
}

#[test]
fn global_catalog_is_independent_and_preserves_only_unchanged_continuity() {
    let runtime = Runtime::default();
    runtime
        .set_global(menu("http://synthetic.invalid/responses"), 0)
        .unwrap();
    assert!(!runtime.menu_active());
    let old = runtime.global_snapshot().unwrap().unwrap();
    runtime
        .set_global(menu("http://synthetic.invalid/responses"), 0)
        .unwrap();
    let next = runtime.global_snapshot().unwrap().unwrap();
    let alias = group().alias();
    assert!(Arc::ptr_eq(
        &old.groups[&alias].bindings,
        &next.groups[&alias].bindings
    ));
    assert!(Arc::ptr_eq(
        &old.groups[&alias].targets[0].scheduling,
        &next.groups[&alias].targets[0].scheduling
    ));
    runtime
        .set_global(menu("http://changed.invalid/responses"), 0)
        .unwrap();
    let changed = runtime.global_snapshot().unwrap().unwrap();
    assert!(!Arc::ptr_eq(
        &old.groups[&alias].bindings,
        &changed.groups[&alias].bindings
    ));
    runtime.clear_menu();
    assert!(runtime.global_snapshot().unwrap().is_some());
    runtime.clear_global();
    assert!(runtime.global_snapshot().unwrap().is_none());
}

#[tokio::test]
#[serial_test::serial]
async fn authenticated_global_models_and_generation_use_alias_policy_and_external_usage(
) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/responses", post(move |headers: http::HeaderMap, Json(body): Json<Value>| {
            let tx = tx.clone();
            async move { tx.send((headers, body)).await.unwrap(); Json(json!({"id":"synthetic-response","usage":{"input_tokens":7,"output_tokens":3}})) }
        }))).await.unwrap();
    });
    let runtime = Arc::new(Runtime::default());
    runtime.set_global(menu(&format!("http://{address}/responses")), 0)?;
    runtime.reserve_global_aliases(["custom-disabled".into()])?;
    let db = Arc::new(Database::memory()?);
    let alias = group().alias();
    let key = db.create_gateway_key(KeyPolicy {
        name: "Synthetic external".into(),
        models: vec![alias.clone(), "custom-disabled".into()],
        all_models: false,
        token_limit: None,
        period: Period::Never,
    })?;
    let other = db.create_gateway_key(KeyPolicy {
        name: "Synthetic other".into(),
        models: vec!["not-a-group".into()],
        all_models: false,
        token_limit: Some(0),
        period: Period::Never,
    })?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    )
    .with_intent_runtime(runtime);
    let info = server.start().await?;
    let root = format!("http://127.0.0.1:{}/gateway/v1", info.port);
    let client = reqwest::Client::builder().no_proxy().build()?;
    assert_eq!(
        client.get(format!("{root}/models")).send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    let list: Value = client
        .get(format!("{root}/models"))
        .bearer_auth(&key.secret)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(list["data"].as_array().unwrap().len(), 1);
    assert_eq!(list["data"][0]["id"], alias);
    assert_eq!(
        list["data"][0]["supported_reasoning_levels"],
        json!([
            {"effort":"low","description":"low"},
            {"effort":"max","description":"max"}
        ])
    );
    // Listing authenticates and filters policy, but does not consume or require token budget.
    let list: Value = client
        .get(format!("{root}/models"))
        .bearer_auth(&other.secret)
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(list["data"], json!([]));
    assert!(db.list_gateway_usage(Some(&key.key.id))?.is_empty());
    let response = client
        .post(format!("{root}/responses"))
        .bearer_auth(&other.secret)
        .json(&json!({"model":alias,"input":"synthetic"}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = client
        .post(format!("{root}/responses"))
        .bearer_auth(&key.secret)
        .json(&json!({"model":"custom-disabled","input":"synthetic"}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = client
        .post(format!("{root}/responses"))
        .bearer_auth(&key.secret)
        .header("cookie", "synthetic-caller-cookie")
        .header("chatgpt-account-id", "synthetic-caller-account")
        .json(&json!({"model":alias,"input":"synthetic","vendor_extension":{"keep":true}}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let _: Value = response.json().await?;
    let (headers, body) = rx.recv().await.ok_or("missing request")?;
    assert_eq!(headers[header::AUTHORIZATION], "Bearer synthetic-upstream");
    assert!(!headers.contains_key("cookie"));
    assert!(!headers.contains_key("chatgpt-account-id"));
    assert_eq!(body["model"], "exact-model");
    assert_eq!(body["vendor_extension"]["keep"], true);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let rows = db.list_gateway_usage(Some(&key.key.id)).unwrap();
            if !rows.is_empty() {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].agent, "external");
                assert_eq!(rows[0].requests, 1);
                assert_eq!(rows[0].tokens, 10);
                assert_eq!(rows[0].model, alias);
                assert_eq!(rows[0].upstream_model.as_deref(), Some("exact-model"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(rx.try_recv().is_err());
    server.stop().await?;
    task.abort();
    Ok(())
}

#[test]
fn conversation_scopes_isolate_consumers_lanes_and_active_leases() {
    let conversations = scopes::Conversations::default();
    let identity = Arc::default();
    let first = conversations
        .acquire(
            "consumer-a",
            &identity,
            Some("same"),
            &json!({"input":"synthetic"}),
        )
        .unwrap();
    first
        .bindings
        .try_lock()
        .unwrap()
        .observe(&json!({"id":"resp-one"}), 0);
    assert!(Arc::ptr_eq(
        &first,
        &conversations
            .acquire("consumer-a", &identity, Some("same"), &json!({}))
            .unwrap()
    ));
    let guard = first.bindings.try_lock().unwrap();
    assert!(conversations
        .acquire(
            "consumer-a",
            &identity,
            None,
            &json!({"previous_response_id":"resp-one"})
        )
        .is_err());
    let unrelated = conversations
        .acquire("consumer-a", &identity, Some("unrelated"), &json!({}))
        .unwrap();
    assert!(unrelated.bindings.try_lock().is_ok());
    drop(guard);
    assert!(Arc::ptr_eq(
        &first,
        &conversations
            .acquire(
                "consumer-a",
                &identity,
                None,
                &json!({"previous_response_id":"resp-one"})
            )
            .unwrap()
    ));
    assert!(conversations
        .acquire(
            "consumer-b",
            &identity,
            None,
            &json!({"previous_response_id":"resp-one"})
        )
        .is_err());
    let other = conversations
        .acquire("consumer-b", &identity, Some("same"), &json!({}))
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &other));
    let changed = Arc::default();
    assert!(conversations
        .acquire(
            "consumer-a",
            &changed,
            None,
            &json!({"previous_response_id":"resp-one"})
        )
        .is_err());
}

#[tokio::test]
#[serial_test::serial]
async fn native_gateway_formats_preserve_wire_extensions_and_gemini_stream_url(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::provider::{Provider, ProviderMeta};
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/*path",post(move |uri: http::Uri, headers: http::HeaderMap, bytes: bytes::Bytes| {
            let tx=tx.clone();
            async move {
                tx.send((uri.clone(), headers, bytes)).await.unwrap();
                if uri.path().ends_with(":streamGenerateContent") {
                    ([(header::CONTENT_TYPE,"text/event-stream")],"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"synthetic\"}]}}],\"usageMetadata\":{\"promptTokenCount\":1,\"candidatesTokenCount\":1}}\n\n").into_response()
                } else { Json(json!({"id":"native-fixture","content":[],"choices":[],"candidates":[],"usage":{"input_tokens":1,"output_tokens":1},"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}})).into_response() }
            }
        }))).await.unwrap();
    });
    use axum::response::IntoResponse;
    let runtime = Arc::new(Runtime::default());
    let mut published = menu(&format!("http://{address}/responses"));
    published.groups.clear();
    let fixtures = [
        (
            Protocol::Responses,
            "/v1/responses",
            json!({"input":"synthetic"}),
        ),
        (
            Protocol::Chat,
            "/v1/chat/completions",
            json!({"messages":[{"role":"user","content":"synthetic"}]}),
        ),
        (
            Protocol::Anthropic,
            "/v1/messages",
            json!({"messages":[{"role":"user","content":[{"type":"text","text":"synthetic"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"c3ludGhldGlj"}},{"type":"future_native","opaque":{"preserve":true}}]}],"max_tokens":16}),
        ),
        (
            Protocol::Gemini,
            "/v1beta/models/exact-model:generateContent",
            json!({"contents":[{"role":"user","parts":[{"text":"synthetic"}]}]}),
        ),
    ];
    for (index, (protocol, path, _)) in fixtures.iter().enumerate() {
        let mut resource = menu(&format!("http://{address}{path}"));
        let session = resource.groups.values_mut().next().unwrap();
        let session = Arc::make_mut(session);
        session.group.id = format!("native-{index}");
        let mut provider =
            Provider::with_id("fixture".into(), "Synthetic native".into(), json!({}), None);
        provider.meta = Some(ProviderMeta {
            api_format: Some(protocol.format().into()),
            ..Default::default()
        });
        session.targets[0].upstream = Some(provider);
        published
            .groups
            .insert(session.group.alias(), Arc::new(session.clone()));
    }
    runtime.set_global(published, 0)?;
    let db = Arc::new(Database::memory()?);
    let key = db.create_gateway_key(KeyPolicy {
        name: "Synthetic formats".into(),
        models: vec![],
        all_models: true,
        token_limit: None,
        period: Period::Never,
    })?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db,
        None,
    )
    .with_intent_runtime(runtime);
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for (index, (protocol, path, fixture)) in fixtures.iter().enumerate() {
        let mut body = fixture.clone();
        body["vendor_extension"] = json!({"retain":true});
        if *protocol != Protocol::Gemini {
            body["model"] = json!(format!("vary-auto/native-{index}"));
        }
        let client_path = if *protocol == Protocol::Gemini {
            format!("/v1beta/models/vary-auto/native-{index}:generateContent")
        } else {
            path.to_string()
        };
        let response = client
            .post(format!(
                "http://127.0.0.1:{}/gateway{client_path}",
                info.port
            ))
            .bearer_auth(&key.secret)
            .json(&body)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{protocol:?}");
        let _: Value = response.json().await?;
        let (uri, _, bytes) = rx.recv().await.ok_or("native request missing")?;
        let upstream: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(upstream["vendor_extension"], body["vendor_extension"]);
        for key in ["input", "messages", "contents", "max_tokens"] {
            assert_eq!(upstream.get(key), body.get(key));
        }
        if *protocol == Protocol::Gemini {
            assert_eq!(uri.path(), "/v1beta/models/exact-model:generateContent");
            assert!(upstream.get("model").is_none());
        } else {
            assert_eq!(upstream["model"], "exact-model");
        }
    }
    let response=client.post(format!("http://127.0.0.1:{}/gateway/v1beta/models/vary-auto/native-3:streamGenerateContent?alt=sse",info.port)).bearer_auth(&key.secret).json(&fixtures[3].2).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.text().await?;
    let (uri, _, _) = rx.recv().await.ok_or("stream request missing")?;
    assert_eq!(
        uri.path(),
        "/v1beta/models/exact-model:streamGenerateContent"
    );
    assert_eq!(uri.query(), Some("alt=sse"));
    server.stop().await?;
    task.abort();
    Ok(())
}

#[test]
fn reserved_aliases_and_native_opaque_continuity_fail_closed() {
    let runtime = Runtime::default();
    for alias in ["vary-auto/deleted", "auto/deleted", "vary-provider/deleted"] {
        assert!(runtime.global_alias_reserved(alias).unwrap());
    }
    runtime
        .reserve_global_aliases(["custom-tombstone".into()])
        .unwrap();
    runtime.clear_global();
    assert!(runtime.global_alias_reserved("custom-tombstone").unwrap());
    assert!(!opaque_continuation(
        Protocol::Gemini,
        &json!({"contents":[{"parts":[{"inlineData":{"mimeType":"image/png","data":"c3ludGhldGlj"}}]}]})
    ));
    assert!(opaque_continuation(
        Protocol::Gemini,
        &json!({"contents":[{"parts":[{"thoughtSignature":"opaque"}]}]})
    ));
}

#[test]
fn chat_visible_reasoning_is_portable_but_signed_or_encrypted_state_is_not() {
    for reasoning in [Value::Null, json!(""), json!("Synthetic visible thought")] {
        assert!(!opaque_continuation(
            Protocol::Chat,
            &json!({"messages":[{
                "role":"assistant", "content":"Synthetic answer", "reasoning_content":reasoning
            }]})
        ));
    }
    for detail in [
        json!({"type":"reasoning.encrypted","data":"synthetic-opaque"}),
        json!({"type":"reasoning.text","signature":"synthetic-signature","text":"Synthetic thought"}),
    ] {
        assert!(opaque_continuation(
            Protocol::Chat,
            &json!({"messages":[{
                "role":"assistant", "content":"Synthetic answer", "reasoning_details":[detail]
            }]})
        ));
    }
}

#[tokio::test]
#[serial_test::serial]
async fn agent_model_switch_keeps_unsigned_reasoning_history_portable(
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
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let task = tokio::spawn(async move {
        let _=axum::serve(listener,Router::new().route("/responses",post(move |Json(body):Json<Value>|{
            let tx=tx.clone();async move {let _=tx.send(body).await;Json(json!({"id":"synthetic","object":"response","model":"grok-4.5","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Synthetic Grok reply"}]}],"usage":{"input_tokens":1,"output_tokens":1}}))}
        }))).await;
    });
    let runtime = Arc::new(Runtime::default());
    let mut shared = menu(&format!("http://{address}/responses"));
    let session = Arc::make_mut(shared.groups.values_mut().next().ok_or("missing session")?);
    let mut upstream = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic Grok".into(),
        json!({"base_url":format!("http://{address}"),"auth":{"OPENAI_API_KEY":"synthetic-upstream"}}),
        None,
    );
    upstream.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_responses".into()),
        ..Default::default()
    });
    session.group.members[0].model_id = "grok-4.5".into();
    session.targets[0].model = "grok-4.5".into();
    session.targets[0].upstream = Some(upstream);
    runtime.set_global(shared, 0)?;
    for app in crate::services::agent_connections::APPS
        .iter()
        .copied()
        .filter(|app| *app != "codex")
        .chain(std::iter::once("trae"))
    {
        runtime
            .connections
            .install(&crate::services::agent_connections::Receipt {
                app: app.into(),
                token: format!("synthetic-{app}"),
                model: group().alias(),
                model_map: Default::default(),
                visible_models: None,
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            })?;
    }
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(runtime);
    let info = proxy.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for app in crate::services::agent_connections::APPS
        .iter()
        .copied()
        .filter(|app| *app != "codex")
        .chain(std::iter::once("trae"))
    {
        for protocol in [Protocol::Chat, Protocol::Anthropic] {
            let endpoint = if protocol == Protocol::Chat {
                "chat/completions"
            } else {
                "messages"
            };
            let url = format!(
                "http://127.0.0.1:{}/connections/{app}/v1/{endpoint}",
                info.port
            );
            for (stream, reasoning) in [false, true].into_iter().flat_map(|stream| {
                [Value::Null, json!(""), json!("Synthetic visible thought")]
                    .into_iter()
                    .map(move |reasoning| (stream, reasoning))
            }) {
                let mut payload = json!({
                    "model":group().alias(),"stream":stream,"messages":[{"role":"user","content":"Synthetic greeting"},{"role":"assistant","content":"Synthetic previous answer","reasoning_content":reasoning,"model":"gemini-synthetic","usage":{"tokens":1}},{"role":"user","content":"Synthetic next question"}]
                });
                if protocol == Protocol::Anthropic {
                    let mut thought =
                        json!({"type":"thinking","thinking":"Synthetic visible thought"});
                    if !reasoning.is_null() {
                        thought["signature"] = if reasoning == "" {
                            Value::Null
                        } else {
                            json!("")
                        };
                    }
                    payload["max_tokens"] = json!(32);
                    payload["messages"][1] = json!({"role":"assistant","content":[thought,{"type":"text","text":"Synthetic previous answer"}]});
                }
                let response = client
                    .post(&url)
                    .bearer_auth(format!("synthetic-{app}"))
                    .header("user-agent", "claude-cli/2.1.0 (external, cli)")
                    .header("x-claude-code-session-id", "synthetic-desktop-session")
                    .json(&payload)
                    .send()
                    .await?;
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "visible reasoning cannot require an opaque binding: {app}, {protocol:?}"
                );
                let text = response.text().await?;
                assert!(text.contains("Synthetic Grok reply"));
                if stream && protocol == Protocol::Chat {
                    assert!(text.contains("[DONE]"));
                }
                let body = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                    .await?
                    .ok_or("missing request")?;
                assert_eq!(body["model"], "grok-4.5");
                assert!(body["input"]
                    .to_string()
                    .contains("Synthetic previous answer"));
            }
            let opaque = if protocol == Protocol::Chat {
                vec![
                    json!({"role":"assistant","content":"Synthetic answer","reasoning_details":[{"type":"reasoning.encrypted","data":"synthetic-opaque"}]}),
                ]
            } else {
                vec![
                    json!({"role":"assistant","content":[{"type":"thinking","thinking":"Synthetic thought","signature":"synthetic-seal"},{"type":"text","text":"Synthetic answer"}]}),
                    json!({"role":"assistant","content":[{"type":"redacted_thinking","data":"synthetic-opaque"},{"type":"text","text":"Synthetic answer"}]}),
                ]
            };
            for assistant in opaque {
                let blocked=client.post(&url).bearer_auth(format!("synthetic-{app}")).json(&json!({"model":group().alias(),"max_tokens":32,"messages":[assistant,{"role":"user","content":"Synthetic next question"}]})).send().await?;
                assert_eq!(blocked.status(), StatusCode::CONFLICT);
                assert!(
                    rx.try_recv().is_err(),
                    "unknown encrypted state must not reach upstream"
                );
            }
        }
    }
    proxy.stop().await?;
    task.abort();
    Ok(())
}

#[test]
fn forwarding_response_headers_are_bounded_and_never_include_credentials() {
    let source = http::HeaderMap::from_iter([
        (
            header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        ),
        (header::RETRY_AFTER, http::HeaderValue::from_static("5")),
        (
            http::HeaderName::from_static("x-request-id"),
            http::HeaderValue::from_static("synthetic-request"),
        ),
        (
            header::SET_COOKIE,
            http::HeaderValue::from_static("synthetic=value"),
        ),
        (
            header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer synthetic-opaque"),
        ),
        (
            header::CONTENT_LENGTH,
            http::HeaderValue::from_static("400"),
        ),
    ]);
    let filtered = public_response_headers(&source);
    assert_eq!(filtered.len(), 3);
    assert_eq!(filtered[header::RETRY_AFTER], "5");
    assert_eq!(filtered["x-request-id"], "synthetic-request");
    assert!(!filtered.contains_key(header::SET_COOKIE));
    assert!(!filtered.contains_key(header::AUTHORIZATION));
    assert!(!filtered.contains_key(header::CONTENT_LENGTH));
}

#[tokio::test]
#[serial_test::serial]
async fn unrelated_gateway_streams_run_concurrently_but_same_conversation_is_busy(
) -> Result<(), Box<dyn std::error::Error>> {
    use axum::response::IntoResponse;
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let upstream_gate = gate.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/responses",post(move || {
            let gate=upstream_gate.clone();
            async move {
                let stream=async_stream::stream! {
                    yield Ok::<_,std::io::Error>(bytes::Bytes::from_static(b"data: {\"type\":\"response.created\",\"response\":{\"id\":\"stream-response\"}}\n\n"));
                    let _permit=gate.acquire().await;
                    yield Ok(bytes::Bytes::from_static(b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"stream-response\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n"));
                };
                ([(header::CONTENT_TYPE,"text/event-stream")],Body::from_stream(stream)).into_response()
            }
        }))).await.unwrap();
    });
    let runtime = Arc::new(Runtime::default());
    runtime.set_global(menu(&format!("http://{address}/responses")), 0)?;
    let db = Arc::new(Database::memory()?);
    let policy = |name: &str| KeyPolicy {
        name: name.into(),
        models: vec![group().alias()],
        all_models: false,
        token_limit: None,
        period: Period::Never,
    };
    let key = db.create_gateway_key(policy("Synthetic stream A"))?;
    let other = db.create_gateway_key(policy("Synthetic stream B"))?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db,
        None,
    )
    .with_intent_runtime(runtime);
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = format!("http://127.0.0.1:{}/gateway/v1/responses", info.port);
    let send = |secret: &str, conversation: &str| {
        client
            .post(&url)
            .bearer_auth(secret)
            .header("session-id", conversation)
            .json(&json!({"model":group().alias(),"input":"synthetic","stream":true}))
            .send()
    };
    let first = tokio::time::timeout(Duration::from_secs(3), send(&key.secret, "same")).await??;
    assert_eq!(first.status(), StatusCode::OK);
    let busy = tokio::time::timeout(Duration::from_secs(3), send(&key.secret, "same")).await??;
    assert_eq!(busy.status(), StatusCode::CONFLICT);
    let unrelated =
        tokio::time::timeout(Duration::from_secs(3), send(&key.secret, "other")).await??;
    assert_eq!(unrelated.status(), StatusCode::OK);
    let isolated =
        tokio::time::timeout(Duration::from_secs(3), send(&other.secret, "same")).await??;
    assert_eq!(isolated.status(), StatusCode::OK);
    gate.add_permits(3);
    for response in [first, unrelated, isolated] {
        tokio::time::timeout(Duration::from_secs(3), response.text()).await??;
    }
    server.stop().await?;
    task.abort();
    Ok(())
}

#[test]
fn gemini_accounts_share_quota_across_models_without_sharing_destinations() {
    use crate::provider::{Provider, ProviderMeta};
    let mut first = menu("http://synthetic.invalid/v1beta/models/one:generateContent")
        .groups
        .values()
        .next()
        .unwrap()
        .targets[0]
        .clone();
    let mut provider = Provider::with_id(
        "fixture".into(),
        "Synthetic Gemini".into(),
        json!({"base_url":"http://synthetic.invalid/v1beta"}),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some("gemini_native".into()),
        ..Default::default()
    });
    first.upstream = Some(provider);
    let mut second = first.clone();
    second.model = "two".into();
    second.url = "http://synthetic.invalid/v1beta/models/two:generateContent"
        .parse()
        .unwrap();
    assert!(first.same_account(&second));
    assert!(!first.same_destination(&second));
    second.headers.insert(
        header::AUTHORIZATION,
        http::HeaderValue::from_static("Bearer synthetic-rotated"),
    );
    assert!(!first.same_account(&second));
}

#[tokio::test]
#[serial_test::serial]
async fn gateway_selected_static_targets_convert_all_client_protocols(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::provider::{Provider, ProviderMeta};
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/*path",post(move |Json(body):Json<Value>| {
            let tx=tx.clone();
            async move {
                tx.send(body.clone()).await.unwrap();
                if body.get("messages").is_some() {
                    Json(json!({"id":"msg_fixture","type":"message","role":"assistant","model":"exact-model","content":[{"type":"text","text":"synthetic answer"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}))
                } else {
                    Json(json!({"id":"resp_fixture","object":"response","status":"completed","model":"exact-model","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic answer"}]}],"usage":{"input_tokens":1,"output_tokens":1}}))
                }
            }
        }))).await.unwrap();
    });
    let runtime = Arc::new(Runtime::default());
    let mut published = menu(&format!("http://{address}/responses"));
    published.groups.clear();
    for (id, output, path) in [
        ("responses", Protocol::Responses, "/v1/responses"),
        ("anthropic", Protocol::Anthropic, "/v1/messages"),
    ] {
        let mut resource = menu(&format!("http://{address}{path}"));
        let session = Arc::make_mut(resource.groups.values_mut().next().unwrap());
        session.group.id = id.into();
        let mut provider = Provider::with_id(
            "fixture".into(),
            "Synthetic cross".into(),
            json!({"env":{"ANTHROPIC_BASE_URL":format!("http://{address}"),"ANTHROPIC_API_KEY":concat!("synthetic","-source")}}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some(output.format().into()),
            ..Default::default()
        });
        session.targets[0].upstream = Some(provider);
        published
            .groups
            .insert(session.group.alias(), Arc::new(session.clone()));
    }
    runtime.set_global(published, 0)?;
    let db = Arc::new(Database::memory()?);
    let key = db.create_gateway_key(KeyPolicy {
        name: "Synthetic cross".into(),
        models: vec![],
        all_models: true,
        token_limit: None,
        period: Period::Never,
    })?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db,
        None,
    )
    .with_intent_runtime(runtime);
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for (path, body) in [
        (
            "/v1/responses",
            json!({"model":"vary-auto/anthropic","input":"synthetic"}),
        ),
        (
            "/v1/chat/completions",
            json!({"model":"vary-auto/anthropic","messages":[{"role":"user","content":"synthetic"}]}),
        ),
        (
            "/v1/messages",
            json!({"model":"vary-auto/responses","max_tokens":16,"messages":[{"role":"user","content":"synthetic"}]}),
        ),
        (
            "/v1beta/models/vary-auto/responses:generateContent",
            json!({"contents":[{"role":"user","parts":[{"text":"synthetic"}]}]}),
        ),
    ] {
        let response = client
            .post(format!("http://127.0.0.1:{}/gateway{path}", info.port))
            .bearer_auth(&key.secret)
            .json(&body)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let _: Value = response.json().await?;
        let body = rx.recv().await.ok_or("cross request missing")?;
        assert_eq!(body["model"], "exact-model");
    }
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn all_agent_connections_accept_anthropic_sdk_beta_without_leaking_it_across_protocols(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::provider::{Provider, ProviderMeta};
    use axum::response::IntoResponse;
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
    let (tx, mut rx) = tokio::sync::mpsc::channel(128);
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().fallback(post(move |uri: http::Uri, headers: http::HeaderMap, Json(body): Json<Value>| {
            let tx = tx.clone();
            async move {
                let _ = tx.send((uri.clone(), headers, body.clone())).await;
                let (value, frames) = match uri.path() {
                    path if path.ends_with("/messages/count_tokens") => (json!({"input_tokens":19}), vec![]),
                    path if path.ends_with("/messages") => (
                        json!({"id":"msg_sdk","type":"message","role":"assistant","model":"exact-model","content":[{"type":"text","text":"synthetic answer"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}),
                        vec![json!({"type":"message_start","message":{"id":"msg_sdk","type":"message","role":"assistant","model":"exact-model","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}),json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"synthetic answer"}}),json!({"type":"content_block_stop","index":0}),json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),json!({"type":"message_stop"})],
                    ),
                    path if path.ends_with("/chat/completions") => (
                        json!({"id":"chat_sdk","object":"chat.completion","model":"exact-model","choices":[{"index":0,"message":{"role":"assistant","content":"synthetic answer"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}}),
                        vec![json!({"id":"chat_sdk","object":"chat.completion.chunk","model":"exact-model","choices":[{"index":0,"delta":{"role":"assistant","content":"synthetic answer"},"finish_reason":null}]}),json!({"id":"chat_sdk","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}})],
                    ),
                    path if path.ends_with("/responses") => (
                        json!({"id":"resp_sdk","object":"response","status":"completed","model":"exact-model","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic answer"}]}],"usage":{"input_tokens":1,"output_tokens":1}}),
                        vec![json!({"type":"response.created","response":{"id":"resp_sdk","model":"exact-model","status":"in_progress","output":[]}}),json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"synthetic answer"}),json!({"type":"response.completed","response":{"id":"resp_sdk","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic answer"}]}],"usage":{"input_tokens":1,"output_tokens":1}}})],
                    ),
                    _ => (
                        json!({"candidates":[{"content":{"role":"model","parts":[{"text":"synthetic answer"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}}),
                        vec![json!({"candidates":[{"content":{"role":"model","parts":[{"text":"synthetic answer"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}})],
                    ),
                };
                if body["stream"] != true && !uri.path().ends_with(":streamGenerateContent") {
                    return Json(value).into_response();
                }
                let frames = frames.iter().map(|value| {
                    let event = value["type"].as_str().map(|kind| format!("event: {kind}\n")).unwrap_or_default();
                    format!("{event}data: {value}\n\n")
                }).collect::<String>();
                ([(header::CONTENT_TYPE, "text/event-stream")], frames).into_response()
            }
        }))).await
    });
    let runtime = Arc::new(Runtime::default());
    let mut published = menu(&format!("http://{address}/v1/messages"));
    published.groups.clear();
    for (id, format, path) in [
        ("sdk-anthropic", "anthropic", "/v1/messages"),
        ("sdk-chat", "openai_chat", "/v1/chat/completions"),
        ("sdk-responses", "openai_responses", "/v1/responses"),
        (
            "sdk-gemini",
            "gemini_native",
            "/v1beta/models/exact-model:generateContent",
        ),
    ] {
        let mut resource = menu(&format!("http://{address}{path}"));
        let session = Arc::make_mut(
            resource
                .groups
                .values_mut()
                .next()
                .ok_or("missing session")?,
        );
        session.group.id = id.into();
        let mut provider = Provider::with_id(
            "fixture".into(),
            "Synthetic SDK".into(),
            json!({"env":{"ANTHROPIC_BASE_URL":format!("http://{address}"),"ANTHROPIC_API_KEY":concat!("synthetic", "-upstream")}}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some(format.into()),
            ..Default::default()
        });
        session.targets[0].upstream = Some(provider);
        published
            .groups
            .insert(session.group.alias(), Arc::new(session.clone()));
    }
    runtime.set_global(published, 0)?;
    for app in crate::services::agent_connections::APPS
        .iter()
        .copied()
        .filter(|app| *app != "codex")
        .chain(std::iter::once("trae"))
    {
        runtime
            .connections
            .install(&crate::services::agent_connections::Receipt {
                app: app.into(),
                token: format!("synthetic-{app}"),
                model: "vary-auto/sdk-anthropic".into(),
                model_map: ["anthropic", "chat", "responses", "gemini"]
                    .into_iter()
                    .map(|protocol| {
                        (
                            format!("wire-{protocol}"),
                            format!("vary-auto/sdk-{protocol}"),
                        )
                    })
                    .collect(),
                visible_models: None,
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            })?;
    }
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
    for app in crate::services::agent_connections::APPS
        .iter()
        .copied()
        .filter(|app| *app != "codex")
        .chain(std::iter::once("trae"))
    {
        let root = format!("http://127.0.0.1:{}/connections/{app}/v1", info.port);
        for protocol in ["anthropic", "chat", "responses", "gemini"] {
            for stream in [false, true] {
                let response = client.post(format!("{root}/messages?beta=true"))
                    .bearer_auth(format!("synthetic-{app}"))
                    .header("user-agent", "claude-cli/2.1.0 (external, cli)")
                    .header("anthropic-version", "2023-06-01")
                    .header("anthropic-beta", "synthetic-beta")
                    .header("x-stainless-lang", "js")
                    .header("x-claude-code-session-id", "synthetic-session")
                    .header("cookie", "synthetic-caller-cookie")
                    .json(&json!({"model":format!("wire-{protocol}"),"max_tokens":32,"stream":stream,"messages":[{"role":"user","content":"synthetic request"}]})).send().await?;
                let status = response.status();
                let reply = response.text().await?;
                let code = serde_json::from_str::<Value>(&reply)
                    .ok()
                    .and_then(|body| body["error"]["code"].as_str().map(str::to_owned));
                assert_eq!(
                    status,
                    StatusCode::OK,
                    "{app} -> {protocol}, stream={stream}, code={code:?}"
                );
                assert!(
                    reply.contains("synthetic answer"),
                    "reply missing for {app} -> {protocol}"
                );
                let (uri, headers, body) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                    .await?
                    .ok_or("missing SDK request")?;
                assert_eq!(
                    uri.query(),
                    if protocol == "anthropic" {
                        Some("beta=true")
                    } else if protocol == "gemini" && stream {
                        Some("alt=sse")
                    } else {
                        None
                    },
                    "wrong SDK query for {app} -> {protocol}"
                );
                if protocol != "gemini" {
                    assert_eq!(body["model"], "exact-model");
                }
                assert!(!headers.contains_key("cookie"));
                assert_ne!(
                    headers
                        .get(header::AUTHORIZATION)
                        .and_then(|value| value.to_str().ok()),
                    Some(format!("Bearer synthetic-{app}").as_str())
                );
                if protocol == "anthropic" {
                    assert_eq!(
                        headers[header::USER_AGENT],
                        "claude-cli/2.1.0 (external, cli)"
                    );
                    assert_eq!(headers["x-stainless-lang"], "js");
                    assert_eq!(headers["x-claude-code-session-id"], "synthetic-session");
                    assert_eq!(headers["anthropic-beta"], "synthetic-beta");
                }
            }
        }
        let count = client.post(format!("{root}/messages/count_tokens?beta=true"))
            .bearer_auth(format!("synthetic-{app}"))
            .header("user-agent", "claude-cli/2.1.0 (external, cli)")
            .json(&json!({"model":"wire-anthropic","messages":[{"role":"user","content":"synthetic count"},{"role":"assistant","content":[{"type":"tool_use","id":"synthetic-call","name":"synthetic_tool","input":{}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-call","content":"synthetic result"}]}],"tools":[{"name":"synthetic_tool","input_schema":{"type":"object"}}],"vendor":{"preserved":true}})).send().await?;
        assert_eq!(
            count.status(),
            StatusCode::OK,
            "token count unavailable for {app}"
        );
        assert_eq!(count.json::<Value>().await?["input_tokens"], 19);
        let (uri, _, body) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await?
            .ok_or("missing count request")?;
        assert!(uri.path().ends_with("/messages/count_tokens"));
        assert_eq!(uri.query(), None);
        assert_eq!(body["model"], "exact-model");
        assert_eq!(body["tools"][0]["name"], "synthetic_tool");
        assert_eq!(
            body["messages"][2]["content"][0]["tool_use_id"],
            "synthetic-call"
        );
        assert_eq!(body["vendor"]["preserved"], true);
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("thinking").is_none());
        assert!(body.get("output_config").is_none());
        for protocol in ["chat", "responses", "gemini"] {
            let count = client.post(format!("{root}/messages/count_tokens?beta=true"))
                .bearer_auth(format!("synthetic-{app}"))
                .json(&json!({"model":format!("wire-{protocol}"),"messages":[{"role":"user","content":"synthetic estimate"}]})).send().await?;
            assert_eq!(
                count.status(),
                StatusCode::OK,
                "local estimate unavailable for {app}"
            );
            assert!(count.json::<Value>().await?["input_tokens"]
                .as_u64()
                .is_some());
        }
        assert!(
            rx.try_recv().is_err(),
            "local estimate made a generation request"
        );
        for (endpoint, query) in [
            ("messages", "beta=false"),
            ("messages", "beta=true&key=synthetic-query"),
            ("messages", "unknown=1"),
            ("messages/count_tokens", "beta=false"),
            ("messages/count_tokens", "beta=true&unknown=1"),
            ("responses", "beta=true"),
            ("chat/completions", "beta=true"),
            ("models", "beta=true"),
        ] {
            let url = format!("{root}/{endpoint}?{query}");
            let request = if endpoint == "models" {
                client.get(url)
            } else {
                client.post(url).json(&json!({"model":"wire-anthropic","input":"synthetic","messages":[{"role":"user","content":"synthetic"}]}))
            };
            let response = request
                .bearer_auth(format!("synthetic-{app}"))
                .send()
                .await?;
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "unexpected query accepted for {app}"
            );
            let error: Value = response.json().await?;
            assert_eq!(error["error"]["code"], "routing_query_unsupported");
        }
        assert!(rx.try_recv().is_err(), "rejected query reached upstream");
    }
    for protocol in ["chat", "responses", "gemini"] {
        let unavailable = client
            .post(format!("http://127.0.0.1:{}/connections/claude/v1/messages/count_tokens", info.port))
            .bearer_auth("synthetic-claude")
            .json(&json!({"model":format!("wire-{protocol}"),"messages":[{"role":"user","content":[{"type":"document","source":{"type":"text","data":"synthetic document"}}]}]}))
            .send().await?;
        assert_eq!(unavailable.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            unavailable.json::<Value>().await?["error"]["code"],
            "routing_token_estimate_unavailable"
        );
    }
    assert!(
        rx.try_recv().is_err(),
        "unsupported estimate reached upstream"
    );
    let mut security = crate::proxy::security::SecurityRuntimeConfig::default();
    security.builtin_action_overrides.insert(
        "rsa_ssh_private_key".into(),
        crate::proxy::security::types::AuditAction::Block,
    );
    crate::proxy::security::runtime::set_security_runtime_for_test(security);
    for protocol in ["anthropic", "responses"] {
        let blocked = client
            .post(format!("http://127.0.0.1:{}/connections/claude/v1/messages/count_tokens", info.port))
            .bearer_auth("synthetic-claude")
            .json(&json!({"model":format!("wire-{protocol}"),"messages":[{"role":"user","content":format!("-----BEGIN {}-----\nsynthetic-only\n-----END {}-----", "RSA PRIVATE KEY", "RSA PRIVATE KEY")}]}))
            .send().await?;
        assert_eq!(blocked.status(), StatusCode::BAD_REQUEST);
        let blocked = blocked.json::<Value>().await?;
        assert_eq!(blocked["type"], "error");
        assert_eq!(blocked["error"]["type"], "invalid_request_error");
    }
    assert!(rx.try_recv().is_err(), "blocked count reached upstream");
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn trae_native_origin_chat_preserves_authentication_and_browser_rejection(
) -> Result<(), Box<dyn std::error::Error>> {
    use axum::response::IntoResponse;
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
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/v1/chat/completions", post(move |headers: http::HeaderMap, Json(body): Json<Value>| {
            let tx = tx.clone();
            async move {
                let _ = tx.send((headers, body.clone())).await;
                if body["stream"] == true {
                    return ([(header::CONTENT_TYPE, "text/event-stream")], concat!(
                        "data: {\"id\":\"synthetic-chat\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"synthetic answer\"},\"finish_reason\":null}]}\n\n",
                        "data: {\"id\":\"synthetic-chat\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                        "data: [DONE]\n\n"
                    )).into_response();
                }
                Json(json!({"id":"synthetic-chat","object":"chat.completion","model":"exact-model","choices":[{"index":0,"message":{"role":"assistant","content":"synthetic answer"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}})).into_response()
            }
        }))).await
    });
    let runtime = Arc::new(Runtime::default());
    let mut published = menu(&format!("http://{address}/v1/chat/completions"));
    let session = Arc::make_mut(
        published
            .groups
            .values_mut()
            .next()
            .ok_or("missing fixture")?,
    );
    let mut provider = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic TRAE upstream".into(),
        json!({"env":{"ANTHROPIC_BASE_URL":format!("http://{address}"),"ANTHROPIC_API_KEY":concat!("synthetic", "-upstream")}}),
        None,
    );
    provider.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_chat".into()),
        ..Default::default()
    });
    session.targets[0].upstream = Some(provider);
    runtime.set_global(published, 0)?;
    for app in ["trae", "claude"] {
        runtime
            .connections
            .install(&crate::services::agent_connections::Receipt {
                app: app.into(),
                token: format!("synthetic-{app}"),
                model: group().alias(),
                model_map: Default::default(),
                visible_models: None,
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            })?;
    }
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
    let root = format!("http://127.0.0.1:{}/connections", info.port);
    let payload = json!({"model":group().alias(),"messages":[{"role":"user","content":"synthetic request"}],"temperature":0.25,"max_tokens":32});
    for (origin, stream) in [
        (None, false),
        (None, true),
        (Some("vscode-file://vscode-app"), false),
        (Some("vscode-file://vscode-app"), true),
    ] {
        let mut body = payload.clone();
        body["stream"] = json!(stream);
        let mut request = client
            .post(format!("{root}/trae/v1/chat/completions"))
            .bearer_auth("synthetic-trae")
            .header("sec-fetch-site", "cross-site")
            .header("sec-fetch-mode", "cors");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        let response = request.json(&body).send().await?;
        let status = response.status();
        assert!(!response
            .headers()
            .contains_key("access-control-allow-origin"));
        assert!(!response
            .headers()
            .contains_key("access-control-allow-credentials"));
        let reply = response.text().await?;
        let code = serde_json::from_str::<Value>(&reply)
            .ok()
            .and_then(|body| body["error"]["code"].as_str().map(str::to_owned));
        assert_eq!(
            status,
            StatusCode::OK,
            "native TRAE origin={}, stream={stream}, code={code:?}",
            origin.is_some()
        );
        assert!(reply.contains("synthetic answer"));
        let (headers, forwarded) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await?
            .ok_or("missing synthetic request")?;
        assert_eq!(forwarded["model"], "exact-model");
        assert_eq!(forwarded["messages"], body["messages"]);
        assert_eq!(forwarded["temperature"], body["temperature"]);
        assert_eq!(forwarded["stream"], stream);
        assert!(!headers.contains_key("origin"));
        assert!(!headers.contains_key("sec-fetch-site"));
        assert!(!headers.contains_key("sec-fetch-mode"));
        assert_eq!(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer synthetic-upstream")
        );
    }
    for (origin, token) in [None, Some("vscode-file://vscode-app")]
        .into_iter()
        .flat_map(|origin| {
            [None, Some("synthetic-wrong"), Some("synthetic-claude")]
                .into_iter()
                .map(move |token| (origin, token))
        })
    {
        let mut request = client
            .post(format!("{root}/trae/v1/chat/completions"))
            .header("sec-fetch-site", "cross-site");
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.json(&payload).send().await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.json::<Value>().await?["error"]["code"],
            "agent_connection_unavailable"
        );
    }
    for (method, app, endpoint, origin, fetch_site) in [
        (
            http::Method::POST,
            "trae",
            "chat/completions",
            Some("https://fixture.invalid"),
            None,
        ),
        (
            http::Method::POST,
            "trae",
            "chat/completions",
            Some("null"),
            Some("cross-site"),
        ),
        (
            http::Method::POST,
            "claude",
            "chat/completions",
            None,
            Some("cross-site"),
        ),
        (
            http::Method::GET,
            "trae",
            "models",
            None,
            Some("cross-site"),
        ),
        (
            http::Method::OPTIONS,
            "trae",
            "chat/completions",
            None,
            Some("cross-site"),
        ),
        (
            http::Method::POST,
            "claude",
            "chat/completions",
            Some("vscode-file://vscode-app"),
            Some("cross-site"),
        ),
        (
            http::Method::GET,
            "trae",
            "models",
            Some("vscode-file://vscode-app"),
            Some("cross-site"),
        ),
        (
            http::Method::OPTIONS,
            "trae",
            "chat/completions",
            Some("vscode-file://vscode-app"),
            Some("cross-site"),
        ),
    ] {
        let mut request = client
            .request(method.clone(), format!("{root}/{app}/v1/{endpoint}"))
            .bearer_auth(format!("synthetic-{app}"));
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        if let Some(fetch_site) = fetch_site {
            request = request.header("sec-fetch-site", fetch_site);
        }
        if method == http::Method::OPTIONS {
            request = request
                .header("access-control-request-method", "POST")
                .header(
                    "access-control-request-headers",
                    "authorization,content-type",
                );
        }
        let response = request.json(&payload).send().await?;
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} {app}/{endpoint}"
        );
        assert!(!response
            .headers()
            .contains_key("access-control-allow-origin"));
        assert!(!response
            .headers()
            .contains_key("access-control-allow-credentials"));
        assert!(!response
            .headers()
            .contains_key("access-control-allow-methods"));
        assert_eq!(
            response.json::<Value>().await?["error"]["code"],
            "routing_local_clients_only"
        );
    }
    let response = client
        .post(format!("{root}/trae/v1/chat/completions?unsupported=1"))
        .bearer_auth("synthetic-trae")
        .header("origin", "vscode-file://vscode-app")
        .header("sec-fetch-site", "cross-site")
        .json(&payload)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.json::<Value>().await?["error"]["code"],
        "routing_query_unsupported"
    );
    let mut security = crate::proxy::security::SecurityRuntimeConfig::default();
    security.builtin_action_overrides.insert(
        "rsa_ssh_private_key".into(),
        crate::proxy::security::types::AuditAction::Block,
    );
    crate::proxy::security::runtime::set_security_runtime_for_test(security);
    let response = client
        .post(format!("{root}/trae/v1/chat/completions"))
        .bearer_auth("synthetic-trae")
        .header("sec-fetch-site", "cross-site")
        .json(&json!({"model":group().alias(),"messages":[{"role":"user","content":format!("-----BEGIN {}-----\nsynthetic-only\n-----END {}-----", "RSA PRIVATE KEY", "RSA PRIVATE KEY")}]}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.json::<Value>().await?["error"]["code"],
        "sensitive_data_blocked"
    );
    runtime.connections.revoke("trae");
    let response = client
        .post(format!("{root}/trae/v1/chat/completions"))
        .bearer_auth("synthetic-trae")
        .header("sec-fetch-site", "cross-site")
        .json(&payload)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(rx.try_recv().is_err(), "rejected request reached upstream");
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn agent_connections_share_global_routes_without_agent_owned_providers(
) -> Result<(), Box<dyn std::error::Error>> {
    struct RestoreSecurity(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for RestoreSecurity {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let restore_security =
        RestoreSecurity((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(
        crate::proxy::security::SecurityRuntimeConfig {
            enabled: false,
            ..Default::default()
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(move |headers: http::HeaderMap, Json(body): Json<Value>| {
            let tx = tx.clone(); async move { let _ = tx.send((headers, body)).await; Json(json!({"id":"synthetic-response", "object":"response", "output":[
                {"type":"reasoning","id":"synthetic-reasoning-one","summary":[]},
                {"type":"reasoning","id":"synthetic-reasoning-two","summary":[],"encrypted_content":"synthetic-opaque-state"},
                {"type":"reasoning","id":"synthetic-reasoning-three","summary":[{"type":"summary_text","text":"Synthetic summary"}]}
            ], "usage":{"input_tokens":1,"output_tokens":1}})) }
        }))).await;
    });
    let runtime = Arc::new(Runtime::default());
    let mut shared = menu(&format!("http://{address}/responses"));
    let session = Arc::make_mut(shared.groups.values_mut().next().ok_or("missing fixture")?);
    // Cross-protocol consumers require an explicit static source, not inferred credentials.
    let mut upstream = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic Responses source".into(),
        json!({"env":{"ANTHROPIC_BASE_URL":format!("http://{address}"),"ANTHROPIC_API_KEY":"synthetic-upstream"}}),
        None,
    );
    upstream.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_responses".into()),
        ..Default::default()
    });
    session.targets[0].upstream = Some(upstream);
    session.targets[0].effort = crate::services::intent_routing::effort::Capability::parse(
        &json!({"reasoningLevels":["low","high"]}),
    );
    runtime.set_global(shared, 0)?;
    for app in crate::services::agent_connections::APPS
        .iter()
        .copied()
        .chain(std::iter::once("trae"))
        .filter(|app| *app != "codex")
    {
        runtime
            .connections
            .install(&crate::services::agent_connections::Receipt {
                app: app.into(),
                token: format!("synthetic-{app}"),
                model: group().alias(),
                model_map: if app == "claude-desktop" {
                    std::collections::BTreeMap::from([(
                        "desktop-hidden-wire".into(),
                        group().alias(),
                    )])
                } else {
                    Default::default()
                },
                visible_models: (app == "claude-desktop").then(Vec::new),
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            })?;
    }
    let db = Arc::new(Database::memory()?);
    assert!(db.get_all_providers("claude")?.is_empty());
    assert!(db.get_all_providers("pi")?.is_empty());
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
    for app in crate::services::agent_connections::APPS
        .iter()
        .copied()
        .chain(std::iter::once("trae"))
        .filter(|app| *app != "codex")
    {
        let root = format!("http://127.0.0.1:{}/connections/{app}/v1", info.port);
        assert_eq!(
            client
                .get(format!("{root}/models"))
                .bearer_auth("synthetic-wrong")
                .send()
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let wire = if app == "claude-desktop" {
            "desktop-hidden-wire".to_owned()
        } else {
            group().alias()
        };
        let first = client
            .post(format!("{root}/responses"))
            .bearer_auth(format!("synthetic-{app}"))
            .json(&json!({"model":wire,"input":"Synthetic first turn"}));
        // Native Fetch Metadata is allowed only on TRAE's Chat endpoint.
        // This common Responses fixture uses the plain local consumer contract.
        let first = first.send().await?;
        assert_eq!(first.status(), StatusCode::OK, "{app}");
        let _ = first.bytes().await?;
        let _ = rx.recv().await.ok_or("missing first request")?;
        let history = json!([
            {"role":"user","content":"Synthetic first turn"},
            {"type":"reasoning","id":"synthetic-reasoning-one","content":[{"type":"reasoning_text","text":"Synthetic retained thought"}],"custom":"kept"},
            {"type":"reasoning","id":"synthetic-reasoning-two","summary":null,"encrypted_content":"synthetic-opaque-state"},
            {"type":"reasoning","id":"synthetic-reasoning-three","summary":[{"type":"summary_text","text":"Synthetic summary"}]},
            {"role":"assistant","content":"Synthetic first answer"},
            {"role":"user","content":"Synthetic second turn"}
        ]);
        let response = client
            .post(format!("{root}/responses"))
            .bearer_auth(format!("synthetic-{app}"))
            .json(&json!({"model":wire, "input":history, "reasoning":{"effort":"max","summary":"auto"}}));
        let response = response.send().await?;
        assert_eq!(response.status(), StatusCode::OK, "{app}");
        let (headers, body) = rx.recv().await.ok_or("missing request")?;
        assert_eq!(body["model"], "exact-model");
        assert_eq!(body["reasoning"]["effort"], "high", "{app}");
        assert_eq!(body["reasoning"]["summary"], "auto", "{app}");
        let mut expected = history;
        if app == "trae" {
            expected[1]["summary"] = json!([]);
        }
        assert_eq!(body["input"], expected, "{app}");
        assert_ne!(
            headers.get("authorization").and_then(|h| h.to_str().ok()),
            Some(format!("Bearer synthetic-{app}").as_str())
        );
    }
    for (app, site, mode, origin, credential, expected) in [
        (
            "trae",
            "cross-site",
            "no-cors",
            None,
            "synthetic-trae",
            StatusCode::FORBIDDEN,
        ),
        (
            "trae",
            "same-origin",
            "cors",
            None,
            "synthetic-trae",
            StatusCode::FORBIDDEN,
        ),
        (
            "trae",
            "none",
            "navigate",
            None,
            "synthetic-trae",
            StatusCode::FORBIDDEN,
        ),
        (
            "trae",
            "none",
            "",
            None,
            "synthetic-trae",
            StatusCode::FORBIDDEN,
        ),
        (
            "trae",
            "none",
            "no-cors",
            Some("null"),
            "synthetic-trae",
            StatusCode::FORBIDDEN,
        ),
        (
            "trae",
            "none",
            "no-cors",
            Some("https://synthetic.example"),
            "synthetic-trae",
            StatusCode::FORBIDDEN,
        ),
        (
            "trae",
            "none",
            "no-cors",
            None,
            "synthetic-wrong",
            StatusCode::UNAUTHORIZED,
        ),
        (
            "claude",
            "none",
            "no-cors",
            None,
            "synthetic-claude",
            StatusCode::FORBIDDEN,
        ),
    ] {
        let mut request = client
            .post(format!(
                "http://127.0.0.1:{}/connections/{app}/v1/responses",
                info.port
            ))
            .bearer_auth(credential)
            .json(&json!({"model":group().alias(),"input":"Synthetic rejected request"}));
        if credential != "synthetic-wrong" {
            request = request
                .header("sec-fetch-site", site)
                .header("sec-fetch-mode", mode);
        }
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        assert_eq!(
            request.send().await?.status(),
            expected,
            "{app} {site} {mode}"
        );
    }
    for peer in [
        None,
        Some("192.0.2.1:12345".parse::<std::net::SocketAddr>()?),
    ] {
        use tower::Service;
        let mut request = http::Request::builder()
            .method("POST")
            .uri("/connections/trae/v1/responses")
            .header("authorization", "Bearer synthetic-trae")
            .header("sec-fetch-site", "none")
            .header("sec-fetch-mode", "no-cors")
            .header("content-type", "application/json")
            .body(axum::body::Body::from("{}"))?;
        if let Some(peer) = peer {
            request.extensions_mut().insert(peer);
        }
        assert_eq!(
            server.build_router().call(request).await?.status(),
            StatusCode::FORBIDDEN
        );
    }
    assert!(rx.try_recv().is_err(), "rejected request reached upstream");
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    let synthetic_phone = ["138", "0000", "1234"].concat();
    let redacted = client
        .post(format!(
            "http://127.0.0.1:{}/connections/qoder/v1/chat/completions",
            info.port
        ))
        .bearer_auth("synthetic-qoder")
        .json(&json!({
            "model":group().alias(),
            "messages":[{"role":"user","content":format!("Synthetic contact: {synthetic_phone}")}]
        }))
        .send()
        .await?;
    assert_eq!(redacted.status(), StatusCode::OK);
    let _ = redacted.bytes().await?;
    let (_, forwarded) = rx.recv().await.ok_or("missing redacted request")?;
    assert!(forwarded["input"].is_array());
    assert!(forwarded.get("messages").is_none());
    let forwarded = forwarded.to_string();
    assert!(
        !forwarded.contains(&synthetic_phone),
        "unredacted synthetic phone reached upstream"
    );
    assert!(forwarded.contains("[REDACTED]"));
    let mut security = crate::proxy::security::SecurityRuntimeConfig::default();
    security.builtin_action_overrides.insert(
        "rsa_ssh_private_key".into(),
        crate::proxy::security::types::AuditAction::Block,
    );
    crate::proxy::security::runtime::set_security_runtime_for_test(security);
    let blocked = client
        .post(format!("http://127.0.0.1:{}/connections/claude/v1/responses", info.port))
        .bearer_auth("synthetic-claude")
        .json(&json!({"model":group().alias(), "input":format!("-----BEGIN {}-----\nsynthetic-only\n-----END {}-----", "RSA PRIVATE KEY", "RSA PRIVATE KEY")}))
        .send().await?;
    drop(restore_security);
    assert_eq!(blocked.status(), StatusCode::BAD_REQUEST);
    let blocked: Value = blocked.json().await?;
    assert_eq!(blocked["error"]["code"], "sensitive_data_blocked");
    assert!(blocked.get("type").is_none());
    runtime.connections.revoke("claude");
    let response = client
        .post(format!(
            "http://127.0.0.1:{}/connections/claude/v1/responses",
            info.port
        ))
        .bearer_auth("synthetic-claude")
        .json(&json!({"model":group().alias(),"input":"synthetic"}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn gemini_agent_connection_maps_url_alias_and_strips_local_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::provider::{Provider, ProviderMeta};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/*path", post(move |uri: http::Uri, headers: http::HeaderMap, Json(body): Json<Value>| {
            let tx = tx.clone();
            async move {
                let _ = tx.send((uri,headers,body)).await;
                Json(json!({"candidates":[{"content":{"role":"model","parts":[{"text":"synthetic"}]}}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}}))
            }
        }))).await
    });
    let runtime = Arc::new(Runtime::default());
    let mut resource = menu(&format!(
        "http://{address}/v1beta/models/exact-model:generateContent"
    ));
    let session = Arc::make_mut(
        resource
            .groups
            .values_mut()
            .next()
            .ok_or("missing session")?,
    );
    let mut provider = Provider::with_id("fixture".into(), "Fixture".into(), json!({}), None);
    provider.meta = Some(ProviderMeta {
        api_format: Some("gemini_native".into()),
        ..Default::default()
    });
    session.targets[0].upstream = Some(provider);
    runtime.set_global(resource, 0)?;
    runtime
        .connections
        .install(&crate::services::agent_connections::Receipt {
            app: "gemini".into(),
            token: concat!("synthetic", "-local").into(),
            model: group().alias(),
            model_map: std::collections::BTreeMap::from([("wire-safe".into(), group().alias())]),
            visible_models: None,
            files: vec![],
            suspended: false,
            applied_at_ms: None,
        })?;
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
    let root = format!(
        "http://127.0.0.1:{}/connections/gemini/v1beta/models/wire-safe",
        info.port
    );
    let fixture = json!({"contents":[{"role":"user","parts":[{"text":"synthetic"}]}],"vendor_extension":{"preserve":true}});
    let models: Value = client
        .get(format!(
            "http://127.0.0.1:{}/connections/gemini/v1beta/models",
            info.port
        ))
        .header("x-goog-api-key", concat!("synthetic", "-local"))
        .send()
        .await?
        .json()
        .await?;
    // Internally constructed legacy menus have no published display catalog.
    assert!(models["models"].is_array());
    let count = client
        .post(format!("{root}:countTokens"))
        .header("x-goog-api-key", concat!("synthetic", "-local"))
        .json(&fixture)
        .send()
        .await?;
    assert_eq!(count.status(), StatusCode::OK);
    let count: Value = count.json().await?;
    assert!(count["totalTokens"].as_u64().is_some());
    let response = client
        .post(format!("{root}:generateContent"))
        .header("x-goog-api-key", "synthetic-local")
        .json(&fixture)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let _: Value = response.json().await?;
    let (uri, headers, body) = rx.recv().await.ok_or("missing request")?;
    assert_eq!(uri.path(), "/v1beta/models/exact-model:generateContent");
    assert_eq!(body, fixture);
    assert!(headers.get("x-goog-api-key").is_none());
    let response = client
        .post(format!("{root}:streamGenerateContent?alt=sse"))
        .header("x-goog-api-key", concat!("synthetic", "-local"))
        .json(&fixture)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await?;
    let (uri, _, _) = rx.recv().await.ok_or("missing stream request")?;
    assert_eq!(
        uri.path(),
        "/v1beta/models/exact-model:streamGenerateContent"
    );
    assert_eq!(uri.query(), Some("alt=sse"));
    for query in ["key=synthetic", "alt=sse", "unknown=true"] {
        assert_eq!(
            client
                .post(format!("{root}:generateContent?{query}"))
                .header("x-goog-api-key", "synthetic-local")
                .json(&fixture)
                .send()
                .await?
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        client
            .post(format!("{root}:generateContent"))
            .json(&fixture)
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn readable_native_catalog_ids_and_legacy_routes_remain_compatible(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(|Json(body): Json<Value>| async move { Json(json!({"id":"synthetic-reply","model":body["model"],"status":"completed","output":[]})) }))).await;
    });
    let runtime = Arc::new(Runtime::default());
    let label = "Synthetic readable · Supplier · VaryAI";
    let mut published = menu(&format!("http://{address}/responses"));
    published.models.insert(
        group().alias(),
        MenuModel {
            alias: group().alias(),
            label: label.into(),
            description: "Synthetic model",
            order: 0,
        },
    );
    runtime.set_global(published, 0)?;
    for app in ["pi", "hermes", "grokbuild", "kimi", "qwen", "zcode"] {
        runtime
            .connections
            .install(&crate::services::agent_connections::Receipt {
                app: app.into(),
                token: "synthetic-readable-token".into(),
                model: group().alias(),
                model_map: std::collections::BTreeMap::from([(label.into(), group().alias())]),
                visible_models: None,
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            })?;
    }
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(runtime);
    let info = proxy.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    for app in ["pi", "hermes", "grokbuild", "kimi", "qwen", "zcode"] {
        let url = format!("http://127.0.0.1:{}/connections/{app}/v1", info.port);
        let catalog: Value = client
            .get(format!("{url}/models"))
            .bearer_auth("synthetic-readable-token")
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(catalog["data"][0]["id"], label, "{app}");
        for model in [label.to_owned(), group().alias()] {
            let response = client
                .post(format!("{url}/responses"))
                .bearer_auth("synthetic-readable-token")
                .json(&json!({"model":model,"input":"Synthetic request"}))
                .send()
                .await?;
            assert_eq!(response.status(), StatusCode::OK, "{app}");
            assert_eq!(response.json::<Value>().await?["model"], "exact-model");
        }
    }
    proxy.stop().await?;
    task.abort();
    Ok(())
}
