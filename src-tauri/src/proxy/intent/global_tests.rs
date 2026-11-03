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

fn group() -> RoutingGroup {
    serde_json::from_value(json!({"id":"global-fixture","name":"Synthetic global","revision":0,"client":"codex","enabled":true,
        "members":[{"id":"a","providerId":"fixture","modelId":"exact-model"}],"defaultMemberId":"a","fallbackMemberIds":[],
        "intentRecognitionEnabled":false,"classifierTarget":null,"classificationBudgetMs":2000,"rules":[],"levels":["low","max"]})).unwrap()
}
fn menu(url: &str) -> Menu {
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
async fn agent_connections_share_global_routes_without_agent_owned_providers(
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, Router::new().route("/responses", post(move |headers: http::HeaderMap, Json(body): Json<Value>| {
            let tx = tx.clone(); async move { let _ = tx.send((headers, body)).await; Json(json!({"id":"synthetic-response", "object":"response", "output":[], "usage":{"input_tokens":1,"output_tokens":1}})) }
        }))).await;
    });
    let runtime = Arc::new(Runtime::default());
    let mut shared = menu(&format!("http://{address}/responses"));
    let session = Arc::make_mut(shared.groups.values_mut().next().ok_or("missing fixture")?);
    session.targets[0].effort = crate::services::intent_routing::effort::Capability::parse(
        &json!({"reasoningLevels":["low","high"]}),
    );
    runtime.set_global(shared, 0)?;
    for app in crate::services::agent_connections::APPS
        .iter()
        .copied()
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
        let response = client
            .post(format!("{root}/responses"))
            .bearer_auth(format!("synthetic-{app}"))
            .json(&json!({"model":if app == "claude-desktop" { "desktop-hidden-wire".to_owned() } else { group().alias() }, "input":"synthetic", "reasoning":{"effort":"max","summary":"auto"}}))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let (headers, body) = rx.recv().await.ok_or("missing request")?;
        assert_eq!(body["model"], "exact-model");
        assert_eq!(body["reasoning"]["effort"], "high", "{app}");
        assert_eq!(body["reasoning"]["summary"], "auto", "{app}");
        assert_ne!(
            headers.get("authorization").and_then(|h| h.to_str().ok()),
            Some(format!("Bearer synthetic-{app}").as_str())
        );
    }
    struct RestoreSecurity(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for RestoreSecurity {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let restore_security =
        RestoreSecurity((*crate::proxy::security::get_security_runtime()).clone());
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
