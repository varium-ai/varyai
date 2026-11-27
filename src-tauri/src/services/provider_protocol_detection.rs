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
const TIMEOUT: Duration = Duration::from_secs(10);
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

pub(crate) fn derived_endpoint_base(base_url: &str, format: &str) -> Option<String> {
    let mut trimmed = base_url.trim().trim_end_matches('/');
    for suffix in [
        "/chat/completions",
        "/responses",
        "/v1/messages",
        "/messages",
    ] {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            trimmed = stripped.trim_end_matches('/');
            break;
        }
    }
    let mut url = url::Url::parse(trimmed).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let path = url.path().trim_end_matches('/').to_owned();
    match format {
        "anthropic" => {
            let next = path
                .strip_suffix("/v1beta")
                .or_else(|| path.strip_suffix("/v1"))
                .unwrap_or(&path);
            url.set_path(next);
        }
        "gemini_native" => {
            if let Some(prefix) = path.strip_suffix("/v1") {
                url.set_path(&format!("{prefix}/v1beta"));
            } else {
                url.set_path(&path);
            }
        }
        "openai_chat" | "openai_responses" => {
            if path.is_empty() {
                url.set_path("/v1");
            } else {
                url.set_path(&path);
            }
        }
        _ => return None,
    }
    let derived = url.as_str().trim_end_matches('/').to_owned();
    crate::services::routing_upstreams::valid_url(&derived).then_some(derived)
}

fn is_non_chat_probe_model(row: &Value, id: &str) -> bool {
    if row["isImageGeneration"].as_bool() == Some(true) {
        return true;
    }
    if let Some(modalities) = row.get("outputModalities").and_then(Value::as_array) {
        if !modalities.is_empty() && !modalities.iter().any(|m| m.as_str() == Some("text")) {
            return true;
        }
    }
    let lower = id.to_ascii_lowercase();
    [
        "dall-e",
        "gpt-image",
        "imagen",
        "flux",
        "midjourney",
        "stable-diffusion",
        "sdxl",
        "embedding",
        "rerank",
        "whisper",
        "tts",
        "sora",
        "veo",
        "kling",
        "seedance",
        "wanx",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern))
}

pub(crate) fn pick_probe_model_for_format(
    models: &[Value],
    catalog: &[Value],
    format: &str,
) -> Option<String> {
    let mut chat_ids: Vec<String> = Vec::new();
    for row in models.iter().chain(catalog.iter()) {
        let Some(id) = crate::services::routing_upstreams::model_id(row) else {
            continue;
        };
        let trimmed = id.trim();
        if trimmed.is_empty()
            || trimmed.chars().any(char::is_control)
            || is_non_chat_probe_model(row, trimmed)
        {
            continue;
        }
        if !chat_ids.iter().any(|existing| existing == trimmed) {
            chat_ids.push(trimmed.to_owned());
        }
    }
    let first = chat_ids.first()?.clone();
    let preferred = match format {
        "openai_responses" => chat_ids.iter().find(|id| {
            let lower = id.to_ascii_lowercase();
            let base = lower.rsplit('/').next().unwrap_or(&lower);
            base.starts_with("gpt-")
                || base.starts_with("o1")
                || base.starts_with("o3")
                || base.starts_with("o4")
                || lower.contains("codex")
        }),
        "anthropic" => chat_ids
            .iter()
            .find(|id| id.to_ascii_lowercase().contains("claude")),
        "gemini_native" => chat_ids
            .iter()
            .find(|id| id.to_ascii_lowercase().contains("gemini")),
        _ => None,
    };
    Some(preferred.cloned().unwrap_or(first))
}

