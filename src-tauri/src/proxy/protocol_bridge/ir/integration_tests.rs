//! Native wire fixtures exercise the shared ingress, inspection and transport seams.
use super::{parse, reply, stream, Part, Protocol};
use crate::{
    database::Database,
    provider::{Provider, ProviderMeta},
    proxy::{
        intent::{Menu, Runtime, Session, Target},
        security::{
            runtime::{set_security_runtime_for_test, CompiledCustomRule},
            AuditAction, RiskLevel, SecurityRuntimeConfig,
        },
        server::ProxyServer,
        ProxyConfig,
    },
    services::{agent_connections::Receipt, intent_routing::RoutingGroup},
};
use axum::{extract::OriginalUri, routing::post, Json, Router};
use http::{HeaderMap, Uri};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

const CLIENT_KEY: &str = "synthetic-client-connection";
const SOURCE_KEY: &str = "synthetic-selected-upstream";
const WIRE: &str = "synthetic-picker-model";
const MODEL: &str = "gemini-3.6-flash";
const SENSITIVE: &str = "SYNTHETIC_IR_SENSITIVE";

fn args() -> Value {
    json!({"model":"argument-model","annotations":[],"_fixture_key":"kept"})
}
fn schema() -> Value {
    json!({"type":"object","properties":{"model":{"type":"string"},"annotations":{"type":"array"},"_fixture_key":{"type":"string"}}})
}

fn request(protocol: Protocol, streaming: bool, history: bool) -> Value {
    let mut value = match protocol {
        Protocol::Chat => {
            json!({"model":WIRE,"messages":[{"role":"system","content":"Synthetic system"},{"role":"user","content":"Synthetic initial turn"}],"max_completion_tokens":128,"temperature":0.25,"top_p":0.9,"stop":["fixture-stop"],"tools":[{"type":"function","function":{"name":"synthetic_tool","parameters":schema(),"strict":false}}]})
        }
        Protocol::Responses => {
            json!({"model":WIRE,"instructions":"Synthetic system","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"Synthetic initial turn"}]}],"max_output_tokens":128,"temperature":0.25,"top_p":0.9,"tools":[{"type":"function","name":"synthetic_tool","parameters":schema(),"strict":false}]})
        }
        Protocol::Anthropic => {
            json!({"model":WIRE,"system":"Synthetic system","messages":[{"role":"user","content":"Synthetic initial turn"}],"max_tokens":128,"temperature":0.25,"top_p":0.9,"stop_sequences":["fixture-stop"],"tools":[{"name":"synthetic_tool","input_schema":schema()}]})
        }
        Protocol::Gemini => {
            json!({"systemInstruction":{"parts":[{"text":"Synthetic system"}]},"contents":[{"role":"user","parts":[{"text":"Synthetic initial turn"}]}],"generationConfig":{"maxOutputTokens":128,"temperature":0.25,"topP":0.9,"stopSequences":["fixture-stop"]},"tools":[{"functionDeclarations":[{"name":"synthetic_tool","parametersJsonSchema":schema()}]}]})
        }
    };
    if protocol != Protocol::Gemini {
        value["stream"] = json!(streaming);
    }
    if history {
        match protocol {
            Protocol::Chat => value["messages"].as_array_mut().unwrap().extend([
                json!({"role":"assistant","content":null,"tool_calls":[{"id":"fixture-call","type":"function","function":{"name":"synthetic_tool","arguments":args().to_string()}}]}),
                json!({"role":"tool","tool_call_id":"fixture-call","content":"Synthetic tool result"}),
                json!({"role":"user","content":SENSITIVE}),
            ]),
            Protocol::Responses => value["input"].as_array_mut().unwrap().extend([
                json!({"type":"function_call","call_id":"fixture-call","name":"synthetic_tool","arguments":args().to_string()}),
                json!({"type":"function_call_output","call_id":"fixture-call","output":"Synthetic tool result"}),
                json!({"type":"message","role":"user","content":[{"type":"input_text","text":SENSITIVE}]}),
            ]),
            Protocol::Anthropic => value["messages"].as_array_mut().unwrap().extend([
                json!({"role":"assistant","content":[{"type":"tool_use","id":"fixture-call","name":"synthetic_tool","input":args()}]}),
                json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"fixture-call","content":"Synthetic tool result"},{"type":"text","text":SENSITIVE}]}),
            ]),
            Protocol::Gemini => value["contents"].as_array_mut().unwrap().extend([
                json!({"role":"model","parts":[{"functionCall":{"id":"fixture-call","name":"synthetic_tool","args":args()}}]}),
                json!({"role":"user","parts":[{"functionResponse":{"id":"fixture-call","name":"synthetic_tool","response":{"output":"Synthetic tool result"}}},{"text":SENSITIVE}]}),
            ]),
        }
    }
    value
}

