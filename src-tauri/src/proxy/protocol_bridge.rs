//! Missing matrix edges reuse the established Anthropic adapters. Application
//! attribution and provider selection remain independent of the wire protocol.
use super::{
    hyper_client::ProxyResponse,
    providers::{self, AuthInfo, AuthStrategy, ClaudeAdapter, ProviderAdapter},
    ProxyError,
};
use crate::{app_config::AppType, provider::Provider};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    time::Duration,
};

mod streaming;
type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Protocol {
    Anthropic,
    Chat,
    Responses,
    Gemini,
}
impl Protocol {
    fn from_format(format: &str) -> Option<Self> {
        match format {
            "anthropic" | "anthropic_messages" => Some(Self::Anthropic),
            "openai_chat" | "chat_completions" => Some(Self::Chat),
            "openai_responses" | "responses" => Some(Self::Responses),
            "gemini_native" => Some(Self::Gemini),
            _ => None,
        }
    }
    fn format(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Chat => "openai_chat",
            Self::Responses => "openai_responses",
            Self::Gemini => "gemini_native",
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Bridge {
    pub input: Protocol,
    pub output: Protocol,
}
pub(super) struct BridgeContext {
    pub tool_context: providers::transform_codex_chat::CodexToolContext,
    pub shadow: std::sync::Arc<providers::gemini_shadow::GeminiShadowStore>,
    pub history: std::sync::Arc<providers::codex_chat_history::CodexChatHistoryStore>,
    pub provider_id: String,
    pub session_id: Option<String>,
    pub hints: providers::transform_gemini::AnthropicToolSchemaHints,
}
impl Bridge {
    pub fn endpoint(self, endpoint: &str) -> String {
        let query = endpoint
            .split_once('?')
            .map(|(_, query)| query)
            .unwrap_or("");
        let query = query
            .split('&')
            .filter(|pair| {
                self.input != Protocol::Gemini
                    || !url::form_urlencoded::parse(pair.as_bytes())
                        .any(|(name, _)| name == "alt" || name == "key")
            })
            .filter(|pair| !pair.is_empty())
            .collect::<Vec<_>>()
            .join("&");
        if query.is_empty() {
            "/v1/messages".into()
        } else {
            format!("/v1/messages?{query}")
        }
    }
    pub fn route(app: &AppType, endpoint: &str, provider: &Provider) -> Option<Self> {
        let path = endpoint.split('?').next().unwrap_or(endpoint);
        let input = if path.ends_with("/chat/completions") {
            Protocol::Chat
        } else if path.ends_with("/responses") {
            Protocol::Responses
        } else if *app == AppType::Gemini
            && (path.ends_with(":generateContent") || path.ends_with(":streamGenerateContent"))
        {
            Protocol::Gemini
        } else {
            return None;
        };
        let default = if *app == AppType::Gemini {
            Protocol::Gemini
        } else if providers::should_convert_codex_responses_to_chat(provider, "/v1/responses") {
            Protocol::Chat
        } else if providers::should_convert_codex_responses_to_anthropic(provider, "/v1/responses")
        {
            Protocol::Anthropic
        } else {
            // Legacy clients without an upstream format keep their native wire path.
            input
        };
        let output = provider
            .meta
            .as_ref()
            .and_then(|m| m.api_format.as_deref())
            .or_else(|| provider.settings_config["api_format"].as_str())
            .and_then(Protocol::from_format)
            .unwrap_or(default);
        let output = if provider.is_codex_oauth() || provider.is_xai_oauth() {
            Protocol::Responses
        } else {
            output
        };
        let missing = match input {
            Protocol::Chat => output != Protocol::Chat,
            Protocol::Responses => output == Protocol::Gemini,
            Protocol::Gemini => output != Protocol::Gemini,
            _ => false,
        };
        missing.then_some(Self { input, output })
    }
    pub fn request(self, body: Value, endpoint: &str) -> Result<Value, ProxyError> {
        match self.input {
            Protocol::Chat => chat_request(body),
            Protocol::Responses => {
                providers::transform_codex_anthropic::responses_request_to_anthropic(body, 8192)
            }
            Protocol::Gemini => gemini_request(body, endpoint),
            Protocol::Anthropic => Ok(body),
        }
    }
    pub async fn response(
        self,
        response: ProxyResponse,
        streaming: bool,
        timeout: Duration,
        context: BridgeContext,
    ) -> Result<ProxyResponse, ProxyError> {
        if !response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let mut headers = response.headers().clone();
        super::response_processor::strip_entity_headers_for_rebuilt_body(&mut headers);
        super::response_processor::strip_hop_by_hop_response_headers(&mut headers);
        if response.is_sse() {
            let source: ByteStream = match self.output {
                Protocol::Anthropic => Box::pin(response.bytes_stream()),
                Protocol::Chat => Box::pin(providers::streaming::create_anthropic_sse_stream(
                    response.bytes_stream(),
                )),
                Protocol::Responses => Box::pin(
                    providers::streaming_responses::create_anthropic_sse_stream_from_responses(
                        response.bytes_stream(),
                    ),
                ),
                Protocol::Gemini => Box::pin(
                    providers::streaming_gemini::create_anthropic_sse_stream_from_gemini(
                        response.bytes_stream(),
                        Some(context.shadow.clone()),
                        Some(context.provider_id.clone()),
                        context.session_id.clone(),
                        Some(context.hints.clone()),
                    ),
                ),
            };
            if streaming {
                headers.insert(
                    http::header::CONTENT_TYPE,
                    http::HeaderValue::from_static("text/event-stream"),
                );
                return Ok(ProxyResponse::Streamed {
                    status,
                    headers,
                    stream: self.stream(source, context.history, context.tool_context),
                });
            }
            let collect = async {
                let mut source = source;
                let mut bytes = Vec::new();
                while let Some(chunk) = source.next().await {
                    let chunk = chunk.map_err(|_| invalid("Incomplete upstream stream"))?;
                    if bytes.len() + chunk.len() > super::hyper_client::MAX_RESPONSE_BODY_BYTES {
                        return Err(invalid("Upstream response too large"));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                let text =
                    std::str::from_utf8(&bytes).map_err(|_| invalid("Invalid upstream UTF-8"))?;
                providers::transform_codex_anthropic::anthropic_sse_to_message_value(text)
            };
            let message = if timeout.is_zero() {
                collect.await?
            } else {
                tokio::time::timeout(timeout, collect)
                    .await
                    .map_err(|_| ProxyError::Timeout("Upstream response timed out".into()))??
            };
            headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("application/json"),
            );
            let encoded = self.encode_with_context(message, &context.tool_context)?;
            if self.input == Protocol::Responses {
                context.history.record_response(&encoded).await;
            }
            return Ok(ProxyResponse::buffered(
                status,
                headers,
                Bytes::from(encoded.to_string()),
            ));
        }
        let (_, _, bytes) =
            super::response_processor::read_decoded_body(response, "ProtocolBridge", timeout)
                .await?;
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| invalid("Invalid upstream JSON"))?;
        let message = match self.output {
            Protocol::Anthropic => body,
            Protocol::Chat => providers::transform::openai_to_anthropic(body)?,
            Protocol::Responses => providers::transform_responses::responses_to_anthropic(body)?,
            Protocol::Gemini => {
                providers::transform_gemini::gemini_to_anthropic_with_shadow_and_hints(
                    body,
                    Some(context.shadow.as_ref()),
                    Some(&context.provider_id),
                    context.session_id.as_deref(),
                    Some(&context.hints),
                )?
            }
        };
        if streaming {
            headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("text/event-stream"),
            );
            let bytes = streaming::message_sse(&message)?;
            return Ok(ProxyResponse::Streamed {
                status,
                headers,
                stream: self.stream(
                    Box::pin(futures::stream::once(async move { Ok(bytes) })),
                    context.history,
                    context.tool_context,
                ),
            });
        }
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
        let encoded = self.encode_with_context(message, &context.tool_context)?;
        if self.input == Protocol::Responses {
            context.history.record_response(&encoded).await;
        }
        Ok(ProxyResponse::buffered(
            status,
            headers,
            Bytes::from(encoded.to_string()),
        ))
    }
    fn stream(
        self,
        source: ByteStream,
        history: std::sync::Arc<providers::codex_chat_history::CodexChatHistoryStore>,
        tool_context: providers::transform_codex_chat::CodexToolContext,
    ) -> ByteStream {
        match self.input {
            Protocol::Responses => Box::pin(providers::codex_chat_history::record_responses_sse_stream(
                providers::streaming_codex_anthropic::create_responses_sse_stream_from_anthropic_with_context(source, tool_context), history)),
            Protocol::Anthropic => source,
            _ => streaming::to_client(source, self.input),
        }
    }
    #[cfg(test)]
    fn encode(self, message: Value) -> Result<Value, ProxyError> {
        self.encode_with_context(message, &Default::default())
    }
    fn encode_with_context(
        self,
        message: Value,
        tool_context: &providers::transform_codex_chat::CodexToolContext,
    ) -> Result<Value, ProxyError> {
        if matches!(self.input, Protocol::Chat | Protocol::Gemini) {
            let blocks = message["content"]
                .as_array()
                .ok_or_else(|| ProxyError::TransformError("Invalid upstream content".into()))?;
            if blocks.iter().any(|block| {
                block.get("signature").is_some()
                    || !matches!(
                        block["type"].as_str(),
                        Some("text" | "thinking" | "tool_use")
                    )
            }) {
                return Err(ProxyError::TransformError(
                    "Upstream content cannot be represented in the client protocol".into(),
                ));
            }
        }
        match self.input {
            Protocol::Responses => {
                providers::transform_codex_anthropic::anthropic_response_to_responses_with_context(
                    message,
                    tool_context,
                )
            }
            Protocol::Chat => Ok(chat_response(message)),
            Protocol::Gemini => Ok(gemini_response(message)),
            Protocol::Anthropic => Ok(message),
        }
    }
    pub fn provider(self, provider: &Provider) -> Provider {
        let mut provider = provider.clone();
        provider
            .meta
            .get_or_insert_with(Default::default)
            .api_format = Some(self.output.format().into());
        provider
    }
}

pub(super) struct BridgeAdapter<'a> {
    pub original: &'a dyn ProviderAdapter,
    pub output: Protocol,
}
impl ProviderAdapter for BridgeAdapter<'_> {
    fn name(&self) -> &'static str {
        "Claude"
    }
    fn extract_base_url(&self, p: &Provider) -> Result<String, ProxyError> {
        self.original.extract_base_url(p)
    }
    fn extract_auth(&self, p: &Provider) -> Option<AuthInfo> {
        self.original.extract_auth(p).map(|mut auth| {
            auth.strategy = match self.output {
                Protocol::Anthropic => {
                    if matches!(
                        auth.strategy,
                        AuthStrategy::ClaudeAuth | AuthStrategy::Bearer
                    ) {
                        auth.strategy
                    } else {
                        AuthStrategy::Anthropic
                    }
                }
                Protocol::Gemini => {
                    if auth.strategy == AuthStrategy::GoogleOAuth {
                        AuthStrategy::GoogleOAuth
                    } else {
                        AuthStrategy::Google
                    }
                }
                _ => AuthStrategy::Bearer,
            };
            auth
        })
    }
    fn build_url(&self, base: &str, endpoint: &str) -> String {
        ClaudeAdapter::new().build_url(base, endpoint)
    }
    fn get_auth_headers(
        &self,
        auth: &AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
        ClaudeAdapter::new().get_auth_headers(auth)
    }
}
fn invalid(message: &str) -> ProxyError {
    ProxyError::InvalidRequest(message.into())
}
fn array<'a>(value: &'a Value, message: &str) -> Result<&'a Vec<Value>, ProxyError> {
    value.as_array().ok_or_else(|| invalid(message))
}
fn reject_unknown(value: &Value, allowed: &[&str]) -> Result<(), ProxyError> {
    if value
        .as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err(invalid("Unsupported field in cross-protocol request"));
    }
    Ok(())
}
fn chat_content(value: &Value) -> Result<Value, ProxyError> {
    if value.is_string() {
        return Ok(value.clone());
    }
    if value.is_null() {
        return Ok(json!([]));
    }
    let mut blocks = Vec::new();
    for part in array(value, "Invalid Chat content")? {
        match part["type"].as_str() {
            Some("text") => reject_unknown(part, &["type", "text"])?,
            Some("image_url") => {
                reject_unknown(part, &["type", "image_url"])?;
                reject_unknown(&part["image_url"], &["url", "detail"])?;
                if part["image_url"]
                    .get("detail")
                    .is_some_and(|v| !v.is_null() && v != "auto")
                {
                    return Err(invalid(
                        "Image detail cannot be represented across protocols",
                    ));
                }
            }
            _ => return Err(invalid("Unsupported Chat content block")),
        }
        blocks.push(match part["type"].as_str(){
            Some("text")=>json!({"type":"text","text":part["text"]}),
            Some("image_url")=>{
                let url=part["image_url"]["url"].as_str().ok_or_else(||invalid("Invalid image URL"))?;
                if let Some(data)=url.strip_prefix("data:") {
                    let (media,data)=data.split_once(";base64,").ok_or_else(||invalid("Unsupported image data URL"))?;
                    json!({"type":"image","source":{"type":"base64","media_type":media,"data":data}})
                } else {json!({"type":"image","source":{"type":"url","url":url}})}
            }
            _=>return Err(invalid("Unsupported Chat content block")),
        });
    }
    Ok(json!(blocks))
}
fn chat_request(body: Value) -> Result<Value, ProxyError> {
    reject_unknown(
        &body,
        &[
            "model",
            "messages",
            "stream",
            "stream_options",
            "max_tokens",
            "max_completion_tokens",
            "temperature",
            "top_p",
            "stop",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "user",
            "n",
        ],
    )?;
    if body["n"].as_u64().is_some_and(|n| n != 1) {
        return Err(invalid("Multiple choices are unsupported across protocols"));
    }
    let mut messages = Vec::new();
    let mut system = Vec::new();
    for message in array(&body["messages"], "Chat messages must be an array")? {
        reject_unknown(
            message,
            &["role", "content", "tool_calls", "tool_call_id", "name"],
        )?;
        let role = message["role"]
            .as_str()
            .ok_or_else(|| invalid("Invalid Chat role"))?;
        let content = chat_content(&message["content"])?;
        if matches!(role, "system" | "developer") {
            if let Some(text) = content.as_str() {
                system.push(json!({"type":"text","text":text}));
            } else {
                for b in array(&content, "Invalid system content")? {
                    if b["type"] != "text" {
                        return Err(invalid("Unsupported system content"));
                    }
                    system.push(b.clone());
                }
            }
        } else if role == "tool" {
            let id = message["tool_call_id"]
                .as_str()
                .ok_or_else(|| invalid("Tool result missing call ID"))?;
            messages.push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":content}]}));
        } else if matches!(role, "assistant" | "user") {
            let mut blocks = if let Some(text) = content.as_str() {
                vec![json!({"type":"text","text":text})]
            } else {
                array(&content, "Invalid content")?.clone()
            };
            if let Some(calls) = message.get("tool_calls") {
                for call in array(calls, "Invalid tool calls")? {
                    reject_unknown(call, &["id", "type", "function"])?;
                    reject_unknown(&call["function"], &["name", "arguments"])?;
                    if call["type"] != "function" {
                        return Err(invalid("Unsupported Chat tool type"));
                    }
                    let args = call["function"]["arguments"]
                        .as_str()
                        .ok_or_else(|| invalid("Tool arguments must be JSON text"))?;
                    let input: Value = serde_json::from_str(args)
                        .map_err(|_| invalid("Invalid tool arguments JSON"))?;
                    if !input.is_object() {
                        return Err(invalid("Tool arguments must be an object"));
                    }
                    blocks.push(json!({"type":"tool_use","id":call["id"],"name":call["function"]["name"],"input":input}));
                }
            }
            messages.push(json!({"role":role,"content":blocks}));
        } else {
            return Err(invalid("Unsupported Chat role"));
        }
    }
    let mut out = json!({"model":body["model"],"messages":messages,"max_tokens":body.get("max_completion_tokens").or(body.get("max_tokens")).cloned().unwrap_or(json!(8192))});
    if !system.is_empty() {
        out["system"] = json!(system)
    }
    for key in ["stream", "temperature", "top_p"] {
        if let Some(v) = body.get(key) {
            out[key] = v.clone()
        }
    }
    if let Some(stop) = body.get("stop") {
        out["stop_sequences"] = if stop.is_string() {
            json!([stop])
        } else {
            stop.clone()
        }
    }
    if let Some(tools) = body.get("tools") {
        out["tools"]=json!(array(tools,"Invalid tools")?.iter().map(|t|{
        reject_unknown(t, &["type", "function"])?;
        if t["type"]!="function" {return Err(invalid("Unsupported Chat tool"))}
        reject_unknown(&t["function"], &["name", "description", "parameters", "strict"])?;
        if t["function"]["strict"] == true {
            return Err(invalid("Strict tool enforcement cannot be represented across protocols"));
        }
        Ok(json!({"name":t["function"]["name"],"description":t["function"]["description"].as_str().unwrap_or(""),"input_schema":t["function"]["parameters"]}))
    }).collect::<Result<Vec<_>,ProxyError>>()?);
    }
    if let Some(choice) = body.get("tool_choice") {
        out["tool_choice"] = match choice.as_str() {
            Some("auto") => json!({"type":"auto"}),
            Some("none") => json!({"type":"none"}),
            Some("required") => json!({"type":"any"}),
            _ if choice["type"] == "function" => {
                json!({"type":"tool","name":choice["function"]["name"]})
            }
            _ => return Err(invalid("Unsupported tool choice")),
        };
    }
    if body["parallel_tool_calls"] == false {
        if out["tool_choice"].is_null() {
            out["tool_choice"] = json!({"type":"auto"})
        }
        out["tool_choice"]["disable_parallel_tool_use"] = json!(true)
    }
    if let Some(user) = body["user"].as_str() {
        out["metadata"] = json!({"user_id":user})
    }
    Ok(out)
}

