//! Codex native authentication contracts from Magpie 4869d0c.

use super::super::{native_gateway, Runtime};
use crate::{
    database::Database,
    proxy::{server::ProxyServer, ProxyConfig},
};
use axum::{
    body::{Body, Bytes},
    extract::Request,
    response::Response,
    Router,
};
use http::{header, HeaderMap, StatusCode};
use std::sync::{Arc, Mutex};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct RestoreSecurity(crate::proxy::security::SecurityRuntimeConfig);
impl Drop for RestoreSecurity {
    fn drop(&mut self) {
        crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
    }
}

fn security() -> RestoreSecurity {
    let previous = RestoreSecurity((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    previous
}

#[tokio::test]
#[serial_test::serial]
async fn magpie_native_relay_uses_each_requests_credentials_without_a_binding() -> Result {
    let _security = security();
    let seen = Arc::new(Mutex::new(Vec::<(HeaderMap, Bytes)>::new()));
    let observed = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = Router::new().fallback(move |request: Request| {
        let observed = observed.clone();
        async move {
            let (parts, body) = request.into_parts();
            let body = axum::body::to_bytes(body, 1024 * 1024)
                .await
                .unwrap_or_default();
            observed
                .lock()
                .expect("synthetic observations")
                .push((parts.headers.clone(), body.clone()));
            let status = if parts.headers.contains_key(header::AUTHORIZATION) {
                StatusCode::CREATED
            } else {
                StatusCode::UNAUTHORIZED
            };
            Response::builder()
                .status(status)
                .header("x-upstream-fixture", "preserved")
                .body(Body::from(body))
                .expect("synthetic response")
        }
    });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let _origin = native_gateway::set_test_origin(format!("http://{address}/backend-api/codex"));
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(Arc::new(Runtime::default()));
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let raw = Bytes::from_static(
        br#"{"model":"synthetic-native","input":"synthetic greeting","opaque":9007199254740993}"#,
    );
    for (token, workspace) in [
        ("synthetic-first", "synthetic-workspace-a"),
        ("synthetic-rotated", "synthetic-workspace-a"),
        ("synthetic-other-account", "synthetic-workspace-b"),
    ] {
        let response = client
            .post(format!(
                "http://127.0.0.1:{}/backend-api/codex/responses",
                info.port
            ))
            .bearer_auth(token)
            .header("chatgpt-account-id", workspace)
            .header("version", "0.200.0")
            .body(raw.clone())
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["x-upstream-fixture"], "preserved");
        assert_eq!(response.bytes().await?, raw);
    }
    let response = client
        .post(format!(
            "http://127.0.0.1:{}/backend-api/codex/responses",
            info.port
        ))
        .body(raw.clone())
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["x-upstream-fixture"], "preserved");
    assert_eq!(response.bytes().await?, raw);
    {
        let seen = seen.lock().expect("synthetic observations");
        assert_eq!(seen.len(), 4);
        assert_eq!(seen[0].0[header::AUTHORIZATION], "Bearer synthetic-first");
        assert_eq!(seen[1].0[header::AUTHORIZATION], "Bearer synthetic-rotated");
        assert_eq!(seen[2].0["chatgpt-account-id"], "synthetic-workspace-b");
        assert!(!seen[3].0.contains_key(header::AUTHORIZATION));
    }
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn magpie_native_model_list_exposes_refreshable_upstream_401() -> Result {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = Router::new().fallback(|| async {
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"error":{"code":"token_expired","message":"Synthetic expired sign-in"}}"#,
            ))
            .expect("synthetic response")
    });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let _origin = native_gateway::set_test_origin(format!("http://{address}/backend-api/codex"));
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(Arc::new(Runtime::default()));
    let info = server.start().await?;
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(format!(
            "http://127.0.0.1:{}/backend-api/codex/models?client_version=0.200.0",
            info.port
        ))
        .bearer_auth("synthetic-expired")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = response.json().await?;
    assert_eq!(body["error"]["code"], "token_expired");
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn magpie_native_catalog_revocation_does_not_hide_managed_models_or_cache_a_failed_sign_in(
) -> Result {
    use std::collections::{BTreeMap, HashMap};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = count.clone();
    let app = Router::new().fallback(move |request: Request| {
        observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        async move {
            let revoked = request.headers().get(header::AUTHORIZATION).is_some_and(|h| h == "Bearer synthetic-revoked");
            if revoked {
                Response::builder().status(StatusCode::UNAUTHORIZED).header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"error":{"code":"token_revoked","message":"Synthetic revoked sign-in"}}"#)).expect("synthetic response")
            } else {
                Response::builder().header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"models":[{"slug":"synthetic-native","priority":1}]}"#)).expect("synthetic response")
            }
        }
    });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let _origin = native_gateway::set_test_origin(format!("http://{address}/backend-api/codex"));
    let runtime = Arc::new(Runtime::default());
    runtime.native.publish_menu(
        Arc::new(super::super::Menu {
            token: "synthetic-menu".into(),
            models: HashMap::new(),
            default_model: None,
            omitted_entries: 0,
            context_settings: Default::default(),
            groups: HashMap::new(),
        }),
        serde_json::json!({"models":[{"slug":"vary-auto/synthetic"}]}),
        BTreeMap::new(),
    )?;
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
    let url = format!(
        "http://127.0.0.1:{}/backend-api/codex/models?client_version=0.200.0",
        info.port
    );
    let revoked = client
        .get(&url)
        .bearer_auth("synthetic-revoked")
        .header("chatgpt-account-id", "synthetic-workspace")
        .send()
        .await?;
    assert_eq!(revoked.status(), StatusCode::OK);
    assert_eq!(
        revoked.json::<serde_json::Value>().await?["models"][0]["slug"],
        "vary-auto/synthetic"
    );
    let renewed = client
        .get(&url)
        .bearer_auth("synthetic-renewed")
        .header("chatgpt-account-id", "synthetic-workspace")
        .send()
        .await?;
    assert_eq!(renewed.status(), StatusCode::OK);
    let models = renewed.json::<serde_json::Value>().await?;
    assert_eq!(models["models"][0]["slug"], "synthetic-native");
    assert_eq!(models["models"][1]["slug"], "vary-auto/synthetic");
    assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 2);
    server.stop().await?;
    task.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn magpie_native_catalog_shares_a_fetch_across_clients_and_token_rotation() -> Result {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let app = Router::new().fallback(move || {
        observed.fetch_add(1, Ordering::Relaxed);
        async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            axum::Json(serde_json::json!({"models":[{"slug":"synthetic-native"}]}))
        }
    });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let _origin = native_gateway::set_test_origin(format!("http://{address}/backend-api/codex"));
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(Arc::new(Runtime::default()));
    let info = server.start().await?;
    let client = reqwest::Client::builder().no_proxy().build()?;
    let url = format!(
        "http://127.0.0.1:{}/backend-api/codex/models?client_version=0.200.0",
        info.port
    );
    let asks = (0..12).map(|i| {
        client
            .get(&url)
            .bearer_auth(format!("synthetic-token-{i}"))
            .header("chatgpt-account-id", "synthetic-shared-workspace")
            .send()
    });
    for response in futures::future::join_all(asks).await {
        let response = response?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<serde_json::Value>().await?["models"][0]["slug"],
            "synthetic-native"
        );
    }
    assert_eq!(count.load(Ordering::Relaxed), 1);
    server.stop().await?;
    task.abort();
    Ok(())
}

