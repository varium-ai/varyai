//! Shared routes must use owned restoration before actual transport, including resent history.
use super::*;
use crate::{
    database::Database,
    proxy::{server::ProxyServer, ProxyConfig},
};
use axum::{routing::post, Json, Router};
use http::HeaderMap;

#[tokio::test]
#[serial_test::serial]
async fn official_native_requests_restore_without_a_routing_alias(
) -> Result<(), Box<dyn std::error::Error>> {
    struct Reset(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for Reset {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = Reset((*crate::proxy::security::get_security_runtime()).clone());
    let mut policy = crate::proxy::security::SecurityRuntimeConfig::default();
    policy.builtin_action_overrides.insert(
        "chinese_id_card".into(),
        crate::proxy::security::AuditAction::RedactRestore,
    );
    crate::proxy::security::runtime::set_security_runtime_for_test(policy);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/responses", post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let tx = tx.clone();
            async move {
                let text = body["input"].as_str().unwrap_or_default();
                let id = uuid::Uuid::new_v4().simple().to_string();
                let item = json!({"id":format!("msg_{id}"),"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]});
                let response = json!({"id":format!("resp_{id}"),"object":"response","status":"completed","output":[item],"model":"gpt-synthetic"});
                let stream = body["stream"] == true;
                let mode = headers.get("x-fixture-mode").and_then(|h| h.to_str().ok()).unwrap_or_default().to_owned();
                let _ = tx.send((headers, body)).await;
                if mode == "error" {
                    return (StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"Synthetic refusal 110101199001010015 Bearer synthetic-native-auth"}}))).into_response();
                }
                if mode == "redirect" {
                    return Response::builder().status(StatusCode::TEMPORARY_REDIRECT)
                        .header(header::LOCATION,"/redirect-target").body(Body::empty()).unwrap();
                }
                if stream {
                    let event = json!({"type":"response.completed","response":response});
                    Response::builder().header(header::CONTENT_TYPE,"text/event-stream")
                        .body(Body::from(format!("event: response.completed\ndata: {event}\n\n"))).unwrap()
                } else { Json(response).into_response() }
            }
        }))).await
    });
    let _origin = native_gateway::set_test_origin(format!("http://{address}"));
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    );
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let identity = "110101199001010015";
    for stream in [false, true] {
        for input in [
            format!("Synthetic identity: {identity}"),
            "Ordinary follow-up".to_owned(),
        ] {
            let response = client.post(format!("http://127.0.0.1:{}/backend-api/codex/responses", info.port))
                .bearer_auth("synthetic-native-auth").header("session_id",format!("synthetic-official-{stream}"))
                .header("chatgpt-account-id","synthetic-account")
                .json(&json!({"model":"gpt-synthetic","input":input,"stream":stream,"store":false,"reasoning":{"effort":"high"}}))
                .send().await?;
            assert_eq!(response.status(), StatusCode::OK, "stream={stream}");
            let text = response.text().await?;
            assert!(text.contains(&input));
            assert!(!text.contains("rredact-"));
            let (headers, sent) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await?
                .ok_or("missing upstream request")?;
            assert_eq!(
                headers[header::AUTHORIZATION],
                "Bearer synthetic-native-auth"
            );
            assert_eq!(headers["chatgpt-account-id"], "synthetic-account");
            assert_eq!(sent["reasoning"]["effort"], "high");
            assert_eq!(sent["model"], "gpt-synthetic");
            assert!(!sent.to_string().contains(identity));
        }
    }
    for mode in ["error", "redirect"] {
        let response = client.post(format!("http://127.0.0.1:{}/backend-api/codex/responses", info.port))
            .bearer_auth("synthetic-native-auth").header("x-fixture-mode",mode)
            .json(&json!({"model":"gpt-synthetic","input":format!("Synthetic identity: {identity}"),"stream":false}))
            .send().await?;
        let status = response.status();
        let text = response.text().await?;
        assert!(!status.is_success());
        if mode == "error" {
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
        assert!(
            !text.contains(identity)
                && !text.contains("synthetic-native-auth")
                && !text.contains("rredact-")
        );
        let (_, sent) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await?
            .ok_or("missing negative-case request")?;
        assert!(!sent.to_string().contains(identity));
        assert!(
            rx.try_recv().is_err(),
            "redirects must not cause an unbound send"
        );
    }
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn shared_routes_restore_contacts_and_keep_normal_followups_usable(
) -> Result<(), Box<dyn std::error::Error>> {
    struct Reset(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for Reset {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = Reset((*crate::proxy::security::get_security_runtime()).clone());
    let mut policy = crate::proxy::security::SecurityRuntimeConfig::default();
    policy.builtin_action_overrides.insert(
        "chinese_phone_number".into(),
        crate::proxy::security::AuditAction::RedactRestore,
    );
    policy.builtin_action_overrides.insert(
        "chinese_id_card".into(),
        crate::proxy::security::AuditAction::RedactRestore,
    );
    crate::proxy::security::runtime::set_security_runtime_for_test(policy.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let sequence = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/*path", post(move |Json(body): Json<Value>| {
        let tx = tx.clone(); let sequence = sequence.clone();
        async move {
            if body["tools"].to_string().contains("\"$ref\"") {
                return (StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"Synthetic Grok schema reference rejection"}}))).into_response();
            }
            let id = format!("synthetic-restored-{}", sequence.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            let content = &body["messages"].as_array().and_then(|items| items.last()).map(|item| &item["content"]);
            let text = content.and_then(|content| content.as_str().or_else(|| content[0]["text"].as_str())).unwrap_or("missing text").to_owned();
            let stream = body["stream"] == true;
            let _ = tx.send(body).await;
            if stream {
                let start = json!({"id":id,"object":"chat.completion.chunk","model":"exact-model","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]});
                let end = json!({"id":id,"object":"chat.completion.chunk","model":"exact-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}});
                Response::builder().header(header::CONTENT_TYPE,"text/event-stream").body(Body::from(format!("data: {start}\n\ndata: {end}\n\ndata: [DONE]\n\n"))).unwrap()
            } else {
                Json(json!({"id":id,"object":"chat.completion","model":"exact-model","choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})).into_response()
            }
        }
    }))).await
    });
    let runtime = Arc::new(Runtime::default());
    let mut menu = super::global_tests::menu(&format!("http://{address}/v1/chat/completions"));
    let session = Arc::make_mut(menu.groups.values_mut().next().ok_or("missing session")?);
    session.group.levels.clear();
    session.group.routing = crate::services::intent_routing::RoutingStrategy::Ordered;
    session.targets[0].model = "grok-4.5".into();
    session.group.members[0].model_id = "grok-4.5".into();
    let mut upstream = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        json!({"base_url":format!("http://{address}/v1"),"auth":{"OPENAI_API_KEY":"synthetic-upstream"},"apiFormat":"openai_chat","config":format!("model_provider='fixture'\n[model_providers.fixture]\nbase_url='http://{address}/v1'\nwire_api='chat'\n")}),
        None,
    );
    upstream.meta = Some(crate::provider::ProviderMeta {
        api_format: Some("openai_chat".into()),
        ..Default::default()
    });
    session.targets[0].upstream = Some(upstream);
    let alias = session.group.alias();
    menu.models.insert(
        "fixture".into(),
        MenuModel {
            alias: alias.clone(),
            label: "Synthetic model".into(),
            description: "VaryAI routing group",
            order: 0,
        },
    );
    let native_wire = "synthetic-native/model";
    let native_home = tempfile::tempdir()?;
    runtime
        .native
        .prepare_checkpoints(&native_home.path().join("checkpoint.key"))?;
    runtime.native.publish_menu(
        Arc::new(menu.clone()),
        json!({"models":[{"slug":native_wire}]}),
        std::collections::BTreeMap::from([(native_wire.into(), alias.clone())]),
    )?;
    runtime.set_menu(Arc::new(menu.clone()))?;
    runtime.set_global(menu, 0)?;
    runtime
        .connections
        .install(&crate::services::agent_connections::Receipt {
            app: "workbuddy-ai".into(),
            token: "synthetic-workbuddy".into(),
            model: alias.clone(),
            model_map: Default::default(),
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
    .with_intent_runtime(runtime.clone());
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let phone = ["138", "0000", "1234"].concat();
    let identity = "110101199001010015";
    let contact = format!("Synthetic contacts: {phone} {identity}");
    for (path, token, root) in [
        ("/intent/v1/responses", "synthetic-local-token", "input"),
        (
            "/backend-api/codex/responses",
            "synthetic-native-token",
            "input",
        ),
        (
            "/connections/workbuddy-ai/v1/chat/completions",
            "synthetic-workbuddy",
            "messages",
        ),
    ] {
        for stream in [false, true] {
            for followup in [false, true] {
                let mut messages = vec![json!({"role":"user","content":contact})];
                if followup {
                    messages.push(json!({"role":"user","content":"Synthetic safe followup"}));
                }
                let model = if path.starts_with("/backend-api/") {
                    native_wire
                } else {
                    alias.as_str()
                };
                let mut body = json!({"model":model,"stream":stream,(root):messages});
                if root == "messages" {
                    let properties: serde_json::Map<String, Value> = (0..12)
                        .map(|index| (format!("argument_{index}"), json!({
                            "type":"string", "description":"Synthetic tool argument description"
                        })))
                        .collect();
                    body["tools"] = Value::Array((0..20).map(|index| json!({
                        "type":"function","function":{
                            "name":format!("synthetic_tool_{index}"),
                            "description":"Synthetic tool definition",
                            "parameters":{"type":"object","properties":properties},"strict":false
                        }
                    })).collect());
                } else {
                    body["tools"] = json!([{"type":"function","name":"synthetic_lookup","parameters":{
                        "type":"object","properties":{"record":{"anyOf":[{"$ref":"#/$defs/Record"},{"type":"null"}]}},
                        "$defs":{"Record":{"type":"object","properties":{"label":{"type":"string"}},"required":["label"],"additionalProperties":false}}
                    }}]);
                }
                let response = client
                    .post(format!("http://127.0.0.1:{}{path}", info.port))
                    .bearer_auth(token)
                    .header("version", "0.167.0")
                    .json(&body)
                    .send()
                    .await?;
                let status = response.status();
                let text = response.text().await?;
                let error: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                let category = match error["error"]["code"].as_str() {
                    Some("dlp_restore_unsupported") => "restore_unsupported",
                    Some("dlp_redaction_failed") => "redaction_failed",
                    Some("dlp_context_unavailable") => "context_unavailable",
                    Some("routing_request_conversion_failed") => "request_conversion_failed",
                    Some("routing_response_conversion_failed") => "response_conversion_failed",
                    Some("routing_conversion_misconfigured") => "conversion_misconfigured",
                    Some("routing_native_unavailable") => "native_unavailable",
                    _ => "other",
                };
                assert_eq!(
                    status,
                    StatusCode::OK,
                    "path={path}, root={root}, stream={stream}, followup={followup}, category={category}"
                );
                assert!(
                    text.contains(if followup {
                        "Synthetic safe followup"
                    } else {
                        &contact
                    }),
                    "client must receive restored text or its safe followup"
                );
                let sent = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                    .await?
                    .ok_or("missing upstream request")?;
                assert!(
                    !sent.to_string().contains(&phone) && !sent.to_string().contains(identity),
                    "upstream must never receive contact plaintext: root={root}, stream={stream}, followup={followup}"
                );
                assert!(sent.to_string().contains("rredact-cn_phone-"));
                assert!(sent.to_string().contains("rredact-cn_id_card-"));
                assert_eq!(sent["model"], "grok-4.5");
                if root == "messages" {
                    assert_eq!(sent["tools"], body["tools"]);
                } else {
                    assert_eq!(
                        sent["tools"][0]["function"]["parameters"]["properties"]["record"]["anyOf"]
                            [0],
                        body["tools"][0]["parameters"]["$defs"]["Record"]
                    );
                }
            }
        }
    }
    let native = json!({"model":alias,"messages":[{"role":"user","content":contact}]});
    let admission = crate::proxy::security::http_restoration::admit_routed_http_request(
        &crate::proxy::security::http_restoration::RestorationGateway::new_default(),
        &crate::app_config::AppType::WorkBuddyAi,
        Protocol::Chat,
        "/v1/chat/completions",
        &mut native.clone(),
        &http::HeaderMap::new(),
    )?;
    if let crate::proxy::security::http_restoration::RestorationAdmission::Admitted(request) =
        admission
    {
        assert!(matches!(
            request.routing_projection(&native),
            Err(crate::proxy::security::http_restoration::DlpForwardError::RestoreUnsupported)
        ));
        let canonical = crate::proxy::group_forward::canonical_request(
            Protocol::Chat,
            &native,
            "/v1/chat/completions",
        )?;
        let (projection, _owner) = request
            .routing_projection(&canonical)
            .map_err(|_| "routing projection failed")?;
        assert!(
            !projection.to_string().contains(&phone) && !projection.to_string().contains(identity),
            "classifier input must be hidden"
        );
        assert!(
            native.to_string().contains(&phone),
            "routing projection must not replace the actual-send source"
        );
    } else {
        return Err("missing routed restoration authority".into());
    }
    let mut unsupported = (*runtime.global_snapshot()?.ok_or("missing global menu")?).clone();
    let session = Arc::make_mut(
        unsupported
            .groups
            .values_mut()
            .next()
            .ok_or("missing target")?,
    );
    session.targets[0]
        .upstream
        .as_mut()
        .ok_or("missing provider")?
        .meta
        .as_mut()
        .ok_or("missing format")?
        .api_format = Some("openai_responses".into());
    runtime.set_global(unsupported, 0)?;
    let denied = client
        .post(format!(
            "http://127.0.0.1:{}/connections/workbuddy-ai/v1/chat/completions",
            info.port
        ))
        .bearer_auth("synthetic-workbuddy")
        .json(&native)
        .send()
        .await?;
    assert_eq!(
        denied.status(),
        StatusCode::BAD_REQUEST,
        "unproven cross-protocol restoration must remain refused"
    );
    assert!(
        rx.try_recv().is_err(),
        "unsupported restoration must not reach upstream"
    );
    policy.builtin_action_overrides.insert(
        "chinese_phone_number".into(),
        crate::proxy::security::AuditAction::Block,
    );
    crate::proxy::security::runtime::set_security_runtime_for_test(policy);
    let denied = client
        .post(format!(
            "http://127.0.0.1:{}/connections/workbuddy-ai/v1/chat/completions",
            info.port
        ))
        .bearer_auth("synthetic-workbuddy")
        .json(&native)
        .send()
        .await?;
    assert_eq!(denied.status(), StatusCode::BAD_REQUEST);
    let blocked: Value = denied.json().await?;
    assert_eq!(blocked["error"]["code"], "dlp_blocked");
    assert!(
        rx.try_recv().is_err(),
        "explicit blocks must never reach upstream"
    );
    server.stop().await?;
    task.abort();
    Ok(())
}
