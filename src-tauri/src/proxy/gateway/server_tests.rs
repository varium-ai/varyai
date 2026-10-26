use super::*;
use crate::provider::Provider;
use crate::proxy::gateway::{KeyPolicy, Period};
use axum::{http::StatusCode, Json};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    let b = db.create_gateway_key(policy.clone())?;
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
    assert_eq!(exhausted.status(), StatusCode::TOO_MANY_REQUESTS);
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
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn desktop_native_route_uses_bound_policy_and_keeps_desktop_auth(
) -> Result<(), Box<dyn std::error::Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let upstream = Router::new().route("/v1/messages",post(move |Json(body):Json<serde_json::Value>| {
        let calls = observed.clone();
        async move {
            assert_eq!(body["model"],"fixture-upstream");
            calls.fetch_add(1,Ordering::SeqCst);
            Json(json!({"id":"fixture-message","type":"message","role":"assistant","model":"fixture-upstream","content":[{"type":"text","text":"42"}],"stop_reason":"end_turn","usage":{"input_tokens":20,"output_tokens":5}}))
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
    assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
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
    let response = client
        .post(&url)
        .bearer_auth("synthetic-native")
        .json(&body)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.bytes().await?;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    proxy.stop().await?;
    task.abort();
    Ok(())
}