#[test]
fn magpie_native_discovery_treats_incomplete_auth_as_unknown() -> Result {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let auth = dir.path().join("auth.json");
    std::fs::write(&config, "")?;
    assert_eq!(super::signed_in(&config), Some(false));
    for incomplete in ["", "{", "{\"tokens\":", "null", "[]"] {
        std::fs::write(&auth, incomplete)?;
        assert_eq!(super::signed_in(&config), None);
    }
    std::fs::write(&auth, r#"{"tokens":{"access_token":"synthetic-token"}}"#)?;
    assert_eq!(super::signed_in(&config), Some(true));
    std::fs::write(&config, "forced_login_method='api'\n")?;
    assert_eq!(super::signed_in(&config), Some(false));
    std::fs::write(&config, "cli_auth_credentials_store='ephemeral'\n")?;
    assert_eq!(super::signed_in(&config), Some(false));
    std::fs::write(&config, "cli_auth_credentials_store='keyring'\n")?;
    assert_eq!(super::signed_in(&config), None);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn magpie_native_stream_outlives_the_old_lease_without_reconciliation() -> Result {
    let _security = security();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = Router::new().fallback(|| async {
        let stream = async_stream::stream! {
            yield Ok::<_, std::io::Error>(Bytes::from_static(b"data: synthetic-first\n\n"));
            tokio::time::sleep(std::time::Duration::from_millis(5500)).await;
            yield Ok(Bytes::from_static(b"data: synthetic-final\n\n"));
        };
        Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(stream))
            .expect("synthetic response")
    });
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let _origin = native_gateway::set_test_origin(format!("http://{address}/backend-api/codex"));
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        Arc::new(Database::memory()?),
        None,
    )
    .with_intent_runtime(Arc::new(Runtime::default()));
    let info = server.start().await?;
    let response = reqwest::Client::builder().no_proxy().build()?.post(format!("http://127.0.0.1:{}/backend-api/codex/responses", info.port))
        .bearer_auth("synthetic-current-token").json(&serde_json::json!({"model":"synthetic-native","input":"synthetic greeting","stream":true})).send().await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body = tokio::time::timeout(std::time::Duration::from_secs(10), response.text()).await??;
    assert_eq!(body, "data: synthetic-first\n\ndata: synthetic-final\n\n");
    server.stop().await?;
    task.abort();
    Ok(())
}
