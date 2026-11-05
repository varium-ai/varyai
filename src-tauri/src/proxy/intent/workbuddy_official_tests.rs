//! WorkBuddy's Chat client can use an explicitly selected managed Responses seat.
use super::*;
use crate::{
    database::Database,
    proxy::{server::ProxyServer, ProxyConfig},
};
use axum::{response::IntoResponse, routing::post, Json, Router};
use std::sync::atomic::{AtomicUsize, Ordering};
const TOKEN: &str = "synthetic-workbuddy-local";
const CALL: &str = "synthetic-official-call";

struct RestoreSecurity(crate::proxy::security::SecurityRuntimeConfig);
impl Drop for RestoreSecurity {
    fn drop(&mut self) {
        crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
    }
}
fn security() -> RestoreSecurity {
    let old = RestoreSecurity((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    old
}
#[derive(Clone, Copy)]
enum Reply {
    Text,
    ToolRound,
    ReasoningToolRound,
    RateLimited,
    Malformed,
    Incomplete,
    Untyped,
}
fn event(kind: &str, value: Value) -> String {
    format!("event: {kind}\ndata: {value}\n\n")
}
fn official_sse(tool: bool) -> String {
    official_sse_with_reasoning(tool, false)
}
fn official_sse_with_reasoning(tool: bool, sealed: bool) -> String {
    let reasoning = json!({"id":"synthetic-native-reasoning","type":"reasoning","summary":[{"type":"summary_text","text":"Synthetic official thought"}],"encrypted_content":"synthetic-private-reasoning-seal"});
    let output_index = usize::from(sealed);
    let item = if tool {
        json!({"id":"synthetic-tool-item","type":"function_call","call_id":CALL,"name":"synthetic_tool","arguments":"{\"value\":7}","status":"completed"})
    } else {
        json!({"id":"synthetic-official-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Synthetic official answer","annotations":[]}]})
    };
    let mut pending = item.clone();
    if tool {
        pending["arguments"] = json!("");
    } else {
        pending["content"] = json!([]);
    }
    pending["status"] = json!("in_progress");
    let output = if sealed {
        vec![reasoning.clone(), item.clone()]
    } else {
        vec![item.clone()]
    };
    let reply = json!({"id":"synthetic-official-reply","object":"response","status":"completed","model":"exact-model","output":output,"usage":{"input_tokens":7,"output_tokens":11,"total_tokens":18}});
    let mut wire = event(
        "response.created",
        json!({"type":"response.created","response":{"id":"synthetic-official-reply","object":"response","status":"in_progress","output":[]}}),
    );
    if sealed {
        wire += &event(
            "response.output_item.added",
            json!({"type":"response.output_item.added","output_index":0,"item":{"id":"synthetic-native-reasoning","type":"reasoning","summary":[]}}),
        );
        wire += &event(
            "response.reasoning_summary_text.delta",
            json!({"type":"response.reasoning_summary_text.delta","item_id":"synthetic-native-reasoning","output_index":0,"summary_index":0,"delta":"Synthetic official thought"}),
        );
        wire += &event(
            "response.output_item.done",
            json!({"type":"response.output_item.done","output_index":0,"item":reasoning}),
        );
    }
    wire += &event(
        "response.output_item.added",
        json!({"type":"response.output_item.added","output_index":output_index,"item":pending}),
    );
    wire += &if tool {
        event(
            "response.function_call_arguments.delta",
            json!({"type":"response.function_call_arguments.delta","item_id":"synthetic-tool-item","output_index":output_index,"delta":"{\"value\":7}"}),
        )
    } else {
        event(
            "response.output_text.delta",
            json!({"type":"response.output_text.delta","item_id":"synthetic-official-message","output_index":output_index,"content_index":0,"delta":"Synthetic official answer"}),
        )
    };
    wire += &event(
        "response.output_item.done",
        json!({"type":"response.output_item.done","output_index":output_index,"item":item}),
    );
    wire += &event(
        "response.completed",
        json!({"type":"response.completed","response":reply}),
    );
    wire
}
struct Fixture {
    server: ProxyServer,
    root: String,
    alias: String,
    client: reqwest::Client,
    requests: tokio::sync::mpsc::Receiver<(http::HeaderMap, Value)>,
    sends: Arc<AtomicUsize>,
    upstream: tokio::task::JoinHandle<()>,
}
impl Fixture {
    async fn new(app: &str, reply: Reply) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (tx, requests) = tokio::sync::mpsc::channel(8);
        let sends = Arc::new(AtomicUsize::new(0));
        let counter = sends.clone();
        let upstream = tokio::spawn(async move {
            let _ = axum::serve(listener, Router::new().route("/responses", post(move |headers: http::HeaderMap, Json(body): Json<Value>| {
                let tx = tx.clone(); let counter = counter.clone();
                async move {
                    let index = counter.fetch_add(1, Ordering::SeqCst);
                    let valid = body["stream"] == true && body["store"] == false && body["instructions"].is_string();
                    let _ = tx.send((headers, body)).await;
                    if !valid { return (StatusCode::BAD_REQUEST, Json(json!({"error":{"message":"Synthetic official contract violation"}}))).into_response(); }
                    let wire = match reply {
                        Reply::RateLimited => return (StatusCode::TOO_MANY_REQUESTS, [(header::RETRY_AFTER,"60")], Json(json!({"error":{"type":"rate_limit_error","message":"synthetic-private-upstream-error"}}))).into_response(),
                        Reply::Malformed => "event: response.completed\ndata: {invalid-synthetic-json}\n\n".into(),
                        Reply::Incomplete => event("response.created", json!({"type":"response.created","response":{"id":"synthetic-incomplete","status":"in_progress","output":[]}})),
                        Reply::ToolRound => official_sse(index == 0),
                        Reply::ReasoningToolRound => official_sse_with_reasoning(index == 0, index == 0),
                        Reply::Text => official_sse(false),
                        Reply::Untyped => return official_sse(false).into_response(),
                    };
                    ([(header::CONTENT_TYPE,"text/event-stream")], wire).into_response()
                }
            }))).await;
        });
        let mut published = global_tests::menu(&format!("http://{address}/responses"));
        let alias = published
            .groups
            .keys()
            .next()
            .ok_or("missing official alias")?
            .clone();
        let session = Arc::make_mut(
            published
                .groups
                .get_mut(&alias)
                .ok_or("missing official session")?,
        );
        session.group.levels.clear();
        session.group.patience = -1;
        session.targets[0].official = true;
        session.targets[0].upstream = None;
        session.targets[0].headers = http::HeaderMap::from_iter([
            (
                header::AUTHORIZATION,
                http::HeaderValue::from_static("Bearer synthetic-official-upstream"),
            ),
            (
                header::HeaderName::from_static("chatgpt-account-id"),
                http::HeaderValue::from_static("synthetic-official-account"),
            ),
        ]);
        let runtime = Arc::new(Runtime::default());
        runtime.set_global(published, 0)?;
        let wire = if app == "claude-desktop" {
            crate::claude_desktop_config::shared_model_name(&alias)
        } else {
            alias.clone()
        };
        runtime
            .connections
            .install(&crate::services::agent_connections::Receipt {
                app: app.into(),
                token: TOKEN.into(),
                model: alias.clone(),
                model_map: std::collections::BTreeMap::from([(wire.clone(), alias.clone())]),
                visible_models: None,
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            })?;
        let server = ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            Arc::new(Database::memory()?),
            None,
        )
        .with_intent_runtime(runtime);
        let info = server.start().await?;
        Ok(Self {
            server,
            root: format!("http://127.0.0.1:{}/connections/{app}/v1", info.port),
            alias: wire,
            client: reqwest::Client::builder().no_proxy().build()?,
            requests,
            sends,
            upstream,
        })
    }
    fn chat(&self, streaming: bool) -> Value {
        json!({"model":self.alias,"stream":streaming,"stream_options":{"include_usage":true},"messages":[{"role":"system","content":"Synthetic official instructions"},{"role":"user","content":"Synthetic official question"}]})
    }
    async fn send(
        &self,
        endpoint: &str,
        body: &Value,
    ) -> Result<reqwest::Response, reqwest::Error> {
        self.client
            .post(format!("{}/{endpoint}", self.root))
            .bearer_auth(TOKEN)
            .header("x-api-key", "synthetic-client-key")
            .header("cookie", "synthetic-client-session")
            .json(body)
            .send()
            .await
    }
    async fn request(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        let (headers, body) = tokio::time::timeout(Duration::from_secs(2), self.requests.recv())
            .await?
            .ok_or("official executor did not send request")?;
        assert_eq!(
            headers[header::AUTHORIZATION],
            "Bearer synthetic-official-upstream"
        );
        assert_eq!(headers["chatgpt-account-id"], "synthetic-official-account");
        assert!(!headers.contains_key("x-api-key"));
        assert!(!headers.contains_key("cookie"));
        assert_eq!(body["model"], "exact-model");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert!(body["instructions"].is_string());
        assert!(body["input"].is_array());
        assert!(body.get("messages").is_none());
        Ok(body)
    }
    async fn stop(self) -> Result<(), Box<dyn std::error::Error>> {
        self.server.stop().await?;
        self.upstream.abort();
        Ok(())
    }
}
async fn chat_reply(
    response: reqwest::Response,
    streaming: bool,
) -> Result<Value, Box<dyn std::error::Error>> {
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let wire = response.text().await?;
    assert!(!wire.contains("synthetic-private-reasoning-seal"));
    assert_eq!(
        status,
        StatusCode::OK,
        "WorkBuddy official request failed, stream={streaming}: {wire}"
    );
    if !streaming {
        assert!(content_type.starts_with("application/json"));
        return Ok(serde_json::from_str(&wire)?);
    }
    assert!(content_type.starts_with("text/event-stream"));
    assert!(wire.contains("[DONE]"));
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool = json!({"type":"function","function":{"arguments":""}});
    let mut arguments = String::new();
    let mut reason = Value::Null;
    let mut usage = Value::Null;
    for data in wire
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
    {
        let frame: Value = serde_json::from_str(data)?;
        let choice = &frame["choices"][0];
        if let Some(delta) = choice["delta"]["content"].as_str() {
            text.push_str(delta);
        }
        if let Some(delta) = choice["delta"]["reasoning_content"].as_str() {
            reasoning.push_str(delta);
        }
        if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
            for part in calls {
                assert_eq!(part["index"], 0);
                if part["id"].is_string() {
                    tool["id"] = part["id"].clone();
                }
                if part["function"]["name"].is_string() {
                    tool["function"]["name"] = part["function"]["name"].clone();
                }
                if let Some(fragment) = part["function"]["arguments"].as_str() {
                    arguments.push_str(fragment);
                }
            }
        }
        if !choice["finish_reason"].is_null() {
            reason = choice["finish_reason"].clone();
        }
        if frame["usage"].is_object() {
            usage = frame["usage"].clone();
        }
    }
    let mut message = json!({"role":"assistant","content":text});
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if tool["id"].is_string() {
        tool["function"]["arguments"] = json!(arguments);
        message["tool_calls"] = json!([tool]);
    }
    Ok(json!({"choices":[{"message":message,"finish_reason":reason}],"usage":usage}))
}
async fn official_text(streaming: bool) -> Result<(), Box<dyn std::error::Error>> {
    let _security = security();
    let mut fixture = Fixture::new("workbuddy", Reply::Text).await?;
    let body = chat_reply(
        fixture
            .send("chat/completions", &fixture.chat(streaming))
            .await?,
        streaming,
    )
    .await?;
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "Synthetic official answer"
    );
    assert_eq!(body["choices"][0]["finish_reason"], "stop");
    assert_eq!(body["usage"]["prompt_tokens"], 7);
    assert_eq!(body["usage"]["completion_tokens"], 11);
    assert_eq!(body["usage"]["total_tokens"], 18);
    let sent = fixture.request().await?;
    assert_eq!(sent["instructions"], "Synthetic official instructions");
    assert!(sent["input"]
        .to_string()
        .contains("Synthetic official question"));
    fixture.stop().await
}
#[tokio::test]
#[serial_test::serial]
async fn workbuddy_official_text_returns_chat_json() -> Result<(), Box<dyn std::error::Error>> {
    official_text(false).await
}
#[tokio::test]
#[serial_test::serial]
async fn workbuddy_official_text_returns_chat_stream() -> Result<(), Box<dyn std::error::Error>> {
    official_text(true).await
}

