//! WorkBuddy inference uses streamed Chat or native Anthropic Messages.
use super::*;
use crate::services::native_subscription::workbuddy as vendor;
use sha2::{Digest, Sha256};

fn prepare(mut body: Value, protocol: Protocol) -> Result<Value, ProxyError> {
    let object = body
        .as_object_mut()
        .ok_or_else(|| ProxyError::InvalidRequest("Invalid WorkBuddy request".into()))?;
    object.insert("stream".into(), json!(true));
    if protocol == Protocol::Chat {
        if let Some(max) = object.remove("max_completion_tokens") {
            object.entry("max_tokens").or_insert(max);
        }
        match object.get("tool_choice") {
            Some(choice) if choice == "none" || choice["type"] == "none" => {
                for key in ["tool_choice", "tools", "functions"] {
                    object.remove(key);
                }
            }
            Some(Value::Object(choice)) => {
                let choice = match choice.get("type").and_then(Value::as_str) {
                    Some("function") => choice
                        .get("function")
                        .and_then(|value| value["name"].as_str()),
                    Some(kind @ ("auto" | "required")) => Some(kind),
                    _ => None,
                }
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    ProxyError::InvalidRequest("Unsupported WorkBuddy tool choice".into())
                })?;
                object.insert("tool_choice".into(), json!(choice));
            }
            _ => {}
        }
    }
    let messages = object
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| ProxyError::InvalidRequest("Missing WorkBuddy messages".into()))?;
    for message in messages.iter_mut() {
        let Some(fields) = message.as_object_mut() else {
            return Err(ProxyError::InvalidRequest(
                "Invalid WorkBuddy message".into(),
            ));
        };
        let role = fields.get("role").and_then(Value::as_str);
        if protocol == Protocol::Chat {
            if role == Some("developer") {
                fields.insert("role".into(), json!("system"));
            } else if role == Some("assistant") {
                let text = fields.remove("reasoning_content");
                let details = fields.remove("reasoning_details");
                let reasoning = text
                    .filter(|value| value.as_str().is_some_and(|s| !s.is_empty()))
                    .or_else(|| {
                        let text: String = details
                            .as_ref()?
                            .as_array()?
                            .iter()
                            .filter_map(|detail| {
                                detail["text"]
                                    .as_str()
                                    .or_else(|| detail["summary"].as_str())
                            })
                            .collect();
                        (!text.is_empty()).then(|| json!(text))
                    });
                if let Some(reasoning) = reasoning {
                    fields.entry("reasoning").or_insert(reasoning);
                }
            }
        } else if role == Some("assistant") {
            if let Some(parts) = fields.get_mut("content").and_then(Value::as_array_mut) {
                let before = parts.len();
                parts.retain(|part| {
                    part["type"] != "thinking"
                        || part["signature"].as_str().is_some_and(|s| !s.is_empty())
                });
                if before > 0 && parts.is_empty() {
                    parts.push(json!({"type":"text","text":""}));
                }
            }
        }
    }
    if protocol == Protocol::Chat
        && messages
            .first()
            .is_some_and(|message| message["role"] != "system")
    {
        messages.insert(
            0,
            json!({"role":"system","content":"You are a helpful assistant."}),
        );
    }
    Ok(body)
}

fn conversation_headers(
    headers: &mut HeaderMap,
    account: &str,
    session: Option<&str>,
    body: &Value,
) -> Result<(), ProxyError> {
    let user_messages = || {
        body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, row)| {
                row["role"] == "user"
                    && !row["content"]
                        .as_array()
                        .is_some_and(|parts| parts.iter().any(|part| part["type"] == "tool_result"))
            })
    };
    let digest = |value: Value| format!("{:x}", Sha256::digest(value.to_string().as_bytes()));
    let conversation = session
        .map(|id| digest(json!([account, "session", id])))
        .or_else(|| {
            user_messages()
                .next()
                .map(|(_, row)| digest(json!([account, "first", row["content"]])))
        });
    let turn = user_messages()
        .last()
        .map(|(index, row)| digest(json!([account, conversation, index, row["content"]])));
    let message = uuid::Uuid::new_v4().simple().to_string();
    let turn = turn.unwrap_or_else(|| message.clone());
    for (name, value) in [
        ("x-conversation-message-id", &message),
        ("x-request-id", &message),
        ("x-conversation-request-id", &turn),
    ] {
        headers.insert(name, HeaderValue::from_str(value).map_err(|_| invalid())?);
    }
    if let Some(conversation) = conversation {
        headers.insert(
            "x-conversation-id",
            HeaderValue::from_str(&conversation).map_err(|_| invalid())?,
        );
    }
    Ok(())
}

