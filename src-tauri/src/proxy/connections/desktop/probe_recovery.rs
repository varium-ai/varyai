//! One compatibility retry for an authenticated Desktop startup check.
use crate::proxy::{hyper_client::ProxyResponse, protocol_bridge::Protocol};
use futures::StreamExt;
use serde_json::{json, Value};

pub(crate) struct Recovery {
    enabled: bool,
    protocol: Protocol,
}

impl Recovery {
    pub(crate) fn new(enabled: bool, protocol: Protocol) -> Self {
        Self { enabled, protocol }
    }

    /// Retain the response, including unread chunks, if inspection cannot finish.
    /// No successful content or ambiguous transport failure authorizes a replay.
    pub(crate) async fn inspect(
        &mut self,
        response: ProxyResponse,
        request: &[u8],
    ) -> (ProxyResponse, Option<Vec<u8>>) {
        let status = response.status();
        let sse = response.is_sse();
        if !self.enabled || !(matches!(status.as_u16(), 400 | 422) || (status.is_success() && sse))
        {
            return (response, None);
        }
        self.enabled = false;
        let headers = response.headers().clone();
        let mut stream = Box::pin(response.bytes_stream());
        let mut chunks = Vec::new();
        let mut bytes = Vec::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let complete = loop {
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(None) => break true,
                Ok(Some(Ok(chunk))) => {
                    let fits = bytes.len().saturating_add(chunk.len()) <= 64 * 1024;
                    if fits {
                        bytes.extend_from_slice(&chunk);
                    }
                    chunks.push(Ok(chunk));
                    if !fits {
                        break false;
                    }
                }
                Ok(Some(Err(error))) => {
                    chunks.push(Err(error));
                    break false;
                }
                Err(_) => break false,
            }
        };
        let retry = (complete && requires_thinking(&bytes, sse))
            .then(|| enable_thinking(request, self.protocol))
            .flatten();
        let response =
            ProxyResponse::streamed(status, headers, futures::stream::iter(chunks).chain(stream));
        (response, retry)
    }
}

fn message_requires_thinking(value: &Value, depth: usize) -> bool {
    if depth > 4 {
        return false;
    }
    let Some(message) = value.pointer("/error/message").and_then(Value::as_str) else {
        return false;
    };
    static PATTERN: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::Regex::new(r"(?i)thinking[._ ]type\W+(?:must|should|has to) be\W+enabled\b").ok()
    });
    if pattern
        .as_ref()
        .is_some_and(|pattern| pattern.is_match(message))
    {
        return true;
    }
    serde_json::from_str::<Value>(message)
        .ok()
        .is_some_and(|nested| message_requires_thinking(&nested, depth + 1))
}

