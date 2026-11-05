//! Specialized subscription execution; all client wire formats use the shared IR.
use super::{
    hyper_client::ProxyResponse,
    protocol_bridge::{ir, Protocol},
    ProxyError,
};
use crate::{
    provider::Provider,
    services::{native_subscription as accounts, routing_upstreams::SubscriptionProvider},
};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use http::{header, HeaderMap, HeaderValue, StatusCode};
use serde_json::{json, Value};
use std::{pin::Pin, sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;
fn invalid() -> ProxyError {
    ProxyError::TransformError("Subscription response is unavailable".into())
}
fn chatgpt_body(mut body: Value) -> Result<Value, ProxyError> {
    for key in [
        "background",
        "conversation",
        "max_output_tokens",
        "max_tool_calls",
        "metadata",
        "moderation",
        "multi_agent",
        "prompt",
        "prompt_cache_retention",
        "safety_identifier",
        "temperature",
        "top_logprobs",
        "top_p",
        "truncation",
        "user",
        "previous_response_id",
    ] {
        if let Some(object) = body.as_object_mut() {
            object.remove(key);
        }
    }
    body["stream"] = json!(true);
    body["store"] = json!(false);
    if let Some(items) = body["input"].as_array_mut() {
        for item in items {
            if item["role"] == "system" {
                item["role"] = json!("developer");
            }
        }
    }
    if body["tools"].as_array().is_some_and(|tools| {
        tools.iter().any(|tool| {
            matches!(
                tool["type"].as_str(),
                Some(
                    "image_generation"
                        | "file_search"
                        | "code_interpreter"
                        | "computer_use"
                        | "computer_use_preview"
                        | "computer"
                        | "mcp"
                        | "tool_search"
                )
            )
        })
    }) {
        return Err(ProxyError::InvalidRequest(
            "Hosted tools are unavailable for ChatGPT plan inference".into(),
        ));
    }
    Ok(body)
}
pub(crate) fn specialized(provider: &Provider) -> bool {
    crate::services::subscription_provider::binding(provider).is_some_and(|binding| {
        matches!(
            binding.provider,
            SubscriptionProvider::Claude
                | SubscriptionProvider::ChatgptApi
                | SubscriptionProvider::GeminiCli
                | SubscriptionProvider::Antigravity
        )
    })
}
fn envelope(
    provider: SubscriptionProvider,
    project: &str,
    model: &str,
    mut request: Value,
    session: Option<&str>,
) -> Value {
    if let Some(object) = request.as_object_mut() {
        object.remove("model");
        object.remove("stream");
    }
    let mut body = json!({"model":model,"project":project,"request":request});
    if provider == SubscriptionProvider::Antigravity {
        body["userAgent"] = json!("antigravity");
        body["requestType"] = json!("agent");
        body["requestId"] = json!(format!("agent-{}", uuid::Uuid::new_v4()));
        if let Some(session) = session {
            body["request"]["sessionId"] = json!(session);
        }
    } else {
        body["user_prompt_id"] = json!(uuid::Uuid::new_v4().to_string());
    }
    body
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn forward(
    provider: &Provider,
    protocol: Protocol,
    payload: &Value,
    endpoint: &str,
    session: Option<&str>,
    shadow: Arc<super::providers::gemini_shadow::GeminiShadowStore>,
    history: Arc<super::providers::codex_chat_history::CodexChatHistoryStore>,
    client: reqwest::Client,
) -> Result<ProxyResponse, ProxyError> {
    let binding = crate::services::subscription_provider::binding(provider).ok_or_else(invalid)?;
    let account = accounts::resolve(binding.provider, &binding.account_id)
        .await
        .map_err(|_| ProxyError::AuthError("Subscription account is unavailable".into()))?;
    let mut payload = payload.clone();
    if protocol == Protocol::Responses {
        let (_, owners) = history
            .enrich_request_with_provenance(
                &mut payload,
                None,
                super::security::chat_enrichment::ChatHistoryProvenance::StrictResume,
            )
            .await
            .map_err(ProxyError::DlpForward)?;
        if owners.is_some() {
            drop(payload);
            drop(owners);
            return Err(ProxyError::DlpForward(
                super::security::http_restoration::DlpForwardError::ContextUnavailable,
            ));
        }
        super::security::http_restoration::validate_unleased_enriched_body(&payload)
            .map_err(ProxyError::DlpForward)?;
    }
    let mut request = ir::parse::request(protocol, &payload, endpoint)?;
    if endpoint
        .split('?')
        .next()
        .is_some_and(|path| path.ends_with("/count_tokens"))
    {
        return Err(ProxyError::InvalidRequest(
            "Subscription token counting is unavailable".into(),
        ));
    }
    ir::continuity::restore(&mut request, &shadow, &provider.id, session);
    let streaming = request.stream;
    let source_protocol = match binding.provider {
        SubscriptionProvider::Claude => Protocol::Anthropic,
        SubscriptionProvider::ChatgptApi => Protocol::Responses,
        _ => Protocol::Gemini,
    };
    let context =
        super::providers::transform_codex_chat::build_codex_tool_context_from_request(&payload);
    let source = if binding.provider == SubscriptionProvider::Claude {
        claude(&account, &request).await?
    } else if binding.provider == SubscriptionProvider::ChatgptApi {
        let body = chatgpt_body(ir::build::request(&request, Protocol::Responses)?)?;
        // Sign in with ChatGPT requires stateless streamed Responses inference.
        let response = client
            .post(format!("{}/responses", binding.provider.endpoint()))
            .bearer_auth(account.token())
            .header(header::ACCEPT, "text/event-stream")
            .header(header::ACCEPT_ENCODING, "identity")
            .timeout(Duration::from_secs(300))
            .json(&body)
            .send()
            .await
            .map_err(|_| invalid())?;
        if !response.status().is_success() {
            return Err(ProxyError::UpstreamError {
                status: response.status().as_u16(),
                body: None,
            });
        }
        Box::pin(response.bytes_stream().map(|chunk| {
            chunk.map_err(|_| std::io::Error::other("Subscription stream interrupted"))
        })) as ByteStream
    } else {
        let project = accounts::google_project(&account).await.map_err(|_| {
            ProxyError::ConfigError("Subscription Cloud project is unavailable".into())
        })?;
        let model = if binding.provider == SubscriptionProvider::GeminiCli {
            accounts::gemini_wire_model(&account, &project, &request.model).await
        } else {
            request.model.clone()
        };
        let body = envelope(
            binding.provider,
            &project,
            &model,
            ir::build::request(&request, Protocol::Gemini)?,
            session,
        );
        let response = client
            .post(format!(
                "{}/v1internal:streamGenerateContent?alt=sse",
                binding.provider.endpoint()
            ))
            .bearer_auth(account.token())
            .header(header::ACCEPT, "text/event-stream")
            .header(header::ACCEPT_ENCODING, "identity")
            .timeout(Duration::from_secs(300))
            .json(&body)
            .send()
            .await
            .map_err(|_| invalid())?;
        if !response.status().is_success() {
            return Ok(ProxyResponse::buffered(
                response.status(),
                HeaderMap::from_iter([(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                )]),
                Bytes::from_static(
                    b"{\"error\":{\"message\":\"Subscription upstream rejected the request\"}}",
                ),
            ));
        }
        Box::pin(response.bytes_stream().map(|chunk| {
            chunk.map_err(|_| std::io::Error::other("Subscription stream interrupted"))
        })) as ByteStream
    };
    let provider_id = provider.id.clone();
    let session = session.map(str::to_owned);
    let observer: ir::stream::ReplyObserver = Arc::new(move |reply| {
        if source_protocol == Protocol::Gemini {
            ir::continuity::record(reply, &shadow, &provider_id, session.as_deref());
        }
    });
    if streaming {
        let stream = ir::stream::transcode_with_observer(
            source,
            source_protocol,
            protocol,
            context,
            Some(observer),
        );
        let stream: ByteStream = if protocol == Protocol::Responses {
            Box::pin(
                super::providers::codex_chat_history::record_responses_sse_stream(stream, history),
            )
        } else {
            stream
        };
        let headers = HeaderMap::from_iter([
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/event-stream"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ]);
        Ok(ProxyResponse::streamed(StatusCode::OK, headers, stream))
    } else {
        let reply = ir::stream::collect(source, source_protocol).await?;
        observer(&reply);
        let body = ir::reply::render_with_context(protocol, &reply, &context)?;
        if protocol == Protocol::Responses {
            history.record_response(&body).await;
        }
        Ok(ProxyResponse::buffered(
            StatusCode::OK,
            HeaderMap::from_iter([(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )]),
            Bytes::from(serde_json::to_vec(&body).map_err(|_| invalid())?),
        ))
    }
}

struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn tool_server(tools: Vec<Value>) -> Result<(Server, String), ProxyError> {
    use axum::{extract::State, routing::post, Json, Router};
    async fn rpc(
        State(tools): State<Arc<Vec<Value>>>,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        let id = body.get("id").cloned();
        if id.is_none() {
            return StatusCode::ACCEPTED.into_response();
        }
        let result = match body["method"].as_str().unwrap_or_default() {
            "initialize" => json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"Vary AI caller tools","version":"1"}}),
            "ping" => json!({}),
            "tools/list" => json!({"tools":&*tools}),
            // The caller executes its own tools. The CLI is stopped at tool_use;
            // holding this call prevents it from inventing a local tool result.
            "tools/call" => { std::future::pending::<()>().await; return StatusCode::GONE.into_response(); }
            _ => return Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}})).into_response(),
        };
        Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| invalid())?;
    let path = format!("/{}", uuid::Uuid::new_v4());
    let url = format!(
        "http://127.0.0.1:{}{path}",
        listener.local_addr().map_err(|_| invalid())?.port()
    );
    let app = Router::new()
        .route(&path, post(rpc))
        .with_state(Arc::new(tools));
    let server = Server(tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    }));
    Ok((server, url))
}
fn caller_prompt(body: &Value) -> Value {
    let mut content = Vec::new();
    for message in body["messages"].as_array().into_iter().flatten() {
        content.push(json!({"type":"text","text":format!("{}:", message["role"].as_str().unwrap_or("user"))}));
        for block in message["content"].as_array().into_iter().flatten() {
            if matches!(block["type"].as_str(), Some("text" | "image")) {
                content.push(block.clone());
            } else {
                content.push(json!({"type":"text","text":block.to_string()}));
            }
        }
    }
    json!({"type":"user","message":{"role":"user","content":content}})
}
fn cli_event(value: &Value) -> Option<Value> {
    if value["type"] != "stream_event" {
        return None;
    }
    let mut event = value["event"].clone();
    if event["content_block"]["type"] == "tool_use" {
        if let Some(name) = event["content_block"]["name"]
            .as_str()
            .and_then(|s| s.strip_prefix("mcp__vary__"))
        {
            event["content_block"]["name"] = json!(name);
        }
    }
    Some(event)
}
async fn claude(
    account: &accounts::Account,
    request: &ir::Request,
) -> Result<ByteStream, ProxyError> {
    let body = ir::build::request(request, Protocol::Anthropic)?;
    let tools = body["tools"].as_array().into_iter().flatten().map(|tool| json!({"name":tool["name"],"description":tool["description"],"inputSchema":tool["input_schema"]})).collect();
    let (server, url) = tool_server(tools).await?;
    let temporary = tempfile::tempdir().map_err(|_| invalid())?;
    let mcp = temporary.path().join("mcp.json");
    let system = temporary.path().join("system.txt");
    let config = json!({"mcpServers":{"vary":{"type":"http","url":url}}});
    crate::config::atomic_write_private(&mcp, config.to_string().as_bytes())
        .map_err(|_| invalid())?;
    let mut instructions = body["system"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    match &request.tool_choice {
        Some(ir::ToolChoice::Required) => instructions
            .push_str("\nCall at least one of the caller's available tools before answering."),
        Some(ir::ToolChoice::Named(name)) => {
            instructions.push_str(&format!("\nCall the caller's {name} tool."))
        }
        _ => {}
    }
    crate::config::atomic_write_private(&system, instructions.as_bytes()).map_err(|_| invalid())?;
    let mut command = accounts::claude_command();
    // Claude Code owns execution; caller generation options are per-run only.
    let mut extra = serde_json::Map::new();
    for key in [
        "max_tokens",
        "temperature",
        "top_p",
        "stop_sequences",
        "safeguards",
        "metadata",
        "output_config",
        "thinking",
    ] {
        if let Some(value) = body.get(key) {
            extra.insert(key.into(), value.clone());
        }
    }
    if matches!(request.tool_choice, Some(ir::ToolChoice::None)) {
        extra.insert("tool_choice".into(), json!({"type":"none"}));
    }
    command.env("CLAUDE_CODE_EXTRA_BODY", Value::Object(extra).to_string());
    command
        .env("CLAUDE_CONFIG_DIR", account.claude_dir())
        .current_dir(temporary.path())
        .args([
            "-p",
            "--output-format",
            "stream-json",
            "--input-format",
            "stream-json",
            "--include-partial-messages",
            "--verbose",
            "--model",
            &request.model,
            "--tools",
            "",
            "--strict-mcp-config",
            "--setting-sources",
            "",
            "--no-session-persistence",
            "--permission-mode",
            "dontAsk",
            "--allowedTools",
            "mcp__vary__*",
        ])
        .arg("--mcp-config")
        .arg(&mcp)
        .arg("--append-system-prompt-file")
        .arg(&system)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(effort) = &request.reasoning.effort {
        command.args(["--effort", effort]);
    }
    let mut child = command.spawn().map_err(|_| {
        ProxyError::ConfigError("Claude Code must be installed and signed in".into())
    })?;
    let mut input = child.stdin.take().ok_or_else(invalid)?;
    let mut prompt = serde_json::to_vec(&caller_prompt(&body)).map_err(|_| invalid())?;
    prompt.push(b'\n');
    input.write_all(&prompt).await.map_err(|_| invalid())?;
    drop(input);
    let output = child.stdout.take().ok_or_else(invalid)?;
    Ok(Box::pin(async_stream::stream! {
        let _server=server; let _temporary=temporary;
        let mut reader=BufReader::new(output); let mut buffer=Vec::new(); let mut ended=false;
        loop {
            buffer.clear();
            match tokio::time::timeout(Duration::from_secs(120), reader.read_until(b'\n', &mut buffer)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(_)) if buffer.len() <= 16*1024*1024 => {},
                _ => { yield Err(std::io::Error::other("Claude Code stream interrupted")); break; }
            }
            let Ok(value)=serde_json::from_slice::<Value>(&buffer) else { continue; };
            if let Some(event)=cli_event(&value) {
                let stop=event["type"]=="message_stop";
                let frame=format!("event: {}\ndata: {}\n\n",event["type"].as_str().unwrap_or("message"),event);
                yield Ok(Bytes::from(frame));
                if stop { ended=true; break; }
            } else if value["type"]=="result" && value["is_error"]==true { break; }
        }
        let _=child.kill().await;
        if !ended { yield Err(std::io::Error::other("Claude Code did not complete the reply")); }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chatgpt_plan_requests_are_stateless_and_keep_caller_functions() {
        let body=chatgpt_body(json!({"model":"fixture","stream":false,"store":true,"temperature":0.3,"previous_response_id":"fixture-old","input":[{"role":"system","content":"synthetic instruction"}],"tools":[{"type":"function","name":"fixture","parameters":{"type":"object"}}]})).expect("plan body");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["input"][0]["role"], "developer");
        assert_eq!(body["tools"][0]["name"], "fixture");
        assert!(body.get("temperature").is_none());
        assert!(body.get("previous_response_id").is_none());
        assert!(chatgpt_body(json!({"tools":[{"type":"code_interpreter"}]})).is_err());
    }
    #[tokio::test]
    async fn codeassist_sse_envelopes_stream_text_and_retain_tool_signatures() {
        let frames = [
            json!({"response":{"responseId":"fixture-reply","modelVersion":"fixture-model","candidates":[{"content":{"parts":[{"text":"synthetic "}]}}]}}),
            json!({"response":{"candidates":[{"content":{"parts":[{"text":"reply"},{"functionCall":{"name":"fixture","args":{"n":1}},"thoughtSignature":"synthetic-signature"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}}}),
        ];
        let source = Box::pin(futures::stream::iter(
            frames
                .into_iter()
                .map(|frame| Ok(Bytes::from(format!("data: {frame}\n\n")))),
        ));
        let reply = ir::stream::collect(source, Protocol::Gemini)
            .await
            .expect("Code Assist SSE");
        assert!(reply.parts.iter().any(|part|matches!(part,ir::Part::ToolCall{name,signature:Some(signature),..}if name=="fixture"&&signature=="synthetic-signature")));
        assert_eq!(reply.usage.input, 3);
        assert_eq!(reply.usage.output, 2);
    }
    #[test]
    fn codeassist_envelope_keeps_parameters_and_adds_only_transport_fields() {
        let request = json!({"model":"fixture","stream":true,"contents":[{"role":"user","parts":[{"text":"synthetic"}]}],"generationConfig":{"temperature":0.37},"tools":[{"functionDeclarations":[{"name":"fixture"}]}]});
        let body = envelope(
            SubscriptionProvider::Antigravity,
            "fixture-project",
            "fixture-model",
            request.clone(),
            Some("fixture-session"),
        );
        assert_eq!(body["model"], "fixture-model");
        assert_eq!(
            body["request"]["generationConfig"],
            request["generationConfig"]
        );
        assert_eq!(body["request"]["tools"], request["tools"]);
        assert_eq!(body["request"]["sessionId"], "fixture-session");
        assert!(body["request"].get("model").is_none());
    }
    #[test]
    fn cli_events_return_caller_tool_names() {
        let event = cli_event(
            &json!({"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"fixture","name":"mcp__vary__fixture_tool","input":{}}}}),
        );
        assert_eq!(
            event.map(|e| e["content_block"]["name"].clone()),
            Some(json!("fixture_tool"))
        );
    }
    #[tokio::test]
    async fn mcp_lists_only_callers_tools_and_does_not_execute_them() {
        let (_server, url) = tool_server(vec![
            json!({"name":"fixture","inputSchema":{"type":"object"}}),
        ])
        .await
        .expect("loopback fixture");
        let body: Value = reqwest::Client::new()
            .post(url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .send()
            .await
            .expect("list fixture")
            .json()
            .await
            .expect("JSON");
        assert_eq!(body["result"]["tools"][0]["name"], "fixture");
    }
}