#[tokio::test]
#[serial_test::serial]
async fn workbuddy_official_transport_supplies_defaults_and_omits_unsupported_sampling(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = security();
    let mut fixture = Fixture::new("workbuddy", Reply::Text).await?;
    let mut request = fixture.chat(false);
    request["messages"]
        .as_array_mut()
        .ok_or("missing messages")?
        .remove(0);
    request["temperature"] = json!(0.7);
    request["top_p"] = json!(0.9);
    request["max_tokens"] = json!(128);
    request["max_completion_tokens"] = json!(128);
    chat_reply(fixture.send("chat/completions", &request).await?, false).await?;
    let sent = fixture.request().await?;
    assert_eq!(sent["instructions"], "");
    assert_eq!(sent["tools"], json!([]));
    assert_eq!(sent["parallel_tool_calls"], true);
    assert!(sent["include"]
        .as_array()
        .ok_or("missing include")?
        .iter()
        .any(|item| item == "reasoning.encrypted_content"));
    for field in [
        "temperature",
        "top_p",
        "max_tokens",
        "max_completion_tokens",
        "max_output_tokens",
    ] {
        assert!(
            sent.get(field).is_none(),
            "unsupported official field {field}"
        );
    }
    fixture.stop().await
}

#[tokio::test]
#[serial_test::serial]
async fn workbuddy_official_reasoning_remains_private_and_returns_for_its_tool_round(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = security();
    for streaming in [false, true] {
        let mut fixture = Fixture::new("workbuddy", Reply::ReasoningToolRound).await?;
        let mut request = fixture.chat(streaming);
        request["tools"] = json!([{"type":"function","function":{"name":"synthetic_tool","parameters":{"type":"object","properties":{"value":{"type":"integer"}}}}}]);
        let reply =
            chat_reply(fixture.send("chat/completions", &request).await?, streaming).await?;
        assert!(!reply
            .to_string()
            .contains("synthetic-private-reasoning-seal"));
        let assistant = reply["choices"][0]["message"].clone();
        assert_eq!(assistant["reasoning_content"], "Synthetic official thought");
        assert_eq!(assistant["tool_calls"][0]["id"], CALL);
        let _ = fixture.request().await?;
        request["messages"].as_array_mut().ok_or("missing messages")?.extend([assistant, json!({"role":"tool","tool_call_id":CALL,"content":"Synthetic private round result"})]);
        let reply =
            chat_reply(fixture.send("chat/completions", &request).await?, streaming).await?;
        assert_eq!(
            reply["choices"][0]["message"]["content"],
            "Synthetic official answer"
        );
        let sent = fixture.request().await?;
        let items = sent["input"].as_array().ok_or("missing input")?;
        let reasoning = items
            .iter()
            .position(|item| item["encrypted_content"] == "synthetic-private-reasoning-seal")
            .ok_or("missing private reasoning continuation")?;
        let call = items
            .iter()
            .position(|item| item["type"] == "function_call" && item["call_id"] == CALL)
            .ok_or("missing continued call")?;
        assert!(reasoning < call);
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn workbuddy_official_tools_continue_in_the_same_scope_for_both_chat_modes(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = security();
    for streaming in [false, true] {
        let mut fixture = Fixture::new("workbuddy", Reply::ToolRound).await?;
        let mut request = fixture.chat(streaming);
        request["tools"] = json!([{"type":"function","function":{"name":"synthetic_tool","description":"Synthetic tool","parameters":{"type":"object","properties":{"value":{"type":"integer"}},"required":["value"]}}}]);
        let reply =
            chat_reply(fixture.send("chat/completions", &request).await?, streaming).await?;
        assert_eq!(reply["choices"][0]["finish_reason"], "tool_calls");
        let assistant = reply["choices"][0]["message"].clone();
        assert_eq!(assistant["tool_calls"][0]["id"], CALL);
        assert_eq!(
            assistant["tool_calls"][0]["function"]["name"],
            "synthetic_tool"
        );
        assert_eq!(
            serde_json::from_str::<Value>(
                assistant["tool_calls"][0]["function"]["arguments"]
                    .as_str()
                    .ok_or("missing call arguments")?
            )?,
            json!({"value":7})
        );
        let first = fixture.request().await?;
        assert_eq!(first["tools"][0]["name"], "synthetic_tool");
        assert_eq!(
            first["tools"][0]["parameters"]["properties"]["value"]["type"],
            "integer"
        );
        request["messages"].as_array_mut().ok_or("missing messages")?.extend([assistant,json!({"role":"tool","tool_call_id":CALL,"content":"Synthetic official tool result"})]);
        let reply =
            chat_reply(fixture.send("chat/completions", &request).await?, streaming).await?;
        assert_eq!(
            reply["choices"][0]["message"]["content"],
            "Synthetic official answer"
        );
        let second = fixture.request().await?;
        let input = second["input"].as_array().ok_or("missing tool history")?;
        let call = input
            .iter()
            .find(|item| item["type"] == "function_call")
            .ok_or("missing forwarded tool call")?;
        assert_eq!(call["call_id"], CALL);
        assert_eq!(
            serde_json::from_str::<Value>(
                call["arguments"]
                    .as_str()
                    .ok_or("missing forwarded arguments")?
            )?,
            json!({"value":7})
        );
        assert!(input
            .iter()
            .any(|item| item["type"] == "function_call_output"
                && item["call_id"] == CALL
                && item["output"] == "Synthetic official tool result"));
        assert_eq!(fixture.sends.load(Ordering::SeqCst), 2);
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn workbuddy_official_rate_limit_keeps_the_existing_routing_refusal(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = security();
    let fixture = Fixture::new("workbuddy", Reply::RateLimited).await?;
    let response = fixture
        .send("chat/completions", &fixture.chat(false))
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body: Value = response.json().await?;
    assert_eq!(body["error"]["code"], "routing_candidates_unavailable");
    assert!(!body
        .to_string()
        .contains("synthetic-private-upstream-error"));
    assert_eq!(fixture.sends.load(Ordering::SeqCst), 1);
    fixture.stop().await
}

#[tokio::test]
#[serial_test::serial]
async fn workbuddy_official_malformed_or_incomplete_sse_never_reports_success(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = security();
    for scenario in [Reply::Malformed, Reply::Incomplete] {
        for streaming in [false, true] {
            let fixture = Fixture::new("workbuddy", scenario).await?;
            let response = fixture
                .send("chat/completions", &fixture.chat(streaming))
                .await?;
            let status = response.status();
            let content = response.text().await;
            if status.is_success() {
                assert!(streaming, "invalid official SSE became successful JSON");
                if let Ok(wire) = content {
                    assert!(wire.contains("protocol_stream_error"));
                    assert!(!wire.contains("[DONE]"));
                    assert!(!wire.contains("\"finish_reason\":\"stop\""));
                }
            } else {
                assert_eq!(status, StatusCode::BAD_GATEWAY);
                let body: Value = serde_json::from_str(&content?)?;
                assert_eq!(body["error"]["code"], "routing_upstream_response_invalid");
            }
            assert_eq!(fixture.sends.load(Ordering::SeqCst), 1);
            fixture.stop().await?;
        }
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn official_native_responses_keep_their_existing_gates(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = security();
    let fixture = Fixture::new("workbuddy", Reply::Text).await?;
    for (stream, store) in [(false, false), (true, true), (false, true)] {
        let response = fixture.send("responses", &json!({"model":fixture.alias,"input":"Synthetic native question","stream":stream,"store":store})).await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.json::<Value>().await?["error"]["code"],
            "routing_official_requires_stream_and_store_false"
        );
    }
    assert_eq!(fixture.sends.load(Ordering::SeqCst), 0);
    fixture.stop().await
}

#[tokio::test]
#[serial_test::serial]
async fn workbuddy_official_sse_with_missing_media_type_uses_the_stream_contract(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = security();
    for streaming in [false, true] {
        let fixture = Fixture::new("workbuddy", Reply::Untyped).await?;
        let reply = chat_reply(
            fixture
                .send("chat/completions", &fixture.chat(streaming))
                .await?,
            streaming,
        )
        .await?;
        assert_eq!(
            reply["choices"][0]["message"]["content"],
            "Synthetic official answer"
        );
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn every_shared_agent_converts_each_native_protocol_to_official_responses(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = security();
    for app in crate::services::agent_connections::APPS
        .iter()
        .copied()
        .chain(std::iter::once("trae"))
        .filter(|app| *app != "codex")
    {
        let mut fixture = Fixture::new(app, Reply::Text).await?;
        for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
            for streaming in [false, true] {
                let (path, body) = match protocol {
                    Protocol::Chat => ("/v1/chat/completions".to_owned(), fixture.chat(streaming)),
                    Protocol::Anthropic => (
                        "/v1/messages".to_owned(),
                        json!({"model":fixture.alias,"stream":streaming,"max_tokens":128,"system":"Synthetic official instructions","messages":[{"role":"user","content":"Synthetic official question"}]}),
                    ),
                    Protocol::Gemini => (
                        format!(
                            "/v1beta/models/{}:{}{}",
                            fixture.alias,
                            if streaming {
                                "streamGenerateContent"
                            } else {
                                "generateContent"
                            },
                            if streaming { "?alt=sse" } else { "" }
                        ),
                        json!({"systemInstruction":{"parts":[{"text":"Synthetic official instructions"}]},"contents":[{"role":"user","parts":[{"text":"Synthetic official question"}]}]}),
                    ),
                    Protocol::Responses => unreachable!(),
                };
                let root = fixture
                    .root
                    .strip_suffix("/v1")
                    .ok_or("missing fixture version")?;
                let response = fixture
                    .client
                    .post(format!("{root}{path}"))
                    .bearer_auth(TOKEN)
                    .json(&body)
                    .send()
                    .await?;
                let status = response.status();
                let text = response.text().await?;
                assert_eq!(
                    status,
                    StatusCode::OK,
                    "{app} {protocol:?} stream={streaming}: {text}"
                );
                assert!(
                    text.contains("Synthetic official answer"),
                    "{app} {protocol:?}"
                );
                let sent = fixture.request().await?;
                assert_eq!(sent["instructions"], "Synthetic official instructions");
                assert_eq!(sent["model"], "exact-model");
            }
        }
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn official_tool_rounds_resume_each_native_protocol_without_session_headers(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = security();
    for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
        for streaming in [false, true] {
            let fixture = Fixture::new("trae", Reply::ToolRound).await?;
            let (path, mut body) = match protocol {
                Protocol::Chat => ("/v1/chat/completions".to_owned(), fixture.chat(streaming)),
                Protocol::Anthropic => (
                    "/v1/messages".to_owned(),
                    json!({"model":fixture.alias,"stream":streaming,"max_tokens":128,"messages":[{"role":"user","content":"Synthetic tool question"}]}),
                ),
                Protocol::Gemini => (
                    format!(
                        "/v1beta/models/{}:{}{}",
                        fixture.alias,
                        if streaming {
                            "streamGenerateContent"
                        } else {
                            "generateContent"
                        },
                        if streaming { "?alt=sse" } else { "" }
                    ),
                    json!({"contents":[{"role":"user","parts":[{"text":"Synthetic tool question"}]}]}),
                ),
                Protocol::Responses => unreachable!(),
            };
            let root = fixture.root.strip_suffix("/v1").ok_or("fixture root")?;
            let url = format!("{root}{path}");
            let response = fixture
                .client
                .post(&url)
                .bearer_auth(TOKEN)
                .json(&body)
                .send()
                .await?;
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = response.bytes().await?;
            let reply = if streaming {
                super::super::protocol_bridge::ir::stream::collect(
                    Box::pin(futures::stream::once(async move { Ok(bytes) })),
                    protocol,
                )
                .await?
            } else {
                super::super::protocol_bridge::ir::reply::parse(
                    protocol,
                    &serde_json::from_slice(&bytes)?,
                )?
            };
            let rendered = super::super::protocol_bridge::ir::reply::render(protocol, &reply)?;
            match protocol {
                Protocol::Chat => body["messages"].as_array_mut().ok_or("messages")?.extend([rendered["choices"][0]["message"].clone(),json!({"role":"tool","tool_call_id":CALL,"content":"Synthetic result"})]),
                Protocol::Anthropic => body["messages"].as_array_mut().ok_or("messages")?.extend([json!({"role":"assistant","content":rendered["content"]}),json!({"role":"user","content":[{"type":"tool_result","tool_use_id":CALL,"content":"Synthetic result"}]})]),
                Protocol::Gemini => body["contents"].as_array_mut().ok_or("contents")?.extend([rendered["candidates"][0]["content"].clone(),json!({"role":"user","parts":[{"functionResponse":{"id":CALL,"name":"synthetic_tool","response":{"result":"Synthetic result"}}}]})]),
                Protocol::Responses => unreachable!(),
            }
            let response = fixture
                .client
                .post(&url)
                .bearer_auth(TOKEN)
                .json(&body)
                .send()
                .await?;
            let status = response.status();
            let text = response.text().await?;
            assert_eq!(
                status,
                StatusCode::OK,
                "{protocol:?} stream={streaming}: {text}"
            );
            assert!(text.contains("Synthetic official answer"));
            fixture.stop().await?;
        }
    }
    Ok(())
}
