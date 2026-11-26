//! Cross-Agent regressions for all four routing templates (`Smart`, `Capability`, `Speed`, `Expiry`).

use super::{trace, Menu, MenuModel, Runtime};
use crate::{
    database::Database,
    provider::{Provider, ProviderMeta},
    proxy::{protocol_bridge::Protocol, server::ProxyServer, types::ProxyConfig},
    services::{
        agent_connections::Receipt,
        intent_routing::{QuotaOrder, RoutingStrategy, RoutingTemplate, Target},
    },
};
use axum::{
    extract::{OriginalUri, Request},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const AGENT_TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const MENU_TOKEN: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

struct RestoreSecurity(crate::proxy::security::SecurityRuntimeConfig);
impl Drop for RestoreSecurity {
    fn drop(&mut self) {
        crate::proxy::security::runtime::set_security_runtime_for_test(self.0.clone());
    }
}

fn disable_security() -> RestoreSecurity {
    let guard = RestoreSecurity((*crate::proxy::security::get_security_runtime()).clone());
    crate::proxy::security::runtime::set_security_runtime_for_test(Default::default());
    guard
}

fn all_executable_agent_protocols() -> Vec<(&'static str, Protocol)> {
    vec![
        ("claude", Protocol::Anthropic),
        ("claude-desktop", Protocol::Anthropic),
        ("codex", Protocol::Responses),
        ("t3code", Protocol::Responses),
        ("grokbuild", Protocol::Responses),
        ("grokbuild", Protocol::Chat),
        ("gemini", Protocol::Gemini),
        ("opencode", Protocol::Chat),
        ("openclaw", Protocol::Chat),
        ("hermes", Protocol::Chat),
        ("hermes", Protocol::Anthropic),
        ("pi", Protocol::Chat),
        ("pi", Protocol::Anthropic),
        ("pi", Protocol::Responses),
        ("workbuddy", Protocol::Chat),
        ("workbuddy-ai", Protocol::Chat),
        ("qoder", Protocol::Chat),
        ("trae", Protocol::Chat),
        ("trae", Protocol::Responses),
        ("trae-intl", Protocol::Chat),
        ("trae-intl", Protocol::Responses),
        ("kimi", Protocol::Chat),
        ("qwen", Protocol::Chat),
        ("zcode", Protocol::Chat),
        ("deepseek-harness", Protocol::Chat),
        ("deepseek-harness", Protocol::Anthropic),
        ("deepseek-harness", Protocol::Responses),
    ]
}

fn static_provider(id: &str, base: &str, format: &str) -> Provider {
    let mut provider = Provider::with_id(
        id.into(),
        format!("Synthetic {id}"),
        json!({
            "base_url": format!("{base}/v1"),
            "auth": { "OPENAI_API_KEY": "synthetic-upstream-key" },
            "apiFormat": format,
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some(format.into()),
        ..Default::default()
    });
    provider
}

fn install_all_agents(runtime: &Runtime, alias: &str) -> Result<(), String> {
    for app in crate::services::agent_connections::APPS
        .iter()
        .copied()
        .filter(|app| *app != "codex")
        .chain(["trae", "trae-intl"])
    {
        runtime.connections.install(&Receipt {
            app: app.into(),
            token: AGENT_TOKEN.into(),
            model: alias.into(),
            model_map: if app == "claude-desktop" {
                BTreeMap::from([(alias.into(), alias.into())])
            } else {
                Default::default()
            },
            visible_models: if app == "claude-desktop" {
                Some(vec![alias.into()])
            } else {
                None
            },
            files: vec![],
            suspended: false,
            applied_at_ms: None,
        })?;
    }
    Ok(())
}

fn upstream_reply(protocol: Protocol, model: &str, tool_id: Option<&str>, text: &str) -> Value {
    match (protocol, tool_id) {
        (Protocol::Chat, Some(call_id)) => json!({
            "id": format!("chat_{call_id}"),
            "object": "chat.completion",
            "model": model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": call_id,
                        "type": "function",
                        "function": { "name": "synthetic_tool", "arguments": "{\"step\":1}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 8, "completion_tokens": 4 }
        }),
        (Protocol::Chat, None) => json!({
            "id": "chat_done",
            "object": "chat.completion",
            "model": model,
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": text },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 8, "completion_tokens": 4 }
        }),
        (Protocol::Anthropic, Some(call_id)) => json!({
            "id": format!("msg_{call_id}"),
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [{
                "type": "tool_use",
                "id": call_id,
                "name": "synthetic_tool",
                "input": { "step": 1 }
            }],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 8, "output_tokens": 4 }
        }),
        (Protocol::Anthropic, None) => json!({
            "id": "msg_done",
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [{ "type": "text", "text": text }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 8, "output_tokens": 4 }
        }),
        (Protocol::Responses, Some(call_id)) => json!({
            "id": format!("resp_{call_id}"),
            "object": "response",
            "status": "completed",
            "model": model,
            "output": [{
                "id": format!("fc_{call_id}"),
                "type": "function_call",
                "call_id": call_id,
                "name": "synthetic_tool",
                "arguments": "{\"step\":1}",
                "status": "completed"
            }],
            "usage": { "input_tokens": 8, "output_tokens": 4 }
        }),
        (Protocol::Responses, None) => json!({
            "id": "resp_done",
            "object": "response",
            "status": "completed",
            "model": model,
            "output": [{
                "id": "msg_done",
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [{ "type": "output_text", "text": text }]
            }],
            "usage": { "input_tokens": 8, "output_tokens": 4 }
        }),
        (Protocol::Gemini, Some(call_id)) => json!({
            "responseId": format!("gem_{call_id}"),
            "modelVersion": model,
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "id": call_id,
                            "name": "synthetic_tool",
                            "args": { "step": 1 }
                        }
                    }]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": { "promptTokenCount": 8, "candidatesTokenCount": 4 }
        }),
        (Protocol::Gemini, None) => json!({
            "responseId": "gem_done",
            "modelVersion": model,
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": text }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": { "promptTokenCount": 8, "candidatesTokenCount": 4 }
        }),
    }
}