// Adapted from Magpie's Gemini tool-result handling (MIT); see
// third_party/magpie/LICENSE and docs/design/protocol-magpie-comparison.md.
fn gemini_image(part: &Value) -> Result<Value, ProxyError> {
    let (field, source_type, payload) = match (part.get("inlineData"), part.get("fileData")) {
        (Some(data), None) => ("inlineData", "base64", data),
        (None, Some(data)) => ("fileData", "url", data),
        _ => return Err(invalid("Invalid Gemini image part")),
    };
    let data_field = if field == "inlineData" {
        "data"
    } else {
        "fileUri"
    };
    reject_unknown(payload, &["mimeType", data_field])?;
    let mime = payload["mimeType"]
        .as_str()
        .filter(|mime| mime.starts_with("image/"))
        .ok_or_else(|| invalid("Only image media can cross protocols"))?;
    let data = payload[data_field]
        .as_str()
        .ok_or_else(|| invalid("Invalid Gemini image payload"))?;
    let source = if source_type == "base64" {
        json!({"type":"base64","media_type":mime,"data":data})
    } else {
        json!({"type":"url","url":data})
    };
    Ok(json!({"type":"image","source":source}))
}

fn gemini_request(body: Value, endpoint: &str) -> Result<Value, ProxyError> {
    reject_unknown(
        &body,
        &[
            "contents",
            "systemInstruction",
            "generationConfig",
            "tools",
            "toolConfig",
        ],
    )?;
    let model = endpoint
        .split("/models/")
        .nth(1)
        .and_then(|s| s.split(':').next())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("Missing Gemini model"))?;
    let mut messages = Vec::new();
    let mut calls: HashMap<String, VecDeque<String>> = HashMap::new();
    let mut next_id = 0;
    for content in array(&body["contents"], "Invalid Gemini contents")? {
        reject_unknown(content, &["role", "parts"])?;
        let base_role = match content["role"].as_str() {
            Some("model" | "assistant") => "assistant",
            Some("user") | None => "user",
            _ => return Err(invalid("Invalid Gemini role")),
        };
        let mut role = base_role;
        let mut blocks = Vec::new();
        for part in array(&content["parts"], "Invalid Gemini parts")? {
            if part.get("thoughtSignature").is_some() || part["thought"] == true {
                return Err(invalid(
                    "Opaque Gemini thought history cannot cross protocols",
                ));
            }
            reject_unknown(
                part,
                &[
                    "text",
                    "inlineData",
                    "fileData",
                    "functionCall",
                    "functionResponse",
                    "thought",
                ],
            )?;
            if [
                "text",
                "inlineData",
                "fileData",
                "functionCall",
                "functionResponse",
            ]
            .iter()
            .filter(|key| part.get(**key).is_some())
            .count()
                != 1
            {
                return Err(invalid("Gemini part must have exactly one content payload"));
            }
            if let Some(call) = part.get("functionCall") {
                reject_unknown(call, &["id", "name", "args"])?;
            }
            if let Some(result) = part.get("functionResponse") {
                reject_unknown(result, &["id", "name", "response", "parts"])?;
            }
            // Tool results belong to the user side even when an Agent labels
            // the enclosing Gemini content as model; keep mixed-part ordering.
            let part_role = if part.get("functionResponse").is_some() {
                "user"
            } else {
                base_role
            };
            if role != part_role && !blocks.is_empty() {
                messages.push(json!({"role":role,"content":std::mem::take(&mut blocks)}));
            }
            role = part_role;
            blocks.push(if let Some(text) = part.get("text") {
                json!({"type":"text","text":text})
            } else if part.get("inlineData").is_some() || part.get("fileData").is_some() {
                gemini_image(part)?
            } else if let Some(call) = part.get("functionCall") {
                let name = call["name"]
                    .as_str()
                    .ok_or_else(|| invalid("Missing tool name"))?;
                let id = call["id"].as_str().map(str::to_owned).unwrap_or_else(|| {
                    next_id += 1;
                    format!("gemini_call_{next_id}")
                });
                calls.entry(name.into()).or_default().push_back(id.clone());
                json!({"type":"tool_use","id":id,"name":name,"input":call["args"]})
            } else if let Some(result) = part.get("functionResponse") {
                let name = result["name"]
                    .as_str()
                    .ok_or_else(|| invalid("Missing tool result name"))?;
                let queue = calls
                    .get_mut(name)
                    .ok_or_else(|| invalid("Unmatched Gemini tool result"))?;
                let id = if let Some(id) = result["id"].as_str() {
                    let position = queue
                        .iter()
                        .position(|candidate| candidate == id)
                        .ok_or_else(|| invalid("Unmatched Gemini tool result"))?;
                    queue
                        .remove(position)
                        .ok_or_else(|| invalid("Unmatched Gemini tool result"))?
                } else {
                    queue
                        .pop_front()
                        .ok_or_else(|| invalid("Unmatched Gemini tool result"))?
                };
                let text = result["response"].to_string();
                let content = if let Some(parts) = result.get("parts") {
                    let mut content = vec![json!({"type":"text","text":text})];
                    for image in array(parts, "Invalid Gemini tool result parts")? {
                        reject_unknown(image, &["inlineData", "fileData"])?;
                        content.push(gemini_image(image)?);
                    }
                    json!(content)
                } else {
                    json!(text)
                };
                json!({"type":"tool_result","tool_use_id":id,"content":content})
            } else {
                return Err(invalid("Unsupported Gemini part"));
            });
        }
        messages.push(json!({"role":role,"content":blocks}));
    }
    let mut out = json!({"model":model,"messages":messages,"max_tokens":8192,"stream":endpoint.contains(":streamGenerateContent")});
    if let Some(system) = body.get("systemInstruction") {
        let mut parts = Vec::new();
        for part in array(&system["parts"], "Invalid system instruction")? {
            reject_unknown(part, &["text"])?;
            parts.push(json!({"type":"text","text":part["text"]}));
        }
        out["system"] = json!(parts);
    }
    if let Some(config) = body.get("generationConfig") {
        reject_unknown(
            config,
            &[
                "maxOutputTokens",
                "temperature",
                "topP",
                "topK",
                "stopSequences",
                "candidateCount",
            ],
        )?;
        if config["candidateCount"].as_u64().is_some_and(|n| n != 1) {
            return Err(invalid("Multiple candidates cannot cross protocols"));
        }
        for (from, to) in [
            ("maxOutputTokens", "max_tokens"),
            ("temperature", "temperature"),
            ("topP", "top_p"),
            ("topK", "top_k"),
            ("stopSequences", "stop_sequences"),
        ] {
            if let Some(v) = config.get(from) {
                out[to] = v.clone()
            }
        }
    }
    if let Some(tools) = body.get("tools") {
        let mut declarations = Vec::new();
        for tool in array(tools, "Invalid tools")? {
            reject_unknown(tool, &["functionDeclarations"])?;
            for f in array(&tool["functionDeclarations"], "Unsupported Gemini tool")? {
                reject_unknown(
                    f,
                    &["name", "description", "parameters", "parametersJsonSchema"],
                )?;
                if f.get("parameters").is_some() && f.get("parametersJsonSchema").is_some() {
                    return Err(invalid("Ambiguous Gemini tool schema"));
                }
                declarations.push(json!({"name":f["name"],"description":f["description"].as_str().unwrap_or(""),"input_schema":gemini_schema_to_json_schema(f.get("parameters").or(f.get("parametersJsonSchema")).cloned().unwrap_or(json!({"type":"object","properties":{}})))}));
            }
        }
        out["tools"] = json!(declarations);
    }
    if let Some(config) = body.get("toolConfig") {
        reject_unknown(config, &["functionCallingConfig"])?;
        let choice = &config["functionCallingConfig"];
        reject_unknown(choice, &["mode", "allowedFunctionNames"])?;
        out["tool_choice"] = match choice["mode"].as_str() {
            None | Some("AUTO") => json!({"type":"auto"}),
            Some("NONE") => json!({"type":"none"}),
            Some("ANY") => match choice["allowedFunctionNames"].as_array() {
                Some(names) if names.len() == 1 => json!({"type":"tool","name":names[0]}),
                Some(names) if !names.is_empty() => {
                    return Err(invalid("Multiple forced tool names cannot cross protocols"))
                }
                _ => json!({"type":"any"}),
            },
            _ => return Err(invalid("Unsupported Gemini tool mode")),
        };
    }
    Ok(out)
}
fn gemini_schema_to_json_schema(mut schema: Value) -> Value {
    fn visit(schema: &mut Value) {
        let Some(object) = schema.as_object_mut() else {
            return;
        };
        if let Some(Value::String(kind)) = object.get_mut("type") {
            if matches!(
                kind.as_str(),
                "STRING" | "NUMBER" | "INTEGER" | "BOOLEAN" | "OBJECT" | "ARRAY" | "NULL"
            ) {
                *kind = kind.to_ascii_lowercase();
            }
        }
        if object.remove("nullable") == Some(Value::Bool(true)) {
            if let Some(Value::String(kind)) = object.get("type") {
                object.insert("type".into(), json!([kind, "null"]));
            }
        }
        for key in ["properties", "$defs", "definitions", "patternProperties"] {
            if let Some(Value::Object(properties)) = object.get_mut(key) {
                for child in properties.values_mut() {
                    visit(child)
                }
            }
        }
        for key in [
            "items",
            "additionalProperties",
            "not",
            "contains",
            "propertyNames",
        ] {
            if let Some(child) = object.get_mut(key) {
                visit(child)
            }
        }
        for key in ["anyOf", "allOf", "oneOf", "prefixItems"] {
            if let Some(Value::Array(items)) = object.get_mut(key) {
                for child in items {
                    visit(child)
                }
            }
        }
    }
    visit(&mut schema);
    schema
}
fn prompt_tokens(usage: &Value) -> u64 {
    [
        "input_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ]
    .iter()
    .map(|k| usage[*k].as_u64().unwrap_or(0))
    .fold(0, u64::saturating_add)
}
fn chat_response(body: Value) -> Value {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    for block in body["content"].as_array().into_iter().flatten() {
        match block["type"].as_str(){Some("text")=>text.push_str(block["text"].as_str().unwrap_or("")),Some("thinking")=>reasoning.push_str(block["thinking"].as_str().unwrap_or("")),Some("tool_use")=>calls.push(json!({"id":block["id"],"type":"function","function":{"name":block["name"],"arguments":block["input"].to_string()}})),_=>{}}
    }
    let usage = &body["usage"];
    let input = prompt_tokens(usage);
    let output = usage["output_tokens"].as_u64().unwrap_or(0);
    let mut message = json!({"role":"assistant","content":if text.is_empty() && !calls.is_empty(){Value::Null}else{json!(text)}});
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls)
    }
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning)
    }
    json!({"id":body["id"],"object":"chat.completion","created":chrono::Utc::now().timestamp(),"model":body["model"],"choices":[{"index":0,"message":message,"finish_reason":chat_finish(body["stop_reason"].as_str())}],"usage":{"prompt_tokens":input,"completion_tokens":output,"total_tokens":input.saturating_add(output),"prompt_tokens_details":{"cached_tokens":usage["cache_read_input_tokens"].as_u64().unwrap_or(0)}}})
}
fn chat_finish(reason: Option<&str>) -> &'static str {
    match reason {
        Some("max_tokens" | "model_context_window_exceeded") => "length",
        Some("tool_use") => "tool_calls",
        Some("refusal") => "content_filter",
        _ => "stop",
    }
}
fn gemini_finish(reason: Option<&str>) -> &'static str {
    match reason {
        Some("max_tokens" | "model_context_window_exceeded") => "MAX_TOKENS",
        Some("refusal") => "SAFETY",
        Some("end_turn" | "stop_sequence" | "tool_use") => "STOP",
        _ => "OTHER",
    }
}
fn gemini_response(body: Value) -> Value {
    let mut parts = Vec::new();
    for block in body["content"].as_array().into_iter().flatten() {
        match block["type"].as_str(){Some("text")=>parts.push(json!({"text":block["text"]})),Some("thinking")=>parts.push(json!({"text":block["thinking"],"thought":true})),Some("tool_use")=>parts.push(json!({"functionCall":{"id":block["id"],"name":block["name"],"args":block["input"]}})),_=>{}}
    }
    let usage = &body["usage"];
    let input = prompt_tokens(usage);
    let output = usage["output_tokens"].as_u64().unwrap_or(0);
    json!({"modelVersion":body["model"],"responseId":body["id"],"candidates":[{"index":0,"content":{"role":"model","parts":parts},"finishReason":gemini_finish(body["stop_reason"].as_str())}],"usageMetadata":{"promptTokenCount":input,"candidatesTokenCount":output,"totalTokenCount":input.saturating_add(output),"cachedContentTokenCount":usage["cache_read_input_tokens"].as_u64().unwrap_or(0)}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_chat_clients_keep_native_routing_without_a_format_override() {
        let provider = Provider::with_id(
            "legacy".into(),
            "Legacy".into(),
            json!({"base_url":"https://relay.example"}),
            None,
        );
        for app in [
            AppType::Codex,
            AppType::OpenCode,
            AppType::OpenClaw,
            AppType::Hermes,
        ] {
            assert!(
                Bridge::route(&app, "/v1/chat/completions", &provider).is_none(),
                "a legacy native Chat client was rerouted without a protocol selection"
            );
        }
    }
    #[test]
    fn anthropic_bridge_honors_the_selected_credential_header() {
        for (field, expected) in [
            ("ANTHROPIC_API_KEY", AuthStrategy::Anthropic),
            ("ANTHROPIC_AUTH_TOKEN", AuthStrategy::Bearer),
        ] {
            let mut provider = Provider::with_id(
                "fixture".into(),
                "Fixture".into(),
                json!({"auth":{"OPENAI_API_KEY":"fixture-key"}}),
                None,
            );
            provider.meta = Some(crate::provider::ProviderMeta {
                api_format: Some("anthropic".into()),
                api_key_field: Some(field.into()),
                ..Default::default()
            });
            let original = providers::CodexAdapter::new();
            let adapter = BridgeAdapter {
                original: &original,
                output: Protocol::Anthropic,
            };
            assert_eq!(adapter.extract_auth(&provider).unwrap().strategy, expected);
        }
    }

    #[test]
    fn chat_tool_roundtrip_preserves_ids_arguments_results_and_images() {
        let request = json!({"model":"fixture","messages":[{"role":"system","content":"rules"},{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,aGVsbG8="}}]},{"role":"assistant","content":null,"tool_calls":[{"id":"call-a","type":"function","function":{"name":"read","arguments":"{\"path\":\"甲\"}"}},{"id":"call-b","type":"function","function":{"name":"read","arguments":"{\"path\":\"乙\"}"}}]},{"role":"tool","tool_call_id":"call-b","content":"second"},{"role":"tool","tool_call_id":"call-a","content":"first"}],"tools":[{"type":"function","function":{"name":"read","parameters":{"type":"object","properties":{"path":{"type":"string"}}}}}],"tool_choice":"required","parallel_tool_calls":false});
        let out = chat_request(request).unwrap();
        assert_eq!(
            out["messages"][0]["content"][0]["source"]["media_type"],
            "image/png"
        );
        assert_eq!(out["messages"][1]["content"][1]["id"], "call-b");
        assert_eq!(out["messages"][1]["content"][1]["input"]["path"], "乙");
        assert_eq!(out["messages"][2]["content"][0]["tool_use_id"], "call-b");
        assert_eq!(out["tool_choice"]["disable_parallel_tool_use"], true);
        let response = chat_response(
            json!({"id":"msg","model":"fixture","content":out["messages"][1]["content"],"stop_reason":"tool_use","usage":{"input_tokens":5,"cache_read_input_tokens":7,"output_tokens":3}}),
        );
        assert_eq!(
            response["choices"][0]["message"]["tool_calls"][1]["function"]["arguments"],
            "{\"path\":\"乙\"}"
        );
        assert_eq!(response["usage"]["total_tokens"], 15);
    }
    #[test]
    fn gemini_parallel_results_with_ids_can_arrive_out_of_order() {
        let result=gemini_request(json!({"contents":[{"role":"model","parts":[{"functionCall":{"id":"a","name":"read","args":{}}},{"functionCall":{"id":"b","name":"read","args":{}}}]},{"role":"user","parts":[{"functionResponse":{"id":"b","name":"read","response":{"result":"second"}}},{"functionResponse":{"name":"read","response":{"result":"first"}}}]}]}),"/v1beta/models/fixture:generateContent").unwrap();
        assert_eq!(result["messages"][1]["content"][0]["tool_use_id"], "b");
        assert_eq!(result["messages"][1]["content"][1]["tool_use_id"], "a");
    }
    #[test]
    fn gemini_tool_results_under_model_role_split_into_user_turns() {
        for role in ["model", "assistant", "user"] {
            let body = json!({"contents":[
                {"role":"model","parts":[{"functionCall":{"id":"call-a","name":"read","args":{}}}]},
                {"role":role,"parts":[{"text":"before"},{"functionResponse":{"id":"call-a","name":"read","response":{"output":"fixture result"}}},{"text":"after"}]}
            ]});
            let converted = gemini_request(body, "/v1beta/models/fixture:generateContent").unwrap();
            let turns = converted["messages"].as_array().unwrap();
            let result_turn = turns
                .iter()
                .find(|m| {
                    m["content"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|b| b["type"] == "tool_result")
                })
                .unwrap();
            assert_eq!(result_turn["role"], "user");
            assert_eq!(
                result_turn["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|b| b["type"] == "tool_result")
                    .count(),
                1
            );
            assert!(
                converted.to_string().contains("before") && converted.to_string().contains("after")
            );
        }
    }

    #[test]
    fn gemini_tool_result_images_survive_conversion_to_each_other_protocol() {
        for role in ["model", "user"] {
            let body = json!({"contents":[
                {"role":"model","parts":[{"functionCall":{"id":"call-a","name":"read","args":{"file_name":"image.png"}}}]},
                {"role":role,"parts":[{"functionResponse":{"id":"call-a","name":"read","response":{"output":"fixture image"},"parts":[{"inlineData":{"mimeType":"image/png","data":"aGVsbG8="}},{"fileData":{"mimeType":"image/png","fileUri":"https://example.test/image.png"}}]}}]}
            ]});
            let converted = gemini_request(body, "/v1beta/models/fixture:generateContent").unwrap();
            let result = &converted["messages"][1]["content"][0];
            assert_eq!(result["type"], "tool_result");
            assert_eq!(converted["messages"][1]["role"], "user");
            assert_eq!(result["content"][1]["source"]["data"], "aGVsbG8=");
            for format in ["anthropic", "openai_chat", "openai_responses"] {
                let mut provider =
                    Provider::with_id("fixture".into(), "Fixture".into(), json!({}), None);
                provider.meta = Some(crate::provider::ProviderMeta {
                    api_format: Some(format.into()),
                    ..Default::default()
                });
                let outbound = providers::transform_claude_request_for_api_format(
                    converted.clone(),
                    &provider,
                    format,
                    None,
                    None,
                );
                assert!(outbound.is_ok(), "{format}: failed image conversion");
                let outbound = outbound.unwrap().to_string();
                assert!(
                    outbound.contains("aGVsbG8=")
                        && outbound.contains("https://example.test/image.png")
                        && outbound.contains("fixture image"),
                    "{format}: tool image lost"
                );
            }
        }
    }

    #[test]
    fn unsupported_fields_and_malformed_arguments_fail_explicitly() {
        assert!(chat_request(
            json!({"model":"x","messages":[],"response_format":{"type":"json_object"}})
        )
        .is_err());
        assert!(chat_request(json!({"model":"x","messages":[{"role":"assistant","tool_calls":[{"id":"c","type":"function","function":{"name":"read","arguments":"private-malformed"}}]}]})).unwrap_err().to_string().find("private-malformed").is_none());
        assert!(gemini_request(
            json!({"contents":[{"parts":[{"text":"hi","thoughtSignature":"opaque"}]}]}),
            "/v1beta/models/test:generateContent"
        )
        .is_err());
    }
    #[test]
    fn nested_protocol_extensions_are_not_silently_discarded() {
        let strict_tool = json!({"messages":[],"tools":[{"type":"function","function":{"name":"read","parameters":{"type":"object"},"strict":true}}]});
        assert!(
            chat_request(strict_tool).is_err(),
            "strict tool semantics were lost"
        );
        let image_detail = json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.test/image.png","detail":"high"}}]}]});
        assert!(
            chat_request(image_detail).is_err(),
            "image resolution semantics were lost"
        );
        let native_tool = json!({"contents":[],"tools":[{"functionDeclarations":[{"name":"read","behavior":"NON_BLOCKING"}]}]});
        assert!(
            gemini_request(native_tool, "/v1beta/models/fixture:generateContent").is_err(),
            "native tool execution semantics were lost"
        );
        let mixed_part = json!({"contents":[{"role":"user","parts":[{"text":"hello","inlineData":{"mimeType":"image/png","data":"aGVsbG8="}}]}]});
        assert!(
            gemini_request(mixed_part, "/v1beta/models/fixture:generateContent").is_err(),
            "an ambiguous content part was truncated"
        );
    }
    #[test]
    fn unsupported_tool_history_metadata_is_rejected_before_forwarding() {
        let chat = json!({"messages":[{"role":"assistant","tool_calls":[{"id":"a","type":"function","function":{"name":"read","arguments":"{}","vendor_execution":"private"}}]}]});
        assert!(
            chat_request(chat).is_err(),
            "tool history metadata was discarded"
        );
        let gemini = json!({"contents":[{"role":"model","parts":[{"functionCall":{"name":"read","args":{},"vendor_execution":"private"}}]}]});
        assert!(
            gemini_request(gemini, "/v1beta/models/fixture:generateContent").is_err(),
            "native call metadata was discarded"
        );
    }

    #[tokio::test]
    async fn opaque_stream_signatures_are_rejected_instead_of_dropped() {
        let frames = [
            json!({"type":"message_start","message":{"id":"m","model":"fixture"}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"reason"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"private-signature"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
            json!({"type":"message_stop"}),
        ];
        for target in [Protocol::Chat, Protocol::Gemini] {
            let bytes = frames
                .iter()
                .map(|v| format!("data: {v}\n\n"))
                .collect::<String>();
            let source: ByteStream = Box::pin(futures::stream::iter(vec![Ok(Bytes::from(bytes))]));
            let result = streaming::from_anthropic(source, target)
                .collect::<Vec<_>>()
                .await;
            assert!(
                result.iter().any(Result::is_err),
                "opaque signature silently discarded"
            );
        }
    }

    #[tokio::test]
    async fn interleaved_tool_arguments_survive_single_byte_utf8_splits() {
        let frames = [
            json!({"type":"message_start","message":{"id":"m","model":"x","usage":{"input_tokens":7}}}),
            json!({"type":"content_block_start","index":8,"content_block":{"type":"tool_use","id":"a","name":"read","input":{}}}),
            json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"b","name":"read","input":{}}}),
            json!({"type":"content_block_delta","index":8,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"甲\"}"}}),
            json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"乙\"}"}}),
            json!({"type":"content_block_stop","index":3}),
            json!({"type":"content_block_stop","index":8}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
        ];
        let bytes = frames
            .iter()
            .map(|v| format!("data:{v}\r\n\r\n"))
            .collect::<String>()
            .into_bytes();
        for target in [Protocol::Chat, Protocol::Gemini] {
            let chunks = bytes
                .iter()
                .map(|b| Ok(Bytes::copy_from_slice(&[*b])))
                .collect::<Vec<Result<Bytes, std::io::Error>>>();
            let mut stream = Box::pin(streaming::from_anthropic(
                Box::pin(futures::stream::iter(chunks)),
                target,
            ));
            let mut result = String::new();
            while let Some(chunk) = stream.next().await {
                result.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
            }
            assert!(result.contains("甲") && result.contains("乙"));
            assert!(!result.contains('\u{fffd}'));
            assert!(result.contains(if target == Protocol::Chat {
                "tool_calls"
            } else {
                "functionCall"
            }));
            assert!(result.contains(if target == Protocol::Chat {
                "total_tokens\":10"
            } else {
                "totalTokenCount\":10"
            }));
        }
    }
    #[tokio::test]
    async fn truncated_stream_never_synthesizes_a_successful_finish() {
        let bytes = Bytes::from_static(
            b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"x\"}}\n\n",
        );
        let stream = streaming::from_anthropic(
            Box::pin(futures::stream::once(async move { Ok(bytes) })),
            Protocol::Chat,
        );
        let result = stream.collect::<Vec<_>>().await;
        assert!(result.last().unwrap().is_err());
        assert!(!result
            .iter()
            .filter_map(|v| v.as_ref().ok())
            .any(|b| b.windows(6).any(|w| w == b"[DONE]")));
    }
    #[tokio::test]
    async fn stream_limits_apply_to_each_event_not_a_coalesced_transport_chunk() {
        let mut frames = vec![
            json!({"type":"message_start","message":{"id":"m","model":"fixture","usage":{"input_tokens":7}}}),
        ];
        frames.push(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}));
        for _ in 0..40 {
            frames.push(json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x".repeat(64*1024)}}));
        }
        frames.extend([
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
        ]);
        let bytes = frames
            .iter()
            .map(|v| format!("data: {v}\n\n"))
            .collect::<String>();
        let source: ByteStream = Box::pin(futures::stream::iter(vec![Ok(Bytes::from(bytes))]));
        let result = Box::pin(streaming::from_anthropic(source, Protocol::Chat))
            .collect::<Vec<_>>()
            .await;
        assert!(
            result.iter().all(Result::is_ok),
            "small events in a coalesced chunk were rejected"
        );
    }
    #[tokio::test]
    async fn a_usage_only_delta_does_not_complete_an_interrupted_stream() {
        let frames = [
            json!({"type":"message_start","message":{"id":"m","model":"fixture","usage":{"input_tokens":7}}}),
            json!({"type":"message_delta","delta":{},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
        ];
        let bytes = frames
            .iter()
            .map(|v| format!("data: {v}\n\n"))
            .collect::<String>();
        let source: ByteStream = Box::pin(futures::stream::iter(vec![Ok(Bytes::from(bytes))]));
        let result = Box::pin(streaming::from_anthropic(source, Protocol::Chat))
            .collect::<Vec<_>>()
            .await;
        assert!(
            result.last().is_some_and(Result::is_err),
            "usage alone was treated as a successful finish"
        );
    }
    #[tokio::test]
    async fn cancelling_a_converted_response_drops_the_upstream_stream() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        struct DropSignal(Arc<AtomicBool>);
        impl Drop for DropSignal {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        for input in [Protocol::Chat, Protocol::Gemini, Protocol::Responses] {
            let dropped = Arc::new(AtomicBool::new(false));
            let signal = DropSignal(dropped.clone());
            let source: ByteStream = Box::pin(async_stream::stream! {
                let _signal = signal;
                yield Ok(Bytes::from_static(b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"model\":\"fixture\"}}\n\n"));
                yield Ok(Bytes::from_static(b"data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"partial\"}}\n\n"));
                futures::future::pending::<()>().await;
            });
            let bridge = Bridge {
                input,
                output: Protocol::Anthropic,
            };
            let mut converted =
                bridge.stream(source, Arc::new(Default::default()), Default::default());
            assert!(
                tokio::time::timeout(Duration::from_secs(1), converted.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .is_ok()
            );
            assert!(!dropped.load(Ordering::SeqCst));
            drop(converted);
            assert!(
                dropped.load(Ordering::SeqCst),
                "cancelled client retained upstream connection"
            );
        }
    }

    #[test]
    fn malformed_upstream_json_is_an_error_instead_of_a_proxy_panic() {
        assert!(streaming::message_sse(&json!([])).is_err());
    }
    #[test]
    fn unsupported_multiple_response_candidates_and_signed_thinking_are_not_truncated() {
        let chat = json!({"choices":[{"message":{"role":"assistant","content":"first"},"finish_reason":"stop"},{"message":{"role":"assistant","content":"second"},"finish_reason":"stop"}]});
        assert!(providers::transform::openai_to_anthropic(chat).is_err());
        let gemini = json!({"candidates":[{"content":{"parts":[{"text":"first"}]},"finishReason":"STOP"},{"content":{"parts":[{"text":"second"}]},"finishReason":"STOP"}]});
        assert!(providers::transform_gemini::gemini_to_anthropic(gemini).is_err());
        for input in [Protocol::Chat, Protocol::Gemini] {
            assert!(Bridge { input, output:Protocol::Anthropic }.encode(json!({"content":[{"type":"thinking","thinking":"reason","signature":"opaque-private"}]})).is_err());
        }
    }

    #[test]
    fn refusals_and_context_limits_keep_their_finish_semantics() {
        let refusal = gemini_response(json!({"content":[],"stop_reason":"refusal"}));
        assert_eq!(refusal["candidates"][0]["finishReason"], "SAFETY");
        assert_eq!(chat_finish(Some("model_context_window_exceeded")), "length");
    }
}

#[cfg(test)]
mod fidelity_tests {
    use super::*;
    #[tokio::test]
    async fn responses_namespace_survives_json_and_sse_bridging() {
        let request = json!({"tools":[{"type":"namespace","name":"files","tools":[{"type":"function","name":"read","parameters":{"type":"object","properties":{}}}]}]});
        let context =
            providers::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let name = providers::transform_codex_chat::flatten_namespace_tool_name("files", "read");
        let message = json!({"id":"m","model":"fixture","role":"assistant","type":"message","content":[{"type":"tool_use","id":"call-a","name":name,"input":{"path":"甲"}}],"stop_reason":"tool_use","usage":{"input_tokens":7,"output_tokens":3}});
        let bridge = Bridge {
            input: Protocol::Responses,
            output: Protocol::Gemini,
        };
        let encoded = bridge
            .encode_with_context(message.clone(), &context)
            .unwrap();
        assert_eq!(encoded["output"][0]["namespace"], "files");
        assert_eq!(encoded["output"][0]["name"], "read");
        let bytes = streaming::message_sse(&message).unwrap();
        let mut stream = bridge.stream(
            Box::pin(futures::stream::once(async move { Ok(bytes) })),
            std::sync::Arc::new(Default::default()),
            context,
        );
        let mut output = String::new();
        while let Some(chunk) = stream.next().await {
            output.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
        }
        let events = output
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .collect::<Vec<_>>();
        let response = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .unwrap();
        assert_eq!(response["response"]["output"][0]["namespace"], "files");
        assert!(response.to_string().contains("甲"));
    }
    #[test]
    fn gemini_tool_schema_case_and_nullable_preserve_literal_values() {
        let schema = gemini_schema_to_json_schema(
            json!({"type":"OBJECT","properties":{"name":{"type":"STRING","nullable":true,"enum":["STRING","MixedCase"]},"rows":{"type":"ARRAY","items":{"type":"INTEGER"}}}}),
        );
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["properties"]["name"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(schema["properties"]["name"]["enum"][0], "STRING");
        assert_eq!(schema["properties"]["rows"]["items"]["type"], "integer");
    }
    #[test]
    fn conversion_preserves_query_options_without_forwarding_gemini_credentials() {
        let chat = Bridge {
            input: Protocol::Chat,
            output: Protocol::Responses,
        };
        assert_eq!(
            chat.endpoint("/v1/chat/completions?api-version=Case%20Value&custom=1"),
            "/v1/messages?api-version=Case%20Value&custom=1"
        );
        let gemini = Bridge {
            input: Protocol::Gemini,
            output: Protocol::Anthropic,
        };
        assert_eq!(
            gemini.endpoint("/v1beta/models/a:streamGenerateContent?alt=sse&key=private&custom=1"),
            "/v1/messages?custom=1"
        );
    }
    #[test]
    fn opaque_response_blocks_are_rejected_instead_of_silently_removed() {
        for input in [Protocol::Chat, Protocol::Gemini] {
            let bridge = Bridge {
                input,
                output: Protocol::Anthropic,
            };
            assert!(bridge.encode(json!({"content":[{"type":"text","text":"visible"},{"type":"redacted_thinking","data":"private"}]})).is_err());
        }
    }
}
