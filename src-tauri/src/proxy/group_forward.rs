//! Explicit cross-protocol execution for normalized static group providers.
//! Native execution and consumer attribution belong to the group runtime.

use super::{
    forwarder::RequestForwarder,
    hyper_client::ProxyResponse,
    protocol_bridge::{Bridge, Protocol},
    providers::{
        codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore, AuthStrategy,
        ClaudeAdapter, ProviderAdapter,
    },
    server::ProxyState,
    ProxyError,
};
use crate::provider::Provider;
use http::HeaderMap;
use serde_json::{json, Value};
use std::sync::Arc;

/// Execute only a selected static cross-protocol target, never choose a provider.
/// The caller supplies an exact target model and isolated continuity stores.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn forward(
    state: &ProxyState,
    protocol: Protocol,
    upstream: &Provider,
    method: &http::Method,
    endpoint: &str,
    body: &Value,
    headers: &HeaderMap,
    extensions: &http::Extensions,
    session_id: Option<&str>,
    history: Arc<CodexChatHistoryStore>,
    shadow: Arc<GeminiShadowStore>,
) -> Result<ProxyResponse, ProxyError> {
    if crate::services::subscription_provider::binding(upstream).is_some()
        && upstream.settings_config["routingSubscriptionResolved"] != true
    {
        return Err(ProxyError::AuthError(
            "Subscription account is unavailable".into(),
        ));
    }
    if upstream.uses_managed_account_auth()
        || upstream
            .meta
            .as_ref()
            .and_then(|meta| meta.auth_binding.as_ref())
            .is_some_and(|binding| {
                binding.source == crate::provider::AuthBindingSource::ManagedAccount
            })
    {
        return Err(ProxyError::ConfigError(
            "Managed providers require their specialized executor".into(),
        ));
    }
    let output = upstream
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .and_then(Protocol::from_format)
        .ok_or_else(|| {
            ProxyError::ConfigError("Group upstream requires an explicit supported protocol".into())
        })?;
    let bridge = Bridge::plan(protocol, output).ok_or_else(|| {
        ProxyError::InvalidRequest("Native group requests require byte-preserving execution".into())
    })?;
    if body
        .get("model")
        .and_then(Value::as_str)
        .is_none_or(|model| model.trim().is_empty())
    {
        return Err(ProxyError::InvalidRequest(
            "Group execution requires an exact target model".into(),
        ));
    }
    if output == Protocol::Gemini {
        let model = body["model"].as_str().unwrap_or_default();
        let model = super::gemini_url::normalize_gemini_model_id(model);
        // Gemini selects its model in a URL path. Keep this seam conservative:
        // reject reserved/control characters rather than reinterpret a model as
        // query, fragment, path traversal, or a pre-encoded URL component.
        if model.is_empty()
            || model == "."
            || model == ".."
            || !model
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(ProxyError::InvalidRequest(
                "Invalid Gemini target model identifier".into(),
            ));
        }
    }
    let mut upstream = bridge.provider(upstream);
    // The normalized OpenAI source retains its key in auth.OPENAI_API_KEY.
    // ClaudeAdapter owns config/auth interpretation; do not derive from the caller.
    let openai_key = upstream
        .settings_config
        .pointer("/auth/OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(key) = openai_key {
        if let Some(settings) = upstream.settings_config.as_object_mut() {
            let env = settings.entry("env").or_insert_with(|| json!({}));
            if let Some(env) = env.as_object_mut() {
                env.entry("OPENAI_API_KEY").or_insert(Value::String(key));
            }
        }
    }
    let adapter = ClaudeAdapter::new();
    let auth = adapter.extract_auth(&upstream).ok_or_else(|| {
        ProxyError::AuthError("Group upstream requires static source credentials".into())
    })?;
    if matches!(
        auth.strategy,
        AuthStrategy::GitHubCopilot
            | AuthStrategy::CodexOAuth
            | AuthStrategy::XaiOAuth
            | AuthStrategy::GoogleOAuth
    ) {
        return Err(ProxyError::ConfigError(
            "OAuth providers require their specialized executor".into(),
        ));
    }
    let config = state.config.read().await.clone();
    let forwarder = RequestForwarder::new(
        state.provider_router.clone(),
        config.non_streaming_timeout,
        state.status.clone(),
        state.current_providers.clone(),
        shadow,
        history,
        state.failover_manager.clone(),
        None,
        String::new(),
        session_id.unwrap_or_default().to_owned(),
        session_id.is_some(),
        config.streaming_first_byte_timeout,
        config.streaming_idle_timeout,
        Default::default(),
        Default::default(),
        Default::default(),
        0,
    );
    let (response, _, _, _, _) = forwarder
        .forward_bridge(
            bridge, method, &upstream, endpoint, body, headers, extensions,
        )
        .await?;
    Ok(response)
}

