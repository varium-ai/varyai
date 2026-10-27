//! WorkBuddy Chat Completions routing: native login or the selected API provider.
use super::{server::ProxyState, ProxyError};
use axum::{
    extract::{Path, Request, State},
    response::Response,
};

pub(crate) async fn handle(
    State(state): State<ProxyState>,
    request: Request,
) -> Result<Response, ProxyError> {
    let upstream = state.workbuddy_upstream.read().await.clone();
    let db = state.db.clone();
    let provider = tokio::task::spawn_blocking(move || {
        let Some(id) = db.get_current_provider("workbuddy")? else {
            return Ok(None);
        };
        let provider = db.get_provider_by_id(&id, "workbuddy")?.ok_or_else(|| {
            crate::error::AppError::Config("WorkBuddy selected provider is missing".into())
        })?;
        Ok::<_, crate::error::AppError>(Some(provider))
    })
    .await
    .map_err(|_| ProxyError::ConfigError("Cannot load WorkBuddy configuration".into()))?
    .map_err(|_| ProxyError::ConfigError("Cannot load WorkBuddy configuration".into()))?;
    forward_to_provider(Some(state), upstream, provider, request).await
}

pub(crate) async fn handle_custom(
    State(state): State<ProxyState>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response, ProxyError> {
    let upstream = state.workbuddy_upstream.read().await.clone();
    let db = state.db.clone();
    let provider = tokio::task::spawn_blocking(move || db.get_provider_by_id(&id, "workbuddy"))
        .await
        .map_err(|_| ProxyError::ConfigError("Cannot load WorkBuddy configuration".into()))?
        .map_err(|_| ProxyError::ConfigError("Cannot load WorkBuddy configuration".into()))?
        .filter(|p| p.category.as_deref() != Some("official"))
        .ok_or_else(|| ProxyError::ConfigError("WorkBuddy API configuration is missing".into()))?;
    forward_to_provider(Some(state), upstream, Some(provider), request).await
}

#[cfg(test)]
async fn forward(upstream: Option<String>, request: Request) -> Result<Response, ProxyError> {
    forward_to_provider(None, upstream, None, request).await
}

async fn forward_to_provider(
    state: Option<ProxyState>,
    upstream: Option<String>,
    provider: Option<crate::provider::Provider>,
    request: Request,
) -> Result<Response, ProxyError> {
    use axum::{
        body::{to_bytes, Body},
        http::header,
    };
    let upstream =
        upstream.ok_or_else(|| ProxyError::ConfigError("WorkBuddy routing is disabled".into()))?;
    let (mut parts, body) = request.into_parts();
    let bytes = to_bytes(body, 100 * 1024 * 1024)
        .await
        .map_err(|_| ProxyError::InvalidRequest("Cannot read WorkBuddy request".into()))?;
    let bytes = super::handlers::decode_codex_request_body(&mut parts.headers, bytes)?;
    let mut payload: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| ProxyError::InvalidRequest("Invalid WorkBuddy request JSON".into()))?;
    if let Some(blocked) = super::security::inspect_and_intercept_mut_with_headers(
        &crate::app_config::AppType::WorkBuddy,
        &mut payload,
        &parts.headers,
    )? {
        return Ok(blocked);
    }
    let is_api = provider
        .as_ref()
        .is_some_and(|p| p.category.as_deref() != Some("official"));
    let mut target = format!("{}/chat/completions", upstream.trim_end_matches('/'));
    if let Some(provider) = provider.filter(|p| p.category.as_deref() != Some("official")) {
        let models = crate::workbuddy_config::configured_models(&provider.settings_config)
            .map_err(|_| ProxyError::ConfigError("Invalid WorkBuddy API configuration".into()))?;
        let config = models
            .iter()
            .find(|model| model.get("id") == payload.get("model"))
            .or_else(|| models.first())
            .ok_or_else(|| ProxyError::ConfigError("Missing WorkBuddy model".into()))?;
        target = config
            .get("url")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ProxyError::ConfigError("Missing WorkBuddy API endpoint".into()))?
            .to_string();
        payload["model"] = config["id"].clone();
        let key = config
            .get("apiKey")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ProxyError::ConfigError("Missing WorkBuddy API key".into()))?;
        let auth = http::HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| ProxyError::ConfigError("Invalid WorkBuddy API key".into()))?;
        // Native account cookies and Tencent-specific credentials must never reach a third party.
        let mut headers = http::HeaderMap::new();
        for name in ["content-type", "accept", "accept-encoding", "user-agent"] {
            if let Some(value) = parts.headers.get(name) {
                headers.insert(http::HeaderName::from_static(name), value.clone());
            }
        }
        headers.insert(header::AUTHORIZATION, auth);
        parts.headers = headers;
        if super::native_protocol::needs_conversion(&provider, &parts.uri)? {
            let state = state
                .as_ref()
                .ok_or_else(|| ProxyError::ConfigError("WorkBuddy route unavailable".into()))?;
            let mut provider = provider.clone();
            provider.settings_config["baseUrl"] = serde_json::Value::String(target.clone());
            provider.settings_config["apiKey"] = config["apiKey"].clone();
            return super::native_protocol::forward(
                state,
                crate::app_config::AppType::WorkBuddy,
                &provider,
                parts,
                payload,
                None,
            )
            .await;
        }
    }
    let body = serde_json::to_vec(&payload)
        .map_err(|_| ProxyError::InvalidRequest("Cannot encode WorkBuddy request".into()))?;
    if let Some(query) = parts.uri.query() {
        target.push(if target.contains('?') { '&' } else { '?' });
        target.push_str(query);
    }
    strip_hop_headers(&mut parts.headers);
    parts.headers.remove(header::CONTENT_LENGTH);
    let proxy = super::http_client::get_current_proxy_url();
    let mut redirects = 0;
    let response = loop {
        let uri: http::Uri = target
            .parse()
            .map_err(|_| ProxyError::ConfigError("Invalid WorkBuddy upstream".into()))?;
        let host = uri
            .authority()
            .ok_or_else(|| ProxyError::ConfigError("Invalid WorkBuddy upstream host".into()))?;
        let host = http::HeaderValue::from_str(host.as_str())
            .map_err(|_| ProxyError::ConfigError("Invalid WorkBuddy upstream host".into()))?;
        parts.headers.insert(header::HOST, host);
        let response = super::hyper_client::send_request(
            uri,
            "WorkBuddy model endpoint",
            parts.method.clone(),
            parts.headers.clone(),
            parts.extensions.clone(),
            body.clone(),
            std::time::Duration::from_secs(600),
            proxy.as_deref(),
        )
        .await
        .map_err(|_| ProxyError::ForwardFailed("WorkBuddy upstream connection failed".into()))?;
        if !is_api || !response.status().is_redirection() {
            break response;
        }
        // Resolve API redirects here: returning a relative Location to WorkBuddy would
        // send its next request to another local route, possibly with the original payload.
        let redirect_error = || {
            ProxyError::ForwardFailed(
            "WorkBuddy API redirect is unsafe or exceeds the limit; configure the final Chat Completions URL".into(),
        )
        };
        if !matches!(response.status().as_u16(), 307 | 308) || redirects >= 3 {
            return Err(redirect_error());
        }
        let current = url::Url::parse(&target).map_err(|_| redirect_error())?;
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(redirect_error)?;
        let mut next = current.join(location).map_err(|_| redirect_error())?;
        if next.origin() != current.origin()
            || !next.username().is_empty()
            || next.password().is_some()
        {
            return Err(redirect_error());
        }
        next.set_fragment(None);
        target = next.into();
        redirects += 1;
    };
    let status = response.status();
    if status.is_success() {
        if let Some(trace) = parts
            .extensions
            .get::<super::gateway::GatewayUpstreamModel>()
        {
            trace.record(payload.get("model").and_then(serde_json::Value::as_str));
        }
    }
    let mut headers = response.headers().clone();
    strip_hop_headers(&mut headers);
    let mut result = Response::new(Body::from_stream(response.bytes_stream()));
    *result.status_mut() = status;
    *result.headers_mut() = headers;
    Ok(result)
}

