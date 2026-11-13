//! Exercise shared Agent identities and native Codex through real local HTTP transport.
use super::*;
use crate::{
    database::Database,
    proxy::{server::ProxyServer, ProxyConfig},
};
use axum::{extract::OriginalUri, routing::post, Json, Router};

fn reply(protocol: Protocol, text: &str, stream: bool, id: &str) -> Response {
    let complete = match protocol {
        Protocol::Chat => {
            json!({"id":id,"object":"chat.completion","model":"exact-model","choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})
        }
        Protocol::Responses => {
            json!({"id":id,"object":"response","status":"completed","model":"exact-model","output":[{"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]}],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}})
        }
        Protocol::Anthropic => {
            json!({"id":id,"type":"message","role":"assistant","model":"exact-model","content":[{"type":"text","text":text}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}})
        }
        Protocol::Gemini => {
            json!({"responseId":id,"modelVersion":"exact-model","candidates":[{"index":0,"content":{"role":"model","parts":[{"text":text}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2}})
        }
    };
    if !stream {
        return Json(complete).into_response();
    }
    let event = |name: &str, value: Value| format!("event: {name}\ndata: {value}\n\n");
    let data = |value: Value| format!("data: {value}\n\n");
    let frames = match protocol {
        Protocol::Chat => format!("{}{}data: [DONE]\n\n", data(json!({"id":id,"object":"chat.completion.chunk","model":"exact-model","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]})), data(json!({"id":id,"object":"chat.completion.chunk","model":"exact-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}))),
        Protocol::Responses => event("response.completed", json!({"type":"response.completed","response":complete})),
        Protocol::Anthropic => [
            event("message_start",json!({"type":"message_start","message":{"id":id,"type":"message","role":"assistant","model":"exact-model","content":[],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":0}}})),
            event("content_block_start",json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})),
            event("content_block_delta",json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}})),
            event("content_block_stop",json!({"type":"content_block_stop","index":0})),
            event("message_delta",json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}})),
            event("message_stop",json!({"type":"message_stop"})),
        ].concat(),
        Protocol::Gemini => data(complete),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(frames))
        .expect("synthetic SSE")
}

async fn matrix(protocol: Protocol) -> Result<(), Box<dyn std::error::Error>> {
    struct Reset(crate::proxy::security::SecurityRuntimeConfig);
    impl Drop for Reset {
        fn drop(&mut self) {
            crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = Reset((*crate::proxy::security::get_security_runtime()).clone());
    let mut policy = crate::proxy::security::SecurityRuntimeConfig::default();
    for rule in ["chinese_phone_number", "chinese_id_card"] {
        policy.builtin_action_overrides.insert(
            rule.into(),
            crate::proxy::security::AuditAction::RedactRestore,
        );
    }
    crate::proxy::security::runtime::set_security_runtime_for_test(policy);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let sequence = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/*path",
                post(
                    move |OriginalUri(uri): OriginalUri, Json(body): Json<Value>| {
                        let tx = tx.clone();
                        let sequence = sequence.clone();
                        async move {
                            let last = match protocol {
                                Protocol::Responses => body["input"]
                                    .as_array()
                                    .and_then(|v| v.last())
                                    .map(|v| v["content"].clone()),
                                Protocol::Gemini => body["contents"]
                                    .as_array()
                                    .and_then(|v| v.last())
                                    .map(|v| v["parts"][0]["text"].clone()),
                                _ => body["messages"]
                                    .as_array()
                                    .and_then(|v| v.last())
                                    .map(|v| v["content"].clone()),
                            }
                            .unwrap_or(Value::Null);
                            let text = last
                                .as_str()
                                .or_else(|| last[0]["text"].as_str())
                                .unwrap_or("missing synthetic text")
                                .to_owned();
                            let stream = body["stream"] == true
                                || uri.path().ends_with(":streamGenerateContent");
                            let id = format!(
                                "synthetic-{}",
                                sequence.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                            );
                            let _ = tx.send((uri, body)).await;
                            reply(protocol, &text, stream, &id)
                        }
                    },
                ),
            ),
        )
        .await
    });
    let endpoint = match protocol {
        Protocol::Chat => "/v1/chat/completions",
        Protocol::Responses => "/v1/responses",
        Protocol::Anthropic => "/v1/messages",
        Protocol::Gemini => "/v1beta/models/exact-model:generateContent",
    };
    let mut menu = super::global_tests::menu(&format!("http://{address}{endpoint}"));
    let session = Arc::make_mut(menu.groups.values_mut().next().ok_or("session")?);
    session.group.levels.clear();
    session.group.routing = crate::services::intent_routing::RoutingStrategy::Manual;
    let base = if protocol == Protocol::Gemini {
        format!("http://{address}")
    } else {
        format!("http://{address}/v1")
    };
    let mut upstream = crate::provider::Provider::with_id(
        "fixture".into(),
        "Synthetic".into(),
        json!({"base_url":base,"auth":{"OPENAI_API_KEY":"synthetic-upstream"},"env":{"ANTHROPIC_BASE_URL":format!("http://{address}"),"ANTHROPIC_AUTH_TOKEN":"synthetic-upstream","GOOGLE_GEMINI_BASE_URL":format!("http://{address}"),"GEMINI_API_KEY":"synthetic-upstream"},"apiFormat":protocol.format(),"config":format!("model_provider='fixture'\n[model_providers.fixture]\nbase_url='{base}'\nwire_api='{}'\n",if protocol==Protocol::Chat {"chat"}else{"responses"})}),
        None,
    );
    upstream.meta = Some(crate::provider::ProviderMeta {
        api_format: Some(protocol.format().into()),
        ..Default::default()
    });
    session.targets[0].upstream = Some(upstream);
    let alias = session.group.alias();
    menu.models.insert(
        "fixture".into(),
        MenuModel {
            alias: alias.clone(),
            label: "Synthetic model".into(),
            description: "VaryAI routing group",
            order: 0,
        },
    );
    let runtime = Arc::new(Runtime::default());
    let native_home = tempfile::tempdir()?;
    if protocol == Protocol::Responses {
        runtime
            .native
            .prepare_checkpoints(&native_home.path().join("checkpoint.key"))?;
        runtime.native.publish_menu(
            Arc::new(menu.clone()),
            json!({"models":[{"slug":"synthetic-model"}]}),
            std::collections::BTreeMap::from([("synthetic-model".into(), alias.clone())]),
        )?;
    }
    runtime.set_menu(Arc::new(menu.clone()))?;
    runtime.set_global(menu, 0)?;
    let mut apps = crate::services::agent_connections::APPS
        .iter()
        .copied()
        .filter(|app| *app != "codex")
        .chain(["trae", "trae-intl"])
        .collect::<Vec<_>>();
    for app in &apps {
        runtime
            .connections
            .install(&crate::services::agent_connections::Receipt {
                app: (*app).into(),
                token: format!("synthetic-{app}"),
                model: alias.clone(),
                model_map: std::collections::BTreeMap::from([(
                    "synthetic-model".into(),
                    alias.clone(),
                )]),
                visible_models: None,
                files: vec![],
                suspended: false,
                applied_at_ms: None,
            })?;
    }
    if protocol == Protocol::Responses {
        apps.push("codex");
    }
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
    let client = reqwest::Client::builder().no_proxy().build()?;
    let contact = "Synthetic contacts: 13800001234 110101199001010015";
    let mut failures = std::collections::BTreeMap::<String, usize>::new();
    for app in apps {
        for stream in [false, true] {
            for followup in [false, true] {
                let mut messages = vec![json!({"role":"user","content":contact})];
                if followup {
                    messages.push(json!({"role":"assistant","content":"Synthetic prior answer"}));
                    messages.push(json!({"role":"user","content":"Synthetic safe followup"}));
                }
                let mut body = json!({"model":"synthetic-model","stream":stream});
                let path = match protocol {
                    Protocol::Gemini => {
                        body.as_object_mut().ok_or("body")?.remove("stream");
                        body["generationConfig"] =
                            json!({"temperature":0.25,"maxOutputTokens":128});
                        body["contents"]=json!(messages.iter().map(|m|json!({"role":if m["role"]=="assistant" {"model"}else{"user"},"parts":[{"text":m["content"]}]})).collect::<Vec<_>>());
                        if stream {
                            "/v1beta/models/synthetic-model:streamGenerateContent?alt=sse"
                        } else {
                            "/v1beta/models/synthetic-model:generateContent"
                        }
                    }
                    Protocol::Responses => {
                        body["input"] = json!(messages);
                        body["max_output_tokens"] = json!(128);
                        "/v1/responses"
                    }
                    Protocol::Chat => {
                        body["messages"] = json!(messages);
                        body["temperature"] = json!(0.25);
                        "/v1/chat/completions"
                    }
                    Protocol::Anthropic => {
                        body["messages"] = json!(messages);
                        body["max_tokens"] = json!(128);
                        "/v1/messages"
                    }
                };
                let path = if app == "codex" {
                    "/backend-api/codex/responses".to_owned()
                } else {
                    format!("/connections/{app}{path}")
                };
                if app != "codex" && !stream && !followup {
                    let denied = client
                        .post(format!("http://127.0.0.1:{}{path}", info.port))
                        .bearer_auth("synthetic-wrong-credential")
                        .json(&body)
                        .send()
                        .await?;
                    assert_eq!(
                        denied.status(),
                        StatusCode::UNAUTHORIZED,
                        "{app}: shared admission requires its credential"
                    );
                    assert!(
                        rx.try_recv().is_err(),
                        "unauthorized requests must not reach upstream"
                    );
                }
                let response = client
                    .post(format!("http://127.0.0.1:{}{path}", info.port))
                    .bearer_auth(format!("synthetic-{app}"))
                    .header("version", "0.167.0")
                    .header("session-id", "synthetic-same-session")
                    .header("x-session-id", "synthetic-same-session")
                    .json(&body)
                    .send()
                    .await?;
                let status = response.status();
                let text = response.text().await?;
                if status != StatusCode::OK {
                    *failures
                        .entry(format!("{app}:{}:status={status}", protocol.format()))
                        .or_default() += 1;
                    while rx.try_recv().is_ok() {}
                    continue;
                }
                let current_policy = crate::proxy::security::get_security_runtime();
                assert!(
                    text.contains(if followup {
                        "Synthetic safe followup"
                    } else {
                        contact
                    }),
                    "{app}:{}:stream={stream}:followup={followup}:client restoration; irreversible={} token={} phone_action={:?} identity_action={:?}",
                    protocol.format(), text.contains("[REDACTED]"), text.contains("rredact-"),
                    current_policy.builtin_action_overrides.get("chinese_phone_number"),
                    current_policy.builtin_action_overrides.get("chinese_id_card")
                );
                let (uri, sent) = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                    .await?
                    .ok_or("upstream capture")?;
                assert!(
                    !sent.to_string().contains("13800001234")
                        && !sent.to_string().contains("110101199001010015"),
                    "{app}:{}:plaintext reached upstream",
                    protocol.format()
                );
                assert!(
                    sent.to_string().contains("rredact-cn_phone-")
                        && sent.to_string().contains("rredact-cn_id_card-"),
                    "{app}:{}:missing mappings",
                    protocol.format()
                );
                match protocol {
                    Protocol::Chat => assert_eq!(sent["temperature"], 0.25),
                    Protocol::Responses => assert_eq!(sent["max_output_tokens"], 128),
                    Protocol::Anthropic => assert_eq!(sent["max_tokens"], 128),
                    Protocol::Gemini => assert_eq!(
                        sent["generationConfig"],
                        json!({"temperature":0.25,"maxOutputTokens":128})
                    ),
                }
                if protocol == Protocol::Gemini {
                    assert!(
                        sent.get("model").is_none(),
                        "Gemini body must not carry group metadata"
                    );
                    assert!(
                        uri.path().contains("/exact-model:"),
                        "actual Gemini target model"
                    );
                } else {
                    assert_eq!(sent["model"], "exact-model");
                }
            }
        }
    }
    server.stop().await?;
    task.abort();
    assert!(
        failures.is_empty(),
        "Agent restoration failures: {failures:?}"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn all_shared_agents_restore_chat() -> Result<(), Box<dyn std::error::Error>> {
    matrix(Protocol::Chat).await
}
#[tokio::test]
#[serial_test::serial]
async fn all_shared_agents_restore_responses() -> Result<(), Box<dyn std::error::Error>> {
    matrix(Protocol::Responses).await
}
#[tokio::test]
#[serial_test::serial]
async fn all_shared_agents_restore_messages() -> Result<(), Box<dyn std::error::Error>> {
    matrix(Protocol::Anthropic).await
}
#[tokio::test]
#[serial_test::serial]
async fn all_shared_agents_restore_gemini() -> Result<(), Box<dyn std::error::Error>> {
    matrix(Protocol::Gemini).await
}