/// Read-only Responses-shaped projection for routing text and tool continuity.
/// It is not a wire conversion or outbound body. Unknown top-level settings are
/// irrelevant to inspection; omitted content is explicitly marked incomplete.
/// Responses already has the desired shape and is returned unchanged.
pub(crate) fn canonical_request(
    protocol: Protocol,
    body: &Value,
    endpoint: &str,
) -> Result<Value, ProxyError> {
    if !body.is_object() {
        return Err(ProxyError::InvalidRequest(
            "Group inspection requires a JSON object".into(),
        ));
    }
    if protocol == Protocol::Responses {
        return Ok(body.clone());
    }
    let mut projection = Projection::default();
    let mut result = json!({});
    for field in [
        "model",
        "metadata",
        "previous_response_id",
        "conversation",
        "prompt_cache_key",
    ] {
        if let Some(value) = body.get(field) {
            result[field] = value.clone();
        }
    }
    match protocol {
        Protocol::Chat | Protocol::Anthropic => {
            if let Some(system) = body.get("system") {
                let text = projection.text(system);
                if !text.is_empty() {
                    result["instructions"] = json!(text);
                }
            }
            if let Some(messages) = body["messages"].as_array() {
                for message in messages {
                    let role = message["role"].as_str().unwrap_or("user");
                    let content = &message["content"];
                    if protocol == Protocol::Chat && role == "tool" {
                        let output = projection.text(content);
                        projection.input.push(json!({"type":"function_call_output", "call_id":message["tool_call_id"], "output":output}));
                    } else {
                        projection.message(role, content);
                    }
                    if protocol == Protocol::Anthropic {
                        if let Some(blocks) = content.as_array() {
                            for block in blocks {
                                match block["type"].as_str() {
                                    Some("tool_use") => projection.input.push(json!({"type":"function_call", "call_id":block["id"], "name":block["name"], "arguments":block["input"].to_string()})),
                                    Some("tool_result") => {
                                        let output = projection.text(&block["content"]);
                                        projection.input.push(json!({"type":"function_call_output", "call_id":block["tool_use_id"], "output":output}));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    } else if let Some(calls) = message["tool_calls"].as_array() {
                        for call in calls {
                            if call["type"] == "function" {
                                projection.input.push(json!({"type":"function_call", "call_id":call["id"], "name":call["function"]["name"], "arguments":call["function"]["arguments"]}));
                            } else {
                                projection.incomplete = true;
                            }
                        }
                    }
                }
            } else {
                projection.incomplete = true;
            }
        }
        Protocol::Gemini => {
            if result.get("model").is_none() {
                if let Some(model) = endpoint
                    .split("/models/")
                    .nth(1)
                    .and_then(|rest| rest.split(':').next())
                {
                    result["model"] = json!(model);
                }
            }
            if let Some(parts) = body["systemInstruction"]["parts"].as_array() {
                let text = projection.gemini_text(parts);
                if !text.is_empty() {
                    result["instructions"] = json!(text);
                }
            }
            let mut calls: std::collections::HashMap<String, std::collections::VecDeque<String>> =
                Default::default();
            let mut next_id = 0;
            if let Some(contents) = body["contents"].as_array() {
                for content in contents {
                    let role = match content["role"].as_str() {
                        Some("model" | "assistant") => "assistant",
                        Some("user") | None => "user",
                        _ => {
                            projection.incomplete = true;
                            "user"
                        }
                    };
                    let Some(parts) = content["parts"].as_array() else {
                        projection.incomplete = true;
                        continue;
                    };
                    for part in parts {
                        if let Some(text) = part["text"].as_str() {
                            if part["thought"] != true {
                                projection.message(role, &json!(text));
                            } else {
                                projection.incomplete = true;
                            }
                        } else if let Some(call) = part.get("functionCall") {
                            let name = call["name"].as_str().unwrap_or("");
                            let id = call["id"].as_str().map(str::to_owned).unwrap_or_else(|| {
                                next_id += 1;
                                format!("gemini_call_{next_id}")
                            });
                            calls.entry(name.into()).or_default().push_back(id.clone());
                            projection.input.push(json!({"type":"function_call", "call_id":id, "name":name, "arguments":call["args"].to_string()}));
                        } else if let Some(output) = part.get("functionResponse") {
                            let name = output["name"].as_str().unwrap_or("");
                            let queue = calls.entry(name.into()).or_default();
                            let id = if let Some(id) = output["id"].as_str() {
                                if let Some(position) =
                                    queue.iter().position(|candidate| candidate == id)
                                {
                                    queue.remove(position);
                                }
                                Some(id.to_owned())
                            } else {
                                queue.pop_front()
                            };
                            if let Some(id) = id {
                                projection.input.push(json!({"type":"function_call_output", "call_id":id, "output":output["response"].to_string()}));
                            } else {
                                projection.incomplete = true;
                            }
                            if output.get("parts").is_some() {
                                projection.incomplete = true;
                            }
                        } else {
                            projection.incomplete = true;
                        }
                    }
                }
            } else {
                projection.incomplete = true;
            }
        }
        Protocol::Responses => {}
    }
    result["input"] = json!(projection.input);
    if projection.incomplete {
        result["_group_projection_incomplete"] = json!(true);
    }
    Ok(result)
}

#[derive(Default)]
struct Projection {
    input: Vec<Value>,
    incomplete: bool,
}
impl Projection {
    fn text(&mut self, content: &Value) -> String {
        if let Some(text) = content.as_str() {
            return text.to_owned();
        }
        if content.is_null() {
            return String::new();
        }
        let Some(blocks) = content.as_array() else {
            self.incomplete = true;
            return String::new();
        };
        let mut texts = Vec::new();
        for block in blocks {
            match block["type"].as_str() {
                Some("text" | "input_text" | "output_text") => {
                    if let Some(text) = block["text"].as_str() {
                        texts.push(text);
                    } else {
                        self.incomplete = true;
                    }
                }
                Some("tool_use" | "tool_result") => {}
                _ => self.incomplete = true,
            }
        }
        texts.join("\n")
    }
    fn message(&mut self, role: &str, content: &Value) {
        let text = self.text(content);
        if !text.is_empty() {
            self.input.push(json!({"type":"message", "role":role, "content":[{"type":if role == "assistant" {"output_text"} else {"input_text"}, "text":text}]}));
        }
    }
    fn gemini_text(&mut self, parts: &[Value]) -> String {
        parts
            .iter()
            .filter_map(|part| {
                let text = part["text"].as_str();
                if text.is_none() {
                    self.incomplete = true;
                }
                text
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{database::Database, provider::ProviderMeta};
    use axum::{
        body::Bytes,
        extract::{OriginalUri, State},
        response::IntoResponse,
        routing::post,
        Json, Router,
    };
    use serde_json::json;
    use std::{collections::HashMap, sync::Arc};
    use tokio::sync::{mpsc, RwLock};

    fn state() -> ProxyState {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Arc::new(Database::memory().unwrap());
        ProxyState {
            pi_runtime: Arc::default(),
            intent_runtime: Arc::default(),
            db: db.clone(),
            config: Arc::new(RwLock::new(super::super::types::ProxyConfig::default())),
            status: Arc::default(),
            start_time: Arc::default(),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            provider_router: Arc::new(super::super::provider_router::ProviderRouter::new(
                db.clone(),
            )),
            workbuddy_upstream: Arc::default(),
            gemini_shadow: Arc::default(),
            codex_chat_history: Arc::default(),
            app_handle: None,
            restoration_gateway:
                super::super::security::http_restoration::RestorationGateway::new_default(),
            failover_manager: Arc::new(super::super::failover_switch::FailoverSwitchManager::new(
                db,
            )),
        }
    }

    const SOURCE_KEY: &str = concat!("synthetic", "-source-key");
    const OPENAI_KEY: &str = concat!("synthetic", "-openai-key");

    fn provider(base: &str, protocol: Protocol) -> Provider {
        let mut provider = Provider::with_id(
            "group-fixture".into(),
            "Synthetic upstream".into(),
            json!({"env":{
                "ANTHROPIC_BASE_URL":base,
                "ANTHROPIC_API_KEY":SOURCE_KEY,
                "ANTHROPIC_MODEL":"conflicting-default",
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"conflicting-role"
            }}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some(protocol.format().into()),
            ..Default::default()
        });
        provider
    }

    async fn upstream(
        protocol: Protocol,
    ) -> (
        String,
        mpsc::Receiver<(http::Uri, HeaderMap, Value)>,
        tokio::task::JoinHandle<()>,
    ) {
        let (tx, rx) = mpsc::channel(4);
        let response = match protocol {
            Protocol::Anthropic => {
                json!({"id":"msg_fixture","type":"message","role":"assistant","model":"fixture","content":[{"type":"text","text":"synthetic answer"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}})
            }
            Protocol::Chat => {
                json!({"id":"chat_fixture","object":"chat.completion","model":"fixture","choices":[{"index":0,"message":{"role":"assistant","content":"synthetic answer"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}})
            }
            Protocol::Responses => {
                json!({"id":"resp_fixture","object":"response","status":"completed","model":"fixture","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic answer"}]}],"usage":{"input_tokens":1,"output_tokens":1}})
            }
            Protocol::Gemini => {
                json!({"responseId":"gemini_fixture","modelVersion":"fixture","candidates":[{"content":{"role":"model","parts":[{"text":"synthetic answer"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}})
            }
        };
        let app = Router::new()
            .route(
                "/*path",
                post(
                    |State((tx, response)): State<(
                        mpsc::Sender<(http::Uri, HeaderMap, Value)>,
                        Value,
                    )>,
                     OriginalUri(uri): OriginalUri,
                     headers: HeaderMap,
                     body: Bytes| async move {
                        let body = serde_json::from_slice(&body).unwrap();
                        tx.send((uri, headers, body)).await.unwrap();
                        Json(response)
                    },
                ),
            )
            .with_state((tx, response));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{address}"), rx, task)
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn exact_group_bridge_observes_native_tool_calls_before_conversion() {
        use super::super::tool_observation::{channel, mount::ClientToolObservation};
        use crate::services::tool_risk::{response_parser::ResponseProcessor, runtime, worker};
        let previous = (*runtime::get_tool_risk_runtime()).clone();
        struct Restore(runtime::ToolRiskRuntime);
        impl Drop for Restore {
            fn drop(&mut self) {
                runtime::set_tool_risk_runtime_for_test(self.0.clone());
            }
        }
        let _restore = Restore(previous);
        runtime::set_tool_risk_runtime_for_test(runtime::ToolRiskRuntime::build(
            true,
            1,
            crate::services::tool_risk::builtin::builtin_rules(),
            &HashMap::new(),
        ));
        let (_, mut receiver) = channel::test_channel().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let reply = json!({"id":"synthetic-chat","object":"chat.completion","model":"fixture",
            "choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{
                "id":"synthetic-call","type":"function","function":{"name":"exec_command",
                "arguments":"{\"cmd\":\"echo SYNTHETIC_AUDIT_MARKER\"}"}}]},"finish_reason":"tool_calls"}]});
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, Router::new().route("/chat/completions", post(move |Json(request): Json<Value>| {
                let reply=reply.clone();
                async move {
                    if request["stream"] == true {
                        let chunk=json!({"id":"synthetic-chat","object":"chat.completion.chunk","choices":[{"index":0,"delta":{
                            "tool_calls": [{"index":0,"id":"synthetic-call","type":"function","function":{"name":"exec_command","arguments":"{\"cmd\":\"echo SYNTHETIC_AUDIT_MARKER\"}"}}]
                        },"finish_reason":null}]});
                        let done=json!({"id":"synthetic-chat","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]});
                        ([("content-type","text/event-stream")],format!("data: {chunk}\n\ndata: {done}\n\ndata: [DONE]\n\n")).into_response()
                    } else { Json(reply).into_response() }
                }
            }))).await;
        });
        let mut extensions = http::Extensions::new();
        extensions.insert(ClientToolObservation("codex"));
        let mut body = json!({"model":"selected-exact","input":"synthetic","tools":[{
            "type":"function","name":"exec_command","parameters":{"type":"object",
            "properties":{"cmd":{"type":"string"}},"required":["cmd"]}}]});
        for streaming in [false, true] {
            body["stream"] = json!(streaming);
            let response = forward(
                &state(),
                Protocol::Responses,
                &provider(&format!("http://{address}"), Protocol::Chat),
                &http::Method::POST,
                "/responses",
                &body,
                &HeaderMap::new(),
                &extensions,
                None,
                Arc::default(),
                Arc::default(),
            )
            .await
            .unwrap();
            assert!(response.status().is_success());
            let forwarded = response.bytes_with_limit(1024 * 1024).await.unwrap();
            assert!(String::from_utf8_lossy(&forwarded).contains("SYNTHETIC_AUDIT_MARKER"));
            let mut processor = ResponseProcessor::default();
            let observations = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    let piece = receiver.recv().await.unwrap();
                    let observations = processor.ingest(&piece);
                    if !observations.is_empty() {
                        break observations;
                    }
                }
            })
            .await
            .expect("the native upstream tool observation must be delivered");
            assert_eq!(observations.len(), 1);
            assert_eq!(observations[0].tool.client.as_deref(), Some("codex"));
            let event = worker::project_event(&observations[0])
                .expect("recognized shell must be auditable");
            assert_eq!(event.operation_category.as_deref(), Some("shell_exec"));
            assert_eq!(event.integrity_status, "complete");
            assert_eq!(
                event.command_summary.as_deref(),
                Some("echo SYNTHETIC_AUDIT_MARKER")
            );
        }
        task.abort();
    }

    #[tokio::test]
    async fn subscription_bridge_requires_resolution_and_preserves_bearer_and_client_headers() {
        let (base, mut captured, task) = upstream(Protocol::Chat).await;
        let mut provider = provider(&base, Protocol::Chat);
        provider.settings_config["routingSubscription"] =
            json!({"provider":"github_copilot","accountId":"fixture-account"});
        let body = json!({"model":"exact-subscription-model","messages":[{"role":"user","content":"synthetic"}],"max_tokens":32});
        let rejected = forward(
            &state(),
            Protocol::Anthropic,
            &provider,
            &http::Method::POST,
            "/v1/messages",
            &body,
            &HeaderMap::new(),
            &http::Extensions::new(),
            None,
            Arc::default(),
            Arc::default(),
        )
        .await;
        assert!(matches!(rejected, Err(ProxyError::AuthError(_))));
        assert!(captured.try_recv().is_err());
        provider.settings_config["routingSubscriptionResolved"] = json!(true);
        provider
            .meta
            .as_mut()
            .expect("fixture metadata")
            .local_proxy_request_overrides = Some(crate::provider::LocalProxyRequestOverrides {
            headers: [("copilot-integration-id".into(), "vscode-chat".into())]
                .into_iter()
                .collect(),
            ..Default::default()
        });
        let response = forward(
            &state(),
            Protocol::Anthropic,
            &provider,
            &http::Method::POST,
            "/v1/messages",
            &body,
            &HeaderMap::new(),
            &http::Extensions::new(),
            None,
            Arc::default(),
            Arc::default(),
        )
        .await
        .expect("resolved bridge");
        assert!(response.status().is_success());
        let (_, headers, sent) = captured.recv().await.expect("synthetic request");
        assert_eq!(headers["authorization"], "Bearer synthetic-source-key");
        assert_eq!(headers["copilot-integration-id"], "vscode-chat");
        assert_eq!(sent["model"], "exact-subscription-model");
        assert!(!headers.contains_key("x-api-key"));
        task.abort();
    }

    #[tokio::test]
    async fn explicit_bridge_keeps_exact_model_and_source_auth_without_app_mapping() {
        let (base, mut captured, task) = upstream(Protocol::Anthropic).await;
        let state = state();
        let provider = provider(&base, Protocol::Anthropic);
        let headers = HeaderMap::from_iter([
            (
                http::header::AUTHORIZATION,
                http::HeaderValue::from_static("Bearer synthetic-caller"),
            ),
            (
                http::HeaderName::from_static("x-api-key"),
                http::HeaderValue::from_static("synthetic-caller"),
            ),
            (
                http::HeaderName::from_static("x-goog-api-key"),
                http::HeaderValue::from_static("synthetic-caller"),
            ),
        ]);
        let body = json!({"model":"claude-sonnet-fixture[1M]","messages":[{"role":"user","content":"synthetic prompt"}]});
        let response = forward(
            &state,
            Protocol::Chat,
            &provider,
            &http::Method::POST,
            "/v1/chat/completions",
            &body,
            &headers,
            &http::Extensions::new(),
            None,
            Arc::default(),
            Arc::default(),
        )
        .await
        .unwrap();
        assert!(response.status().is_success());
        let (_, headers, body) = captured.recv().await.unwrap();
        assert_eq!(body["model"], "claude-sonnet-fixture[1M]");
        assert_eq!(headers["x-api-key"], "synthetic-source-key");
        assert!(!headers.contains_key("authorization"));
        assert!(!headers.contains_key("x-goog-api-key"));
        assert!(!headers.contains_key("anthropic-beta"));
        assert!(state.current_providers.read().await.is_empty());
        assert_eq!(state.status.read().await.total_requests, 0);
        task.abort();
    }

    #[tokio::test]
    async fn workbuddy_gemini_reasoning_reaches_the_selected_upstream() {
        let (base, mut captured, task) = upstream(Protocol::Gemini).await;
        let provider = provider(&base, Protocol::Gemini);
        let body = json!({"model":"gemini-3.6-flash","messages":[{"role":"user","content":"Synthetic prompt"}],"reasoning_effort":"high","tools":[{"type":"function","function":{"name":"synthetic_tool","parameters":{"type":"object","properties":{"value":{"type":"string"}}}}}]});
        let response = forward(
            &state(),
            Protocol::Chat,
            &provider,
            &http::Method::POST,
            "/v1/chat/completions",
            &body,
            &HeaderMap::new(),
            &http::Extensions::new(),
            None,
            Arc::default(),
            Arc::default(),
        )
        .await
        .expect("WorkBuddy Gemini forwarding");
        assert!(response.status().is_success());
        let (uri, headers, sent) = captured.recv().await.expect("synthetic request");
        assert!(uri.path().ends_with("gemini-3.6-flash:generateContent"));
        assert_eq!(headers["x-goog-api-key"], SOURCE_KEY);
        assert_eq!(
            sent["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "HIGH"
        );
        assert_eq!(
            sent["tools"][0]["functionDeclarations"][0]["name"],
            "synthetic_tool"
        );
        task.abort();
    }

    #[tokio::test]
    async fn normalized_openai_auth_is_used_instead_of_caller_credentials() {
        let (base, mut captured, task) = upstream(Protocol::Chat).await;
        let mut provider = provider(&base, Protocol::Chat);
        provider.settings_config["env"]
            .as_object_mut()
            .unwrap()
            .remove("ANTHROPIC_API_KEY");
        provider.settings_config["auth"] = json!({"OPENAI_API_KEY":OPENAI_KEY});
        let response = forward(&state(), Protocol::Anthropic, &provider, &http::Method::POST, "/v1/messages", &json!({"model":"selected-exact","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}]}), &HeaderMap::new(), &http::Extensions::new(), None, Arc::default(), Arc::default()).await.unwrap();
        assert!(response.status().is_success());
        let (_, headers, body) = captured.recv().await.unwrap();
        assert_eq!(headers["authorization"], "Bearer synthetic-openai-key");
        assert!(!headers.contains_key("x-api-key"));
        assert_eq!(body["model"], "selected-exact");
        task.abort();
    }

    #[tokio::test]
    async fn explicit_bridge_preserves_tool_argument_keys_and_ignores_model_body_overrides() {
        let (base, mut captured, task) = upstream(Protocol::Anthropic).await;
        let mut provider = provider(&base, Protocol::Anthropic);
        provider
            .meta
            .as_mut()
            .unwrap()
            .local_proxy_request_overrides = Some(crate::provider::LocalProxyRequestOverrides {
            body: Some(json!({"model":"conflicting-override"})),
            ..Default::default()
        });
        let input = json!({"_fixture_key":"synthetic value"});
        let body = json!({"model":"selected-exact","messages":[{"role":"assistant","content":null,"tool_calls":[{"id":"call_fixture","type":"function","function":{"name":"read","arguments":input.to_string()}}]}]});
        let response = forward(
            &state(),
            Protocol::Chat,
            &provider,
            &http::Method::POST,
            "/v1/chat/completions",
            &body,
            &HeaderMap::new(),
            &http::Extensions::new(),
            None,
            Arc::default(),
            Arc::default(),
        )
        .await
        .unwrap();
        assert!(response.status().is_success());
        let (_, _, body) = captured.recv().await.unwrap();
        assert_eq!(body["model"], "selected-exact");
        assert_eq!(body["messages"][0]["content"][0]["input"], input);
        task.abort();
    }

    #[tokio::test]
    async fn invalid_conversion_never_contacts_upstream() {
        let (base, mut captured, task) = upstream(Protocol::Anthropic).await;
        let error = forward(
            &state(),
            Protocol::Chat,
            &provider(&base, Protocol::Anthropic),
            &http::Method::POST,
            "/v1/chat/completions",
            &json!({"model":"exact","messages":[{"role":"assistant","tool_calls":[{"id":"synthetic-call","type":"function","function":{"name":"synthetic_tool","arguments":"[]"}}]}]}),
            &HeaderMap::new(),
            &http::Extensions::new(),
            None,
            Arc::default(),
            Arc::default(),
        )
        .await;
        assert!(matches!(error, Err(ProxyError::InvalidRequest(_))));
        assert!(captured.try_recv().is_err());
        task.abort();
    }

    #[tokio::test]
    async fn cross_protocol_openai_urls_keep_custom_base_prefix_without_inventing_v1() {
        for output in [Protocol::Chat, Protocol::Responses] {
            let (base, mut captured, task) = upstream(output).await;
            let provider = provider(&format!("{base}/relay"), output);
            let response = forward(&state(), Protocol::Anthropic, &provider, &http::Method::POST, "/v1/messages?fixture=true", &json!({"model":"selected-exact","max_tokens":32,"messages":[{"role":"user","content":"synthetic"}]}), &HeaderMap::new(), &http::Extensions::new(), None, Arc::default(), Arc::default()).await.unwrap();
            assert!(response.status().is_success());
            let (uri, _, body) = captured.recv().await.unwrap();
            let expected = if output == Protocol::Chat {
                "/relay/chat/completions?fixture=true"
            } else {
                "/relay/responses?fixture=true"
            };
            assert_eq!(uri.to_string(), expected);
            assert_eq!(body["model"], "selected-exact");
            task.abort();
        }
    }

    #[tokio::test]
    async fn gemini_target_reserved_model_characters_fail_before_network() {
        let (base, mut captured, task) = upstream(Protocol::Gemini).await;
        let provider = provider(&base, Protocol::Gemini);
        for model in [
            "fixture?key=injected",
            "fixture#fragment",
            "../fixture",
            "fixture%2Fother",
            "fixture:generateContent",
            "fixture\\other",
        ] {
            let result = forward(&state(), Protocol::Anthropic, &provider, &http::Method::POST, "/v1/messages", &json!({"model":model,"max_tokens":32,"messages":[{"role":"user","content":"synthetic"}]}), &HeaderMap::new(), &http::Extensions::new(), None, Arc::default(), Arc::default()).await;
            assert!(matches!(result, Err(ProxyError::InvalidRequest(_))));
        }
        assert!(captured.try_recv().is_err());
        task.abort();
    }

    #[tokio::test]
    async fn gemini_source_url_model_never_overrides_exact_target_model() {
        let (base, mut captured, task) = upstream(Protocol::Anthropic).await;
        let response = forward(&state(), Protocol::Gemini, &provider(&base, Protocol::Anthropic), &http::Method::POST, "/v1beta/models/source-fixture:generateContent?key=synthetic-caller&alt=sse", &json!({"model":"selected-target[1M]","contents":[{"role":"user","parts":[{"text":"synthetic"}]}]}), &HeaderMap::new(), &http::Extensions::new(), None, Arc::default(), Arc::default()).await.unwrap();
        assert!(response.status().is_success());
        let (uri, _, body) = captured.recv().await.unwrap();
        assert_eq!(uri.path(), "/v1/messages");
        assert!(uri.query().is_none());
        assert_eq!(body["model"], "selected-target[1M]");
        task.abort();
    }

    #[test]
    fn canonical_projection_preserves_text_and_tool_continuity_without_validating_extensions() {
        let body = json!({"model":"fixture","custom_native_extension":{"anything":true},"messages":[
            {"role":"user","content":"synthetic prompt"},
            {"role":"assistant","content":null,"tool_calls":[{"id":"call_fixture","type":"function","function":{"name":"read","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"call_fixture","content":"synthetic result"}
        ]});
        let projected = canonical_request(Protocol::Chat, &body, "/v1/chat/completions").unwrap();
        assert_eq!(
            projected["input"][0]["content"][0]["text"],
            "synthetic prompt"
        );
        assert!(projected["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "function_call" && item["call_id"] == "call_fixture"));
        assert!(projected["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |item| item["type"] == "function_call_output" && item["call_id"] == "call_fixture"
            ));
        assert!(body.get("custom_native_extension").is_some());
    }

    #[test]
    fn gemini_projection_retains_tool_ids_and_marks_opaque_history() {
        let body = json!({"contents":[
            {"role":"user","parts":[{"text":"synthetic prompt"}]},
            {"role":"assistant","parts":[{"functionCall":{"id":"call_fixture","name":"read","args":{}}}]},
            {"role":"user","parts":[{"functionResponse":{"id":"call_fixture","name":"read","response":{"result":"synthetic"}}}]},
            {"role":"model","parts":[{"text":"opaque reasoning","thought":true}]}
        ]});
        let projected = canonical_request(
            Protocol::Gemini,
            &body,
            "/v1beta/models/source-fixture:generateContent",
        )
        .unwrap();
        assert_eq!(projected["model"], "source-fixture");
        assert_eq!(
            projected["input"][0]["content"][0]["text"],
            "synthetic prompt"
        );
        assert_eq!(projected["input"][1]["type"], "function_call");
        assert_eq!(projected["input"][1]["call_id"], "call_fixture");
        assert_eq!(projected["input"][2]["type"], "function_call_output");
        assert_eq!(projected["input"][2]["call_id"], "call_fixture");
        assert_eq!(projected["_group_projection_incomplete"], true);
    }

    #[test]
    fn canonical_projection_marks_omitted_content_and_does_not_rewrite_responses() {
        let body = json!({"messages":[{"role":"user","content":[{"type":"text","text":"synthetic"},{"type":"future_content","payload":true}]}]});
        let projected = canonical_request(Protocol::Anthropic, &body, "/v1/messages").unwrap();
        assert_eq!(projected["_group_projection_incomplete"], true);
        assert_eq!(projected["input"][0]["content"][0]["text"], "synthetic");
        let responses = json!({"input":"synthetic","previous_response_id":"resp_fixture","custom_native_extension":123});
        assert_eq!(
            canonical_request(Protocol::Responses, &responses, "/v1/responses").unwrap(),
            responses
        );
    }
}