fn two_member_menu(base: &str, template: RoutingTemplate) -> Result<(Menu, String), String> {
    let mut menu = super::global_tests::menu(&format!("{base}/v1/chat/completions"));
    menu.token = MENU_TOKEN.into();
    let alias = menu
        .groups
        .keys()
        .next()
        .ok_or("missing fixture alias")?
        .clone();
    menu.models.insert(
        "fixture".into(),
        MenuModel {
            alias: alias.clone(),
            label: "Synthetic template group".into(),
            description: "VaryAI routing group",
            order: 0,
        },
    );
    let session = Arc::make_mut(menu.groups.get_mut(&alias).ok_or("missing fixture group")?);
    session.group.levels.clear();
    session.group.template = Some(template);
    session.group.affinity = "turn".into();
    if template == RoutingTemplate::Speed {
        session.group.routing = RoutingStrategy::Fastest;
    }
    if template == RoutingTemplate::Expiry {
        session.group.quota_order = Some(QuotaOrder::EarliestExpiry);
    }
    session.group.members[0].id = "primary".into();
    session.group.members[0].provider_id = "primary-provider".into();
    session.group.members[0].model_id = "strong-model".into();
    session.group.default_member_id = "primary".into();

    let mut backup_member = session.group.members[0].clone();
    backup_member.id = "backup".into();
    backup_member.provider_id = "backup-provider".into();
    backup_member.model_id = "backup-model".into();
    session.group.members.push(backup_member);
    session.group.fallback_member_ids = vec!["backup".into()];

    let mut primary_target = session.targets[0].clone();
    primary_target.provider_id = "primary-provider".into();
    primary_target.model = "strong-model".into();
    primary_target.url = format!("{base}/v1/chat/completions")
        .parse()
        .map_err(|_| "invalid url")?;
    primary_target.upstream = Some(static_provider("primary-provider", base, "openai_chat"));
    primary_target.scheduling = Arc::default();

    let mut backup_target = primary_target.clone();
    backup_target.provider_id = "backup-provider".into();
    backup_target.model = "backup-model".into();
    backup_target.upstream = Some(static_provider("backup-provider", base, "openai_chat"));
    backup_target.scheduling = Arc::default();

    session.targets = vec![primary_target, backup_target];
    Ok((menu, alias))
}