fn endpoint(protocol: Protocol, streaming: bool) -> String {
    match protocol {
        Protocol::Anthropic => "/v1/messages".into(),
        Protocol::Chat => "/v1/chat/completions".into(),
        Protocol::Responses => "/v1/responses".into(),
        Protocol::Gemini => format!(
            "/v1beta/models/{WIRE}:{}{}",
            if streaming {
                "streamGenerateContent"
            } else {
                "generateContent"
            },
            if streaming { "?alt=sse" } else { "" }
        ),
    }
}

fn response(protocol: Protocol) -> Value {
    match protocol {
        Protocol::Anthropic => {
            json!({"id":"fixture-reply","type":"message","role":"assistant","model":MODEL,"content":[{"type":"text","text":"Synthetic reply"},{"type":"tool_use","id":"fixture-call","name":"synthetic_tool","input":args()}],"stop_reason":"tool_use","usage":{"input_tokens":7,"output_tokens":3}})
        }
        Protocol::Chat => {
            json!({"id":"fixture-reply","object":"chat.completion","model":MODEL,"choices":[{"index":0,"message":{"role":"assistant","content":"Synthetic reply","tool_calls":[{"id":"fixture-call","type":"function","function":{"name":"synthetic_tool","arguments":args().to_string()}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":7,"completion_tokens":3}})
        }
        Protocol::Responses => {
            json!({"id":"fixture-reply","object":"response","status":"completed","model":MODEL,"output":[{"id":"fixture-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Synthetic reply","annotations":[]}]},{"id":"fixture-call-item","type":"function_call","call_id":"fixture-call","name":"synthetic_tool","arguments":args().to_string(),"status":"completed"}],"usage":{"input_tokens":7,"output_tokens":3}})
        }
        Protocol::Gemini => {
            json!({"responseId":"fixture-reply","modelVersion":MODEL,"candidates":[{"content":{"role":"model","parts":[{"text":"Synthetic reply"},{"functionCall":{"id":"fixture-call","name":"synthetic_tool","args":args()}}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":3}})
        }
    }
}

struct ResetSecurity(SecurityRuntimeConfig);
impl Drop for ResetSecurity {
    fn drop(&mut self) {
        set_security_runtime_for_test(self.0.clone());
    }
}

fn runtime(base: &str, target: Protocol) -> Arc<Runtime> {
    let group: RoutingGroup = serde_json::from_value(json!({"id":"ir-wire-fixture","name":"Synthetic IR fixture","revision":0,"client":"codex","enabled":true,"members":[{"id":"only","providerId":"fixture-source","modelId":MODEL}],"defaultMemberId":"only","fallbackMemberIds":[],"intentRecognitionEnabled":false,"classifierTarget":null,"classificationBudgetMs":2000,"rules":[],"levels":[],"affinity":"strict"})).unwrap();
    let mut provider = Provider::with_id(
        "fixture-source".into(),
        "Synthetic source".into(),
        json!({"env":{"ANTHROPIC_BASE_URL":base,"ANTHROPIC_API_KEY":SOURCE_KEY,"ANTHROPIC_MODEL":"conflicting-default"}}),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some(target.format().into()),
        ..Default::default()
    });
    let alias = group.alias();
    let menu = Menu {
        token: "synthetic-menu".into(),
        models: Default::default(),
        default_model: Some(alias.clone()),
        omitted_entries: 0,
        context_settings: Default::default(),
        groups: HashMap::from([(
            alias.clone(),
            Arc::new(Session {
                group,
                targets: vec![Target {
                    provider_id: provider.id.clone(),
                    model: MODEL.into(),
                    fixed_effort: None,
                    upstream: Some(provider),
                    effort: Default::default(),
                    url: base.parse().unwrap(),
                    headers: HeaderMap::new(),
                    official: false,
                    credentials: None,
                    scheduling: Arc::default(),
                }],
                classifier: None,
                client: reqwest::Client::builder().no_proxy().build().unwrap(),
                created: Instant::now(),
                bindings: Arc::default(),
            }),
        )]),
    };
    let runtime = Arc::new(Runtime::default());
    runtime.set_global(menu, 0).unwrap();
    runtime
        .connections
        .install(&Receipt {
            app: "workbuddy".into(),
            token: CLIENT_KEY.into(),
            model: alias.clone(),
            model_map: std::collections::BTreeMap::from([(WIRE.into(), alias)]),
            visible_models: None,
            files: vec![],
            suspended: false,
            applied_at_ms: None,
        })
        .unwrap();
    runtime
}

fn verify_wire(target: Protocol, streaming: bool, uri: &Uri, headers: &HeaderMap, body: &Value) {
    assert!(
        !body.to_string().contains(SENSITIVE),
        "inspection must precede conversion"
    );
    assert!(!headers.values().any(|v| v == CLIENT_KEY));
    match target {
        Protocol::Anthropic => {
            assert!(uri.path().ends_with("/v1/messages"));
            assert_eq!(headers["x-api-key"], SOURCE_KEY);
            assert!(!headers.contains_key("authorization"));
            assert_eq!(body["max_tokens"], 128);
            assert!(body["tools"][0]["input_schema"].is_object());
            assert!(body.get("input").is_none() && body.get("contents").is_none());
        }
        Protocol::Chat | Protocol::Responses => {
            assert!(uri.path().ends_with(if target == Protocol::Chat {
                "/chat/completions"
            } else {
                "/responses"
            }));
            assert_eq!(headers["authorization"], format!("Bearer {SOURCE_KEY}"));
            assert!(!headers.contains_key("x-api-key") && !headers.contains_key("x-goog-api-key"));
            if target == Protocol::Chat {
                assert_eq!(body["max_completion_tokens"], 128);
                assert!(body["tools"][0]["function"]["parameters"].is_object());
                assert!(body.get("system").is_none() && body.get("input").is_none());
            } else {
                assert_eq!(body["max_output_tokens"], 128);
                assert!(body["tools"][0]["parameters"].is_object());
                assert!(body.get("messages").is_none() && body.get("system").is_none());
            }
        }
        Protocol::Gemini => {
            assert!(uri.path().ends_with(&format!(
                "{MODEL}:{}",
                if streaming {
                    "streamGenerateContent"
                } else {
                    "generateContent"
                }
            )));
            assert_eq!(headers["x-goog-api-key"], SOURCE_KEY);
            assert!(!headers.contains_key("authorization") && !headers.contains_key("x-api-key"));
            assert_eq!(body["generationConfig"]["maxOutputTokens"], 128);
            assert!(
                body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"].is_object()
            );
            for key in [
                "model",
                "stream",
                "messages",
                "input",
                "system",
                "max_tokens",
            ] {
                assert!(
                    body.get(key).is_none(),
                    "Gemini wire must omit transport/non-native hints"
                );
            }
            assert_eq!(uri.query(), if streaming { Some("alt=sse") } else { None });
        }
    }
    if target != Protocol::Gemini {
        assert_eq!(body["model"], MODEL);
        assert_eq!(body["stream"], streaming);
    }
    let parsed = parse::request(target, body, &uri.to_string()).unwrap();
    assert_eq!(parsed.max_tokens, Some(128));
    assert_eq!(parsed.temperature, Some(json!(0.25)));
    assert_eq!(parsed.top_p, Some(json!(0.9)));
    assert!(parsed.messages.iter().flat_map(|m| &m.parts).any(|p| matches!(p, Part::ToolCall { name, arguments, .. } if name == "synthetic_tool" && arguments == &args())));
    assert!(parsed
        .messages
        .iter()
        .flat_map(|m| &m.parts)
        .any(|p| matches!(p, Part::ToolResult { .. })));
}

#[tokio::test]
#[serial_test::serial]
async fn all_twelve_ir_edges_forward_native_wires_after_security_inspection(
) -> Result<(), Box<dyn std::error::Error>> {
    let _security = ResetSecurity((*crate::proxy::security::get_security_runtime()).clone());
    set_security_runtime_for_test(SecurityRuntimeConfig {
        custom_rules: vec![CompiledCustomRule {
            id: "synthetic-ir-order".into(),
            name: "Synthetic marker".into(),
            rule_type: "keyword".into(),
            pattern: SENSITIVE.into(),
            regex: None,
            risk_level: RiskLevel::Medium,
            action: AuditAction::Redact,
            enabled: true,
        }],
        ..Default::default()
    });
    let protocols = [
        Protocol::Anthropic,
        Protocol::Chat,
        Protocol::Responses,
        Protocol::Gemini,
    ];
    for input in protocols {
        for output in protocols.into_iter().filter(|output| *output != input) {
            for streaming in [false, true] {
                let (tx, mut rx) = tokio::sync::mpsc::channel(4);
                let native_reply = response(output);
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
                let upstream_address = listener.local_addr()?;
                let app = Router::new().route(
                    "/*path",
                    post(
                        move |OriginalUri(uri): OriginalUri,
                              headers: HeaderMap,
                              Json(body): Json<Value>| {
                            let tx = tx.clone();
                            let reply = native_reply.clone();
                            async move {
                                let _ = tx.send((uri, headers, body)).await;
                                Json(reply)
                            }
                        },
                    ),
                );
                let upstream = tokio::spawn(async move { axum::serve(listener, app).await });
                let runtime = runtime(&format!("http://{upstream_address}"), output);
                let server = ProxyServer::new(
                    ProxyConfig {
                        listen_port: 0,
                        ..Default::default()
                    },
                    Arc::new(Database::memory()?),
                    None,
                )
                .with_intent_runtime(runtime);
                let ingress = server.start().await?;
                assert_ne!(ingress.port, upstream_address.port());
                let client = reqwest::Client::builder().no_proxy().build()?;
                for history in [false, true] {
                    let client_stream = history && streaming;
                    let response = client
                        .post(format!(
                            "http://127.0.0.1:{}/connections/workbuddy{}",
                            ingress.port,
                            endpoint(input, client_stream)
                        ))
                        .bearer_auth(CLIENT_KEY)
                        .header("x-conversation-id", "synthetic-ir-conversation")
                        .header("x-api-key", CLIENT_KEY)
                        .header("x-goog-api-key", CLIENT_KEY)
                        .json(&request(input, client_stream, history))
                        .send()
                        .await?;
                    assert_eq!(
                        response.status(),
                        http::StatusCode::OK,
                        "edge {input:?}->{output:?}, stream={client_stream}, history={history}"
                    );
                    let bytes = response.bytes().await?;
                    let (uri, headers, body) =
                        tokio::time::timeout(Duration::from_secs(2), rx.recv())
                            .await?
                            .ok_or("missing native request")?;
                    if history {
                        verify_wire(output, client_stream, &uri, &headers, &body);
                    }
                    let reply = if client_stream {
                        let chunk = bytes.clone();
                        stream::collect(
                            Box::pin(futures::stream::once(async move { Ok(chunk) })),
                            input,
                        )
                        .await?
                    } else {
                        reply::parse(input, &serde_json::from_slice(&bytes)?)?
                    };
                    assert!(reply.parts.iter().any(
                        |p| matches!(p, Part::Text { text, .. } if text == "Synthetic reply")
                    ));
                    assert!(reply.parts.iter().any(|p| matches!(p, Part::ToolCall { name, arguments, .. } if name == "synthetic_tool" && arguments == &args())));
                }
                server.stop().await?;
                upstream.abort();
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn classic_ir_routes_map_models_once_and_preserve_tool_keys_after_overrides(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{
        app_config::AppType,
        proxy::{
            failover_switch::FailoverSwitchManager, forwarder::RequestForwarder,
            provider_router::ProviderRouter, providers::ClaudeAdapter,
        },
    };
    for (app, input, output) in [
        (AppType::Claude, Protocol::Anthropic, Protocol::Gemini),
        (AppType::WorkBuddy, Protocol::Chat, Protocol::Anthropic),
    ] {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let native_reply = response(output);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let service = Router::new().route(
            "/*path",
            post(
                move |OriginalUri(uri): OriginalUri, Json(body): Json<Value>| {
                    let tx = tx.clone();
                    let reply = native_reply.clone();
                    async move {
                        let _ = tx.send((uri, body)).await;
                        Json(reply)
                    }
                },
            ),
        );
        let upstream = tokio::spawn(async move { axum::serve(listener, service).await });
        let db = Arc::new(Database::memory()?);
        let forwarder = RequestForwarder::new(
            Arc::new(ProviderRouter::new(db.clone())),
            0,
            Arc::default(),
            Arc::default(),
            Arc::default(),
            Arc::default(),
            Arc::new(FailoverSwitchManager::new(db)),
            None,
            String::new(),
            String::new(),
            false,
            0,
            0,
            Default::default(),
            Default::default(),
            Default::default(),
            0,
        );
        let mut provider = Provider::with_id(
            "classic-fixture".into(),
            "Synthetic classic source".into(),
            json!({"env":{"ANTHROPIC_BASE_URL":format!("http://{address}"),"ANTHROPIC_API_KEY":SOURCE_KEY,"ANTHROPIC_MODEL":MODEL}}),
            None,
        );
        let patch = if output == Protocol::Gemini {
            json!({"generationConfig":{"temperature":0.5}})
        } else {
            json!({"temperature":0.5})
        };
        provider.meta = Some(ProviderMeta {
            api_format: Some(output.format().into()),
            local_proxy_request_overrides: Some(crate::provider::LocalProxyRequestOverrides {
                body: Some(patch),
                ..Default::default()
            }),
            ..Default::default()
        });
        let mut body = request(input, false, true);
        body["model"] = json!("claude-sonnet-client-alias");
        let (response, ..) = forwarder
            .forward(
                &app,
                &http::Method::POST,
                &provider,
                &endpoint(input, false),
                &body,
                &HeaderMap::new(),
                &http::Extensions::new(),
                &ClaudeAdapter::new(),
            )
            .await?;
        assert!(response.status().is_success());
        let _ = response.bytes_with_limit(1024 * 1024).await?;
        let (uri, sent) = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await?
            .ok_or("missing classic request")?;
        let parsed = parse::request(output, &sent, &uri.to_string())?;
        assert_eq!(parsed.model, MODEL);
        assert_eq!(parsed.temperature, Some(json!(0.5)));
        assert!(parsed
            .messages
            .iter()
            .flat_map(|m| &m.parts)
            .any(|p| matches!(p, Part::ToolCall { arguments, .. } if arguments == &args())));
        upstream.abort();
    }
    Ok(())
}

struct ResetTestHome(Option<std::ffi::OsString>);
impl Drop for ResetTestHome {
    fn drop(&mut self) {
        match &self.0 {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        let _ = crate::settings::reload_settings();
    }
}

#[tokio::test]
#[serial_test::serial]
async fn classic_claude_codex_and_grok_handlers_do_not_convert_ir_replies_twice(
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::app_config::AppType;
    let home = tempfile::tempdir()?;
    let _home = ResetTestHome(std::env::var_os("CC_SWITCH_TEST_HOME"));
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());
    crate::settings::reload_settings()?;
    let _security = ResetSecurity((*crate::proxy::security::get_security_runtime()).clone());
    set_security_runtime_for_test(Default::default());
    for (app, input, route, outputs) in [
        (
            AppType::Claude,
            Protocol::Anthropic,
            "/v1/messages",
            [Protocol::Chat, Protocol::Responses, Protocol::Gemini],
        ),
        (
            AppType::Codex,
            Protocol::Responses,
            "/v1/responses",
            [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini],
        ),
        (
            AppType::GrokBuild,
            Protocol::Responses,
            "/grokbuild/v1/responses",
            [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini],
        ),
    ] {
        for output in outputs {
            for streaming in [false, true] {
                let (tx, mut rx) = tokio::sync::mpsc::channel(1);
                let native_reply = response(output);
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
                let address = listener.local_addr()?;
                let service = Router::new().route(
                    "/*path",
                    post(move |Json(body): Json<Value>| {
                        let tx = tx.clone();
                        let reply = native_reply.clone();
                        async move {
                            let _ = tx.send(body).await;
                            Json(reply)
                        }
                    }),
                );
                let upstream = tokio::spawn(async move { axum::serve(listener, service).await });
                let config = if app == AppType::Claude {
                    json!({"env":{"ANTHROPIC_BASE_URL":format!("http://{address}"),"ANTHROPIC_API_KEY":SOURCE_KEY,"ANTHROPIC_MODEL":MODEL}})
                } else {
                    json!({"base_url":format!("http://{address}"),"auth":{"OPENAI_API_KEY":SOURCE_KEY},"model":MODEL})
                };
                let mut provider = Provider::with_id(
                    "classic-ingress-fixture".into(),
                    "Synthetic classic source".into(),
                    config,
                    None,
                );
                provider.meta = Some(ProviderMeta {
                    api_format: Some(output.format().into()),
                    ..Default::default()
                });
                let db = Arc::new(Database::memory()?);
                db.save_provider(app.as_str(), &provider)?;
                db.set_current_provider(app.as_str(), &provider.id)?;
                crate::settings::set_current_provider(&app, Some(&provider.id))?;
                let server = ProxyServer::new(
                    ProxyConfig {
                        listen_port: 0,
                        ..Default::default()
                    },
                    db,
                    None,
                );
                let ingress = server.start().await?;
                assert_ne!(ingress.port, address.port());
                let client = reqwest::Client::builder().no_proxy().build()?;
                let response = client
                    .post(format!("http://127.0.0.1:{}{route}", ingress.port))
                    .json(&request(input, streaming, false))
                    .send()
                    .await?;
                assert_eq!(
                    response.status(),
                    http::StatusCode::OK,
                    "classic {}->{output:?}, stream={streaming}",
                    app.as_str()
                );
                let bytes = response.bytes().await?;
                let _ = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                    .await?
                    .ok_or("missing classic upstream request")?;
                let parsed = if streaming {
                    let chunk = bytes.clone();
                    stream::collect(
                        Box::pin(futures::stream::once(async move { Ok(chunk) })),
                        input,
                    )
                    .await?
                } else {
                    reply::parse(input, &serde_json::from_slice(&bytes)?)?
                };
                assert!(parsed
                    .parts
                    .iter()
                    .any(|p| matches!(p, Part::Text { text, .. } if text == "Synthetic reply")));
                assert!(parsed.parts.iter().any(|p| matches!(p, Part::ToolCall { name, arguments, .. } if name == "synthetic_tool" && arguments == &args())));
                server.stop().await?;
                upstream.abort();
            }
        }
    }
    Ok(())
}