fn requires_thinking(bytes: &[u8], sse: bool) -> bool {
    if !sse {
        return serde_json::from_slice::<Value>(bytes)
            .ok()
            .is_some_and(|value| message_requires_thinking(&value, 0));
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut matched = false;
    for line in text.lines().filter_map(|line| line.strip_prefix("data:")) {
        let line = line.trim();
        if line == "[DONE]" {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        // Even one content or lifecycle event makes this an ambiguous partial reply.
        if value.get("error").is_none()
            || value.get("choices").is_some()
            || value.get("content").is_some()
            || value.get("delta").is_some()
            || value.get("message").is_some()
            || value["type"].as_str().is_some_and(|kind| kind != "error")
        {
            return false;
        }
        if !message_requires_thinking(&value, 0) {
            return false;
        }
        matched = true;
    }
    matched
}

fn enable_thinking(request: &[u8], protocol: Protocol) -> Option<Vec<u8>> {
    use serde_json::value::{to_raw_value, RawValue};
    let mut fields: std::collections::BTreeMap<String, Box<RawValue>> =
        serde_json::from_slice(request).ok()?;
    let body: Value = serde_json::from_slice(request).ok()?;
    let limit = |value: &Value| {
        let value = value.as_u64().unwrap_or(16);
        if value <= 1024 {
            1024 + value.max(1)
        } else {
            value
        }
    };
    let mut changes = Vec::new();
    match protocol {
        Protocol::Anthropic => {
            if matches!(
                body["thinking"]["type"].as_str(),
                Some("enabled" | "adaptive")
            ) {
                return None;
            }
            changes.push(("thinking", json!({"type":"enabled","budget_tokens":1024})));
            changes.push(("max_tokens", json!(limit(&body["max_tokens"]))));
        }
        Protocol::Chat => {
            if body["reasoning_effort"]
                .as_str()
                .is_some_and(|effort| !matches!(effort, "none" | "disabled"))
            {
                return None;
            }
            changes.push(("reasoning_effort", json!("low")));
            let field = if body.get("max_completion_tokens").is_some() {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            changes.push((field, json!(limit(&body[field]))));
        }
        Protocol::Responses => {
            let mut reasoning = body.get("reasoning").cloned().unwrap_or(json!({}));
            if reasoning["effort"]
                .as_str()
                .is_some_and(|effort| !matches!(effort, "none" | "disabled"))
            {
                return None;
            }
            reasoning
                .as_object_mut()?
                .insert("effort".into(), json!("low"));
            changes.push(("reasoning", reasoning));
            changes.push((
                "max_output_tokens",
                json!(limit(&body["max_output_tokens"])),
            ));
        }
        Protocol::Gemini => {
            let mut config = body.get("generationConfig").cloned().unwrap_or(json!({}));
            let config_fields = config.as_object_mut()?;
            let thinking = config_fields.entry("thinkingConfig").or_insert(json!({}));
            thinking.as_object_mut()?;
            if config["thinkingConfig"]["thinkingBudget"]
                .as_i64()
                .is_some_and(|value| value != 0)
            {
                return None;
            }
            config["thinkingConfig"]["thinkingBudget"] = json!(1024);
            config["maxOutputTokens"] = json!(limit(&config["maxOutputTokens"]));
            config.as_object_mut()?.remove("temperature");
            config.as_object_mut()?.remove("topP");
            config.as_object_mut()?.remove("topK");
            changes.push(("generationConfig", config));
        }
    }
    for field in ["temperature", "top_p", "top_k"] {
        fields.remove(field);
    }
    for (field, value) in changes {
        fields.insert(field.into(), to_raw_value(&value).ok()?);
    }
    serde_json::to_vec(&fields).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::{HeaderMap, StatusCode};

    fn refusal() -> Vec<u8> {
        json!({"error":{"message":"thinking.type must be \"enabled\""}})
            .to_string()
            .into_bytes()
    }

    fn response(status: u16, sse: bool, body: Vec<u8>) -> ProxyResponse {
        let mut headers = HeaderMap::new();
        headers.insert("x-synthetic", "retained".parse().unwrap());
        headers.insert(
            "content-type",
            if sse {
                "text/event-stream"
            } else {
                "application/json"
            }
            .parse()
            .unwrap(),
        );
        ProxyResponse::buffered(StatusCode::from_u16(status).unwrap(), headers, body.into())
    }

    #[tokio::test]
    async fn one_retry_keeps_errors_headers_and_response_bytes() {
        let request = br#"{"model":"fixture","max_tokens":16,"temperature":0,"extension":{"precise":1.2300}}"#;
        let mut recovery = Recovery::new(true, Protocol::Anthropic);
        let (original, retry) = recovery
            .inspect(response(400, false, refusal()), request)
            .await;
        assert_eq!(original.headers()["x-synthetic"], "retained");
        assert_eq!(
            original.bytes_with_limit(65536).await.unwrap(),
            Bytes::from(refusal())
        );
        let retry = retry.unwrap();
        assert!(String::from_utf8_lossy(&retry).contains("1.2300"));
        let body: Value = serde_json::from_slice(&retry).unwrap();
        assert_eq!(body["model"], "fixture");
        assert_eq!(body["max_tokens"], 1040);
        assert_eq!(body["thinking"]["budget_tokens"], 1024);
        assert!(body.get("temperature").is_none());
        let (original, retry) = recovery
            .inspect(response(400, false, refusal()), &retry)
            .await;
        assert!(retry.is_none());
        assert_eq!(original.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unrelated_failures_disabled_recovery_and_partial_output_never_retry() {
        let request = br#"{"model":"fixture","max_tokens":16}"#;
        for (enabled, status, sse, body) in [
            (false, 400, false, refusal()),
            (true, 401, false, refusal()),
            (true, 403, false, refusal()),
            (true, 429, false, refusal()),
            (true, 500, false, refusal()),
            (true, 200, false, refusal()),
            (true, 400, false, b"malformed".to_vec()),
            (true, 400, false, json!({"error":{"message":"Quota exhausted"}}).to_string().into_bytes()),
            (true, 400, false, json!({"text":"thinking.type must be enabled"}).to_string().into_bytes()),
            (true, 200, true, format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"started\"}}}}]}}\n\ndata: {}\n\n", String::from_utf8(refusal()).unwrap()).into_bytes()),
            (true, 400, false, vec![b'x'; 65537]),
        ] {
            let mut recovery = Recovery::new(enabled, Protocol::Anthropic);
            let (original, retry) = recovery.inspect(response(status, sse, body.clone()), request).await;
            assert!(retry.is_none());
            assert_eq!(original.status().as_u16(), status);
            assert_eq!(original.headers()["x-synthetic"], "retained");
            assert_eq!(original.bytes_with_limit(100000).await.unwrap(), Bytes::from(body));
        }
    }

    #[test]
    fn malformed_and_already_reasoning_requests_are_not_rewritten() {
        for (protocol, body) in [
            (Protocol::Anthropic, r#"{"thinking":{"type":"adaptive"}}"#),
            (Protocol::Anthropic, r#"{"thinking":{"type":"enabled"}}"#),
            (Protocol::Chat, r#"{"reasoning_effort":"high"}"#),
            (Protocol::Responses, r#"{"reasoning":false}"#),
            (Protocol::Gemini, r#"{"generationConfig":false}"#),
            (
                Protocol::Gemini,
                r#"{"generationConfig":{"thinkingConfig":false}}"#,
            ),
            (Protocol::Anthropic, "not json"),
        ] {
            assert!(enable_thinking(body.as_bytes(), protocol).is_none());
        }
    }
}
