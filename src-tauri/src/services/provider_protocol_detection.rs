//! Bounded inference probes. Results describe this URL/key/model only.
use futures::future::join_all;
use reqwest::{header::HeaderMap, Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[cfg(test)]
const MARKER: &str = "VARYAI_PROTOCOL_OK";
// Reasoning models may spend the first few hundred output tokens before answering.
const OUTPUT_TOKEN_LIMIT: u32 = 1024;
const BODY_LIMIT: usize = 256 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
const FORMATS: [&str; 4] = [
    "openai_chat",
    "openai_responses",
    "anthropic",
    "gemini_native",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub anthropic_auth_field: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProbeOutcome {
    pub status: &'static str,
    pub status_code: Option<u16>,
}
impl ProbeOutcome {
    fn new(status: &'static str) -> Self {
        Self {
            status,
            status_code: None,
        }
    }
    fn verified(&self) -> bool {
        self.status == "verified"
    }
}

#[derive(Debug, Serialize)]
pub struct ProtocolDetection {
    pub format: &'static str,
    pub text: ProbeOutcome,
    pub stream: Option<ProbeOutcome>,
}

pub async fn detect(config: DetectionConfig) -> Result<Vec<ProtocolDetection>, String> {
    let client = crate::proxy::http_client::get_without_redirects()
        .map_err(|_| "Unable to initialize detection client".to_string())?;
    detect_with_client(&client, &config).await
}

async fn detect_with_client(
    client: &Client,
    config: &DetectionConfig,
) -> Result<Vec<ProtocolDetection>, String> {
    let base =
        url::Url::parse(config.base_url.trim()).map_err(|_| "Invalid detection configuration")?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
        || config.api_key.trim().is_empty()
        || config.model.trim().is_empty()
    {
        return Err("Invalid detection configuration".into());
    }
    Ok(join_all(FORMATS.map(|format| async {
        let text = probe(client, config, &base, format, false).await;
        let stream = if text.verified() {
            Some(probe(client, config, &base, format, true).await)
        } else {
            None
        };
        ProtocolDetection {
            format,
            text,
            stream,
        }
    }))
    .await)
}

fn request_spec(
    config: &DetectionConfig,
    base: &url::Url,
    format: &str,
    stream: bool,
) -> Result<(url::Url, Value, HeaderMap), ProbeOutcome> {
    let mut url = base.clone();
    let base_path = base.path().trim_end_matches('/');
    let prompt = "hi";
    let mut headers = HeaderMap::new();
    let auth = if format == "anthropic"
        && config.anthropic_auth_field.as_deref() != Some("ANTHROPIC_AUTH_TOKEN")
    {
        ("x-api-key", config.api_key.trim().to_owned())
    } else if format == "gemini_native" {
        ("x-goog-api-key", config.api_key.trim().to_owned())
    } else {
        ("authorization", format!("Bearer {}", config.api_key.trim()))
    };
    let mut value = reqwest::header::HeaderValue::from_str(&auth.1)
        .map_err(|_| ProbeOutcome::new("invalid_config"))?;
    value.set_sensitive(true);
    headers.insert(auth.0, value);
    let body = match format {
        "openai_chat" => {
            url.set_path(&openai_probe_path(base_path, "/chat/completions"));
            json!({"model":config.model,"messages":[{"role":"user","content":prompt}],"stream":stream,"max_tokens":OUTPUT_TOKEN_LIMIT})
        }
        "openai_responses" => {
            url.set_path(&openai_probe_path(base_path, "/responses"));
            json!({"model":config.model,"input":[{"role":"user","content":prompt}],"stream":stream,"max_output_tokens":OUTPUT_TOKEN_LIMIT})
        }
        "anthropic" => {
            if !base_path.ends_with("/v1/messages") {
                let prefix = base_path
                    .strip_suffix("/v1beta")
                    .or_else(|| base_path.strip_suffix("/v1"))
                    .unwrap_or(base_path);
                url.set_path(&format!("{prefix}/v1/messages"));
            } else {
                url.set_path(base_path);
            }
            headers.insert(
                "anthropic-version",
                reqwest::header::HeaderValue::from_static("2023-06-01"),
            );
            json!({"model":config.model,"messages":[{"role":"user","content":prompt}],"stream":stream,"max_tokens":OUTPUT_TOKEN_LIMIT})
        }
        _ => {
            let mut endpoint = base.clone();
            endpoint.set_path("/v1beta/models");
            endpoint
                .path_segments_mut()
                .map_err(|_| ProbeOutcome::new("invalid_config"))?
                .push(&format!(
                    "{}:{}",
                    crate::proxy::gemini_url::normalize_gemini_model_id(&config.model),
                    if stream {
                        "streamGenerateContent"
                    } else {
                        "generateContent"
                    }
                ));
            let endpoint = if stream {
                format!("{}?alt=sse", endpoint.path())
            } else {
                endpoint.path().to_owned()
            };
            url = url::Url::parse(&crate::proxy::gemini_url::build_gemini_native_url(
                base.as_str(),
                &endpoint,
            ))
            .map_err(|_| ProbeOutcome::new("invalid_config"))?;
            json!({"contents":[{"role":"user","parts":[{"text":prompt}]}],"generationConfig":{"maxOutputTokens":OUTPUT_TOKEN_LIMIT}})
        }
    };
    Ok((url, body, headers))
}

fn openai_probe_path(base: &str, endpoint: &str) -> String {
    if base.ends_with(endpoint) {
        base.to_owned()
    } else {
        let base = if base.is_empty() { "/v1" } else { base };
        format!("{base}{endpoint}")
    }
}

fn network_error(error: reqwest::Error) -> ProbeOutcome {
    ProbeOutcome::new(if error.is_timeout() {
        "timeout"
    } else {
        "network"
    })
}
fn http_error(status: StatusCode) -> ProbeOutcome {
    let reason = match status.as_u16() {
        401 | 403 => "authentication",
        404 | 405 => "endpoint",
        429 => "rate_limited",
        500..=599 => "unavailable",
        300..=399 => "redirect",
        _ => "rejected",
    };
    ProbeOutcome {
        status: reason,
        status_code: Some(status.as_u16()),
    }
}
async fn read_bounded(mut response: Response) -> Result<Vec<u8>, ProbeOutcome> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if bytes.len() + chunk.len() > BODY_LIMIT {
            return Err(ProbeOutcome::new("invalid_response"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
async fn probe(
    client: &Client,
    config: &DetectionConfig,
    base: &url::Url,
    format: &str,
    stream: bool,
) -> ProbeOutcome {
    let run = async {
        let (url, body, headers) = request_spec(config, base, format, stream)?;
        let response = client
            .post(url)
            .headers(headers)
            .json(&body)
            .timeout(TIMEOUT)
            .send()
            .await
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(http_error(response.status()));
        }
        if stream
            && response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .is_some_and(|s| s.starts_with("text/html"))
        {
            return Err(ProbeOutcome::new("invalid_response"));
        }
        let bytes = read_bounded(response).await?;
        let valid = if stream {
            std::str::from_utf8(&bytes).is_ok_and(|s| valid_stream(format, s))
        } else {
            serde_json::from_slice::<Value>(&bytes).is_ok_and(|v| valid_message(format, &v))
        };
        Ok(ProbeOutcome::new(if valid {
            "verified"
        } else {
            "invalid_response"
        }))
    };
    match tokio::time::timeout(TIMEOUT, run).await {
        Ok(Ok(result)) | Ok(Err(result)) => result,
        Err(_) => ProbeOutcome::new("timeout"),
    }
}
fn valid_message(format: &str, body: &Value) -> bool {
    if body.get("error").is_some() {
        return false;
    }
    match format {
        "openai_chat" => body["choices"].as_array().is_some_and(|c| !c.is_empty()),
        "openai_responses" => {
            body["status"].as_str() != Some("failed")
                && (body["output"].as_array().is_some()
                    || body["id"].is_string()
                    || body["status"].is_string())
        }
        "anthropic" => body["content"].as_array().is_some() || body["type"] == "message",
        _ => body["candidates"].as_array().is_some_and(|c| !c.is_empty()),
    }
}
pub(crate) fn valid_stream(format: &str, source: &str) -> bool {
    let normalized = source.replace("\r\n", "\n");
    let mut saw_event = false;
    for frame in normalized.split("\n\n") {
        let data = frame
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(body) = serde_json::from_str::<Value>(&data) else {
            return false;
        };
        if body.get("error").is_some()
            || matches!(
                body["type"].as_str(),
                Some("error" | "response.failed" | "response.incomplete")
            )
        {
            return false;
        }
        let matched = match format {
            "openai_chat" => body["choices"].as_array().is_some(),
            "openai_responses" => body["type"]
                .as_str()
                .is_some_and(|t| t.starts_with("response.")),
            "anthropic" => matches!(
                body["type"].as_str(),
                Some(
                    "message_start"
                        | "content_block_start"
                        | "content_block_delta"
                        | "content_block_stop"
                        | "message_delta"
                        | "message_stop"
                )
            ),
            _ => body["candidates"].as_array().is_some(),
        };
        saw_event |= matched;
    }
    saw_event
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode as HttpStatus, response::IntoResponse, routing::post, Router};
    use std::sync::{
        atomic::{AtomicU16, AtomicUsize, Ordering},
        Arc,
    };

    fn chat_stream() -> String {
        format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":{"content":MARKER},"finish_reason":null}]}),
            json!({"choices":[{"delta":{},"finish_reason":"stop"}]})
        )
    }
    #[test]
    fn stream_validation_accepts_protocol_events_and_rejects_errors() {
        let good = chat_stream();
        assert!(valid_stream("openai_chat", &good));
        assert!(valid_stream(
            "openai_chat",
            good.split("data: {\"choices\":[{\"delta\":{}")
                .next()
                .unwrap()
        ));
        assert!(!valid_stream(
            "openai_chat",
            &(good.clone() + "data: {\"error\":{\"message\":\"synthetic-private-input\"}}\n\n")
        ));
        assert!(!valid_stream("openai_chat", "data: [DONE]\n\n"));
        assert!(!valid_message("openai_chat", &json!({"message":MARKER})));
        assert!(valid_message(
            "openai_chat",
            &json!({"choices":[{"message":{"content":"Hi there!"},"finish_reason":"length"}]})
        ));
        let anthropic = format!(
            "data: {}\n\ndata: {}\n\ndata: {}\n\n",
            json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"Hi!"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"max_tokens"}}),
            json!({"type":"message_stop"})
        );
        assert!(valid_stream("anthropic", &anthropic));
        for format in FORMATS {
            assert!(!valid_stream(format, "data: {invalid}\n\n"));
        }
    }

    #[tokio::test]
    async fn oversized_responses_cannot_verify_while_valid_events_succeed() {
        let mock = Router::new().fallback(post(|req: axum::extract::Request| async move {
            let chat = req.uri().path().ends_with("/chat/completions");
            axum::body::to_bytes(req.into_body(), 4096).await.unwrap();
            if chat {
                (
                    [("content-type", "text/event-stream")],
                    format!(
                        "data: {}\n\n",
                        json!({"choices":[{"delta":{"content":"Hi!"}}]})
                    ),
                )
            } else {
                (
                    [("content-type", "application/json")],
                    " ".repeat(BODY_LIMIT + 1),
                )
            }
        }));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, mock).await.unwrap();
        });
        let config = DetectionConfig {
            base_url: format!("http://{address}"),
            api_key: "synthetic-key".into(),
            model: "fixture".into(),
            anthropic_auth_field: None,
        };
        let base = url::Url::parse(&config.base_url).unwrap();
        let client = Client::new();
        assert_eq!(
            probe(&client, &config, &base, "openai_chat", true)
                .await
                .status,
            "verified"
        );
        assert_eq!(
            probe(&client, &config, &base, "openai_responses", false)
                .await
                .status,
            "invalid_response"
        );
        server.abort();
    }

    #[test]
    fn paths_and_anthropic_bearer_auth_preserve_explicit_choices() {
        let config = DetectionConfig {
            base_url: "https://fixture.test/gateway/v1beta/".into(),
            api_key: "synthetic-key".into(),
            model: "models/fixture model".into(),
            anthropic_auth_field: Some("ANTHROPIC_AUTH_TOKEN".into()),
        };
        let base = url::Url::parse(&config.base_url).unwrap();
        let (url, _, headers) = request_spec(&config, &base, "anthropic", false).unwrap();
        assert_eq!(url.path(), "/gateway/v1/messages");
        assert_eq!(headers["authorization"], "Bearer synthetic-key");
        assert!(headers["authorization"].is_sensitive());
        assert!(!headers.contains_key("x-api-key"));
        let (url, _, _) = request_spec(&config, &base, "gemini_native", true).unwrap();
        assert_eq!(
            url.path(),
            "/gateway/v1beta/models/fixture%20model:streamGenerateContent"
        );
        assert_eq!(url.query(), Some("alt=sse"));
    }

    #[test]
    fn probes_preserve_compatibility_prefixes_and_recognize_native_endpoints() {
        let config = DetectionConfig {
            base_url: String::new(),
            api_key: "synthetic-key".into(),
            model: "fixture".into(),
            anthropic_auth_field: None,
        };
        for prefix in ["/api/paas/v4", "/v1beta/openai", "/gateway/v1"] {
            let base = url::Url::parse(&format!("https://fixture.test{prefix}")).unwrap();
            let (url, _, _) = request_spec(&config, &base, "openai_chat", false).unwrap();
            assert_eq!(url.path(), format!("{prefix}/chat/completions"));
            let (url, _, _) = request_spec(&config, &base, "openai_responses", false).unwrap();
            assert_eq!(url.path(), format!("{prefix}/responses"));
        }
        for (format, path) in [
            ("openai_chat", "/gateway/v1/chat/completions"),
            ("openai_responses", "/gateway/v1/responses"),
            ("anthropic", "/gateway/v1/messages"),
        ] {
            let base = url::Url::parse(&format!("https://fixture.test{path}")).unwrap();
            let (url, _, _) = request_spec(&config, &base, format, false).unwrap();
            assert_eq!(url.path(), path);
        }
        for path in [
            "/gateway/v1beta/openai",
            "/gateway/v1beta/models/old:generateContent",
        ] {
            let base = url::Url::parse(&format!("https://fixture.test{path}")).unwrap();
            let (url, _, _) = request_spec(&config, &base, "gemini_native", false).unwrap();
            assert_eq!(url.path(), "/gateway/v1beta/models/fixture:generateContent");
        }
    }

    #[tokio::test]
    async fn probes_verify_four_protocols_without_losing_error_categories_or_secrets() {
        let status = Arc::new(AtomicU16::new(200));
        let count = Arc::new(AtomicUsize::new(0));
        let status_control = status.clone();
        let counter = count.clone();
        let mock = Router::new().fallback(post(move |req: axum::extract::Request| {
            let status = status_control.clone(); let count = counter.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                let (parts, body) = req.into_parts();
                let body = http_body_util::BodyExt::collect(body).await.unwrap().to_bytes();
                let b: Value = serde_json::from_slice(&body).unwrap();
                let path = parts.uri.path();
                let header = if path.ends_with("/messages") { "x-api-key" } else if path.contains("generateContent") || path.contains("streamGenerateContent") { "x-goog-api-key" } else { "authorization" };
                let expected = if header == "authorization" { "Bearer synthetic-key" } else { "synthetic-key" };
                assert_eq!(parts.headers[header].to_str().unwrap(), expected);
                let code = status.load(Ordering::SeqCst);
                if code != 200 {
                    return (HttpStatus::from_u16(code).unwrap(), [("location", "/redirect-target")], "synthetic-private-input synthetic-key").into_response();
                }
                // This fixture needs 300 tokens of reasoning before its short answer.
                // A truncated reasoning-only response must not verify the protocol.
                if path.ends_with("responses") && b["max_output_tokens"].as_u64().unwrap_or(0) < 300 {
                    return axum::Json(json!({"status":"incomplete","output":[{"type":"reasoning","summary":[]}]})).into_response();
                }
                if path.ends_with("messages") && b["max_tokens"].as_u64().unwrap_or(0) < 300 {
                    return axum::Json(json!({"content":[{"type":"thinking","thinking":"Synthetic reasoning"}],"stop_reason":"max_tokens"})).into_response();
                }
                let stream = b["stream"] == true || path.contains("streamGenerateContent");
                let (message, events) = if path.ends_with("chat/completions") {
                    (json!({"choices":[{"message":{"content":MARKER},"finish_reason":"stop"}]}), chat_stream())
                } else if path.ends_with("responses") {
                    (json!({"status":"completed","output":[{"content":[{"type":"output_text","text":MARKER}]}]}), format!("data: {}\n\ndata: {}\n\n",json!({"type":"response.output_text.delta","delta":MARKER}),json!({"type":"response.completed","response":{"status":"completed"}})))
                } else if path.ends_with("messages") {
                    (json!({"content":[{"type":"text","text":MARKER}],"stop_reason":"end_turn"}),format!("data: {}\n\ndata: {}\n\ndata: {}\n\n",json!({"type":"content_block_delta","delta":{"type":"text_delta","text":MARKER}}),json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),json!({"type":"message_stop"})))
                } else {
                    assert!(path.starts_with("/gateway/v1beta/models/"));
                    let msg = json!({"candidates":[{"content":{"parts":[{"text":MARKER}]},"finishReason":"STOP"}]});
                    let sse = format!("data: {msg}\n\n"); (msg,sse)
                };
                if stream { ([("content-type","text/event-stream")], events).into_response() }
                else { axum::Json(message).into_response() }
            }
        }));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, mock).await.unwrap();
        });
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let config = DetectionConfig {
            base_url: format!("http://{address}/gateway/v1"),
            api_key: "synthetic-key".into(),
            model: "fixture".into(),
            anthropic_auth_field: None,
        };
        let results = detect_with_client(&client, &config).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 8);
        assert!(results
            .iter()
            .all(|r| r.text.verified() && r.stream.as_ref().is_some_and(ProbeOutcome::verified)));
        for (code, reason) in [
            (401, "authentication"),
            (403, "authentication"),
            (404, "endpoint"),
            (429, "rate_limited"),
            (503, "unavailable"),
            (307, "redirect"),
        ] {
            status.store(code, Ordering::SeqCst);
            let before = count.load(Ordering::SeqCst);
            let results = detect_with_client(&client, &config).await.unwrap();
            assert_eq!(count.load(Ordering::SeqCst) - before, 4);
            assert!(results
                .iter()
                .all(|r| r.text.status == reason && r.stream.is_none()));
            let encoded = serde_json::to_string(&results).unwrap();
            assert!(
                !encoded.contains("synthetic-private-input") && !encoded.contains("synthetic-key")
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn malformed_configuration_is_rejected_without_echoing_values() {
        let config = DetectionConfig {
            base_url: "https://user:synthetic-key@fixture.test".into(),
            api_key: "synthetic-key".into(),
            model: "fixture".into(),
            anthropic_auth_field: None,
        };
        let error = detect_with_client(&Client::new(), &config)
            .await
            .unwrap_err();
        assert_eq!(error, "Invalid detection configuration");
    }
}