fn strip_hop_headers(headers: &mut axum::http::HeaderMap) {
    let nominated: Vec<_> = headers
        .get_all("connection")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(',').map(|name| name.trim().to_owned()))
        .collect();
    for name in nominated {
        headers.remove(name);
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        headers.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        routing::post,
        Router,
    };
    use serde_json::{json, Value};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    async fn upstream(
        expected_messages: Value,
    ) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new().route(
            "/v2/chat/completions",
            post(move |req: Request| {
                let count = count.clone();
                let expected_messages = expected_messages.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    assert!(req.headers()["host"]
                        .to_str()
                        .expect("host")
                        .starts_with("127.0.0.1:"));
                    assert_ne!(req.headers()["host"], "localhost:15721");
                    assert_eq!(
                        req.headers()["authorization"],
                        "Bearer fake-official-session"
                    );
                    assert_eq!(req.headers()["x-codebuddy-test"], "preserve");
                    assert_eq!(
                        req.headers()["x-conversation-id"],
                        "native-workbuddy-session"
                    );
                    assert_eq!(req.uri().query(), Some("test=1"));
                    let body: Value = serde_json::from_slice(
                        &to_bytes(req.into_body(), 1024 * 1024).await.expect("body"),
                    )
                    .expect("JSON");
                    assert_eq!(body["model"], "official-fast");
                    assert_eq!(body["unknown_option"], json!({"keep":true}));
                    assert_eq!(body["messages"], expected_messages);
                    Response::builder()
                        .header("content-type", "text/event-stream")
                        .header("x-request-id", "wb-probe")
                        .body(Body::from("data: {\"choices\":[]}\n\ndata: [DONE]\n\n"))
                        .expect("response")
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral port");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock upstream");
        });
        (format!("http://{address}/v2"), calls, task)
    }

    fn request(messages: Value) -> Request {
        Request::builder()
            .method("POST")
            .uri("/workbuddy/v1/chat/completions?test=1")
            .extension(super::super::hyper_client::OriginalHeaderCases {
                cases: vec![
                    ("host".into(), b"Host".to_vec()),
                    ("authorization".into(), b"Authorization".to_vec()),
                ],
            })
            .header("host", "localhost:15721")
            .header("authorization", "Bearer fake-official-session")
            .header("x-codebuddy-test", "preserve")
            .header("x-conversation-id", "native-workbuddy-session")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"model":"official-fast","messages":messages,"unknown_option":{"keep":true}})
                    .to_string(),
            ))
            .expect("request")
    }

    #[tokio::test]
    async fn native_custom_route_uses_requested_provider_instead_of_current_official() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |req: Request| {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(req.headers()["authorization"], "Bearer synthetic-api-key");
                    assert!(!req.headers().contains_key("cookie"));
                    let payload: Value = serde_json::from_slice(
                        &to_bytes(req.into_body(), 1024 * 1024).await.expect("body"),
                    )
                    .expect("JSON");
                    assert_eq!(payload["model"], "custom-model");
                    assert_eq!(payload["messages"][0]["content"], "[REDACTED_PRIVATE_KEY]");
                    assert_eq!(payload["unknown_option"], json!({"keep":true}));
                    "data: [DONE]\n\n"
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let db = Arc::new(crate::database::Database::memory().expect("db"));
        let custom = crate::provider::Provider::with_id(
            "native-api".into(),
            "API".into(),
            json!({"id":"custom-model","name":"API","url":format!("http://{address}/v1/chat/completions"),"apiKey":"synthetic-api-key"}),
            None,
        );
        db.save_provider("workbuddy", &custom).expect("custom");
        let mut official = crate::provider::Provider::with_id(
            "official".into(),
            "Official".into(),
            json!({}),
            None,
        );
        official.category = Some("official".into());
        db.save_provider("workbuddy", &official).expect("official");
        db.set_current_provider("workbuddy", "official")
            .expect("current");
        let proxy = super::super::server::ProxyServer::new(
            crate::proxy::types::ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            db,
            None,
        );
        proxy
            .set_workbuddy_upstream(Some("http://127.0.0.1:1/v2".into()))
            .await;
        let info = proxy.start().await.expect("proxy");
        let endpoint = format!(
            "http://127.0.0.1:{}/workbuddy/providers/native-api/chat/completions",
            info.port
        );
        let response = reqwest::Client::new().post(&endpoint).header("cookie", "synthetic-native-session").json(&json!({"model":"ignored-client-model","messages":[{"role":"user","content":"-----BEGIN RSA PRIVATE KEY----- SYNTHETIC_ONLY"}],"unknown_option":{"keep":true}})).send().await.expect("request");
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.expect("stream"), "data: [DONE]\n\n");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // A stopped takeover cannot forward using saved credentials.
        proxy.set_workbuddy_upstream(None).await;
        let response = reqwest::Client::new()
            .post(endpoint)
            .json(&json!({"messages":[]}))
            .send()
            .await
            .expect("disabled request");
        assert!(!response.status().is_success());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        proxy.stop().await.expect("stop");
        task.abort();
    }

    #[tokio::test]
    async fn multi_model_provider_forwards_the_clients_selected_model() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|request: Request| async move {
                assert_eq!(
                    request.headers()["authorization"],
                    "Bearer synthetic-shared-key"
                );
                assert!(request.headers().get("cookie").is_none());
                let body: Value = serde_json::from_slice(
                    &to_bytes(request.into_body(), 1024 * 1024)
                        .await
                        .expect("body"),
                )
                .expect("JSON");
                assert_eq!(body["unknown"], json!({"keep": true}));
                axum::Json(body)
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("server");
        });
        let provider = crate::provider::Provider::with_id(
            "group".into(),
            "Gateway".into(),
            json!({
                "id":"model-a","name":"Gateway","url":format!("http://{address}/v1/chat/completions"),
                "apiKey":"synthetic-shared-key","models":[{"id":"model-a"},{"id":"model-b"}]
            }),
            None,
        );
        for (requested, expected) in [
            ("model-a", "model-a"),
            ("model-b", "model-b"),
            ("official-fast", "model-a"),
        ] {
            let request = Request::builder().method("POST").uri("/workbuddy/v1/chat/completions").header("cookie", "synthetic-official-session")
                .body(Body::from(json!({"model":requested,"messages":[{"role":"user","content":"hello"}],"unknown":{"keep":true}}).to_string())).expect("request");
            let response = forward_to_provider(
                None,
                Some("https://official.example/v2".into()),
                Some(provider.clone()),
                request,
            )
            .await
            .expect("forward");
            assert_eq!(response.status(), 200);
            let body: Value = serde_json::from_slice(
                &to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .expect("response"),
            )
            .expect("JSON");
            assert_eq!(body["model"], expected);
        }
        task.abort();
    }

    #[tokio::test]
    async fn upstream_redirects_stay_inside_workbuddy_and_preserve_redacted_post() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app = Router::new()
            .route(
                "/redirect/chat/completions",
                post(|| async {
                    Response::builder()
                        .status(308)
                        .header("location", "/final/chat/completions")
                        .body(Body::empty())
                        .expect("redirect")
                }),
            )
            .route(
                "/final/chat/completions",
                post(move |req: Request| {
                    let count = count.clone();
                    async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(req.headers()["authorization"], "Bearer synthetic-key");
                        let body: Value = serde_json::from_slice(
                            &to_bytes(req.into_body(), 1024 * 1024).await.expect("body"),
                        )
                        .expect("JSON");
                        assert_eq!(body["model"], "custom-model");
                        assert_eq!(body["messages"][0]["content"], "[REDACTED_PRIVATE_KEY]");
                        Response::new(Body::from("data: [DONE]\n\n"))
                    }
                }),
            )
            .route(
                "/cross/chat/completions",
                post(|| async {
                    Response::builder()
                        .status(307)
                        .header("location", "https://other.example/chat/completions")
                        .body(Body::empty())
                        .expect("redirect")
                }),
            )
            .route(
                "/loop/chat/completions",
                post(|| async {
                    Response::builder()
                        .status(308)
                        .header("location", "/loop/chat/completions")
                        .body(Body::empty())
                        .expect("redirect")
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("port");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let provider = |path: &str| {
            crate::provider::Provider::with_id(
                "api".into(),
                "API".into(),
                json!({"id":"custom-model","name":"API","url":format!("http://{address}/{path}/chat/completions"),"apiKey":"synthetic-key"}),
                None,
            )
        };
        let req = || {
            request(
                json!([{"role":"user","content":"-----BEGIN RSA PRIVATE KEY----- SYNTHETIC_ONLY"}]),
            )
        };
        let response = forward_to_provider(
            None,
            Some("https://official.example/v2".into()),
            Some(provider("redirect")),
            req(),
        )
        .await
        .expect("follow upstream redirect");
        assert_eq!(response.status(), 200);
        assert!(!response.headers().contains_key("location"));
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.expect("stream"),
            "data: [DONE]\n\n"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        for path in ["cross", "loop"] {
            let error = forward_to_provider(
                None,
                Some("https://official.example/v2".into()),
                Some(provider(path)),
                req(),
            )
            .await
            .expect_err("unsafe or excessive redirect");
            assert!(!error.to_string().contains("synthetic-key"));
        }
        task.abort();
    }

    #[tokio::test]
    async fn selected_api_uses_its_key_endpoint_and_model_without_native_account_headers() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let app = Router::new().route(
            "/custom/chat/completions",
            post(|req: Request| async move {
                assert_eq!(req.headers()["authorization"], "Bearer synthetic-api-key");
                assert!(!req.headers().contains_key("cookie"));
                assert!(!req.headers().contains_key("x-codebuddy-token"));
                assert!(!req.headers().contains_key("x-codebuddy-test"));
                let payload: Value = serde_json::from_slice(
                    &to_bytes(req.into_body(), 1024 * 1024).await.expect("body"),
                )
                .expect("JSON");
                assert_eq!(payload["model"], "custom-model");
                assert_eq!(payload["unknown_option"], json!({"keep":true}));
                assert_eq!(payload["messages"][0]["content"], "[REDACTED_PRIVATE_KEY]");
                Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from("data: [DONE]\n\n"))
                    .expect("response")
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral");
        let address = listener.local_addr().expect("address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let provider = crate::provider::Provider::with_id(
            "api".into(),
            "API".into(),
            json!({"id":"custom-model","name":"API","url":format!("http://{address}/custom/chat/completions"),"apiKey":"synthetic-api-key"}),
            None,
        );
        let mut req = request(
            json!([{"role":"user","content":"-----BEGIN RSA PRIVATE KEY----- SYNTHETIC_ONLY"}]),
        );
        req.headers_mut().insert(
            "cookie",
            "session=synthetic-cookie".parse().expect("cookie"),
        );
        req.headers_mut().insert(
            "x-codebuddy-token",
            "synthetic-native-token".parse().expect("token"),
        );
        let response = forward_to_provider(
            None,
            Some("http://127.0.0.1:1/v2".into()),
            Some(provider),
            req,
        )
        .await
        .expect("custom upstream");
        assert_eq!(response.status(), 200);
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.expect("stream"),
            "data: [DONE]\n\n"
        );
        task.abort();
    }

    #[tokio::test]
    async fn forwards_native_auth_parameters_query_and_stream_without_conversion() {
        let messages = json!([{"role":"user","content":"Reply OK only"}]);
        let (url, calls, task) = upstream(messages.clone()).await;
        let response = forward(Some(url), request(messages))
            .await
            .expect("forward");
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["x-request-id"], "wb-probe");
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.expect("stream"),
            "data: {\"choices\":[]}\n\ndata: [DONE]\n\n"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        task.abort();
    }

    #[tokio::test]
    async fn sensitive_history_is_redacted_before_forwarding_normal_followup() {
        let messages = json!([
            {"role":"user","content":"-----BEGIN RSA PRIVATE KEY----- SYNTHETIC_ONLY"},
            {"role":"user","content":"Reply OK only"}
        ]);
        let expected = json!([
            {"role":"user","content":"[REDACTED_PRIVATE_KEY]"},
            {"role":"user","content":"Reply OK only"}
        ]);
        let (url, calls, task) = upstream(expected).await;
        let response = forward(Some(url), request(messages))
            .await
            .expect("forward redacted history");
        assert_eq!(response.status(), 200);
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.expect("stream"),
            "data: {\"choices\":[]}\n\ndata: [DONE]\n\n"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        task.abort();
    }
}