#[derive(Debug)]
pub(crate) struct BackfillProbeReport {
    pub verified: Vec<(&'static str, String)>,
    pub decisive: bool,
}

pub(crate) async fn detect_missing_endpoints(
    client: &Client,
    base_url: &str,
    api_key: &str,
    anthropic_auth_field: Option<&str>,
    format_models: &[(&'static str, String)],
) -> Result<BackfillProbeReport, String> {
    let base = url::Url::parse(base_url.trim()).map_err(|_| "Invalid detection configuration")?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
        || api_key.trim().is_empty()
    {
        return Err("Invalid detection configuration".into());
    }
    if format_models.is_empty() {
        return Ok(BackfillProbeReport {
            verified: Vec::new(),
            decisive: true,
        });
    }
    let auth_field = anthropic_auth_field.map(str::to_owned);
    let results = join_all(format_models.iter().map(|(format, model)| {
        let base = &base;
        let auth_field = auth_field.clone();
        async move {
            let config = DetectionConfig {
                base_url: base_url.trim().to_owned(),
                api_key: api_key.trim().to_owned(),
                model: model.clone(),
                anthropic_auth_field: auth_field,
            };
            (*format, probe(client, &config, base, format, false).await)
        }
    }))
    .await;
    let decisive = results.iter().any(|(_, outcome)| {
        !matches!(
            outcome.status,
            "network" | "timeout" | "unavailable" | "rate_limited"
        )
    });
    let mut verified = Vec::new();
    for (format, outcome) in results {
        if outcome.verified() {
            if let Some(derived) = derived_endpoint_base(base_url, format) {
                verified.push((format, derived));
            }
        }
    }
    Ok(BackfillProbeReport { verified, decisive })
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

#[cfg(test)]
fn request_spec(
    config: &DetectionConfig,
    base: &url::Url,
    format: &str,
    stream: bool,
) -> Result<(url::Url, Value, HeaderMap), ProbeOutcome> {
    request_spec_with_token_field(config, base, format, stream, false)
}

fn request_spec_with_token_field(
    config: &DetectionConfig,
    base: &url::Url,
    format: &str,
    stream: bool,
    use_max_completion_tokens: bool,
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
            let token_key = if use_max_completion_tokens {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            json!({"model":config.model,"messages":[{"role":"user","content":prompt}],"stream":stream,(token_key):OUTPUT_TOKEN_LIMIT})
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
async fn read_bounded(response: Response) -> Result<Vec<u8>, ProbeOutcome> {
    read_bounded_limit(response, BODY_LIMIT).await
}

async fn read_bounded_limit(mut response: Response, limit: usize) -> Result<Vec<u8>, ProbeOutcome> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if bytes.len() + chunk.len() > limit {
            return Err(ProbeOutcome::new("invalid_response"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn wants_max_completion_tokens(body: &[u8]) -> bool {
    String::from_utf8_lossy(body)
        .to_ascii_lowercase()
        .contains("max_completion_tokens")
}

fn requires_stream(body: &[u8]) -> bool {
    let lower = String::from_utf8_lossy(body).to_ascii_lowercase();
    lower.contains("stream")
        && [
            "must be true",
            "must be set to true",
            "stream=true",
            "streaming is required",
            "only supports stream",
            "stream only",
            "requires stream",
        ]
        .iter()
        .any(|phrase| lower.contains(phrase))
}

async fn probe(
    client: &Client,
    config: &DetectionConfig,
    base: &url::Url,
    format: &str,
    stream: bool,
) -> ProbeOutcome {
    let (outcome, error_body) = probe_once(client, config, base, format, stream, false).await;
    let Some(bytes) = error_body.as_deref() else {
        return outcome;
    };
    if format == "openai_chat"
        && outcome.status_code == Some(400)
        && wants_max_completion_tokens(bytes)
    {
        return probe_once(client, config, base, format, stream, true)
            .await
            .0;
    }
    if !stream && matches!(outcome.status_code, Some(400 | 422)) && requires_stream(bytes) {
        return probe_once(client, config, base, format, true, false)
            .await
            .0;
    }
    outcome
}

async fn probe_once(
    client: &Client,
    config: &DetectionConfig,
    base: &url::Url,
    format: &str,
    stream: bool,
    use_max_completion_tokens: bool,
) -> (ProbeOutcome, Option<Vec<u8>>) {
    let run = async {
        let (url, body, headers) =
            request_spec_with_token_field(config, base, format, stream, use_max_completion_tokens)
                .map_err(|outcome| (outcome, None))?;
        let response = client
            .post(url)
            .headers(headers)
            .json(&body)
            .timeout(TIMEOUT)
            .send()
            .await
            .map_err(|err| (network_error(err), None))?;
        let status = response.status();
        if !status.is_success() {
            let outcome = http_error(status);
            let error_body = if matches!(status.as_u16(), 400 | 422) {
                read_bounded_limit(response, 8 * 1024).await.ok()
            } else {
                None
            };
            return Err((outcome, error_body));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if content_type.starts_with("text/html") {
            return Err((ProbeOutcome::new("invalid_response"), None));
        }
        if stream || content_type.starts_with("text/event-stream") {
            return read_stream_outcome(format, response)
                .await
                .map(|outcome| (outcome, None))
                .map_err(|outcome| (outcome, None));
        }
        let bytes = read_bounded(response)
            .await
            .map_err(|outcome| (outcome, None))?;
        let valid = serde_json::from_slice::<Value>(&bytes)
            .is_ok_and(|v| valid_message(format, &v))
            || std::str::from_utf8(&bytes).is_ok_and(|text| valid_stream(format, text));
        Ok((
            ProbeOutcome::new(if valid {
                "verified"
            } else {
                "invalid_response"
            }),
            None,
        ))
    };
    match tokio::time::timeout(TIMEOUT, run).await {
        Ok(Ok(result)) | Ok(Err(result)) => result,
        Err(_) => (ProbeOutcome::new("timeout"), None),
    }
}
fn valid_message(format: &str, body: &Value) -> bool {
    if body.get("error").is_some_and(|v| !v.is_null()) {
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
enum FrameVerdict {
    Continue,
    Matched,
    Invalid,
}

fn validate_sse_frame(format: &str, frame: &str) -> FrameVerdict {
    let data = frame
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() || data == "[DONE]" {
        return FrameVerdict::Continue;
    }
    let Ok(body) = serde_json::from_str::<Value>(&data) else {
        return FrameVerdict::Invalid;
    };
    if body.get("error").is_some_and(|v| !v.is_null())
        || matches!(
            body["type"].as_str(),
            Some("error" | "response.failed" | "response.incomplete")
        )
    {
        return FrameVerdict::Invalid;
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
    if matched {
        FrameVerdict::Matched
    } else {
        FrameVerdict::Continue
    }
}

/// Verifies a streaming probe as soon as the first valid SSE event arrives.
async fn read_stream_outcome(
    format: &str,
    mut response: Response,
) -> Result<ProbeOutcome, ProbeOutcome> {
    let mut bytes = Vec::new();
    let mut normalized = String::new();
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if bytes.len() + chunk.len() > BODY_LIMIT {
            return Err(ProbeOutcome::new("invalid_response"));
        }
        bytes.extend_from_slice(&chunk);
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(err) if err.error_len().is_none() => continue,
            Err(_) => return Err(ProbeOutcome::new("invalid_response")),
        };
        normalized.clear();
        normalized.push_str(&text.replace("\r\n", "\n"));
        while let Some(end) = normalized.find("\n\n") {
            let frame = &normalized[..end];
            match validate_sse_frame(format, frame) {
                FrameVerdict::Matched => return Ok(ProbeOutcome::new("verified")),
                FrameVerdict::Invalid => return Err(ProbeOutcome::new("invalid_response")),
                FrameVerdict::Continue => {}
            }
            let remaining = normalized[end + 2..].to_owned();
            normalized = remaining;
            bytes = normalized.as_bytes().to_vec();
        }
    }
    if !normalized.is_empty() {
        if let FrameVerdict::Matched = validate_sse_frame(format, &normalized) {
            return Ok(ProbeOutcome::new("verified"));
        }
    }
    Err(ProbeOutcome::new("invalid_response"))
}

pub(crate) fn valid_stream(format: &str, source: &str) -> bool {
    let normalized = source.replace("\r\n", "\n");
    let mut saw_event = false;
    for frame in normalized.split("\n\n") {
        match validate_sse_frame(format, frame) {
            FrameVerdict::Invalid => return false,
            FrameVerdict::Matched => saw_event = true,
            FrameVerdict::Continue => {}
        }
    }
    saw_event
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode as HttpStatus, response::IntoResponse, routing::post, Router};
    use futures::StreamExt;
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

    #[tokio::test]
    async fn stream_probe_verifies_on_first_valid_event_without_waiting_for_eof() {
        let mock = Router::new().fallback(post(|_req: axum::extract::Request| async move {
            let first = format!(
                "data: {}\n\n",
                json!({"choices":[{"delta":{"content":"Hi"}}]})
            );
            let stream = futures::stream::once(async move {
                Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(first))
            })
            .chain(futures::stream::pending::<
                Result<axum::body::Bytes, std::convert::Infallible>,
            >());
            (
                [("content-type", "text/event-stream")],
                axum::body::Body::from_stream(stream),
            )
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
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            probe(&Client::new(), &config, &base, "openai_chat", true),
        )
        .await
        .expect("stream probe should return on first valid SSE event");
        assert_eq!(outcome.status, "verified");
        server.abort();
    }

    #[test]
    fn derived_endpoint_base_normalizes_for_each_protocol() {
        assert_eq!(
            derived_endpoint_base("https://relay.example", "openai_chat").as_deref(),
            Some("https://relay.example/v1")
        );
        assert_eq!(
            derived_endpoint_base("https://relay.example/", "openai_responses").as_deref(),
            Some("https://relay.example/v1")
        );
        assert_eq!(
            derived_endpoint_base(
                "https://relay.example/v1/chat/completions",
                "openai_responses"
            )
            .as_deref(),
            Some("https://relay.example/v1")
        );
        assert_eq!(
            derived_endpoint_base("https://relay.example/v1", "anthropic").as_deref(),
            Some("https://relay.example")
        );
        assert_eq!(
            derived_endpoint_base("https://relay.example/gateway/v1beta/", "anthropic").as_deref(),
            Some("https://relay.example/gateway")
        );
        assert_eq!(
            derived_endpoint_base("https://relay.example/v1", "gemini_native").as_deref(),
            Some("https://relay.example/v1beta")
        );
        assert_eq!(
            derived_endpoint_base("https://relay.example/api/paas/v4", "openai_responses")
                .as_deref(),
            Some("https://relay.example/api/paas/v4")
        );
        assert_eq!(
            derived_endpoint_base("https://user:secret@relay.example/v1", "openai_chat"),
            None
        );
    }

    #[test]
    fn pick_probe_model_filters_media_and_prefers_protocol_models() {
        let models = vec![
            json!({"model": "gpt-image-2", "isImageGeneration": true}),
            json!({"model": "text-embedding-3-large"}),
            json!({"model": "deepseek-chat"}),
            json!({"model": "claude-sonnet-4-5"}),
            json!({"model": "gpt-6.2-sol"}),
            json!({"model": "gemini-2.5-pro"}),
        ];
        assert_eq!(
            pick_probe_model_for_format(&models, &[], "openai_responses").as_deref(),
            Some("gpt-6.2-sol")
        );
        assert_eq!(
            pick_probe_model_for_format(&models, &[], "anthropic").as_deref(),
            Some("claude-sonnet-4-5")
        );
        assert_eq!(
            pick_probe_model_for_format(&models, &[], "gemini_native").as_deref(),
            Some("gemini-2.5-pro")
        );
        assert_eq!(
            pick_probe_model_for_format(&models, &[], "openai_chat").as_deref(),
            Some("deepseek-chat")
        );
        let fallback_only = vec![json!({"model": "glm-5.3"})];
        assert_eq!(
            pick_probe_model_for_format(&[], &fallback_only, "openai_responses").as_deref(),
            Some("glm-5.3")
        );
        let image_only = vec![
            json!({"model": "flux-1"}),
            json!({"model": "draw", "outputModalities": ["image"]}),
        ];
        assert_eq!(
            pick_probe_model_for_format(&image_only, &[], "openai_responses"),
            None
        );
    }

    #[tokio::test]
    async fn detect_missing_endpoints_distinguishes_decisive_and_transient_outcomes() {
        let transient = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let transient_flag = transient.clone();
        let mock = Router::new().fallback(post(move |req: axum::extract::Request| {
            let transient_flag = transient_flag.clone();
            async move {
                if transient_flag.load(Ordering::SeqCst) {
                    return (HttpStatus::SERVICE_UNAVAILABLE, "transient").into_response();
                }
                let path = req.uri().path().to_owned();
                axum::body::to_bytes(req.into_body(), 4096).await.unwrap();
                if path.ends_with("/responses") {
                    axum::Json(json!({"status": "completed", "output": [{"content": [{"type": "output_text", "text": MARKER}]}]}))
                        .into_response()
                } else {
                    (HttpStatus::NOT_FOUND, "not supported").into_response()
                }
            }
        }));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, mock).await.unwrap();
        });
        let client = Client::new();
        let base_url = format!("http://{address}/v1");
        let report = detect_missing_endpoints(
            &client,
            &base_url,
            "synthetic-key",
            None,
            &[
                ("openai_responses", "gpt-6.2-sol".into()),
                ("anthropic", "gpt-6.2-sol".into()),
            ],
        )
        .await
        .unwrap();
        assert!(report.decisive);
        assert_eq!(
            report.verified,
            vec![("openai_responses", format!("http://{address}/v1"))]
        );

        transient.store(true, Ordering::SeqCst);
        let transient_report = detect_missing_endpoints(
            &client,
            &base_url,
            "synthetic-key",
            None,
            &[("openai_responses", "gpt-6.2-sol".into())],
        )
        .await
        .unwrap();
        assert!(!transient_report.decisive);
        assert!(transient_report.verified.is_empty());
        server.abort();
    }

    #[test]
    fn valid_message_and_sse_accept_explicit_null_error_field() {
        assert!(valid_message(
            "openai_responses",
            &json!({
                "id": "resp_1",
                "object": "response",
                "status": "completed",
                "error": null,
                "output": [{"type": "message", "content": [{"type": "output_text", "text": "hi"}]}]
            })
        ));
        assert!(!valid_message(
            "openai_responses",
            &json!({"id": "resp_1", "status": "failed", "error": {"message": "bad"}})
        ));
        assert!(valid_stream(
            "openai_responses",
            "data: {\"type\":\"response.created\",\"error\":null}\n\n"
        ));
        assert!(!valid_stream(
            "openai_responses",
            "data: {\"type\":\"response.created\",\"error\":{\"message\":\"bad\"}}\n\n"
        ));
    }

    #[tokio::test]
    async fn probe_retries_stream_required_responses_and_accepts_sse_on_nonstream() {
        let always_sse = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let always_sse_flag = always_sse.clone();
        let mock = Router::new().fallback(post(move |req: axum::extract::Request| {
            let always_sse_flag = always_sse_flag.clone();
            async move {
                let path = req.uri().path().to_owned();
                let bytes = axum::body::to_bytes(req.into_body(), 4096).await.unwrap();
                let body: Value = serde_json::from_slice(&bytes).unwrap();
                if !path.ends_with("/responses") {
                    return (HttpStatus::NOT_FOUND, "not found").into_response();
                }
                let sse = format!(
                    "data: {}\n\ndata: {}\n\n",
                    json!({"type": "response.output_text.delta", "delta": "hi"}),
                    json!({"type": "response.completed", "response": {"status": "completed", "error": null}})
                );
                if always_sse_flag.load(Ordering::SeqCst) || body["stream"] == true {
                    return ([("content-type", "text/event-stream")], sse).into_response();
                }
                (
                    HttpStatus::BAD_REQUEST,
                    axum::Json(json!({"error": {"message": "Stream must be set to true"}})),
                )
                    .into_response()
            }
        }));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, mock).await.unwrap();
        });
        let client = Client::new();
        let config = DetectionConfig {
            base_url: format!("http://{address}/v1"),
            api_key: "synthetic-key".into(),
            model: "gpt-5.6-luna".into(),
            anthropic_auth_field: None,
        };
        let results = detect_with_client(&client, &config).await.unwrap();
        let responses = results
            .iter()
            .find(|r| r.format == "openai_responses")
            .unwrap();
        assert_eq!(responses.text.status, "verified");
        assert_eq!(
            responses.stream.as_ref().map(|s| s.status),
            Some("verified")
        );

        always_sse.store(true, Ordering::SeqCst);
        let base = url::Url::parse(&config.base_url).unwrap();
        let outcome = probe(&client, &config, &base, "openai_responses", false).await;
        assert_eq!(outcome.status, "verified");
        server.abort();
    }

    #[tokio::test]
    async fn probe_retries_chat_with_max_completion_tokens_when_max_tokens_rejected() {
        let mock = Router::new().fallback(post(|req: axum::extract::Request| async move {
            let path = req.uri().path().to_owned();
            let bytes = axum::body::to_bytes(req.into_body(), 4096).await.unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            if !path.ends_with("/chat/completions") {
                return (HttpStatus::NOT_FOUND, "not found").into_response();
            }
            if body.get("max_tokens").is_some() {
                return (
                    HttpStatus::BAD_REQUEST,
                    axum::Json(json!({"error": {"message": "Unsupported parameter: 'max_tokens'. Use 'max_completion_tokens' instead."}})),
                )
                    .into_response();
            }
            assert!(body.get("max_completion_tokens").is_some());
            if body["stream"] == true {
                ([("content-type", "text/event-stream")], chat_stream()).into_response()
            } else {
                axum::Json(json!({"choices": [{"message": {"content": "hi"}, "finish_reason": "stop"}]}))
                    .into_response()
            }
        }));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, mock).await.unwrap();
        });
        let client = Client::new();
        let config = DetectionConfig {
            base_url: format!("http://{address}/v1"),
            api_key: "synthetic-key".into(),
            model: "o4-mini".into(),
            anthropic_auth_field: None,
        };
        let results = detect_with_client(&client, &config).await.unwrap();
        let chat = results.iter().find(|r| r.format == "openai_chat").unwrap();
        assert_eq!(chat.text.status, "verified");
        assert_eq!(chat.stream.as_ref().map(|s| s.status), Some("verified"));
        server.abort();
    }
}
