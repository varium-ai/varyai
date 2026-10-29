use super::*;
use crate::provider::Provider;
use crate::proxy::gateway::{KeyPolicy, Period};
use axum::{http::StatusCode, Json};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn concurrent_codex_requests_keep_independent_request_and_upstream_models(
) -> Result<(), Box<dyn std::error::Error>> {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
        middleware,
    };
    use tower::Service;
    let db = Arc::new(Database::memory()?);
    let key = db.create_gateway_key(KeyPolicy {
        name: "Synthetic attribution".into(),
        models: vec![],
        all_models: true,
        token_limit: None,
        period: Period::Never,
    })?;
    db.bind_gateway_agent("codex", Some(&key.key.id))?;
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    );
    let app = Router::new()
        .fallback(|request: Request<Body>| async move {
            let upstream = request
                .extensions()
                .get::<crate::proxy::gateway::GatewayUpstreamModel>()
                .cloned();
            let body = to_bytes(request.into_body(), 4096)
                .await
                .expect("fixture body");
            let body: serde_json::Value = serde_json::from_slice(&body).expect("fixture json");
            let model = body["model"].as_str().expect("fixture model");
            if let Some(upstream) = upstream {
                upstream.record(Some(model));
            }
            // Finish the second model first while both requests share the same agent and key.
            if model == "synthetic-glm" {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
            Json(json!({"model":model,"usage":{"input_tokens":body["tokens"],"output_tokens":0}}))
        })
        .layer(middleware::from_fn_with_state(
            proxy.state.clone(),
            crate::proxy::gateway::enforce_agent,
        ));
    let send = |mut app: Router, model: &'static str, tokens: u64| async move {
        let response = app
            .call(Request::post("/v1/responses").body(Body::from(
                json!({"model":model,"tokens":tokens}).to_string(),
            ))?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        to_bytes(response.into_body(), 4096).await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (first, second) = tokio::join!(
        send(app.clone(), "synthetic-glm", 17),
        send(app, "synthetic-qwen", 29)
    );
    first?;
    second?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if db
                .list_gateway_usage(Some(&key.key.id))
                .is_ok_and(|rows| rows.len() == 2)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    let rows = db.list_gateway_usage(Some(&key.key.id))?;
    for (model, tokens) in [("synthetic-glm", 17), ("synthetic-qwen", 29)] {
        let row = rows
            .iter()
            .find(|row| row.model == model)
            .ok_or("missing model attribution")?;
        assert_eq!(row.upstream_model.as_deref(), Some(model));
        assert_eq!(row.tokens, tokens);
        assert_eq!(row.requests, 1);
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn codex_anthropic_omits_disabled_strict_without_downgrading_strict_tools(
) -> Result<(), Box<dyn std::error::Error>> {
    let upstream = Router::new().route("/v1/messages", post(|Json(body): Json<serde_json::Value>| async move {
        if body["tools"][0].get("strict").is_some() {
            return (StatusCode::BAD_REQUEST, Json(json!({"error":{"type":"invalid_request_error","code":"unsupported_conversion","param":"tools[0].strict","message":"Synthetic gateway rejects strict field"}})));
        }
        assert_eq!(body["tools"][0]["name"], "read_file");
        assert_eq!(body["tools"][0]["input_schema"]["additionalProperties"], false);
        (StatusCode::OK, Json(json!({"id":"synthetic-message","type":"message","role":"assistant","model":"synthetic-model","content":[{"type":"text","text":"synthetic reply"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"output_tokens":2}})))
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, upstream).await;
    });
    let db = Arc::new(Database::memory()?);
    let mut provider = Provider::with_id(
        "codex-strict-fixture".into(),
        "Synthetic".into(),
        json!({"base_url":format!("http://{address}"),"auth":{"OPENAI_API_KEY":"synthetic-upstream"}}),
        None,
    );
    provider.meta = Some(serde_json::from_value(json!({"apiFormat":"anthropic"}))?);
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            max_retries: 0,
            ..Default::default()
        },
        db,
        None,
    );
    let info = proxy.start().await?;
    let client = reqwest::Client::new();
    for strict in [false, true] {
        for stream in [false, true] {
            let response = client.post(format!("http://127.0.0.1:{}/v1/responses", info.port))
                .json(&json!({
                    "model":"synthetic-model", "input":"synthetic question", "stream":stream,
                    "tools":[{"type":"function","name":"read_file","strict":strict,"parameters":{
                        "type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false
                    }}]
                })).send().await?;
            assert_eq!(
                response.status(),
                if strict {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::OK
                }
            );
            let body = response.text().await?;
            if strict {
                assert!(body.contains("unsupported_conversion"));
            } else {
                assert!(body.contains("synthetic reply"));
                if stream {
                    assert!(body.contains("response.completed"));
                }
            }
        }
    }
    proxy.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn gateway_rejects_before_upstream_and_meters_independently(
) -> Result<(), Box<dyn std::error::Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let upstream = Router::new().route("/v1/chat/completions", post(move |headers: axum::http::HeaderMap| {
        let observed = observed.clone();
        async move {
            assert_eq!(headers.get("authorization").and_then(|h| h.to_str().ok()), Some("Bearer synthetic-upstream"));
            observed.fetch_add(1, Ordering::SeqCst);
            Json(json!({"id":"synthetic","model":"allowed","choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":20,"completion_tokens":5}}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, upstream).await;
    });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "gateway-test".into(),
        "Synthetic".into(),
        json!({"base_url":format!("http://{addr}"),"auth":{"OPENAI_API_KEY":"synthetic-upstream"}}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let policy = KeyPolicy {
        name: "Synthetic".into(),
        models: vec!["allowed".into()],
        all_models: false,
        token_limit: Some(100),
        period: Period::Never,
    };
    let a = db.create_gateway_key(policy.clone())?;
    let b = db.create_gateway_key(KeyPolicy {
        name: "Other synthetic".into(),
        ..policy.clone()
    })?;
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    );
    let info = proxy.start().await?;
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/gateway/v1/chat/completions", info.port);
    for (secret, model, expected) in [
        ("invalid", "allowed", StatusCode::UNAUTHORIZED),
        (a.secret.as_str(), "denied", StatusCode::FORBIDDEN),
    ] {
        let response = client
            .post(&url)
            .bearer_auth(secret)
            .json(&json!({"model":model,"messages":[]}))
            .send()
            .await?;
        assert_eq!(response.status(), expected);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let response = client
        .post(&url)
        .bearer_auth(&a.secret)
        .json(&json!({"model":"allowed","messages":[{"role":"user","content":"synthetic"}]}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let _: serde_json::Value = response.json().await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if db.list_gateway_keys().is_ok_and(|keys| {
                keys.iter()
                    .any(|key| key.id == a.key.id && key.used_tokens == 25)
            }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    let mut lowered = policy;
    lowered.token_limit = Some(1);
    db.update_gateway_key(&a.key.id, lowered)?;
    let exhausted = client
        .post(&url)
        .bearer_auth(&a.secret)
        .json(&json!({"model":"allowed"}))
        .send()
        .await?;
    assert_eq!(exhausted.status(), StatusCode::PAYMENT_REQUIRED);
    assert_eq!(exhausted.headers()["x-should-retry"], "false");
    db.revoke_gateway_key(&b.key.id)?;
    let revoked = client
        .post(&url)
        .bearer_auth(&b.secret)
        .json(&json!({"model":"allowed"}))
        .send()
        .await?;
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    proxy.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
async fn every_managed_agent_enforces_and_accounts_without_replacing_native_auth(
) -> Result<(), Box<dyn std::error::Error>> {
    use axum::{
        body::{to_bytes, Body},
        http::Request,
        middleware,
    };
    use tower::Service;
    let db = Arc::new(Database::memory()?);
    let policy = KeyPolicy {
        name: "Shared fixture".into(),
        models: vec!["allowed".into()],
        all_models: false,
        token_limit: None,
        period: Period::Never,
    };
    let key = db.create_gateway_key(policy)?;
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let app = Router::new()
        .fallback(move |request: Request<Body>| {
            let calls = observed.clone();
            async move {
                assert_eq!(
                    request.headers()["authorization"],
                    "Bearer synthetic-native"
                );
                let path = request.uri().path().to_owned();
                let body = to_bytes(request.into_body(), 4096)
                    .await
                    .expect("fixture body");
                assert!(body.ends_with(b" "));
                calls.fetch_add(1, Ordering::SeqCst);
                Json(if path.ends_with("/messages") { json!({"usage":{"input_tokens":20,"output_tokens":5}}) } else if path.contains("/models/") { json!({"usageMetadata":{"promptTokenCount":20,"candidatesTokenCount":5,"totalTokenCount":25}}) } else { json!({"usage":{"prompt_tokens":20,"completion_tokens":5}}) })
            }
        })
        .layer(middleware::from_fn_with_state(
            proxy.state.clone(),
            crate::proxy::gateway::enforce_agent,
        ));
    let routes = [
        ("claude", "/claude/v1/messages"),
        ("claude-desktop", "/claude-desktop/v1/messages"),
        ("codex", "/v1/responses"),
        ("gemini", "/gemini/v1beta/models/allowed:generateContent"),
        ("grokbuild", "/grokbuild/v1/responses"),
        ("pi", "/pi/p/fixture/v1/chat/completions"),
        ("workbuddy", "/workbuddy/providers/fixture/chat/completions"),
        ("qoder", "/qoder/providers/fixture/v1/chat/completions"),
        (
            "doubaowork",
            "/agents/doubaowork/providers/fixture/v1/chat/completions",
        ),
        ("trae", "/agents/trae/providers/fixture/v1/chat/completions"),
        ("kimi", "/agents/kimi/providers/fixture/v1/chat/completions"),
        ("qwen", "/agents/qwen/providers/fixture/v1/messages"),
        ("zcode", "/agents/zcode/providers/fixture/v1/messages"),
    ];
    for (agent, path) in routes {
        db.bind_gateway_agent(agent, Some(&key.key.id))?;
        let blocked_path = path.replace("models/allowed:", "models/denied:");
        let blocked = app
            .clone()
            .call(Request::post(blocked_path).body(Body::from(r#"{"model":"denied"}"#))?)
            .await?;
        assert_eq!(blocked.status(), StatusCode::FORBIDDEN, "{agent}");
        let response = app
            .clone()
            .call(
                Request::post(path)
                    .header("authorization", "Bearer synthetic-native")
                    .body(Body::from("{\"model\":\"allowed\"} "))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{agent}");
        let _ = to_bytes(response.into_body(), 4096).await?;
    }
    assert_eq!(calls.load(Ordering::SeqCst), routes.len());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if db
                .list_gateway_usage(Some(&key.key.id))
                .is_ok_and(|rows| rows.len() == routes.len())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    let usage = db.list_gateway_usage(Some(&key.key.id))?;
    assert!(usage
        .iter()
        .all(|row| row.requests == 1 && row.tokens == 25 && row.unmetered == 0));
    db.update_gateway_key(
        &key.key.id,
        KeyPolicy {
            name: "Shared fixture".into(),
            models: vec!["allowed".into()],
            all_models: false,
            token_limit: Some(0),
            period: Period::Never,
        },
    )?;
    for (agent, path) in routes {
        let response = app
            .clone()
            .call(Request::post(path).body(Body::from(r#"{"model":"allowed"}"#))?)
            .await?;
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED, "{agent}");
        assert_eq!(response.headers()["x-should-retry"], "false", "{agent}");
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await?)?;
        assert_eq!(body["error"]["code"], "gateway_budget_exhausted");
        assert_eq!(body["error"]["type"], "insufficient_quota");
        assert!(body["error"]["resets_at"].is_null());
    }
    assert_eq!(calls.load(Ordering::SeqCst), routes.len());
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn desktop_native_route_uses_bound_policy_and_keeps_desktop_auth(
) -> Result<(), Box<dyn std::error::Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let upstream = Router::new().route("/v1/messages",post(move |headers: axum::http::HeaderMap, Json(body):Json<serde_json::Value>| {
        let calls = observed.clone();
        async move {
            if headers.keys().any(|name| name.as_str().starts_with("anthropic-client-")) {
                return (StatusCode::BAD_REQUEST, Json(json!({"error":{"type":"invalid_request_error","message":"Synthetic gateway rejects Desktop client metadata"}})));
            }
            assert_eq!(headers.get("anthropic-version").and_then(|v| v.to_str().ok()), Some("2023-06-01"));
            assert_eq!(headers.get("authorization").and_then(|v| v.to_str().ok()), Some("Bearer synthetic-upstream"));
            assert_eq!(headers.get("x-fixture-header").and_then(|v| v.to_str().ok()), Some("preserved"));
            assert!(["fixture-upstream", "fixture-upstream-v2"].iter().any(|m| body["model"] == *m));
            calls.fetch_add(1,Ordering::SeqCst);
            (StatusCode::OK, Json(json!({"id":"fixture-message","type":"message","role":"assistant","model":"fixture-upstream","content":[{"type":"text","text":"42"}],"stop_reason":"end_turn","usage":{"input_tokens":20,"output_tokens":5}})))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, upstream).await;
    });
    let db = Arc::new(Database::memory()?);
    let mut provider = Provider::with_id(
        "desktop-fixture".into(),
        "Synthetic Desktop".into(),
        json!({"env":{"ANTHROPIC_BASE_URL":format!("http://{address}"),"ANTHROPIC_AUTH_TOKEN":"synthetic-upstream"}}),
        None,
    );
    provider.meta = Some(serde_json::from_value(
        json!({"apiFormat":"anthropic","claudeDesktopMode":"proxy","claudeDesktopModelRoutes":{"claude-sonnet-fixture":{"model":"fixture-upstream"}}}),
    )?);
    db.save_provider("claude-desktop", &provider)?;
    db.set_current_provider("claude-desktop", &provider.id)?;
    db.set_setting("claude_desktop_gateway_token", "synthetic-native")?;
    let key = db.create_gateway_key(KeyPolicy {
        name: "Desktop budget".into(),
        models: vec!["claude-sonnet-fixture".into()],
        all_models: false,
        token_limit: Some(1),
        period: Period::Never,
    })?;
    db.bind_gateway_agent("claude-desktop", Some(&key.key.id))?;
    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db.clone(),
        None,
    );
    let info = proxy.start().await?;
    let url = format!("http://127.0.0.1:{}/claude-desktop/v1/messages", info.port);
    let client = reqwest::Client::new();
    let body = json!({"model":"claude-sonnet-fixture","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}]});
    let response = client
        .post(&url)
        .bearer_auth("synthetic-native")
        .header("anthropic-client-version", "synthetic-desktop-version")
        .header("anthropic-client-platform", "synthetic-desktop-platform")
        .header("anthropic-client-fixture", "synthetic-client-metadata")
        .header("anthropic-version", "2023-06-01")
        .header("x-fixture-header", "preserved")
        .json(&body)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let reply: serde_json::Value = response.json().await?;
    assert_eq!(reply["content"][0]["text"], "42");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if db.list_gateway_keys().is_ok_and(|k| k[0].used_tokens == 25) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    let denied = client
        .post(&url)
        .bearer_auth("synthetic-native")
        .json(&body)
        .send()
        .await?;
    assert_eq!(denied.status(), StatusCode::PAYMENT_REQUIRED);
    let mut policy = key.key.policy;
    policy.token_limit = Some(100);
    db.update_gateway_key(&key.key.id, policy)?;
    let invalid_auth = client
        .post(&url)
        .bearer_auth("wrong-native-token")
        .json(&body)
        .send()
        .await?;
    assert_ne!(invalid_auth.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    provider
        .meta
        .as_mut()
        .ok_or("missing meta")?
        .claude_desktop_model_routes
        .get_mut("claude-sonnet-fixture")
        .ok_or("missing route")?
        .model = "fixture-upstream-v2".into();
    db.save_provider("claude-desktop", &provider)?;
    let response = client
        .post(&url)
        .bearer_auth("synthetic-native")
        .header("anthropic-client-version", "synthetic-desktop-version")
        .header("anthropic-client-platform", "synthetic-desktop-platform")
        .header("anthropic-client-fixture", "synthetic-client-metadata")
        .header("anthropic-version", "2023-06-01")
        .header("x-fixture-header", "preserved")
        .json(&body)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await?;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let rows = db.list_gateway_usage(Some(&key.key.id)).unwrap_or_default();
            if rows.iter().any(|r| {
                r.upstream_model.as_deref() == Some("fixture-upstream-v2") && r.tokens == 25
            }) {
                assert!(rows
                    .iter()
                    .any(|r| r.upstream_model.as_deref() == Some("fixture-upstream")
                        && r.tokens == 25));
                assert!(rows.iter().all(|r| r.model == "claude-sonnet-fixture"));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    db.save_provider("claude", &provider)?;
    db.set_current_provider("claude", &provider.id)?;
    let response = client
        .post(format!("http://127.0.0.1:{}/v1/messages", info.port))
        .header("anthropic-client-version", "synthetic-code-version")
        .json(&json!({"model":"fixture-upstream","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}]}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error: serde_json::Value = response.json().await?;
    assert_eq!(
        error["error"]["message"],
        "Synthetic gateway rejects Desktop client metadata"
    );
    proxy.stop().await?;
    task.abort();
    Ok(())
}
