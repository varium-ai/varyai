//! Explicit upstream overrides for clients with dedicated native routes.
use super::{
    forwarder::RequestForwarder, protocol_bridge::Protocol, qoder_usage::Job, server::ProxyState,
    ProxyError,
};
use crate::{app_config::AppType, provider::Provider};
use axum::{
    http::{header, request::Parts, Uri},
    response::Response,
};
use serde_json::{json, Value};
use std::time::Instant;

fn invalid() -> ProxyError {
    ProxyError::ConfigError("Invalid native upstream protocol configuration".into())
}

pub(super) async fn handle(
    axum::extract::State(state): axum::extract::State<ProxyState>,
    axum::extract::Path((app, id, _path)): axum::extract::Path<(String, String, String)>,
    request: axum::extract::Request,
) -> Result<Response, ProxyError> {
    let app_type = match app.as_str() {
        "hermes" => AppType::Hermes,
        "trae" => AppType::Trae,
        _ => return Err(invalid()),
    };
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        == Some("Bearer varyai-local")
        || request
            .headers()
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            == Some("varyai-local");
    if !supplied {
        return Err(ProxyError::AuthError(
            "Invalid local routing credential".into(),
        ));
    }
    let db = state.db.clone();
    let provider = tokio::task::spawn_blocking(move || db.get_provider_by_id(&id, &app))
        .await
        .map_err(|_| invalid())?
        .map_err(|_| invalid())?
        .filter(|p| p.category.as_deref() != Some("official"))
        .ok_or_else(invalid)?;
    if !needs_conversion(&provider, request.uri())? {
        return Err(invalid());
    }
    let (mut parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 100 * 1024 * 1024)
        .await
        .map_err(|_| invalid())?;
    let bytes = super::handlers::decode_codex_request_body(&mut parts.headers, bytes)?;
    let mut payload: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let model = payload["model"].as_str().ok_or_else(invalid)?.to_owned();
    if !provider.settings_config["models"]
        .as_array()
        .is_some_and(|models| {
            models
                .iter()
                .any(|m| m["model"] == model || m["id"] == model)
        })
    {
        return Err(invalid());
    }
    if let Some(blocked) = super::security::inspect_and_intercept_mut_with_headers(
        &app_type,
        &mut payload,
        &parts.headers,
    )? {
        return Ok(blocked);
    }
    let job = Job {
        app_type: app_type.as_str().into(),
        db: state.db.clone(),
        provider_id: provider.id.clone(),
        request_model: model,
        session_id: super::security::audit_session_id(&app_type, &parts.headers, &payload),
        usage: Default::default(),
        latency_ms: 0,
        status: 0,
        streaming: payload["stream"].as_bool().unwrap_or(false),
    };
    forward(&state, app_type, &provider, parts, payload, Some(job)).await
}

pub(super) fn needs_conversion(provider: &Provider, uri: &Uri) -> Result<bool, ProxyError> {
    let Some(format) = provider.meta.as_ref().and_then(|m| m.api_format.as_deref()) else {
        return Ok(false);
    };
    let output = Protocol::from_format(format).ok_or_else(invalid)?;
    let input = if uri.path().ends_with("/chat/completions") {
        Protocol::Chat
    } else if uri.path().ends_with("/responses") {
        Protocol::Responses
    } else if uri.path().ends_with("/messages") {
        Protocol::Anthropic
    } else {
        return Err(invalid());
    };
    Ok(input != output)
}

