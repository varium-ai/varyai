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
