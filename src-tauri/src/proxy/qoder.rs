//! Native Qoder official and third-party model routing.
use super::{
    qoder_usage::{Collector, Job},
    server::ProxyState,
    ProxyError,
};
use crate::{app_config::AppType, database::Database};
use axum::{
    body::{to_bytes, Body},
    extract::{Path, Request, State},
    http::{header, HeaderMap},
    response::Response,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
#[derive(Clone)]
pub(crate) struct OfficialState {
    pub db: Arc<Database>,
    pub upstream: String,
}
fn invalid() -> ProxyError {
    ProxyError::InvalidRequest("Invalid Qoder model request; nothing was forwarded".into())
}
async fn enabled(db: Arc<Database>) -> Result<(), ProxyError> {
    let enabled = tokio::task::spawn_blocking(move || {
        db.get_bool_flag(crate::services::qoder_proxy::ENABLED_KEY)
    })
    .await
    .map_err(|_| invalid())?
    .map_err(|_| invalid())?;
    if enabled {
        Ok(())
    } else {
        Err(ProxyError::ConfigError("Qoder routing is disabled".into()))
    }
}
pub(crate) async fn handle_custom(
    State(state): State<ProxyState>,
    Path((id, path)): Path<(String, String)>,
    request: Request,
) -> Result<Response, ProxyError> {
    enabled(state.db.clone()).await?;
    let suffix = path.strip_prefix("v1/").ok_or_else(invalid)?;
    let is_model = matches!(suffix, "chat/completions" | "responses" | "messages")
        && request.method() == http::Method::POST;
    if !is_model && !(suffix == "models" && request.method() == http::Method::GET) {
        return Err(invalid());
    }
    let db = state.db.clone();
    let key = id.clone();
    let provider = tokio::task::spawn_blocking(move || db.get_provider_by_id(&key, "qoder"))
        .await
        .map_err(|_| invalid())?
        .map_err(|_| invalid())?
        .filter(|p| p.category.as_deref() != Some("official"))
        .ok_or_else(invalid)?;
    crate::qoder_config::validate_provider(&provider.settings_config).map_err(|_| invalid())?;
    let supplied = if provider.settings_config["authType"] == "api-key" {
        request
            .headers()
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            == Some("varyai-local")
    } else {
        request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            == Some("Bearer varyai-local")
    };
    if !supplied {
        return Err(ProxyError::InvalidRequest(
            "Qoder managed credentials changed; reapply the configuration in Vary AI".into(),
        ));
    }
    let (mut parts, body) = request.into_parts();
    let bytes = to_bytes(body, 100 * 1024 * 1024)
        .await
        .map_err(|_| invalid())?;
    let bytes = super::handlers::decode_codex_request_body(&mut parts.headers, bytes)?;
    let mut payload = if is_model {
        serde_json::from_slice::<Value>(&bytes).map_err(|_| invalid())?
    } else {
        Value::Null
    };
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if is_model {
        let expected = match provider.settings_config["protocol"].as_str() {
            Some("openai") => "chat/completions",
            Some("openai-responses") => "responses",
            Some("anthropic") => "messages",
            _ => return Err(invalid()),
        };
        if suffix != expected
            || !provider.settings_config["models"]
                .as_array()
                .is_some_and(|ms| ms.iter().any(|m| m["model"].as_str() == Some(&model)))
        {
            return Err(invalid());
        }
        if let Some(blocked) = super::security::inspect_and_intercept_mut_with_headers(
            &AppType::Qoder,
            &mut payload,
            &parts.headers,
        )? {
            return Ok(blocked);
        }
    }
    let session = super::security::audit_session_id(&AppType::Qoder, &parts.headers, &payload);
    let mut target = url::Url::parse(
        provider.settings_config["baseUrl"]
            .as_str()
            .ok_or_else(invalid)?,
    )
    .map_err(|_| invalid())?;
    target.set_path(&format!("{}/{suffix}", target.path().trim_end_matches('/')));
    let key = provider.settings_config["apiKey"]
        .as_str()
        .ok_or_else(invalid)?;
    // Native login cookies and Cosy credentials never leave for third-party models.
    let mut headers = HeaderMap::new();
    for name in [
        "content-type",
        "accept",
        "user-agent",
        "anthropic-version",
        "anthropic-beta",
    ] {
        if let Some(value) = parts.headers.get(name) {
            headers.insert(http::HeaderName::from_static(name), value.clone());
        }
    }
    if provider.settings_config["authType"] == "api-key" {
        headers.insert(
            "x-api-key",
            http::HeaderValue::from_str(key).map_err(|_| invalid())?,
        );
        if !headers.contains_key("anthropic-version") {
            headers.insert(
                "anthropic-version",
                http::HeaderValue::from_static("2023-06-01"),
            );
        }
    } else {
        headers.insert(
            header::AUTHORIZATION,
            http::HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| invalid())?,
        );
    }
    if provider.settings_config["protocol"] == "anthropic"
        && !headers.contains_key("anthropic-version")
    {
        headers.insert(
            "anthropic-version",
            http::HeaderValue::from_static("2023-06-01"),
        );
    }
    headers.insert(
        header::ACCEPT_ENCODING,
        http::HeaderValue::from_static("identity"),
    );
    parts.headers = headers;
    let bytes = if is_model {
        serde_json::to_vec(&payload).map_err(|_| invalid())?
    } else {
        bytes.to_vec()
    };
    let job = is_model.then(|| Job {
        app_type: "qoder".into(),
        db: state.db.clone(),
        provider_id: id,
        request_model: model,
        session_id: session,
        usage: Default::default(),
        latency_ms: 0,
        status: 0,
        streaming: payload["stream"].as_bool().unwrap_or(false),
    });
    send(target.into(), parts, bytes, job).await
}
pub(crate) async fn handle_official(
    State(state): State<OfficialState>,
    request: Request,
) -> Result<Response, ProxyError> {
    enabled(state.db.clone()).await?;
    let is_model = request.uri().path() == "/algo/api/v2/service/pro/sse/agent_chat_generation";
    let target = format!(
        "{}{}",
        state.upstream.trim_end_matches('/'),
        request
            .uri()
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or("/")
    );
    let (mut parts, body) = request.into_parts();
    let bytes = to_bytes(body, 100 * 1024 * 1024)
        .await
        .map_err(|_| invalid())?;
    if !is_model {
        return send(target, parts, bytes.to_vec(), None).await;
    }
    let wire = super::handlers::decode_codex_request_body(&mut parts.headers, bytes)?;
    let decoded =
        super::qoder_codec::call(json!({"operation":"decode","body":STANDARD.encode(&wire)}))
            .await?;
    let mut payload = decoded
        .get("body")
        .cloned()
        .filter(Value::is_object)
        .ok_or_else(invalid)?;
    if !payload["messages"].is_array() || !payload["chat_context"].is_object() {
        return Err(invalid());
    }
    let session = super::security::audit_session_id(&AppType::Qoder, &parts.headers, &payload);
    let model = payload
        .pointer("/model_config/key")
        .and_then(Value::as_str)
        .unwrap_or("qoder-official")
        .to_string();
    let policy_db = state.db.clone();
    let policy_model = model.clone();
    let gateway_grant = match tokio::task::spawn_blocking(move || {
        policy_db.authorize_gateway_agent("qoder", &policy_model, chrono::Utc::now().timestamp())
    })
    .await
    {
        Ok(Ok(grant)) => grant,
        Ok(Err(err)) => return Ok(super::gateway::policy_error(err)),
        Err(_) => {
            return Ok(super::gateway::policy_error(
                super::gateway::GatewayError::Storage,
            ))
        }
    };
    let gateway_db = state.db.clone();
    if let Some(blocked) = super::security::inspect_and_intercept_mut_with_headers(
        &AppType::Qoder,
        &mut payload,
        &parts.headers,
    )? {
        return Ok(blocked);
    }
    let bytes = if decoded["body"] != payload {
        let headers = parts
            .headers
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str(), v)))
            .collect::<std::collections::HashMap<_, _>>();
        let encoded = super::qoder_codec::call(
            json!({"operation":"encode","body":payload,"headers":headers}),
        )
        .await?;
        parts.headers.insert(
            header::AUTHORIZATION,
            http::HeaderValue::from_str(encoded["authorization"].as_str().ok_or_else(invalid)?)
                .map_err(|_| invalid())?,
        );
        if let Some(date) = encoded["date"].as_str() {
            parts.headers.insert(
                "cosy-date",
                http::HeaderValue::from_str(date).map_err(|_| invalid())?,
            );
        }
        STANDARD
            .decode(encoded["body"].as_str().ok_or_else(invalid)?)
            .map_err(|_| invalid())?
    } else {
        wire.to_vec()
    };
    parts.headers.insert(
        header::ACCEPT_ENCODING,
        http::HeaderValue::from_static("identity"),
    );
    let job = Job {
        app_type: "qoder".into(),
        db: state.db,
        provider_id: "qoder-official".into(),
        request_model: model,
        session_id: session,
        usage: Default::default(),
        latency_ms: 0,
        status: 0,
        streaming: true,
    };
    let response = send(target, parts, bytes, Some(job)).await?;
    Ok(match gateway_grant {
        Some(grant) => super::gateway::meter_response(response, gateway_db, grant, true),
        None => response,
    })
}
pub(crate) async fn send(
    target: String,
    mut parts: http::request::Parts,
    bytes: Vec<u8>,
    mut job: Option<Job>,
) -> Result<Response, ProxyError> {
    let start = Instant::now();
    strip_hop_headers(&mut parts.headers);
    parts.headers.remove(header::CONTENT_LENGTH);
    let uri: http::Uri = target.parse().map_err(|_| invalid())?;
    let host = uri.authority().ok_or_else(invalid)?.as_str();
    parts.headers.insert(
        header::HOST,
        http::HeaderValue::from_str(host).map_err(|_| invalid())?,
    );
    let proxy = super::http_client::get_current_proxy_url();
    let response = super::hyper_client::send_request(
        uri,
        "Qoder model endpoint",
        parts.method,
        parts.headers,
        parts.extensions,
        bytes,
        Duration::from_secs(600),
        proxy.as_deref(),
    )
    .await
    .map_err(|_| ProxyError::ForwardFailed("Qoder upstream connection failed".into()))?;
    let status = response.status();
    let mut headers = response.headers().clone();
    strip_hop_headers(&mut headers);
    // Never follow redirects with native credentials or third-party API keys.
    if status.is_redirection() {
        return Err(ProxyError::ForwardFailed(
            "Qoder upstream returned a redirect; configure its final API URL".into(),
        ));
    }
    let mut upstream = response.bytes_stream();
    use futures::StreamExt;
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
        let mut collector = Collector::default();
        while let Some(chunk) = upstream.next().await {
            match chunk {
                Ok(bytes) => {
                    if job.is_some() {
                        collector.push(&bytes);
                    }
                    if tx.send(Ok::<_, std::io::Error>(bytes)).await.is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx
                        .send(Err(std::io::Error::other("Qoder upstream stream failed")))
                        .await;
                    break;
                }
            }
        }
        if let Some(mut item) = job.take() {
            collector.finish();
            item.usage = collector.usage;
            item.status = status.as_u16();
            item.latency_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
            super::qoder_usage::emit(item);
        }
    });
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    let mut result = Response::new(Body::from_stream(stream));
    *result.status_mut() = status;
    *result.headers_mut() = headers;
    Ok(result)
}
fn strip_hop_headers(headers: &mut HeaderMap) {
    let names = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(',').map(|v| v.trim().to_string()))
        .collect::<Vec<_>>();
    for name in names {
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
    use axum::{routing::post, Router};
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[tokio::test]
    async fn qoder_custom_models_use_saved_keys_and_keep_protocol_fields() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let app=Router::new().route("/v1/chat/completions",post(move|req:axum::extract::Request|{let count=count.clone();async move{
   assert_eq!(req.headers()["authorization"],"Bearer synthetic-api-key");assert!(!req.headers().contains_key("cookie"));assert!(!req.headers().contains_key("cosy-key"));
   let body:Value=serde_json::from_slice(&axum::body::to_bytes(req.into_body(),1024*1024).await.expect("body")).expect("json");
   assert_eq!(body["model"],"model-b");assert_eq!(body["messages"][0]["content"],"[REDACTED_PRIVATE_KEY]");assert_eq!(body["future"],json!({"keep":true}));count.fetch_add(1,Ordering::SeqCst);
   axum::Json(json!({"model":"model-b","usage":{"prompt_tokens":12,"completion_tokens":7},"choices":[]}))
  }}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("port");
        let addr = listener.local_addr().expect("addr");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let db = Arc::new(crate::database::Database::memory().expect("db"));
        db.set_setting(crate::services::qoder_proxy::ENABLED_KEY, "true")
            .expect("enable");
        let p = crate::provider::Provider::with_id(
            "api".into(),
            "API".into(),
            json!({"baseUrl":format!("http://{addr}/v1"),"apiKey":"synthetic-api-key","protocol":"openai","model":"model-a","models":[{"model":"model-a"},{"model":"model-b"}]}),
            None,
        );
        db.save_provider("qoder", &p).expect("save");
        let server = super::super::server::ProxyServer::new(
            crate::proxy::ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            db.clone(),
            None,
        );
        let info = server.start().await.expect("gateway");
        let url = format!(
            "http://127.0.0.1:{}/qoder/providers/api/v1/chat/completions",
            info.port
        );
        let client = reqwest::Client::new();
        let payload = json!({"model":"model-b","messages":[{"role":"user","content":"-----BEGIN RSA PRIVATE KEY----- SYNTHETIC_ONLY"}],"future":{"keep":true}});
        let response = client
            .post(&url)
            .header("authorization", "Bearer varyai-local")
            .header("cookie", "synthetic-official-cookie")
            .header("cosy-key", "synthetic-official-key")
            .header("x-session-id", "native-session")
            .json(&payload)
            .send()
            .await
            .expect("send");
        assert_eq!(response.status(), 200);
        let _ = response.bytes().await.expect("response");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let summary = db
                    .get_usage_summary(None, None, Some("qoder"), None, None)
                    .expect("usage summary");
                if summary.total_requests == 1 {
                    assert_eq!(summary.total_input_tokens, 12);
                    assert_eq!(summary.total_output_tokens, 7);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("queued usage persisted");
        let changed = client
            .post(&url)
            .header("authorization", "Bearer user-edited-key")
            .json(&payload)
            .send()
            .await
            .expect("changed native key");
        assert!(!changed.status().is_success());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        db.set_setting(crate::services::qoder_proxy::ENABLED_KEY, "false")
            .expect("off");
        let response = client.post(url).json(&payload).send().await.expect("send");
        assert!(!response.status().is_success());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        server.stop().await.expect("stop");
        task.abort();
    }
}

#[cfg(test)]
mod session_tests {
    #[test]
    fn qoder_audits_use_native_session_ids_with_a_client_prefix() {
        use crate::{app_config::AppType, proxy::security::audit_session_id};
        let mut headers = http::HeaderMap::new();
        let body = serde_json::json!({"session_id":"native-body-session"});
        assert_eq!(
            audit_session_id(&AppType::Qoder, &headers, &body).as_deref(),
            Some("qoder_native-body-session")
        );
        headers.insert(
            "x-session-id",
            "native-header-session".parse().expect("header"),
        );
        assert_eq!(
            audit_session_id(&AppType::Qoder, &headers, &body).as_deref(),
            Some("qoder_native-header-session")
        );
        headers.insert("x-session-id", "invalid session".parse().expect("header"));
        assert!(audit_session_id(&AppType::Qoder, &headers, &body).is_none());
    }
}