pub(super) async fn forward(
    state: &ProxyState,
    app: AppType,
    provider: &Provider,
    mut parts: Parts,
    payload: Value,
    job: Option<Job>,
) -> Result<Response, ProxyError> {
    let start = Instant::now();
    let config = &provider.settings_config;
    let base = config["baseUrl"]
        .as_str()
        .or(config["base_url"].as_str())
        .or(config["url"].as_str())
        .ok_or_else(invalid)?;
    let key = config["apiKey"]
        .as_str()
        .or(config["api_key"].as_str())
        .ok_or_else(invalid)?;
    let mut base = url::Url::parse(base).map_err(|_| invalid())?;
    let query = [base.query(), parts.uri.query()]
        .into_iter()
        .flatten()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("&");
    base.set_query(None);
    base.set_fragment(None);
    let path = base.path().trim_end_matches('/');
    let path = path
        .strip_suffix("/chat/completions")
        .unwrap_or(path)
        .to_string();
    base.set_path(&path);
    let mut upstream = provider.clone();
    let auth_field = if config["authType"] == "api-key" {
        "ANTHROPIC_API_KEY"
    } else {
        "ANTHROPIC_AUTH_TOKEN"
    };
    upstream.settings_config = json!({"env":{"ANTHROPIC_BASE_URL":base.as_str(),(auth_field):key}});
    // Client login credentials and vendor headers do not belong to another protocol.
    let mut headers = http::HeaderMap::new();
    for name in [
        "content-type",
        "accept",
        "user-agent",
        "x-session-id",
        "session_id",
    ] {
        if let Some(value) = parts.headers.get(name) {
            headers.insert(http::HeaderName::from_static(name), value.clone());
        }
    }
    headers.insert(
        header::ACCEPT_ENCODING,
        http::HeaderValue::from_static("identity"),
    );
    parts.headers = headers;
    let session = job.as_ref().and_then(|j| j.session_id.clone());
    let forwarder = RequestForwarder::new(
        state.provider_router.clone(),
        600,
        state.status.clone(),
        state.current_providers.clone(),
        state.gemini_shadow.clone(),
        state.codex_chat_history.clone(),
        state.failover_manager.clone(),
        state.app_handle.clone(),
        provider.id.clone(),
        session.clone().unwrap_or_default(),
        session.is_some(),
        600,
        600,
        Default::default(),
        Default::default(),
        Default::default(),
        0,
    );
    let endpoint = if parts.uri.path().ends_with("/chat/completions") {
        "/v1/chat/completions"
    } else if parts.uri.path().ends_with("/responses") {
        "/v1/responses"
    } else {
        "/v1/messages"
    };
    let endpoint = if query.is_empty() {
        endpoint.to_owned()
    } else {
        format!("{endpoint}?{query}")
    };
    let (response, _, model, _) = forwarder
        .forward(
            &app,
            &parts.method,
            &upstream,
            &endpoint,
            &payload,
            &parts.headers,
            &parts.extensions,
            &super::providers::ClaudeAdapter::new(),
        )
        .await?;
    let trace = parts
        .extensions
        .get::<super::gateway::GatewayUpstreamModel>()
        .cloned();
    if response.status().is_success() {
        if let Some(trace) = &trace {
            trace.record(model.as_deref());
        }
    }
    // Collect usage from the converted client response, using the original agent attribution.
    super::qoder::relay_response(response, job, start, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::Database,
        proxy::{server::ProxyServer, ProxyConfig},
        services::agent_clients::{Backup, Route},
    };
    use axum::{body::Body, http::StatusCode, response::IntoResponse, routing::post, Router};
    use std::sync::{
        atomic::{AtomicU16, AtomicUsize, Ordering},
        Arc,
    };

    #[tokio::test]
    #[serial_test::serial]
    async fn native_routes_convert_gemini_requests_and_json_or_streamed_tool_responses(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let calls = Arc::new(AtomicUsize::new(0));
        let status = Arc::new(AtomicU16::new(200));
        let observed = calls.clone();
        let control = status.clone();
        let upstream = Router::new().fallback(post(move |request: axum::extract::Request| {
            let calls = observed.clone();
            let status = control.clone();
            async move {
                let (parts, body) = request.into_parts();
                assert!(parts.uri.path().starts_with("/v1beta/models/fixture:"));
                let query = parts.uri.query().unwrap_or_default();
                assert!(query.contains("fixture-client="));
                if query.contains("fixture-client=workbuddy") {
                    assert!(query.contains("fixture-base=kept"));
                }
                assert_eq!(parts.headers.get("x-goog-api-key").and_then(|v| v.to_str().ok()), Some("synthetic-upstream"));
                for name in ["authorization", "cookie", "cosy-key", "x-codebuddy-token", "x-api-key"] {
                    assert!(!parts.headers.contains_key(name), "native credential header leaked: {name}");
                }
                let bytes = axum::body::to_bytes(body, 1024 * 1024).await.map_err(|_| StatusCode::BAD_REQUEST)?;
                let body: Value = serde_json::from_slice(&bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
                assert!(body["contents"].to_string().contains("fixture-question"));
                assert!(body["tools"].to_string().contains("read_file"));
                calls.fetch_add(1, Ordering::SeqCst);
                if status.load(Ordering::SeqCst) == 503 {
                    return Ok::<_, StatusCode>((StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({"error":{"message":"synthetic failure"}}))).into_response());
                }
                let response = json!({"candidates":[{"content":{"role":"model","parts":[{"text":"fixture-reply"},{"functionCall":{"id":"fixture-call","name":"read_file","args":{"path":"fixture.txt"}}}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":3,"totalTokenCount":10}});
                Ok(if parts.uri.path().ends_with(":streamGenerateContent") {
                    ([(header::CONTENT_TYPE, "text/event-stream")], Body::from(format!("data: {response}\n\n"))).into_response()
                } else {
                    axum::Json(response).into_response()
                })
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, upstream).await;
        });
        let db = Arc::new(Database::memory()?);
        db.set_setting(crate::services::qoder_proxy::ENABLED_KEY, "true")?;
        let server = ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            db.clone(),
            None,
        );
        server
            .set_workbuddy_upstream(Some("https://official.example/v2".into()))
            .await;
        let info = server.start().await?;
        let client = reqwest::Client::new();
        let dir = tempfile::tempdir()?;
        for app in [
            "qoder",
            "workbuddy",
            "kimi",
            "qwen",
            "zcode",
            "trae",
            "hermes",
        ] {
            for (protocol, suffix) in [
                ("openai", "chat/completions"),
                ("openai-responses", "responses"),
                ("anthropic", "messages"),
            ] {
                if app == "workbuddy" && protocol != "openai" {
                    continue;
                }
                let base = format!("http://{address}/v1beta");
                let mut provider = Provider::with_id(
                    "fixture".into(),
                    "Fixture".into(),
                    json!({"baseUrl":base,"base_url":base,"url":format!("{base}/chat/completions"),"apiKey":"synthetic-upstream","api_key":"synthetic-upstream","protocol":protocol,"authType":"bearer","model":"fixture","id":"fixture","name":"Fixture","models":[{"model":"fixture","id":"fixture"}]}),
                    None,
                );
                provider.meta = Some(serde_json::from_value(
                    json!({"apiFormat":"gemini_native"}),
                )?);
                if app == "workbuddy" {
                    provider.settings_config["baseUrl"] = json!("http://127.0.0.1:1/unused");
                    provider.settings_config["url"] =
                        json!(format!("{base}/chat/completions?fixture-base=kept"));
                }
                db.save_provider(app, &provider)?;
                let prefix = match app {
                    "qoder" => "/qoder/providers/fixture/v1".to_owned(),
                    "workbuddy" => "/workbuddy/providers/fixture".to_owned(),
                    "hermes" | "trae" => format!("/upstream/{app}/providers/fixture/v1"),
                    _ => {
                        db.set_setting(&crate::services::agent_clients::enabled_key(app), "true")?;
                        let prefix = format!("/agents/{app}/providers/fixture/v1");
                        db.save_live_backup(
                            app,
                            &serde_json::to_string(&Backup {
                                path: dir.path().join(app),
                                routes: vec![Route {
                                    id: "fixture".into(),
                                    pointer: String::new(),
                                    upstream: base,
                                    local: format!("http://127.0.0.1:{}{prefix}", info.port),
                                    models: vec!["fixture".into()],
                                    provider_id: "fixture".into(),
                                    created_provider: None,
                                }],
                            })?,
                        )
                        .await?;
                        prefix
                    }
                };
                let url = format!(
                    "http://127.0.0.1:{}{prefix}/{suffix}?fixture-client={app}",
                    info.port
                );
                for (stream, failure) in [(false, false), (true, false), (false, true)] {
                    status.store(if failure { 503 } else { 200 }, Ordering::SeqCst);
                    let schema = json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]});
                    let body = match protocol {
                        "anthropic" => {
                            json!({"model":"fixture","max_tokens":128,"stream":stream,"messages":[{"role":"user","content":"fixture-question"}],"tools":[{"name":"read_file","input_schema":schema}]})
                        }
                        "openai-responses" => {
                            json!({"model":"fixture","stream":stream,"input":"fixture-question","tools":[{"type":"function","name":"read_file","parameters":schema}]})
                        }
                        _ => {
                            json!({"model":"fixture","stream":stream,"messages":[{"role":"user","content":"fixture-question"}],"tools":[{"type":"function","function":{"name":"read_file","parameters":schema}}]})
                        }
                    };
                    let response = client
                        .post(&url)
                        .header("authorization", "Bearer varyai-local")
                        .header("cookie", "synthetic-native-session")
                        .header("cosy-key", "synthetic-native-key")
                        .header("x-codebuddy-token", "synthetic-native-token")
                        .json(&body)
                        .send()
                        .await?;
                    assert_eq!(
                        response.status().as_u16(),
                        if failure { 503 } else { 200 },
                        "{app}/{protocol}"
                    );
                    let response = response.text().await?;
                    if failure {
                        assert!(response.contains("synthetic failure"));
                        continue;
                    }
                    assert!(
                        response.contains("fixture-reply")
                            && response.contains("read_file")
                            && response.contains("fixture.txt"),
                        "{app}/{protocol}: response content lost"
                    );
                    if stream {
                        assert!(
                            response.contains(match protocol {
                                "anthropic" => "message_stop",
                                "openai-responses" => "response.completed",
                                _ => "[DONE]",
                            }),
                            "{app}/{protocol}: missing terminal event"
                        );
                    } else {
                        let response: Value = serde_json::from_str(&response)?;
                        assert_eq!(
                            response["usage"][if protocol == "openai" {
                                "completion_tokens"
                            } else {
                                "output_tokens"
                            }],
                            3,
                            "{app}/{protocol}: usage lost"
                        );
                    }
                }
            }
        }
        assert_eq!(calls.load(Ordering::SeqCst), 57);
        server.stop().await?;
        task.abort();
        Ok(())
    }
}
