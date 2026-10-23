use super::*;
use crate::proxy::server::ProxyState;
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;

fn error(err: GatewayError) -> Response {
    let (status, code, reset) = match err {
        GatewayError::Unauthorized => (StatusCode::UNAUTHORIZED, "invalid_gateway_key", None),
        GatewayError::Forbidden => (StatusCode::FORBIDDEN, "model_not_allowed", None),
        GatewayError::Exhausted(reset) => (
            StatusCode::TOO_MANY_REQUESTS,
            "gateway_budget_exhausted",
            reset,
        ),
        GatewayError::Storage => (
            StatusCode::SERVICE_UNAVAILABLE,
            "gateway_policy_unavailable",
            None,
        ),
    };
    (
        status,
        Json(json!({"error":{"code":code,"message":err.to_string(),"resets_at":reset}})),
    )
        .into_response()
}

pub async fn handle(State(state): State<ProxyState>, request: Request) -> Response {
    let path = request
        .uri()
        .path()
        .strip_prefix("/gateway")
        .unwrap_or("")
        .to_owned();
    if !matches!(
        path.as_str(),
        "/v1/messages" | "/v1/responses" | "/v1/chat/completions"
    ) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let (mut parts, body) = request.into_parts();
    let secret = parts
        .headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .or_else(|| parts.headers.get("x-api-key").and_then(|h| h.to_str().ok()))
        .unwrap_or("")
        .to_owned();
    let bytes = match to_bytes(body, 200 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let model = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|v| v.get("model").and_then(Value::as_str).map(str::to_owned));
    let db = state.db.clone();
    let grant = match tokio::task::spawn_blocking(move || {
        db.authorize_gateway_key(
            &secret,
            model.as_deref().unwrap_or(""),
            Utc::now().timestamp(),
        )
    })
    .await
    {
        Ok(Ok(grant)) => grant,
        Ok(Err(err)) => return error(err),
        Err(_) => return error(GatewayError::Storage),
    };
    for name in [
        "authorization",
        "x-api-key",
        "x-goog-api-key",
        "proxy-authorization",
    ] {
        parts.headers.remove(name);
    }
    let endpoint = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or(parts.uri.path())
        .strip_prefix("/gateway")
        .unwrap_or(&path);
    parts.uri = match endpoint.parse::<Uri>() {
        Ok(uri) => uri,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let request = Request::from_parts(parts, Body::from(bytes));
    let response = match path.as_str() {
        "/v1/messages" => {
            crate::proxy::handlers::handle_messages(State(state.clone()), request).await
        }
        "/v1/chat/completions" => {
            crate::proxy::handlers::handle_chat_completions(State(state.clone()), request).await
        }
        _ => crate::proxy::handlers::handle_responses(State(state.clone()), request).await,
    }
    .unwrap_or_else(IntoResponse::into_response);
    meter(response, state.db, grant, path != "/v1/messages")
}

struct Meter {
    db: Arc<Database>,
    grant: Grant,
    inclusive: bool,
    sse: bool,
    buffer: Vec<u8>,
    events: Vec<Value>,
    overflow: bool,
}
impl Meter {
    fn observe(&mut self, bytes: &[u8]) {
        if self.overflow {
            return;
        }
        if self.buffer.len() + bytes.len() > 1024 * 1024 {
            self.buffer.clear();
            self.overflow = true;
            return;
        }
        self.buffer.extend_from_slice(bytes);
        if self.sse {
            while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
                let line: Vec<_> = self.buffer.drain(..=end).collect();
                if let Some(data) = line.strip_prefix(b"data:") {
                    if let Ok(mut value) = serde_json::from_slice::<Value>(data) {
                        // Keep only usage-bearing envelopes; never retain generated text.
                        if let Some(obj) = value.as_object_mut() {
                            obj.retain(|k, _| {
                                matches!(k.as_str(), "usage" | "type" | "message" | "response")
                            });
                        }
                        for key in ["message", "response"] {
                            if let Some(obj) = value.get_mut(key).and_then(Value::as_object_mut) {
                                obj.retain(|k, _| k == "usage");
                            }
                        }
                        if value.get("usage").is_some()
                            || value.pointer("/message/usage").is_some()
                            || value.pointer("/response/usage").is_some()
                        {
                            let kind = value.get("type").cloned();
                            self.events.retain(|e| e.get("type").cloned() != kind);
                            if self.events.len() < 8 {
                                self.events.push(value);
                            }
                        }
                    }
                }
            }
        }
    }
}
impl Drop for Meter {
    fn drop(&mut self) {
        let usage = if self.sse {
            if self.inclusive {
                TokenUsage::from_codex_stream_events_auto(&self.events)
                    .or_else(|| TokenUsage::from_openai_stream_events(&self.events))
            } else {
                TokenUsage::from_claude_stream_events(&self.events)
            }
        } else {
            serde_json::from_slice::<Value>(&self.buffer)
                .ok()
                .and_then(|v| {
                    if self.inclusive {
                        TokenUsage::from_codex_response_auto(&v)
                            .or_else(|| TokenUsage::from_openai_response(&v))
                    } else {
                        TokenUsage::from_claude_response(&v)
                    }
                })
        };
        if let Some(usage) = usage {
            let tokens = total_tokens(&usage, self.inclusive);
            let (tx, mut rx) = tokio::sync::mpsc::channel(1);
            let db = self.db.clone();
            let id = self.grant.id.clone();
            let window = self.grant.window;
            // Persistence runs off the forwarding task and cannot alter its result.
            tokio::spawn(async move {
                if let Some(tokens) = rx.recv().await {
                    let _ = tokio::task::spawn_blocking(move || {
                        if db.charge_gateway_key(&id, window, tokens).is_err() {
                            log::warn!("Gateway usage persistence failed");
                        }
                    })
                    .await;
                }
            });
            let _ = tx.try_send(tokens);
        }
    }
}
fn meter(response: Response, db: Arc<Database>, grant: Grant, inclusive: bool) -> Response {
    let (parts, body) = response.into_parts();
    let sse = parts
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"));
    let mut meter = Meter {
        db,
        grant,
        inclusive,
        sse,
        buffer: Vec::new(),
        events: Vec::new(),
        overflow: false,
    };
    let stream = async_stream::stream! {
        let mut body = body.into_data_stream();
        while let Some(chunk) = body.next().await {
            if let Ok(bytes) = &chunk { meter.observe(bytes); }
            yield chunk;
        }
        drop(meter);
    };
    Response::from_parts(parts, Body::from_stream(stream))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fragmented_sse_passes_through_and_charges_usage(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = Arc::new(Database::memory()?);
        let created = db.create_gateway_key(KeyPolicy {
            name: "Synthetic".into(),
            models: vec!["allowed".into()],
            token_limit: None,
            period: Period::Never,
        })?;
        let grant = db.authorize_gateway_key(&created.secret, "allowed", Utc::now().timestamp())?;
        let text = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"content\":[{\"text\":\"synthetic\"}],\"usage\":{\"input_tokens\":20,\"cache_read_input_tokens\":4}}}\r\n\r\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":5}}\n\n"
        );
        let chunks: Vec<Result<bytes::Bytes, std::io::Error>> = text
            .as_bytes()
            .chunks(7)
            .map(|b| Ok(bytes::Bytes::copy_from_slice(b)))
            .collect();
        let response = Response::builder()
            .header("content-type", "text/event-stream")
            .body(Body::from_stream(futures::stream::iter(chunks)))?;
        let response = meter(response, db.clone(), grant, false);
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await?;
        assert_eq!(bytes.as_ref(), text.as_bytes());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if db
                    .list_gateway_keys()
                    .is_ok_and(|keys| keys[0].used_tokens == 29)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await?;
        Ok(())
    }
}
