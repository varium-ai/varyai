//! Native Pi requests: a provider scope and original model select one upstream.
mod json;
mod usage;
use super::server::ProxyState;
use crate::{
    app_config::AppType,
    services::pi_routing::catalog::{Protocol, Target},
};
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    response::Response,
};
use http::{header, HeaderMap, StatusCode};
use serde_json::{json, Value};
use std::{
    net::SocketAddr,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn error(protocol: Protocol, status: StatusCode, code: &'static str) -> Response {
    let body = match protocol {
        Protocol::Anthropic => {
            json!({"type":"error","error":{"type":"invalid_request_error","message":code}})
        }
        Protocol::Gemini => {
            json!({"error":{"code":status.as_u16(),"status":"INVALID_ARGUMENT","message":code}})
        }
        _ => json!({"error":{"type":"invalid_request_error","code":code,"message":code}}),
    };
    let mut response = Response::new(Body::from(body.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    response
}
fn route(path: &str) -> Option<(Protocol, Option<&str>, bool)> {
    match path {
        "/pi/v1/chat/completions" => Some((Protocol::Chat, None, false)),
        "/pi/v1/responses" => Some((Protocol::Responses, None, false)),
        "/pi/v1/messages" => Some((Protocol::Anthropic, None, false)),
        _ => {
            let model = path.strip_prefix("/pi/v1beta/models/")?;
            let (id, streaming) = if let Some(id) = model.strip_suffix(":streamGenerateContent") {
                (id, true)
            } else {
                (model.strip_suffix(":generateContent")?, false)
            };
            if id.is_empty() || id.contains(['&', '?', '#']) {
                return None;
            }
            Some((Protocol::Gemini, Some(id), streaming))
        }
    }
}
fn authentic(headers: &HeaderMap, token: &str) -> bool {
    let mut credentials = Vec::new();
    for name in ["authorization", "x-api-key", "x-goog-api-key"] {
        for value in headers.get_all(name) {
            let Ok(text) = value.to_str() else {
                return false;
            };
            credentials.push(if name == "authorization" {
                text.strip_prefix("Bearer ").unwrap_or("")
            } else {
                text
            });
        }
    }
    let [provided] = credentials.as_slice() else {
        return false;
    };
    let mismatch = provided
        .as_bytes()
        .iter()
        .zip(token.bytes())
        .fold(0u8, |v, (a, b)| v | (a ^ b));
    provided.len() == token.len() && !token.is_empty() && mismatch == 0
}
fn endpoint(target: &Target, streaming: bool) -> Result<url::Url, ()> {
    let mut url = target.base_url.clone();
    let path = url.path().trim_end_matches('/').to_string();
    match target.protocol {
        Protocol::Chat => url.set_path(&format!("{path}/chat/completions")),
        Protocol::Responses => url.set_path(&format!("{path}/responses")),
        Protocol::Anthropic => url.set_path(&format!("{path}/v1/messages")),
        Protocol::Gemini => {
            url.set_path(&format!("{path}/models"));
            url.path_segments_mut().map_err(|_| ())?.push(&format!(
                "{}:{}",
                target
                    .model_id
                    .strip_prefix("models/")
                    .unwrap_or(&target.model_id),
                if streaming {
                    "streamGenerateContent"
                } else {
                    "generateContent"
                }
            ));
            if streaming {
                url.query_pairs_mut().append_pair("alt", "sse");
            }
        }
    }
    Ok(url)
}
pub(crate) async fn handle(State(state): State<ProxyState>, req: Request) -> Response {
    let original_path = req.uri().path().to_string();
    let (scope, path) = if let Some(scoped) = original_path.strip_prefix("/pi/p/") {
        let Some((scope, suffix)) = scoped.split_once('/') else {
            return error(Protocol::Chat, StatusCode::NOT_FOUND, "pi_route_not_found");
        };
        (Some(scope), format!("/pi/{suffix}"))
    } else {
        (None, original_path.clone())
    };
    let Some((protocol, path_alias, gemini_stream)) = route(&path) else {
        return error(Protocol::Chat, StatusCode::NOT_FOUND, "pi_route_not_found");
    };
    let fail = |status, code| error(protocol, status, code);
    if req.method() != http::Method::POST {
        return fail(StatusCode::METHOD_NOT_ALLOWED, "pi_post_required");
    }
    if !req
        .extensions()
        .get::<SocketAddr>()
        .is_some_and(|peer| peer.ip().is_loopback())
        || req.headers().contains_key("origin")
        || req.headers().contains_key("sec-fetch-site")
    {
        return fail(StatusCode::FORBIDDEN, "pi_local_clients_only");
    }
    let catalog = match state.pi_runtime.catalog.read() {
        Ok(snapshot) => snapshot.clone(),
        Err(_) => None,
    };
    let Some(catalog) = catalog else {
        return fail(StatusCode::SERVICE_UNAVAILABLE, "pi_routing_unavailable");
    };
    let token = catalog.node["apiKey"].as_str().unwrap_or_default();
    if !authentic(req.headers(), token) {
        return fail(StatusCode::UNAUTHORIZED, "pi_local_token_required");
    }
    // Public SDK query options only; never forward client query credentials.
    let query = req.uri().query().unwrap_or_default();
    if !(query.is_empty()
        || (protocol == Protocol::Gemini && query == "alt=sse")
        || (protocol == Protocol::Anthropic && query == "beta=true"))
    {
        return fail(StatusCode::BAD_REQUEST, "pi_unsupported_query");
    }
    let anthropic_beta = protocol == Protocol::Anthropic && query == "beta=true";
    let start = Instant::now();
    let (parts, body) = req.into_parts();
    let bytes = match to_bytes(body, 32 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::PAYLOAD_TOO_LARGE, "pi_body_too_large"),
    };
    let original = match json::parse(&bytes) {
        Ok(v) if v.is_object() => v,
        _ => return fail(StatusCode::BAD_REQUEST, "pi_invalid_json"),
    };
    let model = path_alias
        .map(|id| {
            url::form_urlencoded::parse(format!("id={}", id.replace('+', "%2B")).as_bytes())
                .next()
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default()
        })
        .or_else(|| original["model"].as_str().map(str::to_string));
    let target = model.as_ref().and_then(|id| {
        if let Some(scope) = scope {
            catalog.targets.values().find(|t| {
                t.model_id == *id
                    && crate::services::pi_routing::catalog::alias(&t.provider_id, "") == scope
            })
        } else {
            catalog.targets.get(id)
        }
    });
    let Some(target) = target else {
        return fail(StatusCode::NOT_FOUND, "pi_model_unavailable");
    };
    if target.protocol != protocol {
        return fail(StatusCode::BAD_REQUEST, "pi_protocol_mismatch");
    }
    if protocol == Protocol::Gemini && original.get("model").is_some() {
        return fail(StatusCode::BAD_REQUEST, "pi_unexpected_model_field");
    }
    state
        .pi_runtime
        .last_request
        .store(chrono::Utc::now().timestamp_millis(), Ordering::Relaxed);
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut value = original.clone();
    let wire = match protocol {
        Protocol::Anthropic => AppType::Claude,
        Protocol::Gemini => AppType::Gemini,
        _ => AppType::Codex,
    };
    match super::security::inspect_with_context(
        &AppType::Pi,
        &wire,
        &target.provider_id,
        Some(&request_id),
        &mut value,
        &parts.headers,
    ) {
        Ok(Some(blocked)) => return blocked,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "pi_inspection_failed"),
        _ => {}
    }
    let session_id = super::security::audit_session_id(&AppType::Pi, &parts.headers, &original);
    if protocol != Protocol::Gemini {
        value["model"] = Value::String(target.model_id.clone());
    }
    let payload = match json::encode_redacted(&bytes, &original, &value) {
        Ok(v) => v,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "pi_unsafe_payload_edit"),
    };
    let mut url = match endpoint(target, gemini_stream) {
        Ok(u) => u,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "pi_invalid_endpoint"),
    };
    if anthropic_beta {
        url.query_pairs_mut().append_pair("beta", "true");
    }
    let uri: http::Uri = match url.as_str().parse() {
        Ok(u) => u,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "pi_invalid_endpoint"),
    };
    let mut headers = parts.headers;
    strip_hop_headers(&mut headers);
    for name in [
        "authorization",
        "x-api-key",
        "x-goog-api-key",
        "cookie",
        "content-length",
        "host",
        "accept-encoding",
        "proxy-authorization",
    ] {
        headers.remove(name);
    }
    let private = headers
        .keys()
        .filter(|k| k.as_str().starts_with("x-vary-"))
        .cloned()
        .collect::<Vec<_>>();
    for name in private {
        headers.remove(name);
    }
    for (name, value) in &target.headers {
        headers.insert(name, value.clone());
    }
    if !headers.contains_key(header::CONTENT_TYPE) {
        headers.insert(
            header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
    }
    if protocol == Protocol::Anthropic && !headers.contains_key("anthropic-version") {
        headers.insert(
            "anthropic-version",
            http::HeaderValue::from_static("2023-06-01"),
        );
    }
    if let Some(host) = uri
        .authority()
        .and_then(|a| http::HeaderValue::from_str(a.as_str()).ok())
    {
        headers.insert(header::HOST, host);
    }
    let proxy = super::http_client::get_current_proxy_url();
    let response = match super::hyper_client::send_request(
        uri,
        "Pi upstream",
        http::Method::POST,
        headers,
        http::Extensions::new(),
        payload,
        Duration::from_secs(600),
        proxy.as_deref(),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => return fail(StatusCode::BAD_GATEWAY, "pi_upstream_connection_failed"),
    };
    let status = response.status();
    if status.is_redirection() {
        return fail(StatusCode::BAD_GATEWAY, "pi_upstream_redirect_rejected");
    }
    // Do not echo upstream error bodies: gateways may include credentials/request payloads.
    if !status.is_success() {
        return fail(status, "pi_upstream_error");
    }
    let mut response_headers = response.headers().clone();
    strip_hop_headers(&mut response_headers);
    response_headers.remove("set-cookie");
    let streaming = gemini_stream || original["stream"].as_bool() == Some(true);
    let job = usage::Job {
        db: state.db.clone(),
        provider_id: target.provider_id.clone(),
        request_model: target.model_id.clone(),
        request_id,
        session_id,
        usage: Default::default(),
        latency_ms: 0,
        status: status.as_u16(),
        streaming,
    };
    use super::tool_observation::parsers::{
        PROTOCOL_ANTHROPIC, PROTOCOL_GEMINI, PROTOCOL_OPENAI_CHAT, PROTOCOL_OPENAI_RESPONSES,
    };
    use super::tool_observation::{
        channel::{OutputQuality, Transport},
        mount,
    };
    let response_protocol = match protocol {
        Protocol::Chat => PROTOCOL_OPENAI_CHAT,
        Protocol::Responses => PROTOCOL_OPENAI_RESPONSES,
        Protocol::Anthropic => PROTOCOL_ANTHROPIC,
        Protocol::Gemini => PROTOCOL_GEMINI,
    };
    let metadata = mount::metadata(
        response_protocol,
        "pi",
        if streaming {
            Transport::Sse
        } else {
            Transport::Json
        },
        OutputQuality::Native,
        mount::collect_tool_schemas(&value, "pi"),
    );
    let mut collector = usage::Collector::new(protocol);
    let mut upstream = response.bytes_stream();
    use futures::StreamExt;
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let runtime = state.pi_runtime.clone();
    // Native identities are accounted from Pi's session usage, which already
    // imports both direct and routed messages. Legacy aliases remain gateway-only.
    let gateway_accounting = scope.is_none();
    tokio::spawn(async move {
        let mut complete = true;
        let mut tap = if streaming {
            mount::start_stream(metadata.clone())
        } else {
            None
        };
        let mut json_bytes = Some(Vec::new());
        loop {
            let next = tokio::select! { _ = tx.closed() => { complete=false; break; }, next = upstream.next() => next };
            let Some(chunk) = next else {
                break;
            };
            match chunk {
                Ok(bytes) => {
                    collector.push(&bytes);
                    if let Some(tap) = tap.as_mut() {
                        tap.push(&bytes);
                    }
                    if !streaming {
                        if let Some(buffer) = json_bytes.as_mut() {
                            if buffer.len() + bytes.len() <= 1024 * 1024 {
                                buffer.extend_from_slice(&bytes);
                            } else {
                                json_bytes = None;
                            }
                        }
                    }
                    if tx.send(Ok::<_, std::io::Error>(bytes)).await.is_err() {
                        complete = false;
                        break;
                    }
                }
                Err(_) => {
                    complete = false;
                    let _ = tx
                        .send(Err(std::io::Error::other("Pi upstream stream failed")))
                        .await;
                    break;
                }
            }
        }
        if !complete {
            if let Some(tap) = tap.as_ref() {
                tap.fail();
            }
        } else if !streaming {
            if let Some(bytes) = json_bytes {
                mount::observe_non_streaming(&bytes.into(), metadata);
            }
        }
        collector.finish();
        if !complete || !collector.observed {
            runtime
                .incomplete_usage_count
                .fetch_add(1, Ordering::Relaxed);
        }
        if collector.observed {
            let mut job = job;
            job.usage = collector.usage;
            job.latency_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
            if !complete {
                job.status = 499;
            }
            if gateway_accounting {
                usage::emit(job);
            }
        } else {
            log::debug!("[Pi] usage unavailable or incomplete");
        }
    });
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    let mut result = Response::new(Body::from_stream(stream));
    *result.status_mut() = status;
    *result.headers_mut() = response_headers;
    result
}
fn strip_hop_headers(headers: &mut HeaderMap) {
    let names = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(',').map(|s| s.trim().to_string()))
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
mod tests;