async fn encode(body: Value, protocol: Protocol) -> Result<Bytes, ProxyError> {
    if protocol == Protocol::Anthropic {
        return serde_json::to_vec(&body)
            .map(Bytes::from)
            .map_err(|_| invalid());
    }
    tokio::task::spawn_blocking(move || {
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        serde_json::to_writer(&mut gzip, &body).map_err(|_| invalid())?;
        gzip.finish().map(Bytes::from).map_err(|_| invalid())
    })
    .await
    .map_err(|_| invalid())?
}

fn request(
    client: &reqwest::Client,
    account: &accounts::Account,
    base: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    body: &Bytes,
    beta: bool,
) -> Result<reqwest::RequestBuilder, ProxyError> {
    let path = if protocol == Protocol::Anthropic {
        if beta {
            "/v1/messages?beta=true"
        } else {
            "/v1/messages"
        }
    } else {
        "/v2/chat/completions"
    };
    let mut request = client
        .post(format!("{base}{path}"))
        .headers(headers.clone())
        .headers(vendor::headers(account).map_err(|_| invalid())?)
        .bearer_auth(account.token())
        .header(header::ACCEPT, "text/event-stream")
        .header(header::ACCEPT_ENCODING, "identity")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-agent-type", "main")
        .header("x-agent-runtime", "local-agent")
        .body(body.clone());
    if protocol == Protocol::Chat {
        request = request.header(header::CONTENT_ENCODING, "gzip");
    } else if !headers.contains_key("anthropic-version") {
        request = request.header("anthropic-version", "2023-06-01");
    }
    Ok(request)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn send(
    client: &reqwest::Client,
    account: &accounts::Account,
    protocol: Protocol,
    body: Value,
    session: Option<&str>,
    caller: &HeaderMap,
    beta: bool,
    default_effort: Option<&str>,
) -> Result<reqwest::Response, ProxyError> {
    let mut body = prepare(body, protocol)?;
    if protocol == Protocol::Chat
        && body.get("reasoning_effort").is_none()
        && body["max_tokens"].as_u64().is_none_or(|max| max > 1024)
    {
        if let Some(effort) = default_effort {
            body["reasoning_effort"] = json!(effort);
        }
    }
    let mut headers = HeaderMap::new();
    if protocol == Protocol::Anthropic {
        for name in ["anthropic-version", "anthropic-beta"] {
            for value in caller.get_all(name) {
                headers.append(name, value.clone());
            }
        }
    }
    conversation_headers(&mut headers, &account.id, session, &body)?;
    let body = encode(body, protocol).await?;
    let send = |account: &accounts::Account| {
        request(
            client,
            account,
            &vendor::account_origin(account),
            protocol,
            &headers,
            &body,
            beta,
        )
    };
    let wait = crate::proxy::http_client::UPSTREAM_HEADER_WAIT;
    let mut response =
        crate::proxy::http_client::send_with_header_timeout(send(account)?, wait).await?;
    if response.status() == StatusCode::UNAUTHORIZED {
        let renewed = match accounts::renew_rejected_workbuddy(account).await {
            Ok(renewed) => renewed,
            Err(_) => {
                return Err(ProxyError::UpstreamError {
                    status: 401,
                    body: None,
                })
            }
        };
        response =
            crate::proxy::http_client::send_with_header_timeout(send(&renewed)?, wait).await?;
        if response.status() == StatusCode::UNAUTHORIZED
            && response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("error=\"invalid_token\""))
        {
            accounts::observe_rejection(&renewed).await;
        }
    }
    if !response.status().is_success() {
        return Err(ProxyError::UpstreamError {
            status: response.status().as_u16(),
            body: None,
        });
    }
    if !response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(';').next() == Some("text/event-stream"))
    {
        return Err(invalid());
    }
    Ok(response)
}

#[cfg(test)]
mod tests;