#[tokio::test]
#[serial_test::serial]
async fn non_classifier_templates_reselect_each_turn_after_closed_tool_calls_across_all_agents(
) -> TestResult {
    let _security = disable_security();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let upstream = tokio::spawn(async move {
        let app = Router::new().fallback(post(
            move |OriginalUri(uri): OriginalUri, Json(body): Json<Value>| {
                let captured = captured.clone();
                async move {
                    let model = body["model"]
                        .as_str()
                        .unwrap_or_else(|| {
                            uri.path()
                                .split("/models/")
                                .nth(1)
                                .and_then(|rest| rest.split(':').next())
                                .unwrap_or("unknown")
                        })
                        .to_owned();
                    if let Ok(mut list) = captured.lock() {
                        list.push(model.clone());
                    }
                    let want_tool = body["tools"]
                        .as_array()
                        .is_some_and(|tools| !tools.is_empty())
                        && !body.to_string().contains("Synthetic tool output");
                    let protocol = if uri.path().ends_with("/messages") {
                        Protocol::Anthropic
                    } else if uri.path().ends_with("/responses") {
                        Protocol::Responses
                    } else if uri.path().contains("/models/") {
                        Protocol::Gemini
                    } else {
                        Protocol::Chat
                    };
                    let call_id = want_tool.then_some("call_turn_1");
                    let reply = upstream_reply(protocol, &model, call_id, "Synthetic final answer");
                    if body["stream"] == true {
                        if protocol == Protocol::Responses {
                            let event = json!({"type":"response.completed","response":reply});
                            return (
                                [(header::CONTENT_TYPE, "text/event-stream")],
                                format!("event: response.completed\ndata: {event}\n\ndata: [DONE]\n\n"),
                            )
                                .into_response();
                        }
                    }
                    Json(reply).into_response()
                }
            },
        ));
        let _ = axum::serve(listener, app).await;
    });

    for template in [
        RoutingTemplate::Capability,
        RoutingTemplate::Speed,
        RoutingTemplate::Expiry,
    ] {
        let (mut menu, alias) = two_member_menu(&base, template)?;
        let runtime = Arc::new(Runtime::default());
        let dir = tempfile::tempdir()?;
        runtime
            .prepare_continuations(dir.path().join("continuations.json"))
            .await;
        install_all_agents(&runtime, &alias)?;
        let db = Arc::new(Database::memory()?);
        let gateway_key = db.create_gateway_key(crate::proxy::gateway::KeyPolicy {
            name: format!("synthetic-{template:?}"),
            models: vec![alias.clone()],
            all_models: false,
            token_limit: None,
            period: crate::proxy::gateway::Period::Never,
        })?;
        let server = ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            db,
            None,
        )
        .with_intent_runtime(runtime.clone());
        let info = server.start().await?;
        let origin = format!("http://127.0.0.1:{}", info.port);
        let client = reqwest::Client::builder().no_proxy().build()?;

        let mut cases: Vec<(&str, Protocol, bool)> = all_executable_agent_protocols()
            .into_iter()
            .map(|(agent, protocol)| (agent, protocol, false))
            .collect();
        cases.push(("external", Protocol::Chat, true));
        cases.push(("external", Protocol::Responses, true));

        for (case_idx, (agent, protocol, is_gateway)) in cases.into_iter().enumerate() {
            let call_id = format!("call_{template:?}_{case_idx}");
            let session_id = format!("ses_{template:?}_{agent}_{protocol:?}_{case_idx}");
            let set_primary_available = |menu: &mut Menu, available: bool| -> Result<(), String> {
                let group = Arc::make_mut(menu.groups.get_mut(&alias).ok_or("missing group")?);
                group.group.enabled = true;
                if template == RoutingTemplate::Speed {
                    // Seed speed telemetry so primary is measurably faster once available.
                    group.targets[0]
                        .scheduling
                        .served(100, 50.0, 500.0, 100, chrono::Utc::now().timestamp());
                    group.targets[1]
                        .scheduling
                        .served(100, 900.0, 2000.0, 20, chrono::Utc::now().timestamp());
                }
                group.group.off = if available {
                    vec![]
                } else {
                    vec!["primary".into()]
                };
                if available {
                    let mut p = group.targets.last().cloned().ok_or("missing target")?;
                    p.provider_id = "primary-provider".into();
                    p.model = "strong-model".into();
                    p.upstream = Some(static_provider("primary-provider", &base, "openai_chat"));
                    p.scheduling = Arc::default();
                    if template == RoutingTemplate::Speed {
                        p.scheduling.served(
                            100,
                            50.0,
                            500.0,
                            100,
                            chrono::Utc::now().timestamp(),
                        );
                    }
                    let mut b = p.clone();
                    b.provider_id = "backup-provider".into();
                    b.model = "backup-model".into();
                    b.upstream = Some(static_provider("backup-provider", &base, "openai_chat"));
                    b.scheduling = Arc::default();
                    if template == RoutingTemplate::Speed {
                        b.scheduling.served(
                            100,
                            900.0,
                            2000.0,
                            20,
                            chrono::Utc::now().timestamp(),
                        );
                    }
                    group.targets = vec![p, b];
                } else {
                    group.targets.retain(|t| t.model == "backup-model");
                }
                runtime.set_global(menu.clone(), 0)?;
                runtime.set_menu(Arc::new(menu.clone()))?;
                Ok(())
            };

            // Step 1: Primary unavailable -> Turn 1 lands on backup and emits a tool call.
            set_primary_available(&mut menu, false)?;
            let (url, bearer) = if is_gateway {
                let path = match protocol {
                    Protocol::Responses => "/gateway/v1/responses",
                    _ => "/gateway/v1/chat/completions",
                };
                (format!("{origin}{path}"), gateway_key.secret.as_str())
            } else if agent == "codex" {
                (format!("{origin}/intent/v1/responses"), MENU_TOKEN)
            } else {
                let path = match protocol {
                    Protocol::Anthropic => "/v1/messages".to_owned(),
                    Protocol::Responses => "/v1/responses".to_owned(),
                    Protocol::Gemini => format!("/v1beta/models/{alias}:generateContent"),
                    Protocol::Chat => "/v1/chat/completions".to_owned(),
                };
                (format!("{origin}/connections/{agent}{path}"), AGENT_TOKEN)
            };

            let build_body = |step: usize| -> Value {
                match protocol {
                    Protocol::Chat => {
                        let mut messages =
                            vec![json!({"role":"user","content":"Synthetic first question"})];
                        if step >= 2 {
                            messages.extend([
                                json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_turn_1","type":"function","function":{"name":"synthetic_tool","arguments":"{\"step\":1}"}}]}),
                                json!({"role":"tool","tool_call_id":if step == 99 { "unknown_call" } else { "call_turn_1" },"content":"Synthetic tool output"}),
                            ]);
                        }
                        if step >= 3 {
                            messages.extend([
                                json!({"role":"assistant","content":"Synthetic final answer"}),
                                json!({"role":"user","content":"Synthetic second turn question"}),
                            ]);
                        }
                        let mut body = json!({"model": alias, "messages": messages});
                        if step <= 2 || step == 99 {
                            body["tools"] = json!([{"type":"function","function":{"name":"synthetic_tool","parameters":{"type":"object"}}}]);
                        }
                        body
                    }
                    Protocol::Anthropic => {
                        let mut messages =
                            vec![json!({"role":"user","content":"Synthetic first question"})];
                        if step >= 2 {
                            messages.extend([
                                json!({"role":"assistant","content":[{"type":"tool_use","id":"call_turn_1","name":"synthetic_tool","input":{"step":1}}]}),
                                json!({"role":"user","content":[{"type":"tool_result","tool_use_id":if step == 99 { "unknown_call" } else { "call_turn_1" },"content":"Synthetic tool output"}]}),
                            ]);
                        }
                        if step >= 3 {
                            messages.extend([
                                json!({"role":"assistant","content":[{"type":"thinking","thinking":"Unsigned visible thought","signature":""},{"type":"text","text":"Synthetic final answer"}]}),
                                json!({"role":"user","content":"Synthetic second turn question"}),
                            ]);
                        }
                        let mut body =
                            json!({"model": alias, "max_tokens": 128, "messages": messages});
                        if step <= 2 || step == 99 {
                            body["tools"] = json!([{"name":"synthetic_tool","input_schema":{"type":"object"}}]);
                        }
                        body
                    }
                    Protocol::Responses => {
                        let mut input =
                            vec![json!({"role":"user","content":"Synthetic first question"})];
                        if step >= 2 {
                            input.extend([
                                json!({"type":"function_call","call_id":"call_turn_1","name":"synthetic_tool","arguments":"{\"step\":1}"}),
                                json!({"type":"function_call_output","call_id":if step == 99 { "unknown_call" } else { "call_turn_1" },"output":"Synthetic tool output"}),
                            ]);
                        }
                        if step >= 3 {
                            if matches!(agent, "codex" | "t3code") {
                                input.push(json!({"type":"reasoning","id":format!("rs_{call_id}"),"summary":[],"content":null,"encrypted_content":null}));
                            }
                            input.extend([
                                json!({"role":"assistant","content":"Synthetic final answer"}),
                                json!({"role":"user","content":"Synthetic second turn question"}),
                            ]);
                        }
                        let mut body = json!({"model": alias, "input": input});
                        if step <= 2 || step == 99 {
                            body["tools"] = json!([{"type":"function","name":"synthetic_tool","parameters":{"type":"object"}}]);
                        }
                        body
                    }
                    Protocol::Gemini => {
                        let mut contents = vec![
                            json!({"role":"user","parts":[{"text":"Synthetic first question"}]}),
                        ];
                        if step >= 2 {
                            contents.extend([
                                json!({"role":"model","parts":[{"functionCall":{"id":"call_turn_1","name":"synthetic_tool","args":{"step":1}}}]}),
                                json!({"role":"user","parts":[{"functionResponse":{"id":if step == 99 { "unknown_call" } else { "call_turn_1" },"name":"synthetic_tool","response":{"result":"Synthetic tool output"}}}]}),
                            ]);
                        }
                        if step >= 3 {
                            contents.extend([
                                json!({"role":"model","parts":[{"text":"Synthetic final answer"}]}),
                                json!({"role":"user","parts":[{"text":"Synthetic second turn question"}]}),
                            ]);
                        }
                        let mut body = json!({"contents": contents});
                        if step <= 2 || step == 99 {
                            body["tools"] = json!([{"functionDeclarations":[{"name":"synthetic_tool","parameters":{"type":"object"}}]}]);
                        }
                        body
                    }
                }
            };

            let send = |step: usize| {
                client
                    .post(&url)
                    .bearer_auth(bearer)
                    .header("x-session-id", &session_id)
                    .json(&build_body(step))
                    .send()
            };

            let r1 = send(1).await?;
            assert_eq!(
                r1.status(),
                StatusCode::OK,
                "{template:?} {agent} {protocol:?} step 1"
            );
            let _ = r1.bytes().await?;
            assert_eq!(
                seen.lock().map_err(|_| "lock")?.last().map(String::as_str),
                Some("backup-model"),
                "{template:?} {agent} {protocol:?} step 1 model"
            );

            // Re-enable primary BEFORE sending the active tool result: active tool output MUST stay on backup.
            set_primary_available(&mut menu, true)?;
            if !matches!(agent, "codex" | "t3code") {
                let bad = send(99).await?;
                assert_eq!(
                    bad.status(),
                    StatusCode::CONFLICT,
                    "{template:?} {agent} {protocol:?} unknown tool call must fail closed"
                );
            }
            let r2 = send(2).await?;
            assert_eq!(
                r2.status(),
                StatusCode::OK,
                "{template:?} {agent} {protocol:?} step 2"
            );
            let _ = r2.bytes().await?;
            assert_eq!(
                seen.lock().map_err(|_| "lock")?.last().map(String::as_str),
                Some("backup-model"),
                "{template:?} {agent} {protocol:?} active tool continuation stays on backup"
            );

            // Step 3: Next user turn after closed tool round -> MUST reselect strong-model!
            let r3 = send(3).await?;
            assert_eq!(
                r3.status(),
                StatusCode::OK,
                "{template:?} {agent} {protocol:?} step 3"
            );
            let _ = r3.bytes().await?;
            assert_eq!(
                seen.lock().map_err(|_| "lock")?.last().map(String::as_str),
                Some("strong-model"),
                "{template:?} {agent} {protocol:?} new turn after closed tool round must reselect primary"
            );
        }

        server.stop().await?;
    }

    upstream.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn smart_template_compaction_and_multimodal_classification_apply_across_all_agents(
) -> TestResult {
    let _security = disable_security();
    let seen = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let classifier_calls = Arc::new(Mutex::new(0usize));
    let captured = seen.clone();
    let cls_counter = classifier_calls.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let upstream = tokio::spawn(async move {
        let app = Router::new()
            .route(
                "/v1/classify/chat/completions",
                post(move |Json(body): Json<Value>| {
                    let cls_counter = cls_counter.clone();
                    async move {
                        if let Ok(mut n) = cls_counter.lock() {
                            *n += 1;
                        }
                        let payload: Value = serde_json::from_str(
                            body["messages"][1]["content"].as_str().unwrap_or("{}"),
                        )
                        .unwrap_or_default();
                        let text = payload["text"].as_str().unwrap_or("");
                        let intent = if text.contains("easy") {
                            json!("easy")
                        } else {
                            json!("hard")
                        };
                        Json(json!({
                            "id": "cls_1",
                            "object": "chat.completion",
                            "choices": [{
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": json!({ "intentId": intent }).to_string()
                                },
                                "finish_reason": "stop"
                            }]
                        }))
                    }
                }),
            )
            .fallback(post(move |req: Request| {
                let captured = captured.clone();
                async move {
                    let uri = req.uri().clone();
                    let bytes = axum::body::to_bytes(req.into_body(), 4 * 1024 * 1024)
                        .await
                        .unwrap_or_default();
                    let body: Value = serde_json::from_slice(&bytes).unwrap_or_default();
                    let model = body["model"]
                        .as_str()
                        .unwrap_or_else(|| {
                            uri.path()
                                .split("/models/")
                                .nth(1)
                                .and_then(|rest| rest.split(':').next())
                                .unwrap_or("unknown")
                        })
                        .to_owned();
                    if let Ok(mut list) = captured.lock() {
                        list.push((model.clone(), body.clone()));
                    }
                    let protocol = if uri.path().ends_with("/messages") {
                        Protocol::Anthropic
                    } else if uri.path().ends_with("/responses") {
                        Protocol::Responses
                    } else if uri.path().contains("/models/") {
                        Protocol::Gemini
                    } else {
                        Protocol::Chat
                    };
                    let reply = upstream_reply(
                        protocol,
                        &model,
                        None,
                        "Synthetic continuation summary or answer",
                    );
                    if body["stream"] == true && protocol == Protocol::Responses {
                        let event = json!({"type":"response.completed","response":reply});
                        return (
                            [(header::CONTENT_TYPE, "text/event-stream")],
                            format!("event: response.completed\ndata: {event}\n\ndata: [DONE]\n\n"),
                        )
                            .into_response();
                    }
                    Json(reply).into_response()
                }
            }));
        let _ = axum::serve(listener, app).await;
    });

    let (mut menu, alias) = two_member_menu(&base, RoutingTemplate::Smart)?;
    {
        let group = Arc::make_mut(menu.groups.get_mut(&alias).ok_or("missing group")?);
        group.group.enabled = true;
        group.group.intent_recognition_enabled = true;
        group.group.classification_budget_ms = 10_000;
        group.group.classifier_target = Some(Target {
            provider_id: "classifier".into(),
            model_id: "cls-model".into(),
        });
        group.classifier = Some(static_provider(
            "classifier",
            &format!("{base}/v1/classify"),
            "openai_chat",
        ));
        group.group.rules = serde_json::from_value(json!([
            {
                "id": "compact",
                "name": "Compaction",
                "description": "",
                "enabled": true,
                "targetMemberId": "primary",
                "conditions": { "compact": true }
            },
            {
                "id": "hard",
                "name": "Hard task",
                "description": "Complex architecture tasks",
                "enabled": true,
                "targetMemberId": "primary"
            },
            {
                "id": "easy",
                "name": "Easy task",
                "description": "Simple direct questions",
                "enabled": true,
                "targetMemberId": "backup",
                "fallbackMemberIds": ["primary"]
            }
        ]))?;
        for target in &mut group.targets {
            target.effort.images = Some(true);
        }
    }

    let runtime = Arc::new(Runtime::default());
    let dir = tempfile::tempdir()?;
    runtime
        .prepare_continuations(dir.path().join("continuations.json"))
        .await;
    runtime
        .native
        .prepare_checkpoints(&dir.path().join("checkpoint.key"))?;
    runtime.set_global(menu.clone(), 0)?;
    runtime.set_menu(Arc::new(menu.clone()))?;
    install_all_agents(&runtime, &alias)?;

    let db = Arc::new(Database::memory()?);
    let gateway_key = db.create_gateway_key(crate::proxy::gateway::KeyPolicy {
        name: "synthetic-smart-gw".into(),
        models: vec![alias.clone()],
        all_models: false,
        token_limit: None,
        period: crate::proxy::gateway::Period::Never,
    })?;
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db,
        None,
    )
    .with_intent_runtime(runtime.clone());
    let info = server.start().await?;
    let origin = format!("http://127.0.0.1:{}", info.port);
    let client = reqwest::Client::builder().no_proxy().build()?;

    let pixel = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    // 1. Test multimodal easy turn + prompt/header compaction across every Agent & protocol.
    for (idx, (agent, protocol)) in all_executable_agent_protocols().into_iter().enumerate() {
        let session_id = format!("smart_ses_{agent}_{protocol:?}_{idx}");
        let (url, bearer) = if agent == "codex" {
            (format!("{origin}/intent/v1/responses"), MENU_TOKEN)
        } else {
            let path = match protocol {
                Protocol::Anthropic => "/v1/messages".to_owned(),
                Protocol::Responses => "/v1/responses".to_owned(),
                Protocol::Gemini => format!("/v1beta/models/{alias}:generateContent"),
                Protocol::Chat => "/v1/chat/completions".to_owned(),
            };
            (format!("{origin}/connections/{agent}{path}"), AGENT_TOKEN)
        };

        // Step A: Easy multimodal question (with inline image / unsigned thinking / empty reasoning) -> backup-model!
        let easy_body = match protocol {
            Protocol::Chat => json!({
                "model": alias,
                "messages": [{
                    "role": "user",
                    "content": [
                        { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{pixel}") } },
                        { "type": "text", "text": "Synthetic easy screenshot question" }
                    ]
                }]
            }),
            Protocol::Anthropic => json!({
                "model": alias,
                "max_tokens": 128,
                "messages": [
                    { "role": "user", "content": "Earlier question" },
                    { "role": "assistant", "content": [
                        { "type": "thinking", "thinking": "Unsigned thought", "signature": "" },
                        { "type": "text", "text": "Earlier answer" }
                    ]},
                    {
                        "role": "user",
                        "content": [
                            { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": pixel } },
                            { "type": "text", "text": "Synthetic easy screenshot question" }
                        ]
                    }
                ]
            }),
            Protocol::Responses => json!({
                "model": alias,
                "input": [
                    { "role": "user", "content": "Earlier question" },
                    if matches!(agent, "codex" | "t3code") {
                        json!({ "type": "reasoning", "id": "rs_empty", "summary": [], "content": null, "encrypted_content": null })
                    } else {
                        json!({ "type": "reasoning", "id": "rs_vis", "summary": [{ "type": "summary_text", "text": "Unsigned thought" }], "content": null, "encrypted_content": null })
                    },
                    { "role": "assistant", "content": "Earlier answer" },
                    {
                        "role": "user",
                        "content": [
                            { "type": "input_image", "image_url": format!("data:image/png;base64,{pixel}") },
                            { "type": "input_text", "text": "Synthetic easy screenshot question" }
                        ]
                    }
                ]
            }),
            Protocol::Gemini => json!({
                "contents": [{
                    "role": "user",
                    "parts": [
                        { "inlineData": { "mimeType": "image/png", "data": pixel } },
                        { "text": "Synthetic easy screenshot question" }
                    ]
                }]
            }),
        };

        let resp = client
            .post(&url)
            .bearer_auth(bearer)
            .header("x-session-id", &session_id)
            .json(&easy_body)
            .send()
            .await?;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "{agent} {protocol:?} easy multimodal"
        );
        let _ = resp.bytes().await?;
        assert_eq!(
            seen.lock()
                .map_err(|_| "lock")?
                .last()
                .map(|(m, _)| m.as_str()),
            Some("backup-model"),
            "{agent} {protocol:?} easy multimodal must classify and select backup-model"
        );

        // Step B: Ordinary user request containing the word "summarize" + "easy" MUST NOT trigger Compaction rule!
        let normal_summary = match protocol {
            Protocol::Chat => json!({
                "model": alias,
                "messages": [{ "role": "user", "content": "Please summarize this easy note in one line" }]
            }),
            Protocol::Anthropic => json!({
                "model": alias,
                "max_tokens": 128,
                "messages": [{ "role": "user", "content": "Please summarize this easy note in one line" }]
            }),
            Protocol::Responses => json!({
                "model": alias,
                "input": [{ "role": "user", "content": "Please summarize this easy note in one line" }]
            }),
            Protocol::Gemini => json!({
                "contents": [{ "role": "user", "parts": [{ "text": "Please summarize this easy note in one line" }] }]
            }),
        };
        let resp = client
            .post(&url)
            .bearer_auth(bearer)
            .header("x-session-id", &session_id)
            .json(&normal_summary)
            .send()
            .await?;
        assert_eq!(resp.status(), StatusCode::OK);
        let _ = resp.bytes().await?;
        assert_eq!(
            seen.lock()
                .map_err(|_| "lock")?
                .last()
                .map(|(m, _)| m.as_str()),
            Some("backup-model"),
            "{agent} {protocol:?} ordinary user summarize text must not falsely trigger compaction"
        );

        // Step C: Agent-initiated context compaction in the same session -> MUST route to strong-model without calling classifier!
        let before_cls = *classifier_calls.lock().map_err(|_| "lock")?;
        let compact_req = match (agent, protocol) {
            ("codex" | "t3code" | "grokbuild", Protocol::Responses) => client
                .post(&url)
                .bearer_auth(bearer)
                .header("x-session-id", &session_id)
                .header("x-openai-subagent", "compact")
                .json(&json!({
                    "model": alias,
                    "instructions": "You are performing a CONTEXT CHECKPOINT COMPACTION.",
                    "input": [{ "role": "user", "content": "Synthetic easy conversation to compact" }]
                })),
            (_, Protocol::Anthropic) => client
                .post(&url)
                .bearer_auth(bearer)
                .header("x-session-id", &session_id)
                .json(&json!({
                    "model": alias,
                    "max_tokens": 256,
                    "system": "You are a helpful AI assistant tasked with summarizing conversations.",
                    "messages": [{
                        "role": "user",
                        "content": "Your task is to create a detailed summary of the conversation so far.\nCRITICAL: Respond with TEXT ONLY. Do NOT call any tools."
                    }]
                })),
            (_, Protocol::Gemini) => client
                .post(&url)
                .bearer_auth(bearer)
                .header("x-session-id", &session_id)
                .json(&json!({
                    "systemInstruction": {
                        "parts": [{ "text": "You are the component that summarizes internal chat history into a given structure." }]
                    },
                    "contents": [{
                        "role": "user",
                        "parts": [{ "text": "First, reason in your scratchpad. Then, generate the <state_snapshot>." }]
                    }]
                })),
            ("pi" | "deepseek-harness" | "openclaw", Protocol::Chat) => client
                .post(&url)
                .bearer_auth(bearer)
                .header("x-session-id", &session_id)
                .json(&json!({
                    "model": alias,
                    "messages": [
                        { "role": "system", "content": "You are a context summarization assistant. Your task is to read a conversation between a user and an AI coding assistant, then produce a structured summary." },
                        { "role": "user", "content": "<conversation>easy</conversation>\n\nThe messages above are a conversation to summarize. Create a structured context checkpoint summary." }
                    ]
                })),
            ("opencode", Protocol::Chat) => client
                .post(&url)
                .bearer_auth(bearer)
                .header("x-session-id", &session_id)
                .json(&json!({
                    "model": alias,
                    "messages": [
                        { "role": "system", "content": "You are a helpful AI assistant tasked with summarizing conversations." },
                        { "role": "user", "content": "Provide a detailed prompt for continuing our conversation above." }
                    ]
                })),
            ("hermes", Protocol::Chat) => client
                .post(&url)
                .bearer_auth(bearer)
                .header("x-session-id", &session_id)
                .json(&json!({
                    "model": alias,
                    "messages": [
                        { "role": "system", "content": "You are a context compression assistant." },
                        { "role": "user", "content": "Compress easy context" }
                    ]
                })),
            (_, Protocol::Responses) => client
                .post(&url)
                .bearer_auth(bearer)
                .header("x-session-id", &session_id)
                .json(&json!({
                    "model": alias,
                    "instructions": "You are performing a CONTEXT CHECKPOINT COMPACTION.",
                    "input": [{ "role": "user", "content": "Synthetic easy conversation" }]
                })),
            (_, Protocol::Chat) => client
                .post(&url)
                .bearer_auth(bearer)
                .header("x-session-id", &session_id)
                .json(&json!({
                    "model": alias,
                    "messages": [
                        { "role": "user", "content": "Your task is to create a detailed summary of the conversation so far.\nPending Tasks:\n- none\nCurrent Work:\n- easy\nCRITICAL: Respond with TEXT ONLY. Do NOT call any tools." }
                    ]
                })),
        };

        let resp = compact_req.send().await?;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "{agent} {protocol:?} compaction"
        );
        let _ = resp.bytes().await?;
        let after_cls = *classifier_calls.lock().map_err(|_| "lock")?;
        assert_eq!(
            after_cls, before_cls,
            "{agent} {protocol:?} compaction rule must short-circuit paid classifier"
        );
        assert_eq!(
            seen.lock()
                .map_err(|_| "lock")?
                .last()
                .map(|(m, _)| m.as_str()),
            Some("strong-model"),
            "{agent} {protocol:?} compaction must route to strong-model even after easy session"
        );
        let traces = runtime.traces.snapshot();
        let latest = traces.first().ok_or("missing trace")?;
        assert_eq!(
            latest.kind.as_deref(),
            Some("compact"),
            "{agent} {protocol:?} trace kind must record compact"
        );
        assert!(
            latest
                .steps
                .iter()
                .any(|step| step.kind == trace::Kind::Rule),
            "{agent} {protocol:?} trace must record rule match"
        );
    }

    // 2. Test `/v1/responses/compact` endpoint & checkpoint round-trip on `/intent/v1`, `/connections/<app>`, and `/gateway`!
    for (label, compact_url, resume_url, bearer) in [
        (
            "codex-intent",
            format!("{origin}/intent/v1/responses/compact"),
            format!("{origin}/intent/v1/responses"),
            MENU_TOKEN,
        ),
        (
            "t3code-connections",
            format!("{origin}/connections/t3code/v1/responses/compact"),
            format!("{origin}/connections/t3code/v1/responses"),
            AGENT_TOKEN,
        ),
        (
            "grokbuild-connections",
            format!("{origin}/connections/grokbuild/v1/responses/compact"),
            format!("{origin}/connections/grokbuild/v1/responses"),
            AGENT_TOKEN,
        ),
        (
            "trae-connections",
            format!("{origin}/connections/trae/v1/responses/compact"),
            format!("{origin}/connections/trae/v1/responses"),
            AGENT_TOKEN,
        ),
        (
            "external-gateway",
            format!("{origin}/gateway/v1/responses/compact"),
            format!("{origin}/gateway/v1/responses"),
            gateway_key.secret.as_str(),
        ),
    ] {
        let compact_resp: Value = client
            .post(&compact_url)
            .bearer_auth(bearer)
            .json(&json!({
                "model": alias,
                "input": [{ "role": "user", "content": "Synthetic easy history for endpoint compact" }]
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        assert_eq!(
            compact_resp["object"], "response.compaction",
            "{label} compact response envelope"
        );
        assert_eq!(
            seen.lock()
                .map_err(|_| "lock")?
                .last()
                .map(|(m, _)| m.as_str()),
            Some("strong-model"),
            "{label} /responses/compact must select strong-model"
        );
        let seal = compact_resp["output"][0]["encrypted_content"]
            .as_str()
            .ok_or("missing checkpoint seal")?;
        assert!(seal.starts_with("varyai1:"), "{label} seal prefix");

        // Resuming with that checkpoint unseals cleanly and preserves consumer isolation.
        let resumed = client
            .post(&resume_url)
            .bearer_auth(bearer)
            .json(&json!({
                "model": alias,
                "input": [
                    { "type": "compaction", "encrypted_content": seal },
                    { "role": "user", "content": "Synthetic easy follow-up" }
                ]
            }))
            .send()
            .await?;
        assert_eq!(resumed.status(), StatusCode::OK, "{label} checkpoint resume");
        let _ = resumed.bytes().await?;
    }

    // 3. Disabling the Compaction rule preserves session binding and still skips classifier on compact.
    {
        let group = Arc::make_mut(menu.groups.get_mut(&alias).ok_or("missing group")?);
        group.group.rules[0].enabled = false;
        runtime.set_global(menu.clone(), 0)?;
    }
    let disabled_session = "smart_disabled_compact_session";
    let easy_resp = client
        .post(format!("{origin}/connections/claude/v1/messages"))
        .bearer_auth(AGENT_TOKEN)
        .header("x-claude-code-session-id", disabled_session)
        .json(&json!({
            "model": alias,
            "max_tokens": 128,
            "messages": [{ "role": "user", "content": "Synthetic easy question" }]
        }))
        .send()
        .await?;
    assert_eq!(easy_resp.status(), StatusCode::OK);
    let _ = easy_resp.bytes().await?;
    assert_eq!(
        seen.lock()
            .map_err(|_| "lock")?
            .last()
            .map(|(m, _)| m.as_str()),
        Some("backup-model")
    );
    let before_cls = *classifier_calls.lock().map_err(|_| "lock")?;
    let disabled_compact = client
        .post(format!("{origin}/connections/claude/v1/messages"))
        .bearer_auth(AGENT_TOKEN)
        .header("x-claude-code-session-id", disabled_session)
        .json(&json!({
            "model": alias,
            "max_tokens": 256,
            "system": "You are a helpful AI assistant tasked with summarizing conversations.",
            "messages": [{
                "role": "user",
                "content": "Your task is to create a detailed summary of the conversation so far.\nCRITICAL: Respond with TEXT ONLY. Do NOT call any tools."
            }]
        }))
        .send()
        .await?;
    assert_eq!(disabled_compact.status(), StatusCode::OK);
    let _ = disabled_compact.bytes().await?;
    assert_eq!(
        *classifier_calls.lock().map_err(|_| "lock")?,
        before_cls,
        "disabled compaction rule must still not classify summarization prompt"
    );
    assert_eq!(
        seen.lock()
            .map_err(|_| "lock")?
            .last()
            .map(|(m, _)| m.as_str()),
        Some("backup-model"),
        "disabling compaction rule preserves existing session binding"
    );

    server.stop().await?;
    upstream.abort();
    Ok(())
}
