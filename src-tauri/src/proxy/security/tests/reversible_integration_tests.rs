//! Loopback-only HTTP integration fixtures. No generated tool input is executed.
use std::sync::{Arc, Mutex};

use axum::{body::Body, extract::State, response::IntoResponse, routing::post, Json, Router};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tower::Service;

use super::{http_restoration::RestorationGateway, runtime};
use crate::{
    database::Database,
    provider::Provider,
    proxy::{
        failover_switch::FailoverSwitchManager,
        handlers,
        provider_router::ProviderRouter,
        providers::{codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore},
        server::{ProxyServer, ProxyState},
        types::{ProxyConfig, ProxyStatus},
    },
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const FIXTURE_PHONE: &str = "13800138000";

struct RuntimeReset(runtime::SecurityRuntimeConfig);
impl Drop for RuntimeReset {
    fn drop(&mut self) {
        runtime::set_security_runtime_for_test(self.0.clone());
    }
}

fn state(db: Arc<Database>) -> ProxyState {
    ProxyState {
        pi_runtime: Arc::default(),
        db: db.clone(),
        config: Arc::new(RwLock::new(ProxyConfig::default())),
        status: Arc::new(RwLock::new(ProxyStatus::default())),
        start_time: Arc::new(RwLock::new(None)),
        current_providers: Arc::default(),
        provider_router: Arc::new(ProviderRouter::new(db.clone())),
        workbuddy_upstream: Arc::default(),
        gemini_shadow: Arc::new(GeminiShadowStore::default()),
        codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
        app_handle: None,
        failover_manager: Arc::new(FailoverSwitchManager::new(db)),
        restoration_gateway: RestorationGateway::with_test_rules(
            &["chinese_phone_number"],
            64 * 1024 * 1024,
        ),
    }
}

#[derive(Default)]
struct FinalClientRecorder {
    submissions: Vec<Value>,
    completed: bool,
    failed: bool,
}
impl FinalClientRecorder {
    // Record JSON executable items without interpreting or executing their inputs.
    fn observe_json(&mut self, value: &Value) {
        self.completed = value["status"] == "completed";
        self.failed = value.get("error").is_some();
        if let Some(output) = value["output"].as_array() {
            self.submissions.extend(
                output
                    .iter()
                    .filter(|item| {
                        matches!(
                            item["type"].as_str(),
                            Some("function_call" | "custom_tool_call")
                        )
                    })
                    .cloned(),
            );
        }
    }

    // Record at the executable item boundary, not only after a success terminal.
    // This fixture never executes a tool, command, shell or generated program.
    fn observe_sse(&mut self, wire: &str) -> TestResult {
        for data in wire.lines().filter_map(|line| line.strip_prefix("data: ")) {
            if data == "[DONE]" {
                continue;
            }
            let event: Value = serde_json::from_str(data)?;
            self.completed |= event["type"] == "response.completed";
            self.failed |= event["type"] == "response.failed";
            if event["type"] == "response.output_item.done" {
                if let Some(item) = event.get("item").filter(|item| {
                    matches!(
                        item["type"].as_str(),
                        Some("function_call" | "custom_tool_call")
                    )
                }) {
                    self.submissions.push(item.clone());
                }
            }
        }
        Ok(())
    }
}

async fn echo_native(
    State(seen): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if let Ok(mut requests) = seen.lock() {
        requests.push(body.clone());
    }
    let text = body["input"].as_str().unwrap_or("unexpected carrier");
    Json(
        json!({"id":"resp_loopback", "object":"response", "status":"completed", "model":"synthetic-model",
        "output":[{"id":"msg_loopback", "type":"message", "role":"assistant", "status":"completed",
            "content":[{"type":"output_text", "text":text, "annotations":[]}]}]}),
    )
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_native_json_tokenizes_upstream_and_restores_client() -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/v1/responses", post(echo_native))
        .with_state(seen.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "dlp-loopback-native".into(),
        "Synthetic loopback".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-loopback-credential"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let request = http::Request::builder().method("POST").uri("/v1/responses")
        .header("content-type", "application/json").header("session-id", "synthetic-loopback-session")
        .body(Body::from(json!({"model":"synthetic-model", "input":format!("Call {FIXTURE_PHONE}"), "stream":false}).to_string()))?;
    let result = handlers::handle_responses(State(state(db)), request).await;
    server.abort();
    let response = result?;
    let status = response.status();
    let body: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "synthetic fixture must restore successfully"
    );
    let upstream = seen.lock().map_err(|_| "fixture capture poisoned")?;
    assert_eq!(upstream.len(), 1);
    let outbound = upstream[0]["input"].as_str().ok_or("missing text")?;
    assert!(
        !outbound.contains(FIXTURE_PHONE),
        "fixture plaintext must not reach mock upstream"
    );
    assert!(
        outbound.contains("rredact-cn_phone-"),
        "upstream must receive reversible reference, not irreversible placeholder"
    );
    assert_eq!(
        body["output"][0]["content"][0]["text"],
        format!("Call {FIXTURE_PHONE}")
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_unsupported_route_rejects_before_provider_selection() -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let request = http::Request::builder().method("POST").uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(json!({"model":"synthetic-model", "messages":[{"role":"user","content":FIXTURE_PHONE}]}).to_string()))?;
    let response =
        handlers::handle_chat_completions(State(state(Arc::new(Database::memory()?))), request)
            .await;
    let response =
        response.map_err(|_| "unsupported restoration route reached provider selection")?;
    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert_eq!(body["error"]["code"], "dlp_restore_unsupported");
    Ok(())
}

#[derive(Clone)]
struct MatrixFixture {
    protocol: &'static str,
    compact: bool,
    streaming: bool,
    complete: bool,
    seen: Arc<Mutex<Vec<Value>>>,
}

fn fixture_text(value: &Value) -> Option<&str> {
    match value {
        Value::String(text)
            if text.contains("rredact-cn_phone-") || text.contains(FIXTURE_PHONE) =>
        {
            Some(text)
        }
        Value::Array(values) => values.iter().find_map(fixture_text),
        Value::Object(values) => values.values().find_map(fixture_text),
        _ => None,
    }
}

async fn matrix_upstream(
    State(fixture): State<MatrixFixture>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    if let Ok(mut seen) = fixture.seen.lock() {
        seen.push(body.clone());
    }
    let text = fixture_text(&body).unwrap_or("missing fixture carrier");
    let complete = fixture.complete;
    let mut native = json!({"id":"resp_matrix","object":"response","status":if complete {"completed"} else {"incomplete"},
        "model":"synthetic-model","output":[{"id":"msg_matrix","type":"message","role":"assistant",
        "content":[{"type":"output_text","text":text,"annotations":[]}]}]});
    if fixture.compact && matches!(fixture.protocol, "native" | "xai") {
        native["object"] = json!("response.compaction");
        native["created_at"] = json!(1);
        if let Some(output) = native["output"].as_array_mut() {
            output.push(json!({"id":"cmp_matrix","type":"compaction","encrypted_content":"synthetic-opaque"}));
        }
    }
    let chat = json!({"id":"chat_matrix","object":"chat.completion","model":"synthetic-model",
        "choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":if complete {"stop"} else {"length"}}]});
    let anthropic = json!({"id":"msg_matrix","type":"message","role":"assistant","model":"synthetic-model",
        "content":[{"type":"text","text":text}],"stop_reason":if complete {"end_turn"} else {"max_tokens"},
        "usage":{"input_tokens":1,"output_tokens":1}});
    if !fixture.streaming {
        return Json(match fixture.protocol {
            "chat" => chat,
            "anthropic" => anthropic,
            _ => native,
        })
        .into_response();
    }
    let events: Vec<Value> = match fixture.protocol {
        "chat" => vec![
            json!({"id":"chat_matrix","object":"chat.completion.chunk","model":"synthetic-model","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
            json!({"id":"chat_matrix","object":"chat.completion.chunk","model":"synthetic-model","choices":[{"index":0,"delta":{},"finish_reason":if complete {"stop"} else {"length"}}]}),
        ],
        "anthropic" => vec![
            json!({"type":"message_start","message":{"id":"msg_matrix","type":"message","role":"assistant","model":"synthetic-model","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":if complete {"end_turn"} else {"max_tokens"}},"usage":{"output_tokens":1}}),
            json!({"type":"message_stop"}),
        ],
        _ => vec![json!({"type":"response.completed","response":native})],
    };
    let mut wire = String::new();
    for event in events {
        if let Some(kind) = event.get("type").and_then(Value::as_str) {
            wire.push_str(&format!("event: {kind}\r\n"));
        }
        wire.push_str(&format!("data: {event}\r\n\r\n"));
    }
    if fixture.protocol != "anthropic" {
        wire.push_str("data: [DONE]\r\n\r\n");
    }
    ([("content-type", "text/event-stream")], wire).into_response()
}

async fn run_matrix_at(
    alias: &str,
    protocol: &'static str,
    streaming: bool,
    complete: bool,
    compact: bool,
) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let fixture = MatrixFixture {
        protocol,
        compact,
        streaming,
        complete,
        seen: seen.clone(),
    };
    let upstream = Router::new()
        .fallback(post(matrix_upstream))
        .with_state(fixture);
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let format = match protocol {
        "chat" => "chat_completions",
        "anthropic" => "anthropic",
        _ => "responses",
    };
    let settings = if protocol == "xai" {
        json!({"auth":{"OPENAI_API_KEY":"synthetic-matrix-credential"},"config":format!(
            "model_provider = \"custom\"\n[model_providers.custom]\nname = \"xai\"\nbase_url = \"http://{addr}/api.x.ai/v1\"\nwire_api = \"responses\"\n")})
    } else {
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-matrix-credential","api_format":format})
    };
    let provider = Provider::with_id(
        "dlp-matrix".into(),
        "Synthetic matrix".into(),
        settings,
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let uri = if compact {
        format!("{alias}/compact")
    } else {
        alias.to_string()
    };
    let request = http::Request::builder().method("POST").uri(uri)
        .header("content-type", "application/json").header("session-id", "synthetic-matrix-session")
        .body(Body::from(json!({"model":"synthetic-model","input":format!("Call {FIXTURE_PHONE}"),"stream":streaming}).to_string()))?;
    let proxy = ProxyServer::new(ProxyConfig::default(), db, None).with_restoration_gateway(
        RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024),
    );
    let response = proxy.build_router().call(request).await?;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    let text = std::str::from_utf8(&bytes)?;
    let captured = seen.lock().map_err(|_| "fixture capture poisoned")?;
    assert_eq!(captured.len(), 1, "exactly one attempt expected");
    let outbound = serde_json::to_string(&captured[0])?;
    assert!(
        !outbound.contains(FIXTURE_PHONE),
        "fixture plaintext must not reach mock upstream"
    );
    assert!(
        outbound.contains("rredact-cn_phone-"),
        "reversible reference required"
    );
    if complete {
        assert_eq!(
            status,
            http::StatusCode::OK,
            "complete fixture must succeed"
        );
        assert!(
            text.contains(FIXTURE_PHONE),
            "client exit must restore supported text"
        );
        assert!(
            !text.contains("rredact-cn_phone-"),
            "client exit must not expose references"
        );
    } else if streaming {
        assert!(
            text.contains("response.failed"),
            "incomplete source must fail closed after SSE headers"
        );
        assert!(
            !text.contains("\"type\":\"response.completed\""),
            "converted completion is not authority"
        );
        assert!(
            !text.contains("[DONE]"),
            "failed SSE must not signal success"
        );
    } else {
        assert_eq!(
            status,
            http::StatusCode::BAD_GATEWAY,
            "incomplete source must fail closed before body"
        );
        let value: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(value["error"]["code"], "dlp_restore_failed");
    }
    Ok(())
}

macro_rules! matrix_test {
    ($name:ident, $protocol:literal, $streaming:literal, $complete:literal, $compact:literal) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            run_matrix_at("/v1/responses", $protocol, $streaming, $complete, $compact).await
        }
    };
}
matrix_test!(reversible_http_native_json, "native", false, true, false);
matrix_test!(reversible_http_xai_json, "xai", false, true, false);
matrix_test!(reversible_http_xai_sse, "xai", true, true, false);
matrix_test!(reversible_http_xai_compact_json, "xai", false, true, true);
matrix_test!(
    reversible_http_xai_incomplete_json,
    "xai",
    false,
    false,
    false
);
matrix_test!(
    reversible_http_xai_incomplete_sse,
    "xai",
    true,
    false,
    false
);
matrix_test!(reversible_http_chat_json, "chat", false, true, false);
matrix_test!(
    reversible_http_anthropic_json,
    "anthropic",
    false,
    true,
    false
);
matrix_test!(reversible_http_native_sse, "native", true, true, false);
matrix_test!(reversible_http_chat_sse, "chat", true, true, false);
matrix_test!(
    reversible_http_anthropic_sse,
    "anthropic",
    true,
    true,
    false
);
matrix_test!(
    reversible_http_native_incomplete_json,
    "native",
    false,
    false,
    false
);
matrix_test!(
    reversible_http_chat_incomplete_json,
    "chat",
    false,
    false,
    false
);
matrix_test!(
    reversible_http_anthropic_incomplete_json,
    "anthropic",
    false,
    false,
    false
);
matrix_test!(
    reversible_http_native_incomplete_sse,
    "native",
    true,
    false,
    false
);
matrix_test!(
    reversible_http_chat_incomplete_sse,
    "chat",
    true,
    false,
    false
);
matrix_test!(
    reversible_http_anthropic_incomplete_sse,
    "anthropic",
    true,
    false,
    false
);
matrix_test!(
    reversible_http_native_compact_json,
    "native",
    false,
    true,
    true
);
matrix_test!(reversible_http_chat_compact_json, "chat", false, true, true);
matrix_test!(
    reversible_http_anthropic_compact_json,
    "anthropic",
    false,
    true,
    true
);

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_all_router_aliases() -> TestResult {
    for alias in [
        "/responses",
        "/v1/responses",
        "/v1/v1/responses",
        "/codex/v1/responses",
    ] {
        for protocol in ["native", "chat", "anthropic"] {
            for compact in [false, true] {
                run_matrix_at(alias, protocol, false, true, compact).await?;
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct WireFixture {
    seen: Arc<Mutex<Vec<Value>>>,
    streaming: bool,
    gzip: bool,
    protocol: &'static str,
}

async fn wire_upstream(
    State(fixture): State<WireFixture>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    use std::io::Write;
    if let Ok(mut seen) = fixture.seen.lock() {
        seen.push(body.clone());
    }
    let text = body["input"]
        .as_str()
        .or_else(|| body.pointer("/messages/0/content").and_then(Value::as_str))
        .or_else(|| fixture_text(&body))
        .unwrap_or("unexpected fixture carrier");
    let response = json!({"id":"resp_wire","object":"response","status":"completed","model":"synthetic-model",
        "output":[{"id":"msg_wire","type":"message","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":text,"annotations":[]}]}]});
    let response = if fixture.protocol == "chat" {
        json!({"id":"chat_wire","object":"chat.completion","model":"synthetic-model",
            "choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}]})
    } else if fixture.protocol == "anthropic" {
        json!({"id":"msg_wire","type":"message","role":"assistant","model":"synthetic-model",
            "content":[{"type":"text","text":text}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}})
    } else {
        response
    };
    let wire = if fixture.streaming && fixture.protocol == "chat" {
        let first = json!({"id":"chat_wire","object":"chat.completion.chunk","model":"synthetic-model",
            "choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]});
        let last = json!({"id":"chat_wire","object":"chat.completion.chunk","model":"synthetic-model",
            "choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
        format!("data: {first}\r\n\r\ndata: {last}\r\n\r\ndata: [DONE]\r\n\r\n")
    } else if fixture.streaming {
        format!(
            "event: response.completed\r\ndata: {}\r\n\r\ndata: [DONE]\r\n\r\n",
            json!({"type":"response.completed","response":response})
        )
    } else {
        response.to_string()
    };
    let content_type = if fixture.streaming {
        "text/event-stream"
    } else {
        "application/json"
    };
    if fixture.gzip {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        if encoder.write_all(wire.as_bytes()).is_err() {
            return http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        match encoder.finish() {
            Ok(bytes) => (
                [("content-type", content_type), ("content-encoding", "gzip")],
                bytes,
            )
                .into_response(),
            Err(_) => http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    } else {
        ([("content-type", content_type)], wire).into_response()
    }
}

async fn run_wire_policy(
    action: super::AuditAction,
    enabled: bool,
    streaming: bool,
    gzip: bool,
) -> TestResult {
    run_wire_policy_for_protocol(action, enabled, streaming, gzip, "native").await
}

async fn run_wire_policy_for_protocol(
    action: super::AuditAction,
    enabled: bool,
    streaming: bool,
    gzip: bool,
    protocol: &'static str,
) -> TestResult {
    run_wire_policy_with_prompt(action, enabled, streaming, gzip, protocol, None).await
}

async fn run_wire_policy_with_prompt(
    action: super::AuditAction,
    enabled: bool,
    streaming: bool,
    gzip: bool,
    protocol: &'static str,
    prompt_mode: Option<&str>,
) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    let mut config = runtime::SecurityRuntimeConfig::default();
    config.enabled = enabled;
    config
        .builtin_action_overrides
        .insert("chinese_phone_number".into(), action);
    runtime::set_security_runtime_for_test(config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let fixture = WireFixture {
        seen: seen.clone(),
        streaming,
        gzip,
        protocol,
    };
    let app = Router::new()
        .fallback(post(wire_upstream))
        .with_state(fixture);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let db = Arc::new(Database::memory()?);
    let mut provider = Provider::with_id(
        "dlp-wire".into(),
        "Synthetic wire".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-wire-credential", "api_format":if protocol == "chat" {"chat_completions"} else if protocol == "anthropic" {"anthropic"} else {"responses"}}),
        None,
    );
    if prompt_mode.is_some() {
        provider.meta = Some(crate::provider::ProviderMeta {
            prompt_cache_routing: Some(
                if prompt_mode == Some("disabled") {
                    "disabled"
                } else {
                    "enabled"
                }
                .into(),
            ),
            ..Default::default()
        });
    }
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let store = super::tokenization::DlpTokenStore::new();
    let budget = super::payload_budget::PayloadBudget::with_limit(64 * 1024 * 1024);
    let gateway = RestorationGateway::with_test_store_and_rules(store.clone(), &[], budget.clone());
    let baseline = budget.used();
    let proxy =
        ProxyServer::new(ProxyConfig::default(), db, None).with_restoration_gateway(gateway);
    let mut body = json!({"model":"synthetic-model","input":format!("Call {FIXTURE_PHONE}"),"stream":streaming});
    if prompt_mode.is_some() && prompt_mode != Some("no-key") {
        body["prompt_cache_key"] = json!(" \u{2003}synthetic-explicit-key\t ");
    }
    let mut request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json");
    if prompt_mode != Some("no-key") {
        request = request.header("session-id", "synthetic-wire-session");
    }
    let request = request.body(Body::from(body.to_string()))?;
    let response = if prompt_mode.is_some() {
        let (_hook, probe) = super::http_restoration::transform_input_test_support::install();
        let mut router = proxy.build_router();
        let mut pending = Box::pin(router.call(request));
        tokio::select! {
            _ = probe.reached.notified() => {},
            result = &mut pending => panic!("request finished before send barrier: {:?}", result.as_ref().map(|response| response.status())),
            _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => panic!("send barrier timeout"),
        }
        assert_eq!(probe.observation.lock().expect("synthetic probe").prompt_candidates, 0,
            "legacy, disabled key routing and generated session must not build restoration candidates");
        probe.resume.notify_one();
        pending.await?
    } else {
        proxy.build_router().call(request).await?
    };
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    let text = std::str::from_utf8(&bytes)?;
    let captured = seen.lock().map_err(|_| "fixture capture poisoned")?;
    assert_eq!(captured.len(), 1);
    if let Some(mode) = prompt_mode {
        if mode == "explicit" {
            assert_eq!(captured[0]["prompt_cache_key"], "synthetic-explicit-key");
        } else {
            assert!(
                captured[0].get("prompt_cache_key").is_none(),
                "disabled routing and generated UUIDs must not inject a key"
            );
        }
    }
    let input = captured[0]["input"]
        .as_str()
        .or_else(|| {
            captured[0]
                .pointer("/messages/0/content")
                .and_then(Value::as_str)
        })
        .or_else(|| fixture_text(&captured[0]))
        .ok_or("missing fixture input")?;
    if enabled && action == super::AuditAction::RedactRestore {
        assert!(
            input.contains("rredact-cn_phone-"),
            "real policy must activate production tokenization"
        );
        assert!(
            !input.contains(FIXTURE_PHONE),
            "synthetic plaintext cannot reach fixture upstream"
        );
        if streaming && gzip {
            assert_eq!(
                status,
                http::StatusCode::BAD_GATEWAY,
                "compressed leased SSE must reject before client headers"
            );
            let error: Value = serde_json::from_slice(&bytes)?;
            assert_eq!(error["error"]["code"], "dlp_restore_failed");
            assert!(!text.contains("response.completed"));
        } else {
            assert_eq!(status, http::StatusCode::OK);
            assert!(
                text.contains(FIXTURE_PHONE),
                "supported client text must restore"
            );
            assert!(!text.contains("rredact-cn_phone-"));
        }
    } else {
        assert_eq!(status, http::StatusCode::OK);
        if enabled {
            assert_eq!(input, "Call [REDACTED]");
            assert!(text.contains("[REDACTED]"));
        } else {
            assert_eq!(input, format!("Call {FIXTURE_PHONE}"));
        }
        let stats = store.stats()?;
        assert_eq!(
            stats.entries, 0,
            "ordinary traffic must not create token mappings"
        );
        assert_eq!(
            stats.namespaces, 0,
            "ordinary traffic must not allocate a restoration namespace"
        );
        assert_eq!(
            budget.used(),
            baseline,
            "ordinary traffic must not retain restoration payload buffers"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_real_policy_json() -> TestResult {
    run_wire_policy(super::AuditAction::RedactRestore, true, false, false).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_http_real_policy_gzip_json() -> TestResult {
    run_wire_policy(super::AuditAction::RedactRestore, true, false, true).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_http_compressed_leased_sse_rejects_before_client_headers() -> TestResult {
    run_wire_policy(super::AuditAction::RedactRestore, true, true, true).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_http_legacy_redact_has_no_restoration_mapping_or_buffer() -> TestResult {
    run_wire_policy(super::AuditAction::Redact, true, false, false).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_http_disabled_policy_has_no_restoration_mapping_or_buffer() -> TestResult {
    run_wire_policy(super::AuditAction::RedactRestore, false, false, false).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_compressed_chat_sse_rejects_before_conversion() -> TestResult {
    run_wire_policy_for_protocol(super::AuditAction::RedactRestore, true, true, true, "chat").await
}

#[derive(Clone)]
struct CacheOwnerFixture {
    streaming: bool,
}

async fn cache_owner_upstream(
    State(fixture): State<CacheOwnerFixture>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let text = fixture_text(&body).unwrap_or("missing synthetic carrier");
    let arguments = json!({"phone":text}).to_string();
    if !fixture.streaming {
        return Json(json!({"id":"chat_cache_owner","object":"chat.completion","model":"synthetic-model",
            "choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{
                "id":"call_cache_owner","type":"function","function":{"name":"synthetic_lookup","arguments":arguments}}]},
                "finish_reason":"tool_calls"}]})).into_response();
    }
    let chunks = [
        json!({"id":"chat_cache_owner","object":"chat.completion.chunk","model":"synthetic-model",
            "choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,
                "id":"call_cache_owner","type":"function","function":{"name":"synthetic_lookup","arguments":arguments}}]},"finish_reason":null}]}),
        json!({"id":"chat_cache_owner","object":"chat.completion.chunk","model":"synthetic-model",
            "choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    ];
    let mut wire = String::new();
    for chunk in chunks {
        wire.push_str(&format!("data: {chunk}\n\n"));
    }
    wire.push_str("data: [DONE]\n\n");
    ([("content-type", "text/event-stream")], wire).into_response()
}

async fn run_cache_owner_http(streaming: bool) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let upstream = Router::new()
        .fallback(post(cache_owner_upstream))
        .with_state(CacheOwnerFixture { streaming });
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "dlp-cache-owner".into(),
        "Synthetic cache owner".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-cache-credential","api_format":"chat_completions"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let budget = super::payload_budget::PayloadBudget::with_limit(64 * 1024 * 1024);
    let mut proxy_state = state(db);
    proxy_state.restoration_gateway = RestorationGateway::with_test_store_and_rules(
        super::tokenization::DlpTokenStore::new(),
        &["chinese_phone_number"],
        budget.clone(),
    );
    let history = proxy_state.codex_chat_history.clone();
    let request = http::Request::builder().method("POST").uri("/v1/responses")
        .header("content-type", "application/json").header("session-id", "synthetic-cache-session")
        .body(Body::from(json!({"model":"synthetic-model","input":format!("Call {FIXTURE_PHONE}"),"stream":streaming,
            "tools":[{"type":"function","name":"synthetic_lookup","parameters":{"type":"object","properties":{"phone":{"type":"string"}}}}]}).to_string()))?;
    let response = handlers::handle_responses(State(proxy_state), request).await?;
    assert_eq!(response.status(), http::StatusCode::OK);
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    let text = std::str::from_utf8(&bytes)?;
    assert!(
        text.contains(FIXTURE_PHONE),
        "client tool argument must be restored without executing it"
    );
    assert!(!text.contains("rredact-cn_phone-"));
    drop(bytes);
    let followup_owners;
    let mut followup = json!({"input":[{"type":"function_call_output","call_id":"call_cache_owner","output":"synthetic result"}]});
    let (restored, owners) = history
        .enrich_request_with_payload_owners(&mut followup)
        .await
        .map_err(|error| std::io::Error::other(error.code()))?;
    followup_owners = owners;
    assert!(restored > 0, "synthetic call must remain cached");
    let cached = followup.to_string();
    assert!(
        cached.contains("rredact-cn_phone-"),
        "history must retain upstream-tokenized arguments"
    );
    assert!(!cached.contains(FIXTURE_PHONE));
    drop(followup);
    drop(followup_owners);
    assert!(
        budget.used() > 0,
        "cached tokenized calls must retain the upstream processing reservation"
    );
    drop(history);
    assert_eq!(
        budget.used(),
        0,
        "eviction/drop releases the last retained response owner"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_chat_json_cache_retains_upstream_payload_owner() -> TestResult {
    run_cache_owner_http(false).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_chat_sse_cache_retains_upstream_payload_owner() -> TestResult {
    run_cache_owner_http(true).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_actual_send_audit_is_request_action_not_restoration_success() -> TestResult
{
    use super::http_restoration::{admit_http_request, RestorationAdmission};
    use crate::proxy::{
        forwarder::RequestForwarder,
        types::{CopilotOptimizerConfig, OptimizerConfig, RectifierConfig},
    };
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let fixture = MatrixFixture {
        protocol: "native",
        compact: false,
        streaming: false,
        complete: false,
        seen: seen.clone(),
    };
    let upstream = Router::new()
        .fallback(post(matrix_upstream))
        .with_state(fixture);
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "dlp-actual-audit".into(),
        "Synthetic audit".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-audit-credential"}),
        None,
    );
    let proxy_state = state(db);
    let mut body = json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":false});
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", "synthetic-audit-session".parse()?);
    let request = match admit_http_request(
        &proxy_state.restoration_gateway,
        &crate::app_config::AppType::Codex,
        true,
        false,
        &mut body,
        &headers,
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic audit request not admitted".into()),
    };
    let observe = request.audit_observer_for_test();
    assert!(
        observe().is_empty(),
        "admission alone must not claim an outbound action"
    );
    let forwarder = RequestForwarder::new(
        proxy_state.provider_router.clone(),
        5,
        proxy_state.status.clone(),
        proxy_state.current_providers.clone(),
        proxy_state.gemini_shadow.clone(),
        proxy_state.codex_chat_history.clone(),
        proxy_state.failover_manager.clone(),
        None,
        provider.id.clone(),
        "synthetic-audit-session".into(),
        true,
        5,
        5,
        RectifierConfig::default(),
        OptimizerConfig::default(),
        CopilotOptimizerConfig::default(),
        0,
    )
    .with_dlp_request(Some(request));
    let mut result = forwarder
        .forward_with_retry(
            &crate::app_config::AppType::Codex,
            http::Method::POST,
            "/v1/responses",
            body,
            headers,
            http::Extensions::new(),
            vec![provider.clone()],
        )
        .await
        .map_err(|_| "synthetic actual send failed")?;
    assert_eq!(
        seen.lock().map_err(|_| "synthetic capture poisoned")?.len(),
        1
    );
    let events = observe();
    assert_eq!(
        events.len(),
        1,
        "actual network attempt must record its request action exactly once"
    );
    assert_eq!(events[0].log.action_taken, "redact_restore");
    assert_eq!(events[0].log.provider_id, provider.id);
    let raw = serde_json::to_string(&events[0].log)?;
    assert!(!raw.contains(FIXTURE_PHONE));
    assert!(!raw.contains("rredact-cn_phone-"));
    let attempt = result
        .dlp_attempt
        .take()
        .ok_or("missing synthetic attempt")?;
    let bytes = result
        .response
        .bytes_with_limit(1024 * 1024)
        .await
        .map_err(|_| "synthetic body read failed")?;
    let response = http::Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Body::from(bytes))?;
    let restored = attempt
        .restore_client_http_response(
            response,
            super::response_restoration::ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    assert_eq!(
        restored.status(),
        http::StatusCode::BAD_GATEWAY,
        "request audit does not authorize incomplete response restoration"
    );
    assert_eq!(
        observe().len(),
        1,
        "response failure cannot rewrite or duplicate request-action audit"
    );
    server.abort();
    Ok(())
}

async fn check_upstream_error_echo(
    upstream_status: http::StatusCode,
    streaming: bool,
    protocol: &str,
) -> TestResult {
    use crate::proxy::{forwarder::upstream_error_test_support, hyper_client::ProxyResponse};
    let transport = upstream_error_test_support::install_with(move |body| {
        let reference = fixture_text(body).unwrap_or("synthetic-fallback");
        let payload = json!({"error":{"message":format!("synthetic-safe-reason {reference}"),
            "type":"synthetic_upstream_error","code":"synthetic_error"}});
        ProxyResponse::buffered(
            upstream_status,
            http::HeaderMap::new(),
            bytes::Bytes::from(payload.to_string()),
        )
    });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(6);
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-private-provider-marker".into(),
        "Synthetic private provider label".into(),
        json!({"base_url":"http://127.0.0.1:1","api_key":"synthetic-error-credential", "api_format":protocol}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let proxy_state = state(db.clone());
    let status = proxy_state.status.clone();
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":streaming}).to_string(),
        ))?;
    let response = tokio::time::timeout_at(
        deadline,
        handlers::handle_responses(State(proxy_state), request),
    )
    .await
    .map_err(|_| "synthetic upstream forwarding deadline exceeded")??;
    let http_status = response.status();
    assert_eq!(response.headers()["content-type"], "application/json");
    let bytes = tokio::time::timeout_at(deadline, response.into_body().collect())
        .await
        .map_err(|_| "synthetic upstream body deadline exceeded")??
        .to_bytes();
    assert!(transport.attempts() == 1, "unexpected_retry");
    let text = std::str::from_utf8(&bytes)?;
    let last_error = tokio::time::timeout_at(deadline, status.read())
        .await
        .map_err(|_| "synthetic upstream status deadline exceeded")?
        .last_error
        .clone()
        .unwrap_or_default();
    let (recorded_status, recorded_error, recorded_streaming): (u16, String, bool) = {
        let conn = db
            .conn
            .lock()
            .map_err(|_| "synthetic database lock poisoned")?;
        conn.query_row(
            "SELECT status_code, error_message, is_streaming FROM proxy_request_logs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?
    };
    assert_eq!(recorded_status, upstream_status.as_u16());
    assert_eq!(recorded_streaming, streaming);
    assert!(
        recorded_error
            == format!(
                "upstream_error HTTP {} upstream_error",
                upstream_status.as_u16()
            ),
        "diagnostic_category_changed"
    );
    for diagnostic in [&last_error, &recorded_error] {
        assert!(
            !diagnostic.contains("synthetic-safe-reason"),
            "diagnostic_body_exposure"
        );
    }
    for observed in [text, last_error.as_str(), recorded_error.as_str()] {
        for forbidden in [
            FIXTURE_PHONE,
            "rredact-cn_phone-",
            "synthetic-upstream-private-marker",
            "synthetic-private-provider-marker",
            "Synthetic private provider label",
            "synthetic-model",
            "synthetic-error-credential",
            "dlp_restore_failed",
            "restoration failed",
            "DLP forwarding error",
        ] {
            assert!(
                !observed.contains(forbidden),
                "leased upstream errors must be safe, non-enriched upstream failures"
            );
        }
    }
    assert_eq!(
        http_status, upstream_status,
        "upstream status must survive isolation"
    );
    let body: Value = serde_json::from_slice(&bytes)?;
    assert!(body["error"]["code"] == "synthetic_error", "safe_code_lost");
    assert!(
        body["error"]["type"] == "synthetic_upstream_error",
        "safe_type_lost"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("synthetic-safe-reason")),
        "safe_reason_lost"
    );
    assert_eq!(
        tokio::time::timeout_at(deadline, status.read())
            .await
            .map_err(|_| "synthetic upstream status deadline exceeded")?
            .failed_requests,
        1
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_upstream_error_echo_is_fixed_without_reference_or_diagnostics(
) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    check_upstream_error_echo(http::StatusCode::BAD_REQUEST, false, "responses").await?;
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_upstream_error_status_stream_protocol_matrix() -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    for status in [400, 401, 403, 404, 413, 422, 429, 500, 502, 503, 504] {
        for streaming in [false, true] {
            for protocol in ["responses", "chat_completions", "anthropic"] {
                check_upstream_error_echo(http::StatusCode::from_u16(status)?, streaming, protocol)
                    .await?;
            }
        }
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_chat_enrichment_owner_survives_cache_replacement_during_send() -> TestResult
{
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (owner, used) = super::UpstreamPayloadOwner::test_fixture()
        .map_err(|error| std::io::Error::other(error.code()))?;
    let (_candidate_probe, candidate_state) = super::chat_enrichment::test_support::install();
    let observed_candidate = Arc::new(AtomicUsize::new(0));
    let used = Arc::new(used);
    let history = Arc::new(CodexChatHistoryStore::default());
    let observed = Arc::new(AtomicUsize::new(0));
    let captured = Arc::new(Mutex::new(Value::Null));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let fixture_history = history.clone();
    let fixture_observed = observed.clone();
    let fixture_captured = captured.clone();
    let fixture_candidate_state = candidate_state.clone();
    let fixture_candidate_observed = observed_candidate.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let upstream = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let history = fixture_history.clone();
        let call = calls.fetch_add(1, Ordering::SeqCst);
        let used = used.clone();
        let observed = fixture_observed.clone();
        let captured = fixture_captured.clone();
        let candidate_state = fixture_candidate_state.clone();
        let candidate_observed = fixture_candidate_observed.clone();
        async move {
            if call == 0 { return cache_owner_upstream(State(CacheOwnerFixture { streaming:false }), Json(body)).await; }
            let replacement = json!({"id":"resp_chat_cache_owner","output":[{"type":"function_call",
                "id":"fc_enrichment_owner","call_id":"call_cache_owner","name":"synthetic_lookup",
                "arguments":"{\"marker\":\"synthetic-replacement\"}"}]});
            history.record_response(&replacement).await;
            observed.store(used(), Ordering::SeqCst);
            candidate_observed.store(candidate_state.lock().expect("synthetic observer").live_bytes, Ordering::SeqCst);
            if let Ok(mut captured) = captured.lock() { *captured = body.clone(); }
            let text = fixture_text(&body).unwrap_or("missing synthetic carrier");
            Json(json!({"id":"chat_enrichment_owner","object":"chat.completion","model":"synthetic-model",
                "choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}]})).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "dlp-enrichment-owner".into(),
        "Synthetic owner send".into(),
        json!({"base_url":format!("http://{addr}"),"auth":{"OPENAI_API_KEY":"synthetic-owner-credential"},"api_format":"chat_completions"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let mut proxy_state = state(db);
    proxy_state.codex_chat_history = history.clone();
    let initial = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-owner-session")
        .body(Body::from(
            json!({"model":"synthetic-model","stream":false,"input":FIXTURE_PHONE}).to_string(),
        ))?;
    let initial = handlers::handle_responses(State(proxy_state.clone()), initial).await?;
    assert_eq!(initial.status(), http::StatusCode::OK);
    drop(initial.into_body().collect().await?);
    let from_cache_owners;
    let mut from_cache = json!({"input":[{"type":"function_call_output","call_id":"call_cache_owner","output":"synthetic result"}]});
    let (restored, owners) = history
        .enrich_request_with_payload_owners(&mut from_cache)
        .await
        .map_err(|error| std::io::Error::other(error.code()))?;
    from_cache_owners = owners;
    assert!(restored > 0);
    let call_item = from_cache["input"][0].clone();
    let cached = json!({"id":"resp_chat_cache_owner","output":[call_item]});
    assert_eq!(
        history
            .record_response_with_payload_owner(&cached, Some(owner))
            .await,
        1
    );
    drop(cached);
    drop(from_cache);
    drop(from_cache_owners);
    *candidate_state.lock().expect("synthetic observer") = Default::default();
    let request = http::Request::builder().method("POST").uri("/v1/responses")
        .header("content-type", "application/json").header("session-id", "synthetic-owner-session")
        .body(Body::from(json!({"model":"synthetic-model","stream":false,"input":[
            {"role":"user","content":FIXTURE_PHONE},
            {"type":"function_call_output","call_id":"call_cache_owner","output":"synthetic result"}]}).to_string()))?;
    let response = handlers::handle_responses(State(proxy_state), request).await?;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    assert!(
        captured
            .lock()
            .map_err(|_| "fixture capture poisoned")?
            .to_string()
            .contains("call_cache_owner"),
        "actual outbound request must use the enriched cached call; status={status}, body={}",
        std::str::from_utf8(&bytes)?
    );
    assert!(
        observed_candidate.load(Ordering::SeqCst) > 0,
        "the complete independent candidate charge must survive the actual send"
    );
    assert_eq!(
        observed.load(Ordering::SeqCst),
        0,
        "a current-budget candidate must not pin the retired source workspace"
    );
    let final_candidate = *candidate_state.lock().expect("synthetic observer");
    assert_eq!(final_candidate.created, 1);
    assert_eq!(final_candidate.released, 1);
    assert_eq!(final_candidate.live_bytes, 0);
    assert_eq!(status, http::StatusCode::OK);
    assert!(std::str::from_utf8(&bytes)?.contains(FIXTURE_PHONE));
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_ownerless_request_guarded_history_capacity_refuses_before_send(
) -> TestResult {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (owner, used) =
        super::upstream_guard::UpstreamPayloadOwner::test_fixture_with_limit(64 * 1024 + 32 * 1024)
            .map_err(|error| std::io::Error::other(error.code()))?;
    let history = Arc::new(CodexChatHistoryStore::default());
    let item = json!({"type":"function_call","id":"fc_capacity_fixture",
        "call_id":"call_capacity_fixture","name":"synthetic_lookup",
        "arguments":"{\"marker\":\"synthetic-cache-value\"}"});
    assert_eq!(
        history
            .record_response_with_payload_owner(
                &json!({"id":"resp_capacity_fixture","output":[item.clone()]}),
                Some(owner.clone()),
            )
            .await,
        1,
    );
    // Each pair drops its actual JSON allocation before its copy reservation.
    let mut retained_copies = Vec::new();
    loop {
        match owner.try_clone_json(&item) {
            Ok(copy) => retained_copies.push(copy),
            Err(super::http_restoration::DlpForwardError::CapacityExceeded) => break,
            Err(error) => return Err(std::io::Error::other(error.code()).into()),
        }
    }
    assert!(!retained_copies.is_empty());
    let retained_charge = used();
    let sends = Arc::new(AtomicUsize::new(0));
    let fixture_sends = sends.clone();
    let upstream = Router::new().fallback(post(move || {
        fixture_sends.fetch_add(1, Ordering::SeqCst);
        async {
            Json(
                json!({"id":"chat_capacity_fixture","object":"chat.completion",
                "choices":[{"index":0,"message":{"role":"assistant","content":"synthetic response"},
                    "finish_reason":"stop"}]}),
            )
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "dlp-capacity-provider".into(),
        "Synthetic capacity provider".into(),
        json!({"base_url":format!("http://{addr}"),
            "auth":{"OPENAI_API_KEY":"synthetic-capacity-credential"},"api_format":"chat_completions"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let mut proxy_state = state(db);
    proxy_state.codex_chat_history = history.clone();
    // No enabled restoration action, token, or current restoration dependency.
    proxy_state.restoration_gateway = RestorationGateway::new_default();
    let request = http::Request::builder().method("POST").uri("/v1/responses")
        .header("content-type", "application/json")
        .body(Body::from(json!({"model":"synthetic-model","stream":false,"input":[
            {"type":"function_call_output","call_id":"call_capacity_fixture","output":"synthetic result"}
        ]}).to_string()))?;
    let response = match handlers::handle_responses(State(proxy_state), request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    assert_eq!(
        sends.load(Ordering::SeqCst),
        0,
        "capacity refusal must precede every actual provider send"
    );
    assert_eq!(status, http::StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(body["error"]["code"], "dlp_capacity_exceeded");
    assert_eq!(body["error"]["type"], "server_error");
    assert!(body["error"]["param"].is_null());
    let text = std::str::from_utf8(&bytes)?;
    for private in [
        "synthetic-cache-value",
        "call_capacity_fixture",
        "resp_capacity_fixture",
        "dlp-capacity-provider",
        "synthetic-model",
        "synthetic-capacity-credential",
    ] {
        assert!(!text.contains(private), "capacity response must stay fixed");
    }
    assert_eq!(
        used(),
        retained_charge,
        "refusal must release all temporary copies"
    );
    drop(history);
    drop(retained_copies);
    assert_eq!(used(), 64 * 1024);
    drop(owner);
    assert_eq!(used(), 0);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_guarded_history_owner_releases_when_send_is_cancelled() -> TestResult {
    use super::{
        http_restoration::transform_input_test_support, payload_budget::PayloadBudget,
        upstream_error::ErrorCredentials,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (owner, used) = super::upstream_guard::UpstreamPayloadOwner::test_fixture()
        .map_err(|error| std::io::Error::other(error.code()))?;
    let history = Arc::new(CodexChatHistoryStore::default());
    let sends = Arc::new(AtomicUsize::new(0));
    let pending_sends = sends.clone();
    let wire_length = Arc::new(AtomicUsize::new(0));
    let pending_wire_length = wire_length.clone();
    let credential_charge = Arc::new(AtomicUsize::new(0));
    let pending_credential_charge = credential_charge.clone();
    let entered = Arc::new(tokio::sync::Notify::new());
    let pending_entered = entered.clone();
    let pending_history = history.clone();
    let upstream = Router::new().fallback(post(
        move |headers: http::HeaderMap, Json(body): Json<Value>| {
            let send = pending_sends.fetch_add(1, Ordering::SeqCst);
            let entered = pending_entered.clone();
            let history = pending_history.clone();
            let wire_length = pending_wire_length.clone();
            let credential_charge = pending_credential_charge.clone();
            async move {
                if send == 0 {
                    return Json(
                        json!({"id":"resp_cancel_fixture","object":"chat.completion",
                        "model":"synthetic-model","choices":[{"index":0,"message":{
                            "role":"assistant","content":null,"tool_calls":[{
                                "id":"call_cancel_fixture","type":"function","function":{
                                    "name":"synthetic_lookup",
                                    "arguments":"{\"marker\":\"synthetic-cancel-value\"}"}}]},
                            "finish_reason":"tool_calls"}]}),
                    )
                    .into_response();
                }
                assert_eq!(send, 1, "only the strict continuation may remain pending");
                let length = headers
                    .get(http::header::CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or_default();
                assert_eq!(
                    length,
                    serde_json::to_vec(&body).expect("synthetic wire").len()
                );
                wire_length.store(length, Ordering::SeqCst);
                assert!(
                    body["messages"].as_array().is_some_and(|messages| {
                        messages.iter().any(|message| {
                            message["tool_calls"].as_array().is_some_and(|calls| {
                                calls.iter().any(|call| {
                                    call["id"] == "call_cancel_fixture"
                                        && call["function"]["arguments"]
                                            == "{\"marker\":\"synthetic-cancel-value\"}"
                                })
                            })
                        })
                    }),
                    "the actual send must contain the independently copied cached call"
                );
                // Measure actual synthetic header ownership on a separate ledger.
                let credentials_budget = PayloadBudget::with_limit(64 * 1024);
                let credentials = ErrorCredentials::capture(&headers, credentials_budget.clone())
                    .expect("synthetic credential capture");
                credential_charge.store(credentials_budget.used(), Ordering::SeqCst);
                drop(credentials);
                assert_eq!(credentials_budget.used(), 0);
                // Remove the guarded cache allocation while the request's independent
                // enriched copy is still retained by the actual-send future.
                assert_eq!(
                    history
                        .record_response(&json!({"id":"resp_cancel_fixture","output":[{
                            "type":"function_call","id":"fc_cancel_fixture",
                            "call_id":"call_cancel_fixture","name":"synthetic_lookup",
                            "arguments":"{}"}]}))
                        .await,
                    1
                );
                entered.notify_one();
                std::future::pending::<axum::response::Response>().await
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "dlp-cancel-provider".into(),
        "Synthetic cancellation provider".into(),
        json!({"base_url":format!("http://{addr}"),"auth":{"OPENAI_API_KEY":"synthetic-cancel-credential"},
            "api_format":"chat_completions"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let request_budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let mut proxy_state = state(db);
    proxy_state.codex_chat_history = history.clone();
    proxy_state.restoration_gateway = RestorationGateway::with_test_store_and_rules(
        super::tokenization::DlpTokenStore::new(),
        &["chinese_phone_number"],
        request_budget.clone(),
    );
    // Publish real completion authority under the same provider, session and rules.
    let initial = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-cancel-session")
        .body(Body::from(
            json!({"model":"synthetic-model","stream":false,
            "input":"Synthetic cancellation admission",
            "tools":[{"type":"function","name":"synthetic_lookup",
                "parameters":{"type":"object","properties":{"marker":{"type":"string"}}}}]})
            .to_string(),
        ))?;
    let initial = handlers::handle_responses(State(proxy_state.clone()), initial).await?;
    assert_eq!(initial.status(), http::StatusCode::OK);
    let completed: Value =
        serde_json::from_slice(&initial.into_body().collect().await?.to_bytes())?;
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["id"], "resp_cancel_fixture");
    let cached_call = completed["output"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["type"] == "function_call"))
        .ok_or("missing synthetic completed call")?;
    let before_copy = used();
    let probe = owner
        .try_clone_json(cached_call)
        .map_err(|error| std::io::Error::other(error.code()))?;
    assert!(used() > before_copy);
    drop(probe);
    assert_eq!(used(), before_copy);
    assert_eq!(
        history
            .record_response_with_payload_owner(
                &json!({"id":"resp_cancel_fixture","output":[cached_call]}),
                Some(owner),
            )
            .await,
        1
    );
    drop(completed);
    assert_eq!(request_budget.used(), 0);
    let cache_charge = used();
    assert!(
        cache_charge > before_copy,
        "the guarded cache must retain its own copy charge"
    );
    let (_probe_guard, candidate_state) = super::chat_enrichment::test_support::install();
    let (_transform_guard, transform_probe) = transform_input_test_support::install();
    // The missing call plus previous_response_id force StrictResume, not readable replay.
    let request = http::Request::builder().method("POST").uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-cancel-session")
        .body(Body::from(json!({"model":"synthetic-model","stream":false,
            "previous_response_id":"resp_cancel_fixture","input":[
                {"type":"function_call_output","call_id":"call_cancel_fixture","output":"synthetic result"}
            ]}).to_string()))?;
    let mut forwarding = Box::pin(handlers::handle_responses(State(proxy_state), request));
    let prepared = tokio::select! {
        _ = transform_probe.reached.notified() => true,
        _ = &mut forwarding => false,
        _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => false,
    };
    if !prepared {
        server.abort();
        return Err("synthetic strict continuation did not reach admitted preparation".into());
    }
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(
        used(),
        cache_charge,
        "preparation only borrows the guarded cache owner"
    );
    let prepared_candidate = *candidate_state.lock().expect("synthetic observer");
    assert_eq!(prepared_candidate.created, 1);
    assert_eq!(prepared_candidate.released, 0);
    assert!(prepared_candidate.live_bytes > 0);
    // All admitted preparation owners are live here, before wire serialization
    // and credential capture. The continuation has no audit findings to grow.
    let prepared_charge = request_budget.used();
    assert!(prepared_charge > prepared_candidate.live_bytes);
    transform_probe.resume.notify_one();
    let reached_upstream = tokio::select! {
        _ = entered.notified() => true,
        _ = &mut forwarding => false,
        _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => false,
    };
    if !reached_upstream {
        server.abort();
        return Err("synthetic request did not remain pending at upstream".into());
    }
    let serialized_length = wire_length.load(Ordering::SeqCst);
    let credentials_charge = credential_charge.load(Ordering::SeqCst);
    let charged_during_send = request_budget.used();
    assert_eq!(
        used(),
        0,
        "the independent candidate must not pin the retired cache workspace"
    );
    let pending_candidate = *candidate_state.lock().expect("synthetic observer");
    assert_eq!(pending_candidate.created, 1);
    assert_eq!(pending_candidate.released, 0);
    assert_eq!(pending_candidate.live_bytes, prepared_candidate.live_bytes);
    drop(forwarding);
    assert!(
        serialized_length > 0,
        "synthetic request must have an exact wire length"
    );
    assert!(credentials_charge > 0);
    assert_eq!(
        charged_during_send,
        prepared_charge + serialized_length + credentials_charge,
        "active send retains exact admitted preparation, independent wire and credential charges"
    );
    server.abort();
    let released_candidate = *candidate_state.lock().expect("synthetic observer");
    assert_eq!(released_candidate.released, 1);
    assert_eq!(released_candidate.live_bytes, 0);
    assert_eq!(
        used(),
        0,
        "cancelling the send must leave no guarded cache payload owner"
    );
    assert_eq!(
        request_budget.used(),
        0,
        "cancelling the send must release all admission, independent candidate and wire owners"
    );
    Ok(())
}

#[tokio::test]
async fn chat_enrichment_current_budget_noop_releases_plans_and_preserves_body() -> TestResult {
    use super::{chat_enrichment::ChatEnrichmentBudget, payload_budget::PayloadBudget};
    let history = CodexChatHistoryStore::default();
    let budget = PayloadBudget::with_limit(64 * 1024);
    let mut body =
        json!({"model":"synthetic-noop","input":[{"role":"user","content":"synthetic text"}]});
    let encoded = serde_json::to_string(&body)?;
    let (_probe, candidate) = super::chat_enrichment::test_support::install();
    let plans = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let observed = plans.clone();
    let _hook = super::payload_metadata::test_support::install(move |event| {
        if event.phase == super::payload_metadata::test_support::Phase::BeforeAllocation {
            observed.set(observed.get() + 1);
        }
        super::payload_metadata::test_support::Action::Continue
    });
    let (changed, owner) = history
        .enrich_request_with_budget(&mut body, Some(ChatEnrichmentBudget::new(budget.clone())))
        .await
        .map_err(|error| std::io::Error::other(error.code()))?;
    assert_eq!(changed, 0);
    assert!(owner.is_none());
    assert!(
        plans.get() > 0,
        "current-budget no-op still charges actual plan backing"
    );
    assert_eq!(candidate.lock().expect("synthetic observer").created, 0);
    assert_eq!(serde_json::to_string(&body)?, encoded);
    assert_eq!(budget.used(), 0);
    let fill = budget
        .reserve(budget.limit())
        .map_err(|error| std::io::Error::other(error.code()))?;
    let result = history
        .enrich_request_with_budget(&mut body, Some(ChatEnrichmentBudget::new(budget.clone())))
        .await;
    assert!(matches!(
        result,
        Err(super::http_restoration::DlpForwardError::CapacityExceeded)
    ));
    assert_eq!(serde_json::to_string(&body)?, encoded);
    assert_eq!(candidate.lock().expect("synthetic observer").created, 0);
    assert_eq!(budget.used(), fill.bytes());
    drop(fill);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[derive(Clone, Copy)]
enum EnrichmentRefusal {
    Unavailable,
    PlanCapacity,
    CandidateCapacity,
}

#[tokio::test]
#[serial_test::serial]
async fn chat_enrichment_refusal_keeps_audit_and_half_open_health_neutral() -> TestResult {
    chat_enrichment_refusal_fixture(EnrichmentRefusal::Unavailable).await
}

#[tokio::test]
#[serial_test::serial]
async fn chat_enrichment_refusal_real_plan_capacity_is_nonretryable() -> TestResult {
    chat_enrichment_refusal_fixture(EnrichmentRefusal::PlanCapacity).await
}

#[tokio::test]
#[serial_test::serial]
async fn chat_enrichment_refusal_real_candidate_capacity_is_nonretryable() -> TestResult {
    chat_enrichment_refusal_fixture(EnrichmentRefusal::CandidateCapacity).await
}

async fn chat_enrichment_refusal_fixture(refusal: EnrichmentRefusal) -> TestResult {
    use super::{
        http_restoration::{admit_http_request, source_copy_test_support, RestorationAdmission},
        payload_budget::PayloadBudget,
        payload_json::PayloadJson,
        tokenization::DlpTokenStore,
    };
    use crate::{app_config::AppType, proxy::forwarder::RequestForwarder};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let sends = Arc::new(AtomicUsize::new(0));
    let observed_sends = sends.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = Router::new().fallback(post(move || {
        observed_sends.fetch_add(1, Ordering::SeqCst);
        async { Json(json!({"error":"synthetic unexpected send"})) }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-enrichment-provider".into(),
        "Synthetic enrichment provider".into(),
        json!({"base_url":format!("http://{address}"),"auth":{"OPENAI_API_KEY":"synthetic-enrichment-credential"},"api_format":"chat_completions"}),
        None,
    );
    let fallback = Provider::with_id(
        "synthetic-enrichment-fallback".into(),
        "Synthetic enrichment fallback".into(),
        provider.settings_config.clone(),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.save_provider("codex", &fallback)?;
    let state = state(db);
    state
        .codex_chat_history
        .record_response(&json!({"id":"resp_enrichment_refusal","output":[{
            "type":"function_call","call_id":"call_enrichment_refusal","name":"synthetic_lookup",
            "arguments":json!({"synthetic_padding":"x".repeat(16 * 1024)}).to_string()
        }]}))
        .await;
    let _ = state
        .provider_router
        .allow_provider_request(&provider.id, "codex")
        .await;
    state
        .provider_router
        .update_app_configs(
            "codex",
            crate::proxy::circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                timeout_seconds: 0,
                ..Default::default()
            },
        )
        .await;
    state
        .provider_router
        .record_result(&provider.id, "codex", false, false, None)
        .await?;
    let permit = state
        .provider_router
        .allow_provider_request(&provider.id, "codex")
        .await;
    assert!(permit.allowed && permit.used_half_open_permit);
    state
        .provider_router
        .release_permit_neutral(&provider.id, "codex", true)
        .await;
    let before = state
        .provider_router
        .get_circuit_breaker_stats(&provider.id, "codex")
        .await
        .ok_or("missing synthetic breaker")?;
    let health = serde_json::to_value(state.db.get_provider_health(&provider.id, "codex").await?)?;
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let gateway = RestorationGateway::with_test_store_and_rules(
        DlpTokenStore::new(),
        &["chinese_phone_number"],
        budget.clone(),
    );
    let mut body = json!({"model":"synthetic-model","input":[
        {"role":"user","content":FIXTURE_PHONE},
        {"type":"function_call_output","call_id":"call_enrichment_refusal","output":"ok"}
    ]});
    let request = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut body,
        &Default::default(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic enrichment request was not admitted".into()),
    };
    let source_charge = {
        let before = budget.used();
        let copy = PayloadJson::try_from_value(&body, budget.clone(), budget.limit())
            .map_err(|error| std::io::Error::other(error.code()))?;
        let charge = budget.used() - before;
        drop(copy);
        charge
    };
    let free_after_source = match refusal {
        EnrichmentRefusal::Unavailable => None,
        EnrichmentRefusal::PlanCapacity => Some(0),
        // Enough for all borrowed plan backing, but less than the 16 KiB
        // cached argument alone. Candidate admission must reject before build.
        EnrichmentRefusal::CandidateCapacity => Some(8 * 1024),
    };
    let fill = free_after_source
        .map(|free| budget.reserve(budget.limit() - budget.used() - 2 * source_charge - free))
        .transpose()
        .map_err(|error| std::io::Error::other(error.code()))?;
    let audit = request.audit_observer_for_test();
    let forwarder = RequestForwarder::new(
        state.provider_router.clone(),
        5,
        state.status.clone(),
        state.current_providers.clone(),
        state.gemini_shadow.clone(),
        state.codex_chat_history.clone(),
        state.failover_manager.clone(),
        None,
        provider.id.clone(),
        "synthetic-enrichment-session".into(),
        false,
        5,
        5,
        Default::default(),
        Default::default(),
        Default::default(),
        3,
    )
    .with_dlp_request(Some(request));
    let plan_attempts = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let attempts = plan_attempts.clone();
    let _hook = super::payload_metadata::test_support::install(move |event| {
        if event.phase == super::payload_metadata::test_support::Phase::BeforeAllocation {
            attempts.set(attempts.get() + 1);
            if matches!(refusal, EnrichmentRefusal::Unavailable) {
                return super::payload_metadata::test_support::Action::Fail;
            }
        }
        super::payload_metadata::test_support::Action::Continue
    });
    let json_builds = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let builds = json_builds.clone();
    let _json_hook = super::payload_json::test_support::install(move |event| {
        if event.phase == super::payload_json::test_support::Phase::BeforeBuild {
            builds.set(builds.get() + 1);
        }
        super::payload_json::test_support::Action::Continue
    });
    let source_probe = source_copy_test_support::install();
    let mapping_probe = super::http_restoration::mapping_copy_test_support::install();
    let (_probe, candidate) = super::chat_enrichment::test_support::install();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        forwarder.forward_with_retry(
            &AppType::Codex,
            http::Method::POST,
            "/v1/responses",
            body,
            Default::default(),
            Default::default(),
            vec![provider.clone(), fallback.clone()],
        ),
    )
    .await?;
    server.abort();
    let error = match result {
        Err(error) => error,
        Ok(_) => return Err("enrichment refusal sent upstream".into()),
    };
    let expected = match refusal {
        EnrichmentRefusal::Unavailable => super::http_restoration::DlpForwardError::Unavailable,
        _ => super::http_restoration::DlpForwardError::CapacityExceeded,
    };
    assert!(
        matches!(&error.error, crate::proxy::error::ProxyError::DlpForward(found) if *found == expected)
    );
    match refusal {
        EnrichmentRefusal::Unavailable => assert_eq!(plan_attempts.get(), 1),
        EnrichmentRefusal::PlanCapacity => assert_eq!(plan_attempts.get(), 0),
        EnrichmentRefusal::CandidateCapacity => assert!(plan_attempts.get() > 0),
    }
    let source = source_probe.snapshot();
    assert_eq!(
        source.attempts, 1,
        "enrichment refusal must not retry a provider"
    );
    assert_eq!(
        source.created, 1,
        "the independent source copy must succeed first"
    );
    assert_eq!(source.released, 1);
    assert_eq!(source.live_charge, 0);
    let mapping = mapping_probe.snapshot();
    assert_eq!(mapping.attempts, 1);
    assert_eq!(
        mapping.created, 1,
        "mapping must succeed before enrichment refusal"
    );
    assert_eq!(mapping.released, 1);
    assert_eq!(mapping.live_charge, 0);
    assert_eq!(
        json_builds.get(),
        2,
        "only source and mapping JSON may reach construction"
    );
    assert_eq!(candidate.lock().expect("synthetic observer").created, 0);
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert!(
        audit().is_empty(),
        "a rejected plan must not claim a send action"
    );
    let after = state
        .provider_router
        .get_circuit_breaker_stats(&provider.id, "codex")
        .await
        .ok_or("missing synthetic breaker")?;
    assert_eq!(
        after.state,
        crate::proxy::circuit_breaker::CircuitState::HalfOpen
    );
    assert_eq!(after.total_requests, before.total_requests);
    assert_eq!(after.failed_requests, before.failed_requests);
    assert_eq!(after.consecutive_failures, before.consecutive_failures);
    assert_eq!(after.consecutive_successes, before.consecutive_successes);
    assert_eq!(
        serde_json::to_value(state.db.get_provider_health(&provider.id, "codex").await?)?,
        health
    );
    let permit = state
        .provider_router
        .allow_provider_request(&provider.id, "codex")
        .await;
    assert!(permit.allowed && permit.used_half_open_permit);
    state
        .provider_router
        .release_permit_neutral(&provider.id, "codex", true)
        .await;
    assert!(state
        .provider_router
        .get_circuit_breaker_stats(&fallback.id, "codex")
        .await
        .is_none());
    let response = error.error.into_response();
    assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
    let bytes = response.into_body().collect().await?.to_bytes();
    let value: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(value["error"]["code"], expected.code());
    for private in [
        FIXTURE_PHONE,
        "synthetic-enrichment-provider",
        "synthetic-enrichment-credential",
        "call_enrichment_refusal",
        "synthetic-model",
    ] {
        assert!(!std::str::from_utf8(&bytes)?.contains(private));
    }
    drop(forwarder);
    drop(audit);
    drop(gateway);
    if let Some(fill) = fill {
        assert_eq!(budget.used(), fill.bytes());
        drop(fill);
    }
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[derive(Clone, Copy)]
enum SourceBoundary {
    SourceCapacity,
    MappingCapacity,
    ModelGrowth,
    PendingMapped,
    Pending,
}

async fn source_copy_forwarding_fixture(boundary: SourceBoundary) -> TestResult {
    let capacity_denied = matches!(
        boundary,
        SourceBoundary::SourceCapacity
            | SourceBoundary::MappingCapacity
            | SourceBoundary::ModelGrowth
    );
    let mapping_denied = matches!(
        boundary,
        SourceBoundary::MappingCapacity | SourceBoundary::ModelGrowth
    );
    use super::{
        http_restoration::{admit_http_request, source_copy_test_support, RestorationAdmission},
        payload_budget::PayloadBudget,
        payload_json::PayloadJson,
        tokenization::DlpTokenStore,
    };
    use crate::{app_config::AppType, proxy::forwarder::RequestForwarder};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let sends = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let fixture_sends = sends.clone();
    let fixture_entered = entered.clone();
    let upstream = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let sends = fixture_sends.clone();
        let entered = fixture_entered.clone();
        async move {
            sends.fetch_add(1, Ordering::SeqCst);
            assert!(body["input"]
                .as_str()
                .is_some_and(|value| value.contains("rredact-cn_phone-")));
            if matches!(boundary, SourceBoundary::PendingMapped) {
                assert_eq!(body["model"].as_str().map(str::len), Some(16 * 1024));
            }
            entered.notify_one();
            std::future::pending::<Json<Value>>().await
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let mut provider = Provider::with_id(
        "synthetic-source-provider".into(),
        "Synthetic source provider".into(),
        json!({"base_url":format!("http://{address}"), "api_key":"synthetic-source-credential"}),
        None,
    );
    if mapping_denied {
        // A later-stage sentinel: missing mapping admission must reach auth failure,
        // not accidentally pass on a capacity refusal from request preparation.
        provider.settings_config["api_key"] = json!("synthetic-invalid\ncredential");
    }
    if matches!(
        boundary,
        SourceBoundary::ModelGrowth | SourceBoundary::PendingMapped
    ) {
        provider.settings_config["env"] = json!({"ANTHROPIC_MODEL":"m".repeat(16 * 1024)});
    }
    db.save_provider("codex", &provider)?;
    let fallback = Provider::with_id(
        "synthetic-source-fallback".into(),
        "Synthetic source fallback".into(),
        provider.settings_config.clone(),
        None,
    );
    db.save_provider("codex", &fallback)?;
    let state = state(db);
    let health_before = if capacity_denied {
        let _ = state
            .provider_router
            .allow_provider_request(&provider.id, "codex")
            .await;
        state
            .provider_router
            .update_app_configs(
                "codex",
                crate::proxy::circuit_breaker::CircuitBreakerConfig {
                    failure_threshold: 1,
                    timeout_seconds: 0,
                    ..Default::default()
                },
            )
            .await;
        state
            .provider_router
            .record_result(&provider.id, "codex", false, false, None)
            .await?;
        let probe = state
            .provider_router
            .allow_provider_request(&provider.id, "codex")
            .await;
        assert!(probe.allowed && probe.used_half_open_permit);
        state
            .provider_router
            .release_permit_neutral(&provider.id, "codex", true)
            .await;
        Some((
            state
                .provider_router
                .get_circuit_breaker_stats(&provider.id, "codex")
                .await
                .ok_or("missing synthetic breaker")?,
            serde_json::to_value(state.db.get_provider_health(&provider.id, "codex").await?)?,
        ))
    } else {
        None
    };
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let gateway = RestorationGateway::with_test_store_and_rules(
        DlpTokenStore::new(),
        &["chinese_phone_number"],
        budget.clone(),
    );
    let mut body =
        json!({"model":"synthetic-model","stream":false,"input":format!("Call {FIXTURE_PHONE}")});
    let request = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut body,
        &Default::default(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic source fixture not admitted".into()),
    };
    let observe_audit = request.audit_observer_for_test();
    assert!(observe_audit().is_empty());
    let forwarder = RequestForwarder::new(
        state.provider_router.clone(),
        5,
        state.status.clone(),
        state.current_providers.clone(),
        state.gemini_shadow.clone(),
        state.codex_chat_history.clone(),
        state.failover_manager.clone(),
        None,
        provider.id.clone(),
        "synthetic-source-session".into(),
        false,
        5,
        5,
        Default::default(),
        Default::default(),
        Default::default(),
        3,
    )
    .with_dlp_request(Some(request));
    let baseline = budget.used();
    let source_charge = {
        let before = budget.used();
        let copy = PayloadJson::try_from_value(&body, budget.clone(), budget.limit())
            .map_err(|error| std::io::Error::other(error.code()))?;
        let charge = budget.used() - before;
        drop(copy);
        charge
    };
    let remaining = match boundary {
        SourceBoundary::SourceCapacity => 0,
        SourceBoundary::MappingCapacity => source_charge,
        SourceBoundary::ModelGrowth => 2 * source_charge,
        SourceBoundary::Pending | SourceBoundary::PendingMapped => budget.limit() - baseline,
    };
    let fill = budget
        .reserve(budget.limit() - baseline - remaining)
        .map_err(|error| std::io::Error::other(error.code()))?;
    let source_probe = source_copy_test_support::install();
    let mapping_probe = super::http_restoration::mapping_copy_test_support::install();
    let json_builds = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let builds = json_builds.clone();
    let _json_hook = super::payload_json::test_support::install(move |event| {
        if event.phase == super::payload_json::test_support::Phase::BeforeBuild {
            builds.set(builds.get() + 1);
            assert!(event.charged > 0);
        }
        super::payload_json::test_support::Action::Continue
    });
    let app_type = AppType::Codex;
    let mut forwarding = Box::pin(forwarder.forward_with_retry(
        &app_type,
        http::Method::POST,
        "/v1/responses",
        body,
        Default::default(),
        Default::default(),
        vec![provider.clone(), fallback.clone()],
    ));
    if capacity_denied {
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(2), &mut forwarding).await?;
        let error = match result {
            Err(error) => error,
            Ok(_) => return Err("source capacity refusal sent upstream".into()),
        };
        assert!(
            matches!(
                &error.error,
                crate::proxy::error::ProxyError::DlpForward(
                    super::http_restoration::DlpForwardError::CapacityExceeded
                )
            ),
            "capacity must refuse at the selected boundary before the auth sentinel: {}",
            error.error
        );
        assert!(
            error.provider.is_none(),
            "capacity refusal must not enrich provider identity"
        );
        let source = source_probe.snapshot();
        assert_eq!(
            source.attempts, 1,
            "source refusal must occur at the source-copy boundary exactly once"
        );
        assert_eq!(source.created, usize::from(mapping_denied));
        assert_eq!(source.live_charge, 0);
        let mapping = mapping_probe.snapshot();
        assert_eq!(mapping.attempts, usize::from(mapping_denied));
        assert_eq!(
            mapping.created, 0,
            "refusal must precede mapping construction"
        );
        assert_eq!(mapping.live_charge, 0);
        assert_eq!(
            json_builds.get(),
            usize::from(mapping_denied),
            "only the source may reach JSON construction"
        );
        assert!(
            observe_audit().is_empty(),
            "source refusal must not claim a send action"
        );
        let status = state.status.read().await;
        assert_eq!(status.total_requests, 1);
        assert_eq!(status.failed_requests, 1);
        assert_eq!(status.success_requests, 0);
        assert_eq!(status.success_rate, 0.0);
        drop(status);
        let (breaker_before, health_before) =
            health_before.as_ref().ok_or("missing synthetic health")?;
        let breaker_after = state
            .provider_router
            .get_circuit_breaker_stats(&provider.id, "codex")
            .await
            .ok_or("missing synthetic breaker")?;
        assert_eq!(
            breaker_after.state,
            crate::proxy::circuit_breaker::CircuitState::HalfOpen
        );
        assert_eq!(breaker_after.total_requests, breaker_before.total_requests);
        assert_eq!(
            breaker_after.failed_requests,
            breaker_before.failed_requests
        );
        assert_eq!(
            breaker_after.consecutive_failures,
            breaker_before.consecutive_failures
        );
        assert_eq!(
            breaker_after.consecutive_successes,
            breaker_before.consecutive_successes
        );
        assert_eq!(
            &serde_json::to_value(state.db.get_provider_health(&provider.id, "codex").await?)?,
            health_before
        );
        let permit = state
            .provider_router
            .allow_provider_request(&provider.id, "codex")
            .await;
        assert!(
            permit.allowed && permit.used_half_open_permit,
            "source refusal must release the half-open permit neutrally"
        );
        state
            .provider_router
            .release_permit_neutral(&provider.id, "codex", true)
            .await;
        assert!(
            state
                .provider_router
                .get_circuit_breaker_stats(&fallback.id, "codex")
                .await
                .is_none(),
            "source failure must not retry the fallback provider"
        );
        let response = error.error.into_response();
        assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
        let bytes = response.into_body().collect().await?.to_bytes();
        let value: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(value["error"]["code"], "dlp_capacity_exceeded");
        let text = std::str::from_utf8(&bytes)?;
        for private in [
            FIXTURE_PHONE,
            "synthetic-source-provider",
            "synthetic-source-credential",
            "synthetic-model",
        ] {
            assert!(!text.contains(private));
        }
        assert_eq!(sends.load(Ordering::SeqCst), 0);
    } else {
        let reached = tokio::select! {
            _ = entered.notified() => true,
            _ = &mut forwarding => false,
            _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => false,
        };
        assert!(reached, "synthetic source send must remain pending");
        assert_eq!(sends.load(Ordering::SeqCst), 1);
        let source = source_probe.snapshot();
        assert_eq!(source.attempts, 1);
        assert_eq!(
            source.created, 1,
            "pending send must retain an actual source owner"
        );
        assert_eq!(source.released, 0);
        assert_eq!(
            source.live_charge, source_charge,
            "wire/preparation charges cannot substitute for the source charge"
        );
        let mapping = mapping_probe.snapshot();
        assert_eq!(mapping.attempts, 1);
        assert_eq!(mapping.created, 1);
        assert_eq!(mapping.released, 0);
        let expected_mapping_charge = if matches!(boundary, SourceBoundary::PendingMapped) {
            source_charge + 16 * 1024 - "synthetic-model".len()
        } else {
            source_charge
        };
        assert_eq!(
            mapping.live_charge, expected_mapping_charge,
            "the final mapping tree needs its own charge, including the expanded model"
        );
        assert_eq!(observe_audit().len(), 1);
        assert!(
            budget.used() >= baseline + source_charge + expected_mapping_charge,
            "pending send must retain independent source and mapping copy charges"
        );
    }
    drop(forwarding);
    drop(fill);
    let source = source_probe.snapshot();
    assert_eq!(
        source.live_charge, 0,
        "cancel must release the source-specific owner"
    );
    assert_eq!(
        source.released,
        usize::from(!capacity_denied || mapping_denied)
    );
    let mapping = mapping_probe.snapshot();
    assert_eq!(mapping.live_charge, 0);
    assert_eq!(mapping.released, usize::from(!capacity_denied));
    // A successful actual send retains one four-slot audit dedup table in the
    // still-live request, not an attempt/source owner (two fingerprints per key).
    let audit_charge = if capacity_denied {
        0
    } else {
        4 * 2 * std::mem::size_of::<super::identity::ContentFingerprint>()
    };
    assert_eq!(
        budget.used(),
        baseline + audit_charge,
        "only the request-level audit dedup table may remain after attempt cancellation"
    );
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while state.status.read().await.active_connections != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    drop(observe_audit);
    drop(source_probe);
    drop(forwarder);
    drop(gateway);
    assert_eq!(budget.used(), 0);
    server.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_source_copy_capacity_refuses_before_send() -> TestResult {
    source_copy_forwarding_fixture(SourceBoundary::SourceCapacity).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_source_copy_owner_releases_on_send_cancellation() -> TestResult {
    source_copy_forwarding_fixture(SourceBoundary::Pending).await
}

async fn upstream_success_error_echo(Json(body): Json<Value>) -> axum::response::Response {
    let reference = fixture_text(&body).unwrap_or("synthetic-fallback");
    Json(
        json!({"type":"error","error":{"type":"synthetic_upstream_error",
        "message":format!("synthetic-private-diagnostic {reference}")}}),
    )
    .into_response()
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_leased_2xx_error_envelopes_are_fixed_without_diagnostics() -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    for protocol in ["anthropic", "chat_completions", "responses"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let upstream = Router::new().fallback(post(upstream_success_error_echo));
        let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
        let db = Arc::new(Database::memory()?);
        let provider = Provider::with_id(
            "synthetic-error-provider".into(),
            "Synthetic error provider label".into(),
            json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-error-credential","api_format":protocol}),
            None,
        );
        db.save_provider("codex", &provider)?;
        db.set_current_provider("codex", &provider.id)?;
        let request = http::Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":false}).to_string(),
            ))?;
        let response = handlers::handle_responses(State(state(db)), request).await?;
        let status = response.status();
        let bytes = response.into_body().collect().await?.to_bytes();
        server.abort();
        let text = std::str::from_utf8(&bytes)?;
        for forbidden in [
            FIXTURE_PHONE,
            "rredact-cn_phone-",
            "synthetic-private-diagnostic",
            "synthetic-error-provider",
            "synthetic-model",
        ] {
            assert!(
                !text.contains(forbidden),
                "leased error envelope must remain fixed for {protocol}"
            );
        }
        assert_eq!(status, http::StatusCode::BAD_GATEWAY);
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes)?["error"]["code"],
            "dlp_restore_failed"
        );
    }
    Ok(())
}

async fn upstream_unlabeled_chat_sse_selected_choice() -> axum::response::Response {
    (
        [("content-type", "text/plain; note=synthetic-private-diagnostic")],
        "data: {\"id\":\"chat_fixture\",\"choices\":[{\"index\":1,\"delta\":{\"role\":\"assistant\",\"content\":\"safe\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    )
        .into_response()
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_leased_post_guard_conversion_error_is_fixed() -> TestResult {
    post_guard_conversion_error_fixture(true).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_empty_lease_post_guard_conversion_error_is_fixed() -> TestResult {
    post_guard_conversion_error_fixture(false).await
}

async fn post_guard_conversion_error_fixture(active_tokens: bool) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let upstream = Router::new().fallback(post(upstream_unlabeled_chat_sse_selected_choice));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-conversion-provider".into(),
        "Synthetic conversion provider label".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-conversion-credential","api_format":"chat_completions"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model":"synthetic-model","input":if active_tokens {FIXTURE_PHONE} else {"synthetic-safe-input"},"stream":false}).to_string(),
        ))?;
    // Exercise the router's error conversion as well as the handler result.
    let response = match handlers::handle_responses(State(state(db)), request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    let text = std::str::from_utf8(&bytes)?;
    assert!(
        !text.contains("synthetic-private-diagnostic"),
        "leased conversion errors must not expose upstream diagnostics"
    );
    assert_eq!(status, http::StatusCode::BAD_GATEWAY);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes)?["error"]["code"],
        "dlp_restore_failed"
    );
    for forbidden in [
        FIXTURE_PHONE,
        "rredact-cn_phone-",
        "synthetic-conversion-provider",
        "synthetic-model",
    ] {
        assert!(
            !text.contains(forbidden),
            "leased conversion errors must remain fixed"
        );
    }
    Ok(())
}

async fn run_unknown_chat_carrier_http(streaming: bool, upstream_sse: bool) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let upstream = Router::new().fallback(post(move |Json(body): Json<Value>| async move {
        let token = body["messages"]
            .as_array()
            .and_then(|messages| messages.last())
            .and_then(|message| message["content"].as_str())
            .unwrap_or("");
        if upstream_sse {
            let chunk = json!({"id":"chat_unknown_carrier","choices":[{"index":0,
                "delta":{"role":"assistant","future_text":token},"finish_reason":"stop"}]});
            (
                [(
                    "content-type",
                    if streaming {
                        "text/event-stream"
                    } else {
                        "text/plain"
                    },
                )],
                format!("data: {chunk}\n\ndata: [DONE]\n\n"),
            )
                .into_response()
        } else {
            Json(json!({"id":"chat_unknown_carrier","choices":[{"index":0,
                "message":{"role":"assistant","content":[{"type":"future_text","text":token}]},
                "finish_reason":"stop"}]}))
            .into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-unknown-carrier".into(),
        "Synthetic carrier fixture".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-credential","api_format":"chat_completions"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":streaming}).to_string(),
        ))?;
    let response = match handlers::handle_responses(State(state(db)), request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    let text = std::str::from_utf8(&bytes)?;
    assert!(!text.contains(FIXTURE_PHONE) && !text.contains("rredact-cn_phone-"));
    assert!(!text.contains("response.completed") && !text.contains("[DONE]"));
    if streaming {
        assert!(
            text.contains("\"code\":\"dlp_restore_unsupported\""),
            "unsupported carrier must retain its fixed classification: {text}"
        );
    } else {
        assert_eq!(status, http::StatusCode::BAD_GATEWAY);
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes)?["error"]["code"],
            "dlp_restore_unsupported"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_unknown_chat_json_carrier_is_unsupported() -> TestResult {
    run_unknown_chat_carrier_http(false, false).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_unknown_chat_buffered_sse_carrier_is_unsupported() -> TestResult {
    run_unknown_chat_carrier_http(false, true).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_unknown_chat_streaming_carrier_is_unsupported() -> TestResult {
    run_unknown_chat_carrier_http(true, true).await
}

async fn run_unknown_anthropic_sibling_http(streaming: bool, upstream_sse: bool) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let upstream = Router::new().fallback(post(move |Json(body): Json<Value>| async move {
        let token = fixture_text(&body).unwrap_or("missing synthetic carrier");
        if upstream_sse {
            let frames = [
                json!({"type":"message_start","message":{"id":"msg_unknown_sibling","type":"message","role":"assistant","content":[],"model":"synthetic-model","usage":{"input_tokens":1,"output_tokens":0}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"synthetic ordinary text","future_text":token}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}),
                json!({"type":"message_stop"}),
            ];
            let wire: String = frames.iter().map(|frame| format!("event: {}\ndata: {frame}\n\n", frame["type"].as_str().unwrap_or("unknown"))).collect();
            ([("content-type", if streaming {"text/event-stream"} else {"text/plain"})], wire).into_response()
        } else {
            Json(json!({"id":"msg_unknown_sibling","type":"message","role":"assistant","model":"synthetic-model",
                "content":[{"type":"text","text":"synthetic ordinary text","future_text":token}],
                "stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}})).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-anthropic-sibling".into(),
        "Synthetic sibling fixture".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-credential","api_format":"anthropic"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":streaming}).to_string(),
        ))?;
    let response = match handlers::handle_responses(State(state(db)), request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    let text = std::str::from_utf8(&bytes)?;
    assert!(!text.contains(FIXTURE_PHONE) && !text.contains("rredact-cn_phone-"));
    if streaming {
        assert!(
            !text.contains("response.completed") && !text.contains("[DONE]"),
            "unknown original carrier cannot become apparently complete"
        );
        assert!(
            text.contains("\"code\":\"dlp_restore_unsupported\""),
            "unknown original carrier needs fixed unsupported classification"
        );
    } else {
        assert_eq!(
            status,
            http::StatusCode::BAD_GATEWAY,
            "unknown original carrier must not be dropped by conversion"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes)?["error"]["code"],
            "dlp_restore_unsupported"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_unknown_anthropic_json_sibling_is_unsupported() -> TestResult {
    run_unknown_anthropic_sibling_http(false, false).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_unknown_anthropic_buffered_sse_sibling_is_unsupported() -> TestResult {
    run_unknown_anthropic_sibling_http(false, true).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_unknown_anthropic_streaming_sibling_is_unsupported() -> TestResult {
    run_unknown_anthropic_sibling_http(true, true).await
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_model_mapping_capacity_refuses_before_auth_or_send() -> TestResult {
    source_copy_forwarding_fixture(SourceBoundary::MappingCapacity).await
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_model_mapping_final_growth_refuses_before_auth_or_send() -> TestResult {
    source_copy_forwarding_fixture(SourceBoundary::ModelGrowth).await
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_model_mapping_final_tree_retained_until_send_cancellation() -> TestResult {
    source_copy_forwarding_fixture(SourceBoundary::PendingMapped).await
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_transform_input_retention_refuses_competing_forward_before_auth() -> TestResult {
    outbound_retention_fixture(false).await
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_outbound_tail_retention_refuses_competing_forward_before_auth() -> TestResult {
    outbound_retention_fixture(true).await
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_prompt_cache_explicit_retention_refuses_competing_forward_before_auth(
) -> TestResult {
    outbound_retention_fixture_with_prompt(false, Some(true)).await
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_prompt_cache_session_retention_refuses_competing_forward_before_auth() -> TestResult
{
    outbound_retention_fixture_with_prompt(false, Some(false)).await
}

async fn outbound_retention_fixture(tail: bool) -> TestResult {
    outbound_retention_fixture_with_prompt(tail, None).await
}

async fn outbound_retention_fixture_with_prompt(tail: bool, prompt: Option<bool>) -> TestResult {
    use super::{
        http_restoration::{
            admit_http_request, transform_input_test_support, DlpForwardError, RestorationAdmission,
        },
        payload_budget::PayloadBudget,
        payload_json::PayloadJson,
        tokenization::DlpTokenStore,
    };
    use crate::{
        app_config::AppType,
        proxy::{error::ProxyError, forwarder::RequestForwarder},
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let sends = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let server_sends = sends.clone();
    let server_entered = entered.clone();
    let upstream = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let sends = server_sends.clone();
        let entered = server_entered.clone();
        async move {
            if prompt.is_some() {
                assert_eq!(
                    body["metadata"]["padding"].as_str().map(str::len),
                    Some(32 * 1024)
                );
                assert_eq!(
                    body["prompt_cache_key"],
                    format!("synthetic-cache-{}", "k".repeat(4096))
                );
            } else {
                assert_eq!(body["padding"].as_str().map(str::len), Some(32 * 1024));
            }
            if tail {
                assert_eq!(body["tail_extra"].as_str().map(str::len), Some(16 * 1024));
                assert_eq!(
                    body["stream"], false,
                    "top-level stream override is protected"
                );
            }
            let text = if prompt.is_some() {
                body["messages"][0]["content"]
                    .as_str()
                    .expect("synthetic Chat input")
            } else {
                body["input"].as_str().expect("synthetic input")
            };
            assert!(text.contains("rredact-cn_phone-") && !text.contains(FIXTURE_PHONE));
            sends.fetch_add(1, Ordering::SeqCst);
            entered.notify_one();
            std::future::pending::<Json<Value>>().await
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let mut provider = Provider::with_id(
        "transform-first".into(),
        "Synthetic first".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-transform-key"}),
        None,
    );
    if tail {
        provider.meta = Some(crate::provider::ProviderMeta {
            local_proxy_request_overrides: Some(crate::provider::LocalProxyRequestOverrides {
                body: Some(json!({"tail_extra":"q".repeat(16 * 1024), "stream":true})),
                ..Default::default()
            }),
            ..Default::default()
        });
    }
    if prompt.is_some() {
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".into()),
            prompt_cache_routing: Some("enabled".into()),
            ..Default::default()
        });
    }
    let mut denied = provider.clone();
    denied.id = "transform-second".into();
    denied.settings_config["api_key"] = json!("synthetic-invalid\ncredential");
    let mut fallback = denied.clone();
    fallback.id = "transform-fallback".into();
    let db = Arc::new(Database::memory()?);
    for entry in [&provider, &denied, &fallback] {
        db.save_provider("codex", entry)?;
    }
    let state = state(db);
    // Exercise the real half-open permit path for the competing local refusal.
    let _ = state
        .provider_router
        .allow_provider_request(&denied.id, "codex")
        .await;
    state
        .provider_router
        .update_app_configs(
            "codex",
            crate::proxy::circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                timeout_seconds: 0,
                ..Default::default()
            },
        )
        .await;
    state
        .provider_router
        .record_result(&denied.id, "codex", false, false, None)
        .await?;
    let permit = state
        .provider_router
        .allow_provider_request(&denied.id, "codex")
        .await;
    assert!(permit.allowed && permit.used_half_open_permit);
    state
        .provider_router
        .release_permit_neutral(&denied.id, "codex", true)
        .await;
    let health_before =
        serde_json::to_value(state.db.get_provider_health(&denied.id, "codex").await?)?;
    let breaker_before = state
        .provider_router
        .get_circuit_breaker_stats(&denied.id, "codex")
        .await
        .ok_or("missing breaker")?;
    let budget = PayloadBudget::with_limit(8 * 1024 * 1024);
    let gateway = RestorationGateway::with_test_store_and_rules(
        DlpTokenStore::new(),
        &["chinese_phone_number"],
        budget.clone(),
    );
    let admit = |body: &mut Value| -> Result<_, Box<dyn std::error::Error>> {
        match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            body,
            &Default::default(),
        )? {
            RestorationAdmission::Admitted(request) => Ok(request),
            _ => Err("synthetic transform admission failed".into()),
        }
    };
    let mut first = json!({"model":"synthetic-model", "input":format!("Call {FIXTURE_PHONE}"), "padding":"p".repeat(32 * 1024),"stream":false});
    let cache_key = format!(" \u{2003}synthetic-cache-{}\t ", "k".repeat(4096));
    if let Some(explicit) = prompt {
        let padding = first
            .as_object_mut()
            .expect("synthetic object")
            .remove("padding")
            .expect("padding");
        first["metadata"] = json!({"padding":padding});
        if explicit {
            first["prompt_cache_key"] = json!(cache_key);
        }
    }
    let first_request = admit(&mut first)?;
    let mut second =
        json!({"model":"synthetic-model", "input":format!("Call {FIXTURE_PHONE}"), "stream":false});
    let second_request = admit(&mut second)?;
    let observe_first = first_request.audit_observer_for_test();
    let observe_second = second_request.audit_observer_for_test();
    let baseline = budget.used();
    let charge = |value: &Value| -> Result<usize, Box<dyn std::error::Error>> {
        let before = budget.used();
        let copy = PayloadJson::try_from_value(value, budget.clone(), budget.limit())
            .map_err(|error| std::io::Error::other(error.code()))?;
        let bytes = budget.used() - before;
        drop(copy);
        Ok(bytes)
    };
    let first_charge = charge(&first)?;
    let second_charge = charge(&second)?;
    let mut expected_output = first.clone();
    if tail {
        expected_output["tail_extra"] = json!("q".repeat(16 * 1024));
    }
    if prompt.is_some() {
        expected_output =
            crate::proxy::providers::transform_codex_chat::responses_to_chat_completions(
                expected_output,
            )?;
        expected_output["prompt_cache_key"] = json!(cache_key.trim());
    }
    let output_charge = charge(&expected_output)?;
    assert!(first_charge + second_charge / 2 >= 2 * second_charge);
    let make = |request, id: String| {
        RequestForwarder::new(
            state.provider_router.clone(),
            5,
            state.status.clone(),
            state.current_providers.clone(),
            state.gemini_shadow.clone(),
            state.codex_chat_history.clone(),
            state.failover_manager.clone(),
            None,
            id,
            if prompt == Some(false) {
                cache_key.clone()
            } else {
                "synthetic-transform-session".into()
            },
            prompt == Some(false),
            5,
            5,
            Default::default(),
            Default::default(),
            Default::default(),
            3,
        )
        .with_dlp_request(Some(request))
    };
    let first_forwarder = make(first_request, provider.id.clone());
    let second_forwarder = make(second_request, denied.id.clone());
    let (_hook, probe) = transform_input_test_support::install();
    let app_type = AppType::Codex;
    let mut pending = Box::pin(first_forwarder.forward_with_retry(
        &app_type,
        http::Method::POST,
        "/v1/responses",
        first,
        Default::default(),
        Default::default(),
        vec![provider],
    ));
    tokio::select! {
        _ = probe.reached.notified() => {},
        result = &mut pending => panic!("first forward ended before transform barrier: {}", result.err().map_or("success".into(), |error| error.error.to_string())),
        _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => panic!("transform barrier timeout"),
    }
    // Both fixtures now retain source, mapping, moved prepared input, and the
    // independently rebuilt tail. Keep the input test's distinguishing charge
    // separate from the new tail's fixed baseline; never infer it from owners.
    let before_competing = budget.used();
    if prompt.is_none() {
        assert_eq!(
            before_competing,
            baseline + 3 * first_charge + output_charge
        );
    }
    let retained_input_trees = if tail || prompt.is_some() { 3 } else { 2 };
    let fixed_tail_charge = if tail { 0 } else { output_charge };
    let distinguishing_charge = if tail || prompt.is_some() {
        output_charge
    } else {
        first_charge
    };
    let filler = budget
        .reserve(
            budget.limit()
                - baseline
                - retained_input_trees * first_charge
                - fixed_tail_charge
                - (distinguishing_charge + second_charge / 2),
        )
        .map_err(|error| std::io::Error::other(error.code()))?;
    let result = second_forwarder
        .forward_with_retry(
            &app_type,
            http::Method::POST,
            "/v1/responses",
            second,
            Default::default(),
            Default::default(),
            vec![denied.clone(), fallback.clone()],
        )
        .await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("competing request unexpectedly sent"),
    };
    assert!(
        matches!(
            error.error,
            ProxyError::DlpForward(DlpForwardError::CapacityExceeded)
        ),
        "retained independent tree must refuse competing source allocation before auth sentinel (tail={tail}, prompt={prompt:?}): {}",
        error.error
    );
    if prompt.is_some() {
        assert_eq!(
            before_competing,
            baseline + 3 * first_charge + 2 * output_charge,
            "independent prompt candidate must retain its own complete-tree reservation"
        );
    }
    assert!(error.provider.is_none());
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert!(observe_first().is_empty() && observe_second().is_empty());
    let response = error.error.into_response();
    assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
    let value: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert_eq!(value["error"]["code"], "dlp_capacity_exceeded");
    let breaker_after = state
        .provider_router
        .get_circuit_breaker_stats(&denied.id, "codex")
        .await
        .ok_or("missing breaker")?;
    assert_eq!(
        breaker_after.state,
        crate::proxy::circuit_breaker::CircuitState::HalfOpen
    );
    assert_eq!(breaker_after.total_requests, breaker_before.total_requests);
    assert_eq!(
        breaker_after.failed_requests,
        breaker_before.failed_requests
    );
    assert_eq!(
        serde_json::to_value(state.db.get_provider_health(&denied.id, "codex").await?)?,
        health_before
    );
    let permit = state
        .provider_router
        .allow_provider_request(&denied.id, "codex")
        .await;
    assert!(permit.allowed && permit.used_half_open_permit);
    state
        .provider_router
        .release_permit_neutral(&denied.id, "codex", true)
        .await;
    assert!(state
        .provider_router
        .get_circuit_breaker_stats(&fallback.id, "codex")
        .await
        .is_none());
    {
        let observed = probe.observation.lock().expect("synthetic probe");
        assert_eq!(observed.prompt_candidates, usize::from(prompt.is_some()));
        if prompt == Some(true) {
            assert_ne!(observed.prepared_key_pointer, 0);
            assert_eq!(
                observed.prepared_key_pointer, observed.moved_key_pointer,
                "explicit key must move the SAME allocation"
            );
        }
        assert_ne!(observed.prepared_pointer, 0);
        assert_eq!(
            observed.prepared_pointer, observed.input_pointer,
            "owned production adapter must not clone"
        );
    }
    drop(filler);
    probe.resume.notify_one();
    tokio::select! {
        _ = entered.notified() => {},
        _ = &mut pending => panic!("first forward did not remain pending"),
        _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => panic!("loopback send timeout"),
    }
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(observe_first().len(), 1);
    assert!(budget.used() >= baseline + 3 * first_charge);
    drop(pending);
    drop(first_forwarder);
    drop(second_forwarder);
    drop(observe_first);
    drop(observe_second);
    assert_eq!(
        budget.used(),
        0,
        "cancel must release both requests and the prepared input"
    );
    server.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_prompt_cache_legacy_redact_has_no_mapping_buffer_or_candidate() -> TestResult {
    run_wire_policy_with_prompt(
        super::AuditAction::Redact,
        true,
        false,
        false,
        "chat",
        Some("explicit"),
    )
    .await
}
#[tokio::test]
#[serial_test::serial]
async fn guarded_prompt_cache_disabled_dlp_has_no_mapping_buffer_or_candidate() -> TestResult {
    run_wire_policy_with_prompt(
        super::AuditAction::RedactRestore,
        false,
        false,
        false,
        "chat",
        Some("explicit"),
    )
    .await
}
#[tokio::test]
#[serial_test::serial]
async fn guarded_prompt_cache_disabled_routing_has_no_candidate() -> TestResult {
    run_wire_policy_with_prompt(
        super::AuditAction::RedactRestore,
        true,
        false,
        false,
        "chat",
        Some("disabled"),
    )
    .await
}
#[tokio::test]
#[serial_test::serial]
async fn guarded_prompt_cache_generated_session_has_no_candidate() -> TestResult {
    run_wire_policy_with_prompt(
        super::AuditAction::RedactRestore,
        true,
        false,
        false,
        "chat",
        Some("no-key"),
    )
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_namespace_capacity_refuses_actual_forward_before_send() -> TestResult {
    use super::{
        http_restoration::{admit_http_request, DlpForwardError, RestorationAdmission},
        payload_budget::PayloadBudget,
        payload_json::PayloadJson,
        tokenization::DlpTokenStore,
    };
    use crate::{
        app_config::AppType,
        proxy::{error::ProxyError, forwarder::RequestForwarder},
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let sends = Arc::new(AtomicUsize::new(0));
    let captured = sends.clone();
    let upstream = Router::new().fallback(post(move |uri: http::Uri, Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            assert!(uri.path().contains("api.x.ai"));
            assert_eq!(body["tools"][0]["name"], "synthetic_ns__tool_0");
            assert!(body["input"].as_str().is_some_and(
                |text| text.contains("rredact-cn_phone-") && !text.contains(FIXTURE_PHONE)
            ));
            captured.fetch_add(1, Ordering::SeqCst);
            Json(
                json!({"id":"resp_namespace","object":"response","status":"completed","output":[]}),
            )
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    assert!(address.ip().is_loopback());
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let url = format!("http://{address}/api.x.ai/v1");
    let provider = Provider::with_id(
        "synthetic-namespace".into(),
        "Synthetic namespace".into(),
        json!({
            "base_url":url,"api_key":"synthetic-namespace-key", "api_format":"responses",
            "config":format!("model_provider = \"synthetic\"\n[model_providers.synthetic]\nwire_api = \"responses\"\nbase_url = \"{url}\"\n")
        }),
        None,
    );
    assert!(crate::proxy::providers::provider_needs_responses_namespace_flatten(&provider));
    let mut fallback = provider.clone();
    fallback.id = "synthetic-namespace-fallback".into();
    let db = Arc::new(Database::memory()?);
    for entry in [&provider, &fallback] {
        db.save_provider("codex", entry)?;
    }
    let state = state(db);
    let _ = state
        .provider_router
        .allow_provider_request(&provider.id, "codex")
        .await;
    state
        .provider_router
        .update_app_configs(
            "codex",
            crate::proxy::circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                timeout_seconds: 0,
                ..Default::default()
            },
        )
        .await;
    state
        .provider_router
        .record_result(&provider.id, "codex", false, false, None)
        .await?;
    let permit = state
        .provider_router
        .allow_provider_request(&provider.id, "codex")
        .await;
    assert!(permit.allowed && permit.used_half_open_permit);
    state
        .provider_router
        .release_permit_neutral(&provider.id, "codex", true)
        .await;
    let health_before =
        serde_json::to_value(state.db.get_provider_health(&provider.id, "codex").await?)?;
    let breaker_before = state
        .provider_router
        .get_circuit_breaker_stats(&provider.id, "codex")
        .await
        .ok_or("missing breaker")?;
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let gateway = RestorationGateway::with_test_store_and_rules(
        DlpTokenStore::new(),
        &["chinese_phone_number"],
        budget.clone(),
    );
    let tools: Vec<Value> = (0..48).map(|index| json!({"type":"function","name":format!("tool_{index}"),"description":"s".repeat(1024),"parameters":{"type":"object","properties":{"synthetic":{"type":"string"}}}})).collect();
    let mut body = json!({"model":"synthetic-model","stream":false,"input":format!("Call {FIXTURE_PHONE}"),"tools":[{"type":"namespace","name":"synthetic_ns","tools":tools}]});
    let request = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut body,
        &Default::default(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("namespace fixture not admitted".into()),
    };
    let audit = request.audit_observer_for_test();
    let baseline = budget.used();
    let charge = |value: &Value| -> Result<usize, Box<dyn std::error::Error>> {
        let before = budget.used();
        let copy = PayloadJson::try_from_value(value, budget.clone(), budget.limit())
            .map_err(|error| std::io::Error::other(error.code()))?;
        let bytes = budget.used() - before;
        drop(copy);
        Ok(bytes)
    };
    let source_charge = charge(&body)?;
    let mut expected = body.clone();
    crate::proxy::providers::transform_codex_responses_namespace::flatten_request_namespaces(
        &mut expected,
    )?;
    let output_charge = charge(&expected)?;
    // Three source trees survive forwarding. The legacy tail and final validation
    // retain three output trees; leave half a candidate of fixture headroom,
    // deliberately less than the additional independent namespace candidate.
    let available = 3 * source_charge + 3 * output_charge + output_charge / 2;
    let filler = budget
        .reserve(budget.limit() - baseline - available)
        .map_err(|error| std::io::Error::other(error.code()))?;
    let forwarder = RequestForwarder::new(
        state.provider_router.clone(),
        5,
        state.status.clone(),
        state.current_providers.clone(),
        state.gemini_shadow.clone(),
        state.codex_chat_history.clone(),
        state.failover_manager.clone(),
        None,
        provider.id.clone(),
        "synthetic-namespace-session".into(),
        false,
        5,
        5,
        Default::default(),
        Default::default(),
        Default::default(),
        3,
    )
    .with_dlp_request(Some(request));
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        forwarder.forward_with_retry(
            &AppType::Codex,
            http::Method::POST,
            "/v1/responses",
            body,
            Default::default(),
            Default::default(),
            vec![provider.clone(), fallback.clone()],
        ),
    )
    .await?;
    eprintln!("namespace fixture result={}, sends={}, audit={}, used={}, source={}, output={}, baseline={}",result.as_ref().err().map_or("sent successfully".into(),|error|error.error.to_string()),sends.load(Ordering::SeqCst),audit().len(),budget.used(),source_charge,output_charge,baseline);
    assert!(
        matches!(&result,Err(error) if matches!(error.error,ProxyError::DlpForward(DlpForwardError::CapacityExceeded))),
        "namespace candidate overlap must refuse actual forwarding; observed sends={}, result={}",
        sends.load(Ordering::SeqCst),
        result
            .as_ref()
            .err()
            .map_or("sent successfully".into(), |error| error.error.to_string())
    );
    let error = result.err().ok_or("expected capacity error")?;
    assert!(error.provider.is_none());
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert!(
        audit()
            .iter()
            .all(|event| event.log.action_taken == "block"),
        "refusal observations must not be confused with send audits"
    );
    assert!(state
        .provider_router
        .get_circuit_breaker_stats(&fallback.id, "codex")
        .await
        .is_none());
    let after = state
        .provider_router
        .get_circuit_breaker_stats(&provider.id, "codex")
        .await
        .ok_or("missing breaker")?;
    assert_eq!(after.failed_requests, breaker_before.failed_requests);
    assert_eq!(after.total_requests, breaker_before.total_requests);
    assert_eq!(
        serde_json::to_value(state.db.get_provider_health(&provider.id, "codex").await?)?,
        health_before
    );
    let permit = state
        .provider_router
        .allow_provider_request(&provider.id, "codex")
        .await;
    assert!(permit.allowed && permit.used_half_open_permit);
    state
        .provider_router
        .release_permit_neutral(&provider.id, "codex", true)
        .await;
    let response = error.error.into_response();
    assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
    let value: Value = serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
    assert_eq!(value["error"]["code"], "dlp_capacity_exceeded");
    drop(forwarder);
    drop(audit);
    drop(filler);
    server.abort();
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_namespace_candidate_survives_actual_pending_send_and_cancel() -> TestResult {
    namespace_wire_lifecycle(true, true).await
}
#[tokio::test]
#[serial_test::serial]
async fn guarded_namespace_ordinary_legacy_has_no_candidate() -> TestResult {
    namespace_wire_lifecycle(false, true).await
}
#[tokio::test]
#[serial_test::serial]
async fn guarded_namespace_absent_tools_has_no_candidate() -> TestResult {
    namespace_wire_lifecycle(true, false).await
}
async fn namespace_wire_lifecycle(participating: bool, namespace: bool) -> TestResult {
    use super::{
        http_restoration::{
            admit_http_request, transform_input_test_support, RestorationAdmission,
        },
        payload_budget::PayloadBudget,
        payload_json::PayloadJson,
        tokenization::DlpTokenStore,
    };
    use crate::{app_config::AppType, proxy::forwarder::RequestForwarder};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let sends = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let captured = sends.clone();
    let notify = entered.clone();
    let upstream = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let captured = captured.clone();
        let notify = notify.clone();
        async move {
            if namespace {
                assert_eq!(body["tools"][0]["name"], "synthetic__work");
            } else {
                assert!(body.get("tools").is_none());
            }
            let input = body["input"].as_str().expect("synthetic input");
            assert!(!input.contains(FIXTURE_PHONE));
            assert!(input.contains(if participating {
                "rredact-cn_phone-"
            } else {
                "[REDACTED]"
            }));
            captured.fetch_add(1, Ordering::SeqCst);
            notify.notify_one();
            std::future::pending::<Json<Value>>().await
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let url = format!("http://{address}/api.x.ai/v1");
    let provider = Provider::with_id(
        "namespace-pending".into(),
        "Synthetic pending".into(),
        json!({"base_url":url,"api_key":"synthetic-key","config":format!("model_provider = \"synthetic\"\n[model_providers.synthetic]\nwire_api = \"responses\"\nbase_url = \"{url}\"\n")}),
        None,
    );
    assert!(crate::proxy::providers::provider_needs_responses_namespace_flatten(&provider));
    let db = Arc::new(Database::memory()?);
    db.save_provider("codex", &provider)?;
    let state = state(db);
    let budget = PayloadBudget::with_limit(8 * 1024 * 1024);
    let gateway = RestorationGateway::with_test_store_and_rules(
        DlpTokenStore::new(),
        &["chinese_phone_number"],
        budget.clone(),
    );
    let mut body =
        json!({"model":"synthetic-model","stream":false,"input":format!("Call {FIXTURE_PHONE}")});
    if namespace {
        body["tools"] = json!([{"type":"namespace","name":"synthetic","tools":[{"type":"function","name":"work","description":"p".repeat(32*1024),"parameters":{"type":"object","properties":{}}}]}]);
    }
    let request = if participating {
        match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            &mut body,
            &Default::default(),
        )? {
            RestorationAdmission::Admitted(request) => Some(request),
            _ => return Err("not admitted".into()),
        }
    } else {
        body["input"] = json!("Call [REDACTED]");
        None
    };
    let audit = request
        .as_ref()
        .map(|request| request.audit_observer_for_test());
    let baseline = budget.used();
    let charge = |value: &Value| -> Result<usize, Box<dyn std::error::Error>> {
        let before = budget.used();
        let owner = PayloadJson::try_from_value(value, budget.clone(), budget.limit())
            .map_err(|error| std::io::Error::other(error.code()))?;
        let charge = budget.used() - before;
        drop(owner);
        Ok(charge)
    };
    let source_charge = charge(&body)?;
    let mut expected = body.clone();
    crate::proxy::providers::transform_codex_responses_namespace::flatten_request_namespaces(
        &mut expected,
    )?;
    let output_charge = charge(&expected)?;
    let forwarder = RequestForwarder::new(
        state.provider_router.clone(),
        5,
        state.status.clone(),
        state.current_providers.clone(),
        state.gemini_shadow.clone(),
        state.codex_chat_history.clone(),
        state.failover_manager.clone(),
        None,
        provider.id.clone(),
        "namespace-pending-session".into(),
        false,
        5,
        5,
        Default::default(),
        Default::default(),
        Default::default(),
        3,
    )
    .with_dlp_request(request);
    let (_hook, probe) = transform_input_test_support::install();
    let app = AppType::Codex;
    let mut pending = Box::pin(forwarder.forward_with_retry(
        &app,
        http::Method::POST,
        "/v1/responses",
        body,
        Default::default(),
        Default::default(),
        vec![provider],
    ));
    tokio::select! {_=probe.reached.notified()=>{},result=&mut pending=>panic!("before barrier: {}",result.err().map_or("success".into(),|error|error.error.to_string())),_=tokio::time::sleep(std::time::Duration::from_secs(3))=>panic!("barrier timeout")}
    let retained = if participating {
        baseline + 3 * source_charge + output_charge * (1 + usize::from(namespace))
    } else {
        0
    };
    assert_eq!(
        budget.used(),
        retained,
        "only namespace requests retain an independent complete candidate"
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    if let Some(audit) = &audit {
        assert!(audit().is_empty());
    }
    probe.resume.notify_one();
    tokio::select! {_=entered.notified()=>{},result=&mut pending=>panic!("before pending send: {}",result.err().map_or("success".into(),|error|error.error.to_string())),_=tokio::time::sleep(std::time::Duration::from_secs(3))=>panic!("send timeout")}
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    if let Some(audit) = &audit {
        let events = audit();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].log.action_taken, "redact_restore");
    }
    assert!(budget.used() >= retained, "candidate survives network wait");
    drop(pending);
    drop(forwarder);
    drop(audit);
    server.abort();
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_response_namespace_handler_producer_refuses_before_any_forwarding() -> TestResult {
    use super::{
        http_restoration::{admit_http_request, DlpForwardError, RestorationAdmission},
        payload_budget::PayloadBudget,
        tokenization::DlpTokenStore,
    };
    use crate::{
        app_config::AppType,
        proxy::{error::ProxyError, handler_context::RequestContext},
    };
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-response-table".into(),
        "Synthetic response table".into(),
        json!({"base_url":format!("http://{address}/api.x.ai/v1"),"api_key":"synthetic-key","api_format":"responses"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let state = state(db);
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let gateway = RestorationGateway::with_test_store_and_rules(
        DlpTokenStore::new(),
        &["chinese_phone_number"],
        budget.clone(),
    );
    let mut body = json!({"model":"synthetic-model","input":FIXTURE_PHONE,"tools":[
        {"type":"namespace","name":"synthetic_ns","tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}}]}
    ]});
    let request = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut body,
        &Default::default(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic request must reach the handler producer".into()),
    };
    let _ctx = RequestContext::new(
        &state,
        &body,
        &Default::default(),
        AppType::Codex,
        "Codex",
        "codex",
        Some(&request),
    )
    .await?;
    assert!(body["input"]
        .as_str()
        .is_some_and(|text| text.contains("rredact-cn_phone-")));
    let before = budget.used();
    let foreign = budget.reserve(budget.limit() - before)?;
    let result = handlers::build_codex_response_namespace_table(&body, Some(&request));
    assert!(matches!(result, Err(ProxyError::DlpForward(DlpForwardError::CapacityExceeded))),
        "the actual handler table producer returned a successful uncharged table with zero remaining budget");
    assert_eq!(
        budget.used(),
        budget.limit(),
        "a failed table must not alter other owners"
    );
    drop(result);
    drop(foreign);
    assert_eq!(budget.used(), before);
    drop(request);
    assert_eq!(budget.used(), 0);
    Ok(())
}

async fn run_guarded_response_namespace_router(mode: &str, compact: bool) -> TestResult {
    use super::{
        http_restoration::response_namespace_test_support, payload_budget::PayloadBudget,
        tokenization::DlpTokenStore,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    let mut config = runtime::SecurityRuntimeConfig::default();
    config.builtin_action_overrides.insert(
        "chinese_phone_number".into(),
        if mode == "legacy" {
            super::AuditAction::Redact
        } else {
            super::AuditAction::RedactRestore
        },
    );
    runtime::set_security_runtime_for_test(config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    assert!(address.ip().is_loopback());
    let sends = Arc::new(AtomicUsize::new(0));
    let reached = Arc::new(tokio::sync::Notify::new());
    let captured = sends.clone();
    let reached_upstream = reached.clone();
    let mode_upstream = mode.to_string();
    let upstream = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let captured = captured.clone();
        let reached = reached_upstream.clone();
        let mode = mode_upstream.clone();
        async move {
            captured.fetch_add(1, Ordering::SeqCst);
            assert_eq!(body["tools"][0]["name"], "synthetic_ns__lookup");
            reached.notify_one();
            if mode == "pending-send" { futures::future::pending::<()>().await; }
            let value = json!({"id":"resp_table","object":"response","status":"completed","model":"synthetic-model",
                "output":[{"id":"fc_table","type":"function_call","status":"completed","call_id":"call_table","name":"synthetic_ns__lookup","arguments":"{}"}]});
            if mode == "sse" {
                let wire = format!("event: response.completed\ndata: {}\n\n", json!({"type":"response.completed","response":value}));
                let stream = futures::stream::once(async { Ok::<_, std::io::Error>(bytes::Bytes::from(wire)) });
                return ([ ("content-type", "text/event-stream") ], Body::from_stream(stream)).into_response();
            }
            if mode == "pending-json" || mode == "body-error" {
                let stream = async_stream::stream! {
                    yield Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"{"));
                    reached.notify_one();
                    if mode == "body-error" { yield Err(std::io::Error::other("synthetic body error")); }
                    else { futures::future::pending::<()>().await; }
                };
                return ([("content-type", "application/json")], Body::from_stream(stream)).into_response();
            }
            Json(value).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let url = format!("http://{address}/api.x.ai/v1");
    let provider = Provider::with_id(
        "synthetic-response-table".into(),
        "Synthetic response table".into(),
        json!({"base_url":url,"api_key":"synthetic-key","api_format":"responses",
            "config":format!("model_provider = \"synthetic\"\n[model_providers.synthetic]\nwire_api = \"responses\"\nbase_url = \"{url}\"\n")}),
        None,
    );
    assert!(crate::proxy::providers::provider_needs_responses_namespace_flatten(&provider));
    let db = Arc::new(Database::memory()?);
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let foreign = budget.reserve(47)?;
    let gateway =
        RestorationGateway::with_test_store_and_rules(DlpTokenStore::new(), &[], budget.clone());
    let proxy = ProxyServer::new(ProxyConfig::default(), db.clone(), None)
        .with_restoration_gateway(gateway);
    db.update_provider_health(&provider.id, "codex", true, None)
        .await?;
    let health_before = serde_json::to_value(db.get_provider_health(&provider.id, "codex").await?)?;
    let body = json!({"model":"synthetic-model","stream":mode == "sse","input":FIXTURE_PHONE,"tools":[
        {"type":"namespace","name":"synthetic_ns","tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}}]}]});
    let request = http::Request::builder()
        .method("POST")
        .uri(if compact {
            "/v1/responses/compact"
        } else {
            "/v1/responses"
        })
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))?;
    let probe = response_namespace_test_support::install(mode == "refuse");
    let mut router = proxy.build_router();
    let mut pending = Box::pin(router.call(request));
    if mode == "pending-send" || mode == "pending-json" {
        tokio::select! {
            _ = reached.notified() => {},
            result = &mut pending => panic!("handler completed before synthetic await: {:?}", result.as_ref().map(|r| r.status())),
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => panic!("synthetic table barrier timeout"),
        }
        // Leased JSON waits in the upstream guard; the direct helper below
        // separately verifies the xAI namespace rewrite's JSON read-await.
        assert!(futures::poll!(pending.as_mut()).is_pending());
        let snapshot = probe.snapshot();
        assert_eq!(snapshot.created, 1);
        assert!(snapshot.live_charge > 0);
        assert_eq!(snapshot.released, 0);
        drop(pending);
    } else {
        let response = pending.await?;
        if mode == "refuse" {
            assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
            let value: Value =
                serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?;
            assert_eq!(value["error"]["code"], "dlp_capacity_exceeded");
            assert_eq!(sends.load(Ordering::SeqCst), 0);
            let snapshot = probe.snapshot();
            assert_eq!(snapshot.attempts, 1);
            assert_eq!(snapshot.created, 0);
            assert_eq!(
                snapshot.failure,
                Some(super::http_restoration::DlpForwardError::CapacityExceeded)
            );
            assert_eq!(
                health_before,
                serde_json::to_value(db.get_provider_health(&provider.id, "codex").await?)?
            );
            probe.release_blocker();
        } else if mode == "body-error" {
            assert!(!response.status().is_success());
            drop(response);
        } else {
            assert_eq!(response.status(), http::StatusCode::OK);
            if mode == "sse" {
                assert!(
                    probe.snapshot().live_charge > 0,
                    "unpolled response body owns table"
                );
            }
            let bytes = response.into_body().collect().await?.to_bytes();
            let text = std::str::from_utf8(&bytes)?;
            assert!(text.contains("\"namespace\":\"synthetic_ns\""));
            assert!(text.contains("\"name\":\"lookup\""));
        }
    }
    server.abort();
    let snapshot = probe.snapshot();
    if mode == "legacy" {
        assert_eq!(snapshot.attempts, 0);
        assert_eq!(snapshot.created, 0);
    } else if mode != "refuse" {
        assert_eq!(snapshot.created, 1);
        assert_eq!(snapshot.released, 1);
        assert_eq!(snapshot.live_charge, 0);
        assert_eq!(sends.load(Ordering::SeqCst), 1);
    }
    drop(router);
    drop(proxy);
    assert_eq!(
        budget.used(),
        47,
        "only the independent foreign owner remains"
    );
    drop(foreign);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_response_namespace_router_capacity_both_routes_no_send() -> TestResult {
    for compact in [false, true] {
        run_guarded_response_namespace_router("refuse", compact).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_response_namespace_actual_json_sse_legacy_and_cancel() -> TestResult {
    for mode in [
        "json",
        "sse",
        "legacy",
        "pending-send",
        "pending-json",
        "body-error",
    ] {
        run_guarded_response_namespace_router(mode, false).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_response_namespace_actual_send_error_releases_owner() -> TestResult {
    use super::{
        http_restoration::response_namespace_test_support, payload_budget::PayloadBudget,
        tokenization::DlpTokenStore,
    };
    use tokio::io::AsyncReadExt;

    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    let mut config = runtime::SecurityRuntimeConfig::default();
    config.builtin_action_overrides.insert(
        "chinese_phone_number".into(),
        super::AuditAction::RedactRestore,
    );
    runtime::set_security_runtime_for_test(config);
    for compact in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        assert!(address.ip().is_loopback());
        let reached = Arc::new(tokio::sync::Notify::new());
        let disconnect = Arc::new(tokio::sync::Notify::new());
        let accepted = reached.clone();
        let release = disconnect.clone();
        let server = tokio::spawn(async move {
            let (mut socket, peer) = listener.accept().await?;
            assert!(peer.ip().is_loopback());
            let mut bytes = [0_u8; 1024];
            let read = socket.read(&mut bytes).await?;
            assert!(
                read > 0,
                "the actual HTTP send must reach the loopback socket"
            );
            accepted.notify_one();
            release.notified().await;
            // Close before any response headers: this is a send failure, not an
            // HTTP status or response-body decoding failure.
            drop(socket);
            Ok::<_, std::io::Error>(())
        });
        let url = format!("http://{address}/api.x.ai/v1");
        let provider = Provider::with_id(
            "synthetic-table-send-error".into(),
            "Synthetic table send error".into(),
            json!({"base_url":url,"api_key":"synthetic-key","api_format":"responses",
                "config":format!("model_provider = \"synthetic\"\n[model_providers.synthetic]\nwire_api = \"responses\"\nbase_url = \"{url}\"\n")}),
            None,
        );
        assert!(crate::proxy::providers::provider_needs_responses_namespace_flatten(&provider));
        let db = Arc::new(Database::memory()?);
        db.save_provider("codex", &provider)?;
        db.set_current_provider("codex", &provider.id)?;
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        let foreign = budget.reserve(59)?;
        let gateway = RestorationGateway::with_test_store_and_rules(
            DlpTokenStore::new(),
            &[],
            budget.clone(),
        );
        let proxy =
            ProxyServer::new(ProxyConfig::default(), db, None).with_restoration_gateway(gateway);
        let body = json!({"model":"synthetic-model","input":FIXTURE_PHONE,"tools":[
            {"type":"namespace","name":"synthetic_ns","tools":[{"type":"function","name":"lookup"}]}]});
        let request = http::Request::builder()
            .method("POST")
            .uri(if compact {
                "/v1/responses/compact"
            } else {
                "/v1/responses"
            })
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))?;
        let probe = response_namespace_test_support::install(false);
        let mut router = proxy.build_router();
        let mut pending = Box::pin(router.call(request));
        tokio::select! {
            _ = reached.notified() => {},
            result = &mut pending => panic!("handler completed before actual send: {:?}", result.as_ref().map(|r| r.status())),
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => panic!("synthetic send barrier timeout"),
        }
        assert_eq!(probe.snapshot().created, 1);
        assert_eq!(probe.snapshot().released, 0);
        assert!(probe.snapshot().live_charge > 0);
        disconnect.notify_one();
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), pending).await??;
        assert!(!response.status().is_success());
        drop(response);
        server.await??;
        assert_eq!(probe.snapshot().attempts, 1);
        assert_eq!(probe.snapshot().released, 1);
        assert_eq!(probe.snapshot().live_charge, 0);
        drop(router);
        drop(proxy);
        assert_eq!(
            budget.used(),
            59,
            "send failure releases all request/table owners"
        );
        drop(foreign);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn guarded_response_namespace_json_rewrite_await_and_error_release() -> TestResult {
    use super::{
        http_restoration::{
            admit_http_request, response_namespace_test_support, RestorationAdmission,
        },
        payload_budget::PayloadBudget,
        tokenization::DlpTokenStore,
    };
    use crate::{
        app_config::AppType,
        proxy::{handler_context::RequestContext, hyper_client::ProxyResponse},
    };
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let db = Arc::new(Database::memory()?);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let provider = Provider::with_id(
        "synthetic-table-read".into(),
        "Synthetic table read".into(),
        json!({"base_url":format!("http://{}", listener.local_addr()?),"api_key":"synthetic-key","api_format":"responses"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let state = state(db);
    for mode in ["cancel", "complete", "error", "http-error"] {
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        let foreign = budget.reserve(53)?;
        let gateway = RestorationGateway::with_test_store_and_rules(
            DlpTokenStore::new(),
            &["chinese_phone_number"],
            budget.clone(),
        );
        let mut body = json!({"model":"synthetic-model","input":FIXTURE_PHONE,"tools":[
            {"type":"namespace","name":"synthetic_ns","tools":[{"type":"function","name":"lookup"}]}]});
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            &mut body,
            &Default::default(),
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic handler request not admitted".into()),
        };
        let ctx = RequestContext::new(
            &state,
            &body,
            &Default::default(),
            AppType::Codex,
            "Codex",
            "codex",
            Some(&request),
        )
        .await?;
        let probe = response_namespace_test_support::install(false);
        let table = handlers::build_codex_response_namespace_table(&body, Some(&request))?;
        let held = budget.used();
        let resume = Arc::new(tokio::sync::Notify::new());
        let ready = resume.clone();
        let stream = async_stream::stream! {
            ready.notified().await;
            if mode == "error" { yield Err(std::io::Error::other("synthetic response body error")); }
            else { yield Ok(bytes::Bytes::from_static(b"{\"id\":\"resp_json_table\",\"object\":\"response\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"name\":\"synthetic_ns__lookup\",\"arguments\":\"{}\"}]}")); }
        };
        let status = if mode == "http-error" {
            http::StatusCode::BAD_GATEWAY
        } else {
            http::StatusCode::OK
        };
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "content-type",
            http::HeaderValue::from_static("application/json"),
        );
        let response = ProxyResponse::streamed(status, headers, stream);
        let mut pending = Box::pin(handlers::response_namespace_rewrite_for_test(
            response, &ctx, &state, table,
        ));
        if mode != "http-error" {
            assert!(
                futures::poll!(pending.as_mut()).is_pending(),
                "actual xAI JSON handler must await response bytes"
            );
            assert_eq!(probe.snapshot().created, 1);
            assert_eq!(probe.snapshot().released, 0);
            assert_eq!(budget.used(), held);
        }
        if mode == "cancel" {
            drop(pending);
        } else {
            resume.notify_one();
            let result = pending.await;
            if mode == "error" {
                assert!(result.is_err());
            } else {
                let response = result?;
                assert_eq!(response.status(), status);
                let bytes = response.into_body().collect().await?.to_bytes();
                let output: Value = serde_json::from_slice(&bytes)?;
                if mode == "complete" {
                    assert_eq!(output["output"][0]["namespace"], "synthetic_ns");
                    assert_eq!(output["output"][0]["name"], "lookup");
                } else {
                    assert_eq!(
                        output["output"][0]["name"], "synthetic_ns__lookup",
                        "error body is not rewritten"
                    );
                }
            }
        }
        assert_eq!(probe.snapshot().released, 1);
        assert_eq!(probe.snapshot().live_charge, 0);
        drop(request);
        assert_eq!(budget.used(), 53);
        drop(foreign);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

// Actual HTTP acceptance regressions. No generated tool arguments are executed.
async fn run_p1_cache_authorization(mode: &str) -> TestResult {
    async fn upstream(
        State(seen): State<Arc<Mutex<Vec<Value>>>>,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        let first = {
            let mut requests = seen.lock().expect("synthetic capture");
            requests.push(body.clone());
            requests.len() == 1
        };
        if !first {
            return Json(json!({"id":"chat_p1_followup","object":"chat.completion","model":"synthetic-model",
                "choices":[{"index":0,"message":{"role":"assistant","content":"synthetic acknowledgment"},"finish_reason":"stop"}]})).into_response();
        }
        cache_owner_upstream(State(CacheOwnerFixture { streaming: false }), Json(body)).await
    }

    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .fallback(post(upstream))
        .with_state(seen.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "p1-cache-authorization".into(),
        "Synthetic cache authorization".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-p1-cache-credential","api_format":"chat_completions"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let mut proxy_state = state(db);
    if mode == "legacy" {
        proxy_state.restoration_gateway =
            RestorationGateway::with_test_rules(&[], 64 * 1024 * 1024);
    }
    let initial = http::Request::builder()
        .method("POST").uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-p1-cache-session")
        .body(Body::from(json!({"model":"synthetic-model","input":if mode == "legacy" { "synthetic ordinary prompt".to_string() } else { format!("Call {FIXTURE_PHONE}") },"stream":false,
            "tools":[{"type":"function","name":"synthetic_lookup","parameters":{"type":"object","properties":{"phone":{"type":"string"}}}}]}).to_string()))?;
    let initial = handlers::handle_responses(State(proxy_state.clone()), initial).await?;
    assert_eq!(
        initial.status(),
        http::StatusCode::OK,
        "cache-generation control must succeed"
    );
    let initial_bytes = initial.into_body().collect().await?.to_bytes();
    let initial_text = std::str::from_utf8(&initial_bytes)?;
    assert!(
        mode == "legacy" || initial_text.contains(FIXTURE_PHONE),
        "control must restore the synthetic argument"
    );
    assert!(
        !initial_text.contains("rredact-cn_phone-"),
        "control must not expose references"
    );
    let followup_body = json!({"model":"synthetic-model","stream":false,"input":[{
        "type":"function_call_output","call_id":"call_cache_owner","output":"synthetic tool result"}]});
    assert!(
        !followup_body.to_string().contains("rredact-cn_phone-"),
        "initial followup must carry no token"
    );
    let mut cache_probe = followup_body.clone();
    let (cached_count, owners) = proxy_state
        .codex_chat_history
        .enrich_request_with_payload_owners(&mut cache_probe)
        .await
        .map_err(|error| std::io::Error::other(error.code()))?;
    assert!(
        cached_count > 0,
        "control must generate real cached tool history"
    );
    assert_eq!(
        cache_probe.to_string().contains("rredact-cn_phone-"),
        mode != "legacy",
        "cache token presence must match control mode"
    );
    assert!(
        !cache_probe.to_string().contains(FIXTURE_PHONE),
        "cache must not contain restored plaintext"
    );
    drop(cache_probe);
    drop(owners);
    let initial_sends = seen.lock().map_err(|_| "synthetic capture poisoned")?.len();
    assert_eq!(
        initial_sends, 1,
        "control must use actual HTTP exactly once"
    );
    if mode == "disabled" {
        let config = runtime::SecurityRuntimeConfig {
            enabled: false,
            ..Default::default()
        };
        runtime::set_security_runtime_for_test(config);
    } else if mode == "changed" {
        let mut config = runtime::SecurityRuntimeConfig::default();
        config
            .builtin_action_overrides
            .insert("chinese_phone_number".into(), super::AuditAction::Allow);
        runtime::set_security_runtime_for_test(config);
    } else if mode == "provider" {
        let mut changed = provider.clone();
        changed.id = "p1-changed-provider".into();
        proxy_state.db.save_provider("codex", &changed)?;
        proxy_state.db.set_current_provider("codex", &changed.id)?;
    }
    let followup = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-p1-cache-session")
        .body(Body::from(followup_body.to_string()))?;
    let result = handlers::handle_responses(State(proxy_state), followup).await;
    let followup_status = match result {
        Ok(response) => {
            let status = response.status().as_u16();
            let _ = response.into_body().collect().await?;
            Some(status)
        }
        Err(_) => None,
    };
    server.abort();
    let requests = seen.lock().map_err(|_| "synthetic capture poisoned")?;
    let second_sends = requests.len().saturating_sub(initial_sends);
    let late_token_sent = requests
        .get(initial_sends)
        .is_some_and(|body| body.to_string().contains("rredact-cn_phone-"));
    let allowed = matches!(mode, "valid" | "legacy");
    eprintln!("P1_CACHE mode={mode} control_success=true initial_followup_token_free=true second_upstream_sends={second_sends} late_token_sent={late_token_sent} followup_status={followup_status:?}");
    assert_eq!(second_sends, usize::from(allowed), "only currently authorized or ordinary token-free history may reach the second actual upstream send");
    if allowed {
        assert_eq!(followup_status, Some(200));
    } else {
        assert!(
            followup_status.is_none_or(|status| status >= 400),
            "refusal must remain a failure"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn p1_acceptance_late_cache_authorization() -> TestResult {
    run_p1_cache_authorization("disabled").await
}

#[tokio::test]
#[serial_test::serial]
async fn p1_cache_authorization_policy_binding_and_legacy_controls() -> TestResult {
    for mode in ["valid", "changed", "provider", "legacy"] {
        run_p1_cache_authorization(mode).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn p1_acceptance_xai_raw_completion() -> TestResult {
    #[derive(Clone)]
    struct Fixture {
        mode: String,
        seen: Arc<Mutex<Vec<Value>>>,
    }
    struct Observed {
        tool_frames: usize,
        restored: bool,
        reference_exposed: bool,
        failed: bool,
        context_reused: bool,
        followup_sends: usize,
    }
    async fn upstream(
        State(fixture): State<Fixture>,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        fixture
            .seen
            .lock()
            .expect("synthetic capture")
            .push(body.clone());
        if body.get("previous_response_id").is_some() {
            return Json(json!({"id":"resp_p1_continuation","object":"response","status":"completed","model":"synthetic-model",
                "output":[{"id":"msg_p1_continuation","type":"message","role":"assistant","status":"completed",
                "content":[{"type":"output_text","text":"synthetic continuation","annotations":[]}]}]})).into_response();
        }
        let text = fixture_text(&body).unwrap_or("missing synthetic carrier");
        let arguments = json!({"phone":text,"count":3.0,"label":"合成"}).to_string();
        let item = json!({"id":"fc_p1_xai","type":"function_call","call_id":"call_p1_xai",
            "name":"synthetic_lookup","arguments":arguments,"status":"completed"});
        let events = [
            json!({"type":"response.output_item.added","output_index":0,"item":{
                "id":"fc_p1_xai","type":"function_call","call_id":"call_p1_xai","name":"synthetic_lookup","arguments":"","status":"in_progress"}}),
            json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_p1_xai","delta":arguments}),
            json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_p1_xai","arguments":arguments}),
            json!({"type":"response.output_item.done","output_index":0,"item":item}),
            json!({"type":"response.completed","response":{"id":"resp_p1_xai","object":"response","status":"completed","model":"synthetic-model","output":[item]}}),
        ];
        let mut wire = String::new();
        for (index, event) in events.iter().enumerate() {
            let kind = event["type"].as_str().expect("synthetic event type");
            let encoded = if fixture.mode == "duplicate" && index + 1 == events.len() {
                event.to_string().replacen(
                    "\"status\":\"completed\"",
                    "\"status\":\"incomplete\",\"status\":\"completed\"",
                    1,
                )
            } else {
                event.to_string()
            };
            wire.push_str(&format!("event: {kind}\ndata: {encoded}"));
            if fixture.mode != "truncated" || index + 1 != events.len() {
                wire.push_str("\n\n");
            }
        }
        if fixture.mode == "crlf" {
            wire = wire.replace('\n', "\r\n");
        }
        if fixture.mode == "late_failed" {
            wire.push_str("event: response.failed\ndata: {\"type\":\"response.failed\"}\n\n");
        }
        if fixture.mode == "second_completed" {
            let final_event = &events[events.len() - 1];
            wire.push_str(&format!(
                "event: response.completed\ndata: {final_event}\n\n"
            ));
        }
        if fixture.mode == "cancel_half" {
            wire.truncate(wire.find("\n\n").expect("synthetic delimiter") / 2);
        }
        let mut bytes = wire.into_bytes();
        if fixture.mode == "invalid_utf8" {
            let offset = bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .expect("synthetic newline")
                + 1;
            bytes.splice(offset..offset, [b':', 0xff, b'\n']);
        } else if fixture.mode == "utf8_tail" {
            bytes.extend_from_slice(b": \xe4\xb8");
        }
        let chunks: Vec<_> = if matches!(fixture.mode.as_str(), "crlf" | "one_byte") {
            bytes
                .chunks(if fixture.mode == "one_byte" { 1 } else { 7 })
                .map(|chunk| Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(chunk)))
                .collect()
        } else {
            vec![Ok(bytes::Bytes::from(bytes))]
        };
        let mode = fixture.mode;
        let stream = async_stream::stream! {
            for chunk in chunks { yield chunk; }
            if matches!(mode.as_str(), "cancel_half" | "cancel_terminal") {
                futures::future::pending::<()>().await;
            } else if mode == "io_error" {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                yield Err(std::io::Error::other("Synthetic upstream failure."));
            }
        };
        (
            [("content-type", "text/event-stream")],
            Body::from_stream(stream),
        )
            .into_response()
    }
    async fn run(mode: &str) -> Result<Observed, Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let seen = Arc::new(Mutex::new(Vec::new()));
        let fixture = Fixture {
            mode: mode.to_owned(),
            seen: seen.clone(),
        };
        let app = Router::new().fallback(post(upstream)).with_state(fixture);
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let db = Arc::new(Database::memory()?);
        let settings = json!({"auth":{"OPENAI_API_KEY":"synthetic-p1-xai-credential"},"config":format!(
            "model_provider = \"custom\"\n[model_providers.custom]\nname = \"xai\"\nbase_url = \"http://{addr}/api.x.ai/v1\"\nwire_api = \"responses\"\n")});
        let provider = Provider::with_id(
            "p1-xai-completion".into(),
            "Synthetic xAI completion".into(),
            settings,
            None,
        );
        db.save_provider("codex", &provider)?;
        db.set_current_provider("codex", &provider.id)?;
        let proxy_state = state(db);
        let initial = http::Request::builder().method("POST").uri("/v1/responses")
            .header("content-type", "application/json").header("session-id", "synthetic-p1-xai-session")
            .body(Body::from(json!({"model":"synthetic-model","input":format!("Call {FIXTURE_PHONE}"),"stream":true,
                "tools":[{"type":"function","name":"synthetic_lookup","parameters":{"type":"object","properties":{"phone":{"type":"string"}}}}]}).to_string()))?;
        let response = handlers::handle_responses(State(proxy_state.clone()), initial).await?;
        assert_eq!(
            response.status(),
            http::StatusCode::OK,
            "synthetic xAI stream must reach client stream boundary"
        );
        let bytes = if mode.starts_with("cancel_") {
            let mut stream = response.into_body().into_data_stream();
            let mut received = bytes::BytesMut::new();
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(150);
            loop {
                match tokio::time::timeout_at(deadline, futures::StreamExt::next(&mut stream)).await
                {
                    Ok(Some(chunk)) => received.extend_from_slice(&chunk?),
                    Ok(None) => panic!("fixture must remain before original EOF"),
                    Err(_) => break,
                }
            }
            drop(stream);
            received.freeze()
        } else {
            response.into_body().collect().await?.to_bytes()
        };
        let text = std::str::from_utf8(&bytes)?;
        let mut recorder = FinalClientRecorder::default();
        recorder.observe_sse(text)?;
        let success = matches!(mode, "complete" | "crlf" | "one_byte");
        assert_eq!(recorder.submissions.len(), usize::from(success),
            "client records exactly one restored tool only after valid original completion; mode={mode}");
        assert_eq!(
            recorder.completed, success,
            "invalid original lifecycle cannot expose completion; mode={mode}"
        );
        if success {
            let arguments: Value = serde_json::from_str(
                recorder.submissions[0]["arguments"]
                    .as_str()
                    .ok_or("missing synthetic recorded arguments")?,
            )?;
            assert!(arguments["phone"]
                .as_str()
                .is_some_and(|text| text.contains(FIXTURE_PHONE)));
            assert!(!recorder.submissions[0]
                .to_string()
                .contains("rredact-cn_phone-"));
        }
        let mut tool_frames = 0;
        let mut failed = false;
        for line in text.lines() {
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            let kind = event["type"].as_str().unwrap_or("");
            failed |= kind == "response.failed";
            if kind.starts_with("response.function_call_arguments.")
                || event.pointer("/item/type").and_then(Value::as_str) == Some("function_call")
                || event
                    .pointer("/response/output")
                    .and_then(Value::as_array)
                    .is_some_and(|output| output.iter().any(|item| item["type"] == "function_call"))
            {
                tool_frames += 1;
            }
        }
        let restored = text.contains(FIXTURE_PHONE);
        let reference_exposed = text.contains("rredact-cn_phone-");
        let before = seen.lock().map_err(|_| "synthetic capture poisoned")?.len();
        assert_eq!(
            before, 1,
            "xAI control must traverse exactly one actual upstream request"
        );
        {
            let captured = seen.lock().map_err(|_| "synthetic capture poisoned")?;
            assert!(
                captured[0].to_string().contains("rredact-cn_phone-"),
                "xAI upstream must receive an authorized generated reference"
            );
            assert!(
                !captured[0].to_string().contains(FIXTURE_PHONE),
                "xAI upstream must not receive fixture plaintext"
            );
        }
        let followup = http::Request::builder().method("POST").uri("/v1/responses")
            .header("content-type", "application/json").header("session-id", "synthetic-p1-xai-session")
            .body(Body::from(json!({"model":"synthetic-model","input":"synthetic continuation request","stream":false,
                "previous_response_id":"resp_p1_xai"}).to_string()))?;
        let followup_status = match handlers::handle_responses(State(proxy_state), followup).await {
            Ok(response) => {
                let status = response.status();
                let _ = response.into_body().collect().await?;
                Some(status)
            }
            Err(_) => None,
        };
        server.abort();
        let followup_sends = seen
            .lock()
            .map_err(|_| "synthetic capture poisoned")?
            .len()
            .saturating_sub(before);
        let context_reused = followup_sends == 1 && followup_status == Some(http::StatusCode::OK);
        eprintln!("P1_XAI mode={mode} actual_initial_sends={before} tool_frames={tool_frames} restored={restored} reference_exposed={reference_exposed} failed={failed} followup_upstream_sends={followup_sends} context_reused={context_reused}");
        Ok(Observed {
            tool_frames,
            restored,
            reference_exposed,
            failed,
            context_reused,
            followup_sends,
        })
    }

    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let control = run("complete").await?;
    assert!(control.tool_frames > 0 && control.restored && !control.reference_exposed && !control.failed
        && control.context_reused && control.followup_sends == 1,
        "fully framed tool-stream control must restore, release tool frames and authorize its continuation");
    let truncated = run("truncated").await?;
    assert!(truncated.tool_frames == 0 && truncated.followup_sends == 0 && !truncated.context_reused
        && truncated.failed && !truncated.restored && !truncated.reference_exposed,
        "original unterminated completed frame must release zero client tool segments and authorize zero reusable completed contexts");
    for mode in [
        "crlf",
        "one_byte",
        "duplicate",
        "invalid_utf8",
        "utf8_tail",
        "late_failed",
        "second_completed",
        "io_error",
        "cancel_half",
        "cancel_terminal",
    ] {
        let observed = run(mode).await?;
        if matches!(mode, "crlf" | "one_byte") {
            assert!(
                observed.tool_frames > 0
                    && observed.context_reused
                    && observed.restored
                    && !observed.failed,
                "valid chunked CRLF control must pass"
            );
        } else {
            assert!(observed.tool_frames == 0 && observed.followup_sends == 0 && !observed.context_reused
                && !observed.restored && !observed.reference_exposed && (mode.starts_with("cancel_") || observed.failed),
                "invalid original bytes must release zero tool frames and authorize zero continuation; mode={mode}");
        }
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn p1_xai_original_frame_limits_budget_and_cancel() -> TestResult {
    use super::http_restoration::{
        admit_http_request, observe_xai_original_sse, RestorationAdmission,
    };
    use super::payload_budget::PayloadBudget;
    use futures::StreamExt;
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    for mode in ["exact", "over", "budget", "cancel", "unpolled"] {
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        let foreign = budget.reserve(59)?;
        let store = super::tokenization::DlpTokenStore::new();
        let gateway = RestorationGateway::with_test_store_and_rules(
            store,
            &["chinese_phone_number"],
            budget.clone(),
        );
        let mut body = json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":true});
        let headers = http::HeaderMap::new();
        let request = match admit_http_request(
            &gateway,
            &crate::app_config::AppType::Codex,
            true,
            false,
            &mut body,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic frame-limit request not admitted".into()),
        };
        let (attempt, input_owner) = request.prepare_owned_attempt_for_proxy(
            "p1-frame",
            "openai_responses",
            "http://127.0.0.1:1/v1/responses",
            Some("synthetic-frame-principal"),
            &mut body,
            Ok,
        )?;
        let evidence = attempt
            .xai_original_sse_evidence_tracker()
            .ok_or("missing synthetic evidence")?;
        let tokenized = body["input"].as_str().ok_or("missing synthetic carrier")?;
        let response = json!({"id":"resp_p1_frame_limit","object":"response","status":"completed","model":"synthetic-model",
            "output":[{"id":"msg_frame","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":tokenized,"annotations":[]}]}]});
        let terminal = format!(
            "event: response.completed\ndata: {}\n\n",
            json!({"type":"response.completed","response":response})
        );
        let length = 1024 * 1024 + usize::from(mode == "over");
        let prefix = format!(":{}\n\n", "p".repeat(length - 3));
        assert_eq!(prefix.len(), length);
        let chunks = vec![
            Ok::<_, std::io::Error>(bytes::Bytes::from(prefix)),
            Ok(bytes::Bytes::from(terminal)),
        ];
        let held = if mode == "budget" {
            Some(budget.reserve(budget.limit() - budget.used())?)
        } else {
            None
        };
        let mut stream =
            observe_xai_original_sse(futures::stream::iter(chunks), Some(evidence.clone()));
        if mode == "unpolled" {
            drop(stream);
        } else if mode == "cancel" {
            assert!(stream
                .next()
                .await
                .ok_or("missing synthetic first frame")?
                .is_ok());
            drop(stream);
        } else {
            while let Some(_chunk) = stream.next().await {}
            drop(stream);
        }
        drop(held);
        let prepared = http::Response::builder()
            .header("content-type", "text/event-stream")
            .body(Body::from(format!(
                "event: response.completed\ndata: {}\n\n",
                json!({"type":"response.completed","response":response})
            )))?;
        let restored = attempt
            .restore_client_http_response(
                prepared,
                super::response_restoration::ArgumentNormalizationMode::XaiWholeFloat,
                Some(evidence),
            )
            .await;
        let bytes = restored.into_body().collect().await?.to_bytes();
        let text = std::str::from_utf8(&bytes)?;
        assert_eq!(
            text.contains(FIXTURE_PHONE),
            mode == "exact",
            "only exact valid frame may authorize restoration; mode={mode}"
        );
        assert_eq!(
            text.contains("response.failed"),
            mode != "exact",
            "failed/pending original evidence must refuse; mode={mode}"
        );
        assert!(!text.contains("rredact-cn_phone-"));
        drop(bytes);
        drop(body);
        drop(input_owner);
        drop(request);
        drop(gateway);
        assert_eq!(
            budget.used(),
            59,
            "observer/cancel must release only its own payload charges; mode={mode}"
        );
        drop(foreign);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn p1_xai_legacy_redact_keeps_native_namespace_without_restoration_budget() -> TestResult {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(6);
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    assert!(
        crate::proxy::forwarder::final_send_binding_test_support::is_active(),
        "legacy xAI fixture requires explicit no-proxy transport"
    );
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    let mut config = runtime::SecurityRuntimeConfig::default();
    config
        .builtin_action_overrides
        .insert("chinese_phone_number".into(), super::AuditAction::Redact);
    runtime::set_security_runtime_for_test(config);
    let listener = tokio::time::timeout_at(deadline, tokio::net::TcpListener::bind("127.0.0.1:0"))
        .await
        .map_err(|_| "synthetic legacy xAI bind deadline exceeded")??;
    let addr = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = seen.clone();
    let app = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            captured.lock().expect("synthetic capture").push(body);
            let event = json!({"type":"response.completed","response":{"id":"resp_p1_legacy","object":"response","status":"completed","output":[{
                "type":"function_call","id":"fc_legacy","call_id":"call_legacy","name":"synthetic__lookup","arguments":"{\"phone\":\"[REDACTED]\"}","status":"completed"}]}});
            // Preserve the preexisting native compatibility path, including tail repair.
            ([("content-type", "text/event-stream")], format!("event: response.completed\ndata: {event}")).into_response()
        }
    }));
    let server =
        FinalSendBindingServer::new(tokio::spawn(
            async move { axum::serve(listener, app).await },
        ));
    let outcome = tokio::time::timeout_at(deadline, async {
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "p1-legacy-xai".into(),
        "Synthetic legacy xAI".into(),
        json!({"auth":{"OPENAI_API_KEY":"synthetic-legacy-credential"},"config":format!(
            "model_provider = \"custom\"\n[model_providers.custom]\nname = \"xai\"\nbase_url = \"http://{addr}/api.x.ai/v1\"\nwire_api = \"responses\"\n")}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let budget = super::payload_budget::PayloadBudget::with_limit(59);
    let foreign = budget.reserve(59)?;
    let store = super::tokenization::DlpTokenStore::new();
    // History discovery admits an empty published namespace before action selection.
    // Compare against that exact admission, not against an uninitialized store.
    const SESSION: &str = "synthetic-legacy-xai-session";
    drop(store.admit_published_policy(
        "codex",
        Some(SESSION),
        runtime::admit_security_runtime()?.restoration_policy_revision,
    )?);
    let before = store.stats()?;
    assert!(before.namespaces == 1 && before.entries == 0 && before.plaintext_bytes == 0);
    let mut proxy_state = state(db);
    proxy_state.restoration_gateway =
        RestorationGateway::with_test_store_and_rules(store.clone(), &[], budget.clone());
    let request = http::Request::builder().method("POST").uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session_id", SESSION)
        .body(Body::from(json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":true,
            "tools":[{"type":"namespace","name":"synthetic","tools":[{"type":"function","name":"lookup","parameters":{"type":"object","properties":{}}}]}]}).to_string()))?;
    let response = handlers::handle_responses(State(proxy_state), request).await?;
    assert_eq!(response.status(), http::StatusCode::OK);
    let bytes = response.into_body().collect().await?.to_bytes();
    let text = std::str::from_utf8(&bytes)?;
    assert!(text.contains("\"namespace\":\"synthetic\"") && text.contains("\"name\":\"lookup\""));
    assert!(
        !text.contains("response.failed")
            && !text.contains("rredact-cn_phone-")
            && !text.contains(FIXTURE_PHONE)
    );
    let requests = seen.lock().map_err(|_| "synthetic capture poisoned")?;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].to_string().contains("[REDACTED]"));
    assert!(!requests[0].to_string().contains(FIXTURE_PHONE));
    assert_eq!(
        store.stats()?,
        before,
        "legacy redact must add no mappings or charges beyond empty history admission"
    );
    assert_eq!(
        budget.used(),
        59,
        "legacy must work while the unrelated restoration budget is exhausted"
    );
    drop(foreign);
    assert_eq!(budget.used(), 0);
    Ok(()) as TestResult
    })
    .await;
    // Observe cleanup even when forwarding or collection returned an error.
    let cleanup = server.finish_at(deadline).await;
    let result = outcome.map_err(|_| "synthetic legacy xAI shared deadline exceeded")?;
    cleanup?;
    result
}

// Retained remaining-closure regressions: actual HTTP, synthetic values, no execution.
async fn run_closure_snapshots(mode: &'static str) -> TestResult {
    #[derive(Clone)]
    struct Fixture {
        mode: &'static str,
        seen: Arc<Mutex<Vec<Value>>>,
    }
    async fn upstream(
        State(fixture): State<Fixture>,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        fixture
            .seen
            .lock()
            .expect("synthetic capture")
            .push(body.clone());
        if body.get("previous_response_id").is_some() {
            return Json(json!({"id":"resp_closure_followup","object":"response","status":"completed",
                "output":[{"id":"msg_followup","type":"message","role":"assistant","status":"completed",
                "content":[{"type":"output_text","text":"synthetic acknowledgment"}]}]})).into_response();
        }
        let tokenized = fixture_text(&body).expect("synthetic supported input");
        let mut arguments = json!({"phone":tokenized,"count":if fixture.mode.contains("negative_zero") { json!(0.0) } else if fixture.mode == "xai_float_control" { json!(3.0) } else { json!(3) }}).to_string();
        let argument_case = fixture.mode.strip_prefix("xai_").unwrap_or(fixture.mode);
        if fixture.mode == "xai_negative_zero_no_delta" {
            arguments = arguments.replace("0.0", "-0.0");
        } else if fixture.mode == "xai_float_no_delta_control" {
            arguments = json!({"phone":tokenized,"count":3.0}).to_string();
        } else if argument_case == "arg_array_order" {
            arguments = json!({"phone":tokenized,"values":[1,2]}).to_string();
        }
        let mut tool = json!({"id":"fc_closure","type":"function_call","call_id":"call_closure",
            "name":"synthetic_lookup","arguments":arguments,"status":"completed"});
        let mut tool_events = vec![
            json!({"type":"response.output_item.added","output_index":0,"item":{
                "id":"fc_closure","type":"function_call","call_id":"call_closure","name":"synthetic_lookup",
                "arguments":"","status":"in_progress"}}),
            json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_closure","delta":arguments}),
            json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_closure","arguments":arguments}),
            json!({"type":"response.output_item.done","output_index":0,"item":tool}),
        ];
        let reasoning = fixture.mode.contains("reasoning");
        let before = fixture.mode.contains("before");
        let part = fixture.mode.contains("part");
        let added = fixture.mode.contains("added");
        let consistent = fixture.mode == "control"
            || added
            || fixture.mode.starts_with("semantic")
            || fixture.mode.contains("unsupported")
            || fixture.mode.contains("negative_zero")
            || fixture.mode.contains("wrong_id")
            || fixture.mode == "metadata_after"
            || fixture.mode == "xai_float_control"
            || argument_case.starts_with("arg_")
            || fixture.mode == "xai_float_no_delta_control"
            || fixture.mode.starts_with("matrix_");
        let final_text = "synthetic final text";
        let raw_text = if consistent {
            final_text
        } else {
            "synthetic contradictory text"
        };
        let mut text_item = if reasoning {
            json!({"id":"rs_closure","type":"reasoning","summary":[{"type":"summary_text","text":final_text}]})
        } else {
            json!({"id":"msg_closure","type":"message","role":"assistant","status":"completed",
                "content":[{"type":"output_text","text":final_text}]})
        };
        if added {
            if fixture.mode == "added_tool" {
                tool_events[0]["item"]["arguments"] = Value::String(arguments.clone());
            } else if reasoning {
                text_item["summary"][0]["text"] = Value::String(tokenized.to_owned());
            } else {
                text_item["content"][0]["text"] = Value::String(tokenized.to_owned());
            }
        }
        if fixture.mode.starts_with("semantic") {
            let changed = format!(
                "{{ \"count\": 3, \"phone\": {} }}",
                serde_json::to_string(tokenized).expect("synthetic string")
            );
            tool_events[2]["arguments"] = Value::String(changed);
        }
        if fixture.mode.contains("negative_zero") && fixture.mode != "xai_negative_zero_no_delta" {
            let changed = Value::String(arguments.replace("0.0", "-0.0"));
            match fixture.mode {
                "xai_negative_zero_added" => tool_events[0]["item"]["arguments"] = changed,
                "xai_negative_zero_item_done" => tool_events[3]["item"]["arguments"] = changed,
                _ => tool_events[2]["arguments"] = changed,
            }
        }
        if fixture.mode == "metadata_after" {
            tool_events[3]["item"]["input"] = json!("synthetic metadata before");
            tool["input"] = json!("synthetic metadata after");
        }
        if argument_case.starts_with("arg_") {
            let phone = serde_json::to_string(tokenized).expect("synthetic string");
            let changed = match argument_case {
                "arg_duplicate" => format!("{{\"phone\":{phone},\"count\":3,\"count\":3}}"),
                "arg_number_type" => json!({"phone":tokenized,"count":3.0}).to_string(),
                "arg_number_value" => json!({"phone":tokenized,"count":4}).to_string(),
                "arg_array_order" => json!({"phone":tokenized,"values":[2,1]}).to_string(),
                "arg_empty_done" | "arg_empty_item_done" => String::new(),
                _ => arguments.clone(),
            };
            if argument_case == "arg_empty_item_done" {
                tool_events[3]["item"]["arguments"] = Value::String(changed);
            } else {
                tool_events[2]["arguments"] = Value::String(changed);
            }
        }
        if fixture.mode.contains("no_delta") {
            tool_events.remove(1);
        }
        let mut raw_item = text_item.clone();
        if !added {
            if reasoning {
                raw_item["summary"][0]["text"] = Value::String(raw_text.to_owned());
            } else {
                raw_item["content"][0]["text"] = Value::String(raw_text.to_owned());
            }
        }
        if fixture.mode.contains("unsupported") {
            raw_item["content"][0]["annotations"] = json!([{"future_text":tokenized}]);
            text_item["content"][0]["annotations"] = json!([{"future_text":"synthetic metadata"}]);
        }
        if fixture.mode.contains("missing") {
            if part {
                raw_item["content"][0]
                    .as_object_mut()
                    .expect("synthetic part")
                    .remove("text");
            } else {
                raw_item
                    .as_object_mut()
                    .expect("synthetic item")
                    .remove("content");
            }
        }
        if fixture.mode.starts_with("matrix_") {
            if fixture.mode.contains("item_identity") {
                raw_item["id"] = json!("synthetic_changed_item");
            }
            if fixture.mode.contains("part_kind") {
                text_item["content"][0] = json!({"type":"refusal","refusal":final_text});
            }
            if fixture.mode.contains("part_omitted") {
                text_item["content"] = json!([]);
            }
            if fixture.mode.contains("empty_done") || fixture.mode.contains("empty_added") {
                raw_item["content"][0]["text"] = json!("");
            }
            if fixture.mode.contains("empty_final") || fixture.mode.contains("empty_equal") {
                text_item["content"][0]["text"] = json!("");
            }
            if fixture.mode.contains("empty_equal") {
                raw_item["content"][0]["text"] = json!("");
            }
            if fixture.mode.contains("call_identity") {
                tool_events[3]["item"]["call_id"] = json!("synthetic_changed_call");
            }
        }
        let text_event = if fixture.mode.contains("wrong_id") {
            json!({"type":"response.output_text.done","output_index":1,"item_id":"wrong_synthetic_item","content_index":0,"text":final_text})
        } else if fixture.mode == "omitted_done_before" {
            json!({"type":"response.output_text.done","output_index":1,"item_id":"msg_closure","content_index":0,"text":final_text})
        } else if part {
            if reasoning {
                json!({"type":if added {"response.reasoning_summary_part.added"} else {"response.reasoning_summary_part.done"},
                    "output_index":1,"item_id":"rs_closure","summary_index":0,"part":raw_item["summary"][0]})
            } else {
                json!({"type":if added {"response.content_part.added"} else {"response.content_part.done"},
                    "output_index":1,"item_id":"msg_closure","content_index":0,"part":raw_item["content"][0]})
            }
        } else {
            json!({"type":if added {"response.output_item.added"} else {"response.output_item.done"},"output_index":1,"item":raw_item})
        };
        let mut events = Vec::new();
        if before {
            events.push(text_event.clone());
        }
        events.extend(tool_events);
        if !before && fixture.mode != "added_tool" {
            events.push(text_event);
        }
        let output = if fixture.mode == "omitted_done_before" {
            json!([tool])
        } else {
            json!([tool, text_item])
        };
        events.push(json!({"type":"response.completed","response":{"id":"resp_closure_snapshot","object":"response",
            "status":"completed","output":output}}));
        for (index, event) in events.iter_mut().enumerate() {
            event["sequence_number"] = json!(index);
            if fixture.mode.starts_with("matrix_") {
                event["synthetic_metadata"] = json!({"position":index,"values":["inert",false,7]});
            }
        }
        let wire = events
            .iter()
            .map(|event| {
                format!(
                    "event: {}\ndata: {event}\n\n",
                    event["type"].as_str().expect("synthetic type")
                )
            })
            .collect::<String>();
        ([("content-type", "text/event-stream")], wire).into_response()
    }
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().fallback(post(upstream)).with_state(Fixture {
        mode,
        seen: seen.clone(),
    });
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "closure-snapshots".into(),
        "Synthetic snapshots".into(),
        if mode.starts_with("xai_") {
            json!({"auth":{"OPENAI_API_KEY":"synthetic-closure-credential"},"config":format!(
                "model_provider = \"custom\"\n[model_providers.custom]\nname = \"xai\"\nbase_url = \"http://{addr}/api.x.ai/v1\"\nwire_api = \"responses\"\n")})
        } else {
            json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-closure-credential"})
        },
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let proxy_state = state(db);
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-closure-session")
        .body(Body::from(
            json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":true,
            "tools":[{"type":"function","name":"synthetic_lookup","parameters":{"type":"object"}}]})
            .to_string(),
        ))?;
    let response = handlers::handle_responses(State(proxy_state.clone()), request).await?;
    assert_eq!(
        response.status(),
        http::StatusCode::OK,
        "must reach actual client SSE boundary"
    );
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        response.into_body().collect(),
    )
    .await??
    .to_bytes();
    let wire = std::str::from_utf8(&bytes)?;
    let mut recorder = FinalClientRecorder::default();
    recorder.observe_sse(wire)?;
    let mut tools = 0usize;
    let mut failed = false;
    let mut completed = false;
    let mut events = Vec::new();
    for data in wire.lines().filter_map(|line| line.strip_prefix("data: ")) {
        if data == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(data)?;
        failed |= event["type"] == "response.failed";
        completed |= event["type"] == "response.completed";
        if event["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("response.function_call_arguments."))
            || event.pointer("/item/type").and_then(Value::as_str) == Some("function_call")
        {
            tools += 1;
        }
        events.push(event);
    }
    {
        let capture = seen.lock().map_err(|_| "synthetic capture poisoned")?;
        assert_eq!(capture.len(), 1, "one actual initial upstream send");
        assert!(
            !capture[0].to_string().contains(FIXTURE_PHONE),
            "no plaintext outbound"
        );
        assert!(
            capture[0].to_string().contains("rredact-cn_phone-"),
            "authorized token upstream"
        );
    }
    let continuation = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-closure-session")
        .body(Body::from(
            json!({"model":"synthetic-model","input":"synthetic continuation","stream":false,
            "previous_response_id":"resp_closure_snapshot"})
            .to_string(),
        ))?;
    let continuation_ok = match handlers::handle_responses(State(proxy_state), continuation).await {
        Ok(response) => {
            let ok = response.status() == http::StatusCode::OK;
            let _ = response.into_body().collect().await?;
            ok
        }
        Err(_) => false,
    };
    server.abort();
    let followup_sends = seen
        .lock()
        .map_err(|_| "synthetic capture poisoned")?
        .len()
        .saturating_sub(1);
    eprintln!("CLOSURE_SNAPSHOT mode={mode} initial_sends=1 tool_events={tools} failed={failed} completed={completed} followup_sends={followup_sends} context_reused={continuation_ok}");
    let success = mode == "control"
        || mode.contains("added")
            && !mode.contains("unsupported")
            && !mode.contains("negative_zero")
        || mode.starts_with("semantic")
        || mode == "metadata_after"
        || mode == "xai_float_control"
        || mode == "xai_float_no_delta_control"
        || mode.starts_with("matrix_ok_");
    if success {
        assert_eq!(
            recorder.submissions.len(),
            1,
            "exactly one completed tool reaches the nonexecuting client"
        );
        assert!(recorder.completed && !recorder.failed);
        let expected_tools = if mode.contains("no_delta") { 3 } else { 4 };
        assert!(
            completed
                && !failed
                && tools == expected_tools
                && continuation_ok
                && followup_sends == 1,
            "consistent supported snapshots must restore and authorize exactly one continuation"
        );
        assert!(
            wire.contains(FIXTURE_PHONE)
                && (mode == "reasoning_part_added" || !wire.contains("rredact-cn_phone-")),
            "tools restore while supported reasoning remains original"
        );
        for (index, event) in events.iter().enumerate() {
            assert!(
                event["sequence_number"] == json!(index),
                "event ordering and sequence identity must be preserved"
            );
            if mode.starts_with("matrix_") {
                assert!(
                    event["synthetic_metadata"]
                        == json!({"position":index,"values":["inert",false,7]}),
                    "event replay must preserve unknown token-free metadata"
                );
            }
        }
        if mode == "metadata_after" {
            let item_done = events
                .iter()
                .find(|e| e["type"] == "response.output_item.done" && e["output_index"] == 0)
                .ok_or("missing synthetic item done")?;
            assert!(
                item_done["item"]["input"] == "synthetic metadata before",
                "replay must preserve original unknown token-free metadata"
            );
        }
        if !mode.contains("no_delta") {
            let delta = events
                .iter()
                .find(|event| event["type"] == "response.function_call_arguments.delta")
                .ok_or("missing synthetic delta")?;
            let done = events
                .iter()
                .find(|event| event["type"] == "response.function_call_arguments.done")
                .ok_or("missing synthetic done")?;
            assert!(
                delta["delta"] == done["arguments"],
                "restored replay must be byte-identical"
            );
        }
    } else {
        assert!(
            recorder.submissions.is_empty(),
            "failed original snapshots must make zero client submissions"
        );
        assert!(
            failed && !completed && tools == 0 && !continuation_ok && followup_sends == 0,
            "contradictory raw snapshot must discard buffered tools and authorize no context"
        );
        assert!(
            !wire.contains(FIXTURE_PHONE) && !wire.contains("rredact-cn_phone-"),
            "failure releases no restored tool or reference"
        );
    }
    Ok(())
}

macro_rules! closure_snapshot_test {
    ($name:ident, $mode:literal) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            run_closure_snapshots($mode).await
        }
    };
}
closure_snapshot_test!(reversible_closure_resp2_message_after, "message_after");
closure_snapshot_test!(reversible_closure_resp2_message_before, "message_before");
closure_snapshot_test!(reversible_closure_resp2_reasoning_after, "reasoning_after");
closure_snapshot_test!(
    reversible_closure_resp2_reasoning_before,
    "reasoning_before"
);
closure_snapshot_test!(reversible_closure_resp2_part_after, "part_after");
closure_snapshot_test!(reversible_closure_resp2_part_before, "part_before");
closure_snapshot_test!(
    reversible_closure_resp2_reasoning_part_after,
    "reasoning_part_after"
);
closure_snapshot_test!(
    reversible_closure_resp2_reasoning_part_before,
    "reasoning_part_before"
);
closure_snapshot_test!(reversible_closure_snapshot_control, "control");
closure_snapshot_test!(reversible_closure_resp4_message_added, "message_added");
closure_snapshot_test!(
    reversible_closure_resp4_reasoning_part_added,
    "reasoning_part_added"
);
closure_snapshot_test!(reversible_closure_resp4_tool_added, "added_tool");
closure_snapshot_test!(
    reversible_closure_resp5_semantic_arguments,
    "semantic_arguments"
);

async fn run_closure_chat_sibling(
    streaming: bool,
    upstream_sse: bool,
    function_sibling: bool,
) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let capture = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let upstream = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let capture = capture.clone();
        async move {
            capture.lock().expect("synthetic capture").push(body.clone());
            let text = fixture_text(&body).unwrap_or("Synthetic inert continuation");
            let positive = text.contains("Synthetic compatibility control");
            let mut tool = json!({"id":"call_closure_chat","type":"function","index":0,
                "function":{"name":"synthetic_lookup","arguments":json!({"phone":text}).to_string()}});
            let sibling = if positive {Value::Null} else {Value::String(text.to_owned())};
            if function_sibling { tool["function"]["future_text"] = sibling; }
            else { tool["future_text"] = sibling; }
            if upstream_sse {
                let event = json!({"id":"chat_closure_carrier","object":"chat.completion.chunk","model":"synthetic-model",
                    "choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[tool]},"finish_reason":"tool_calls"}]});
                ([ ("content-type", if streaming { "text/event-stream" } else { "text/plain" }) ],
                    format!("data: {event}\n\ndata: [DONE]\n\n")).into_response()
            } else {
                Json(json!({"id":"chat_closure_carrier","object":"chat.completion","model":"synthetic-model",
                    "choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[tool]},"finish_reason":"tool_calls"}]})).into_response()
            }
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "closure-chat-sibling".into(),
        "Synthetic Chat siblings".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-closure-credential","api_format":"chat_completions"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let proxy_state = state(db);
    let initial = |session: &str, text: String| -> Result<_, http::Error> {
        http::Request::builder().method("POST").uri("/v1/responses")
            .header("content-type", "application/json").header("session-id", session)
            .body(Body::from(json!({"model":"synthetic-model","input":text,"stream":streaming,
                "tools":[{"type":"function","name":"synthetic_lookup","parameters":{"type":"object"}}]}).to_string()))
    };
    let control = handlers::handle_responses(
        State(proxy_state.clone()),
        initial(
            "synthetic-chat-positive-session",
            format!("Synthetic compatibility control {FIXTURE_PHONE}"),
        )?,
    )
    .await?;
    assert_eq!(control.status(), http::StatusCode::OK);
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        control.into_body().collect(),
    )
    .await??
    .to_bytes();
    let control_wire = std::str::from_utf8(&bytes)?;
    let converted = if streaming {
        let mut completed = None;
        for data in control_wire
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
        {
            if data == "[DONE]" {
                continue;
            }
            let event: Value = serde_json::from_str(data)?;
            if event["type"] == "response.completed" {
                completed = event.get("response").cloned();
            }
        }
        completed.ok_or("synthetic positive completion missing")?
    } else {
        serde_json::from_slice::<Value>(&bytes)?
    };
    let converted_id = converted["id"]
        .as_str()
        .ok_or("synthetic converted identity missing")?
        .to_owned();
    assert_ne!(
        converted_id, "chat_closure_carrier",
        "control must establish the actual converted identity"
    );
    assert!(control_wire.contains(FIXTURE_PHONE) && !control_wire.contains("rredact-cn_phone-"));
    assert_eq!(converted["output"][0]["type"], "function_call");
    let continuation = |session: &str| -> Result<_, http::Error> {
        http::Request::builder().method("POST").uri("/v1/responses")
            .header("content-type", "application/json").header("session-id", session)
            .body(Body::from(json!({"model":"synthetic-model","input":"Synthetic inert continuation","stream":false,
                "previous_response_id":converted_id}).to_string()))
    };
    let response = match handlers::handle_responses(
        State(proxy_state.clone()),
        continuation("synthetic-chat-positive-session")?,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    let _ = response.into_body().collect().await?;
    assert_eq!(
        seen.lock().map_err(|_| "synthetic capture poisoned")?.len(),
        2,
        "positive converted identity must actually authorize a continuation send"
    );

    let response = match handlers::handle_responses(
        State(proxy_state.clone()),
        initial(
            "synthetic-chat-sibling-session",
            format!("Synthetic rejected carrier {FIXTURE_PHONE}"),
        )?,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    let status = response.status();
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        response.into_body().collect(),
    )
    .await??
    .to_bytes();
    let wire = std::str::from_utf8(&bytes)?;
    let leaked_tool = wire.contains("response.function_call_arguments")
        || wire.contains("\"type\":\"function_call\"");
    let unsupported = wire.contains("dlp_restore_unsupported");
    let exposed = wire.contains(FIXTURE_PHONE) || wire.contains("rredact-cn_phone-");
    let completed = wire.contains("response.completed") || wire.contains("[DONE]");
    {
        let captured = seen.lock().map_err(|_| "synthetic capture poisoned")?;
        assert_eq!(
            captured.len(),
            3,
            "rejected original must traverse actual upstream HTTP once"
        );
        for index in [0, 2] {
            assert!(
                captured[index].to_string().contains("rredact-cn_phone-")
                    && !captured[index].to_string().contains(FIXTURE_PHONE)
            );
        }
    }
    let followup = match handlers::handle_responses(
        State(proxy_state),
        continuation("synthetic-chat-sibling-session")?,
    )
    .await
    {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    assert_eq!(
        followup.status(),
        http::StatusCode::BAD_REQUEST,
        "failed original must not publish the independently established converted identity"
    );
    let _ = followup.into_body().collect().await?;
    server.abort();
    let followup_sends = seen
        .lock()
        .map_err(|_| "synthetic capture poisoned")?
        .len()
        .saturating_sub(3);
    assert!(unsupported && !leaked_tool && !exposed && !completed && followup_sends == 0,
        "original unknown Chat sibling must fail unsupported before tool replay or context publication");
    if !streaming {
        assert_eq!(status, http::StatusCode::BAD_GATEWAY);
    }
    Ok(())
}

macro_rules! closure_chat_sibling_test {
    ($name:ident, $streaming:literal, $sse:literal, $function:literal) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            run_closure_chat_sibling($streaming, $sse, $function).await
        }
    };
}
closure_chat_sibling_test!(reversible_closure_resp3_tool_json, false, false, false);
closure_chat_sibling_test!(reversible_closure_resp3_function_json, false, false, true);
closure_chat_sibling_test!(reversible_closure_resp3_tool_sse, true, true, false);
closure_chat_sibling_test!(reversible_closure_resp3_function_sse, true, true, true);
closure_chat_sibling_test!(
    reversible_closure_resp3_tool_buffered_sse,
    false,
    true,
    false
);
closure_chat_sibling_test!(
    reversible_closure_resp3_function_buffered_sse,
    false,
    true,
    true
);

closure_snapshot_test!(
    reversible_closure_resp2_omitted_done_before,
    "omitted_done_before"
);
closure_snapshot_test!(
    reversible_closure_resp4_missing_added_before,
    "missing_added_before"
);
closure_snapshot_test!(
    reversible_closure_resp4_missing_part_added_before,
    "missing_part_added_before"
);
closure_snapshot_test!(
    reversible_closure_resp4_missing_part_added_after,
    "missing_part_added_after"
);
closure_snapshot_test!(
    reversible_closure_resp2_unsupported_item_after,
    "unsupported_item_after"
);
closure_snapshot_test!(
    reversible_closure_resp2_unsupported_part_added_before,
    "unsupported_part_added_before"
);
closure_snapshot_test!(reversible_closure_resp5_negative_zero, "negative_zero");

async fn run_closure_digests(streaming: bool, sensitive: bool) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let capture = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let upstream = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let capture = capture.clone();
        async move {
            capture.lock().expect("synthetic capture").push(body.clone());
            let text = body.get("input").and_then(Value::as_str).expect("synthetic string input");
            let item = json!({"id":"msg_digest","type":"message","role":"assistant","status":"completed",
                "content":[{"type":"output_text","text":text}]});
            let final_body = json!({"id":"resp_digest","object":"response","status":"completed","output":[item]});
            let wire = if streaming {
                [json!({"type":"response.output_item.done","output_index":0,"item":item}),
                    json!({"type":"response.completed","response":final_body})].iter()
                    .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().expect("synthetic type"))).collect::<String>()
            } else { final_body.to_string() };
            ([("content-type", if streaming { "text/event-stream" } else { "application/json" }),
                ("content-digest", "sha-256=:c3ludGhldGlj:"), ("repr-digest", "sha-256=:c3ludGhldGlj:")], wire).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "closure-digest".into(),
        "Synthetic digest".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-digest-credential"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let request = http::Request::builder().method("POST").uri("/v1/responses")
        .header("content-type", "application/json").header("session-id", "synthetic-digest-session")
        .body(Body::from(json!({"model":"synthetic-model","input":if sensitive { FIXTURE_PHONE } else { "synthetic ordinary text" },
            "stream":streaming}).to_string()))?;
    let response = handlers::handle_responses(State(state(db)), request).await?;
    let status = response.status();
    let content_digest = response.headers().contains_key("content-digest");
    let repr_digest = response.headers().contains_key("repr-digest");
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        response.into_body().collect(),
    )
    .await??
    .to_bytes();
    server.abort();
    let wire = std::str::from_utf8(&bytes)?;
    assert_eq!(status, http::StatusCode::OK);
    let capture = seen.lock().map_err(|_| "synthetic capture poisoned")?;
    assert_eq!(capture.len(), 1, "one actual upstream send");
    if sensitive {
        assert!(
            !capture[0].to_string().contains(FIXTURE_PHONE)
                && wire.contains(FIXTURE_PHONE)
                && !wire.contains("rredact-cn_phone-"),
            "must reach successful final entity restoration"
        );
        assert!(
            !content_digest && !repr_digest,
            "mutated restored entity must not retain stale digest headers"
        );
    } else {
        assert!(
            content_digest && repr_digest,
            "ordinary unmodified response must preserve digest headers"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_resp6_json() -> TestResult {
    run_closure_digests(false, true).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_resp6_sse() -> TestResult {
    run_closure_digests(true, true).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_resp6_ordinary_control() -> TestResult {
    run_closure_digests(false, false).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req4_audit_entropy_degrades_without_blocking_http() -> TestResult {
    let _fault = super::identity::test_support::fail_audit_fingerprint();
    // This retains the safe-send and successful final-restoration assertions of the HTTP fixture.
    run_closure_digests(false, true).await
}

async fn run_closure_anthropic_json(mode: &'static str) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let capture = seen.clone();
    let body_pending = Arc::new(tokio::sync::Notify::new());
    let body_release = Arc::new(tokio::sync::Notify::new());
    let pending = body_pending.clone();
    let release = body_release.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let app = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let pending = pending.clone();
        let release = release.clone();
        let capture = capture.clone();
        async move {
            let initial = {
                let mut capture = capture.lock().expect("synthetic capture");
                capture.push(body.clone());
                capture.len() == 1
            };
            let tokenized = initial.then(|| fixture_text(&body)).flatten();
            let content = if let Some(text) = tokenized {
                json!([{"type":"tool_use","id":"call_json_closure","name":"synthetic_lookup","input":{"phone":text}}])
            } else { json!([{"type":"text","text":"synthetic continuation"}]) };
            let value = json!({"id":if initial { "msg_closure_json" } else { "msg_closure_json_followup" },"type":"message","role":"assistant","model":"synthetic-model",
                "content":content,
                "stop_reason":if mode == "incomplete" { "max_tokens" } else if tokenized.is_some() { "tool_use" } else { "end_turn" },
                "usage":{"input_tokens":1,"output_tokens":1}});
            let mut raw = value.to_string();
            if mode == "truncated" { raw.pop(); }
            if mode == "duplicate" { raw = raw.replacen("{", "{\"stop_reason\":\"max_tokens\",", 1); }
            if mode == "error" { raw = json!({"type":"error","error":{"type":"synthetic_error","message":"synthetic safe error"}}).to_string(); }
            if initial && matches!(mode, "io_error" | "cancel_partial" | "cancel_full") {
                if mode == "cancel_partial" { raw.truncate(raw.len() / 2); }
                let stream = async_stream::stream! {
                    yield Ok::<_, std::io::Error>(bytes::Bytes::from(raw));
                    // A complete JSON value is still not a fully read original body.
                    pending.notify_one();
                    if mode == "io_error" {
                        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                        yield Err(std::io::Error::other("Synthetic JSON body failure."));
                    } else {
                        release.notified().await;
                    }
                };
                return ([("content-type", "application/json")], Body::from_stream(stream)).into_response();
            }
            ([("content-type","application/json")], raw).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "closure-anthropic-json".into(),
        "Synthetic JSON witness".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-json-credential","api_format":"anthropic"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let proxy_state = state(db);
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-json-session")
        .body(Body::from(
            json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":true,
            "tools":[{"type":"function","name":"synthetic_lookup","parameters":{"type":"object"}}]})
            .to_string(),
        ))?;
    let wire = if mode.starts_with("cancel_") {
        let mut handling = Box::pin(handlers::handle_responses(
            State(proxy_state.clone()),
            request,
        ));
        tokio::select! {
            result = &mut handling => {
                let _ = result;
                panic!("pending original JSON body must not expose a client response");
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                panic!("synthetic original body did not reach the pending boundary");
            }
            _ = body_pending.notified() => {}
        }
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut handling)
                .await
                .is_err(),
            "JSON syntax alone must not replace natural body EOF"
        );
        drop(handling);
        body_release.notify_one();
        bytes::Bytes::new()
    } else {
        let response = match handlers::handle_responses(State(proxy_state.clone()), request).await {
            Ok(response) => response,
            Err(error) => error.into_response(),
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            response.into_body().collect(),
        )
        .await??
        .to_bytes()
    };
    let text = std::str::from_utf8(&wire)?;
    let mut recorder = FinalClientRecorder::default();
    recorder.observe_sse(text)?;
    let mut completed = false;
    let mut tools = 0usize;
    for line in text.lines().filter_map(|line| line.strip_prefix("data: ")) {
        if line == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(line)?;
        completed |= event["type"] == "response.completed";
        if event.pointer("/item/type").and_then(Value::as_str) == Some("function_call")
            || event["type"]
                .as_str()
                .is_some_and(|t| t.starts_with("response.function_call_arguments."))
        {
            tools += 1;
        }
    }
    let cursor = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-json-session")
        .body(Body::from(
            json!({"model":"synthetic-model","input":"synthetic continuation","stream":false,
            "previous_response_id":"resp_msg_closure_json"})
            .to_string(),
        ))?;
    let continuation_ok = match handlers::handle_responses(State(proxy_state), cursor).await {
        Ok(response) => {
            let ok = response.status() == http::StatusCode::OK;
            let _ = response.into_body().collect().await?;
            ok
        }
        Err(_) => false,
    };
    server.abort();
    let capture = seen.lock().map_err(|_| "synthetic capture poisoned")?;
    assert!(
        !capture.is_empty() && !capture[0].to_string().contains(FIXTURE_PHONE),
        "initial HTTP must be tokenized"
    );
    let sends = capture.len();
    eprintln!("CLOSURE_JSON mode={mode} completed={completed} tool_events={tools} sends={sends} context_reused={continuation_ok}");
    if mode == "complete" {
        assert_eq!(
            recorder.submissions.len(),
            1,
            "genuine JSON submits one complete restored tool without executing it"
        );
        assert!(
            completed && tools > 0 && continuation_ok && sends == 2,
            "genuine full JSON must authorize restored synthetic SSE and completed context"
        );
        assert!(
            text.contains(FIXTURE_PHONE) && !text.contains("rredact-cn_phone-"),
            "tool payload restored only at final client boundary"
        );
    } else {
        assert!(
            recorder.submissions.is_empty(),
            "invalid or cancelled JSON submits zero tools"
        );
        assert!(
            !completed && tools == 0 && !continuation_ok && sends == 1,
            "invalid original JSON must release no tool or context"
        );
        assert!(!text.contains(FIXTURE_PHONE) && !text.contains("rredact-cn_phone-"));
    }
    Ok(())
}
macro_rules! closure_anthropic_json_test {
    ($name:ident, $mode:literal) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            run_closure_anthropic_json($mode).await
        }
    };
}
closure_anthropic_json_test!(reversible_closure_resp7_genuine_json_to_sse, "complete");
closure_anthropic_json_test!(
    reversible_closure_resp7_incomplete_json_control,
    "incomplete"
);
closure_anthropic_json_test!(reversible_closure_resp7_truncated_json_control, "truncated");
closure_anthropic_json_test!(reversible_closure_resp7_duplicate_json_control, "duplicate");
closure_anthropic_json_test!(reversible_closure_resp7_error_json_control, "error");
closure_anthropic_json_test!(
    reversible_closure_resp7_json_body_io_failure_control,
    "io_error"
);
closure_anthropic_json_test!(
    reversible_closure_resp7_json_partial_body_cancel_control,
    "cancel_partial"
);
closure_anthropic_json_test!(
    reversible_closure_resp7_json_complete_without_eof_cancel_control,
    "cancel_full"
);

closure_snapshot_test!(
    reversible_closure_resp2_wrong_text_id_before,
    "wrong_id_before"
);
closure_snapshot_test!(
    reversible_closure_resp2_wrong_text_id_after,
    "wrong_id_after"
);
closure_snapshot_test!(reversible_closure_resp2_metadata_replay, "metadata_after");
closure_snapshot_test!(
    reversible_closure_resp5_xai_negative_zero,
    "xai_negative_zero"
);

closure_snapshot_test!(
    reversible_closure_resp5_xai_negative_zero_added,
    "xai_negative_zero_added"
);
closure_snapshot_test!(
    reversible_closure_resp5_xai_negative_zero_item_done,
    "xai_negative_zero_item_done"
);
closure_snapshot_test!(
    reversible_closure_resp5_xai_float_control,
    "xai_float_control"
);

closure_snapshot_test!(reversible_closure_resp5_arg_duplicate, "arg_duplicate");
closure_snapshot_test!(reversible_closure_resp5_arg_number_type, "arg_number_type");
closure_snapshot_test!(
    reversible_closure_resp5_arg_number_value,
    "arg_number_value"
);
closure_snapshot_test!(reversible_closure_resp5_arg_array_order, "arg_array_order");
closure_snapshot_test!(reversible_closure_resp5_arg_empty_done, "arg_empty_done");
closure_snapshot_test!(
    reversible_closure_resp5_arg_empty_item_done,
    "arg_empty_item_done"
);
closure_snapshot_test!(
    reversible_closure_resp5_xai_arg_duplicate,
    "xai_arg_duplicate"
);
closure_snapshot_test!(
    reversible_closure_resp5_xai_arg_number_type,
    "xai_arg_number_type"
);
closure_snapshot_test!(
    reversible_closure_resp5_xai_arg_number_value,
    "xai_arg_number_value"
);
closure_snapshot_test!(
    reversible_closure_resp5_xai_arg_array_order,
    "xai_arg_array_order"
);
closure_snapshot_test!(
    reversible_closure_resp5_xai_arg_empty_done,
    "xai_arg_empty_done"
);
closure_snapshot_test!(
    reversible_closure_resp5_xai_arg_empty_item_done,
    "xai_arg_empty_item_done"
);

closure_snapshot_test!(
    reversible_closure_resp5_xai_negative_zero_no_delta,
    "xai_negative_zero_no_delta"
);
closure_snapshot_test!(
    reversible_closure_resp5_xai_float_no_delta_control,
    "xai_float_no_delta_control"
);

async fn run_closure_resp7_xai_foreign_json_witness(pending: bool) -> TestResult {
    use super::http_restoration::{
        admit_http_request, observe_xai_original_sse, RestorationAdmission,
    };
    use futures::StreamExt;
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let mut body = json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":true});
    let request = match admit_http_request(
        &gateway,
        &crate::app_config::AppType::Codex,
        true,
        false,
        &mut body,
        &http::HeaderMap::new(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic mixed-witness request not admitted".into()),
    };
    let (attempt, _owner) = request.prepare_owned_attempt_for_proxy(
        "synthetic-mixed-witness",
        "openai_responses",
        "http://127.0.0.1:1/v1/responses",
        Some("synthetic-principal"),
        &mut body,
        Ok,
    )?;
    let evidence = attempt
        .xai_original_sse_evidence_tracker()
        .ok_or("missing synthetic evidence")?;
    let token = body["input"].as_str().ok_or("missing synthetic text")?;
    let response = json!({"id":"resp_foreign_json_witness","object":"response","status":"completed","output":[{
        "type":"function_call","id":"fc_foreign","call_id":"call_foreign","name":"synthetic_tool",
        "arguments":json!({"phone":token}).to_string(),"status":"completed"}]});
    let terminal = format!(
        "event: response.completed\ndata: {}\n\n",
        json!({"type":"response.completed","response":response})
    );
    if pending {
        evidence
            .lock()
            .map_err(|_| "synthetic evidence poisoned")?
            .observe_fully_read_anthropic_json("{}");
    }
    let observed = observe_xai_original_sse(
        futures::stream::iter([Ok::<_, std::io::Error>(bytes::Bytes::from(
            terminal.clone(),
        ))]),
        Some(evidence.clone()),
    );
    let chunks: Vec<_> = observed.collect().await;
    assert!(
        chunks.iter().all(Result::is_ok),
        "synthetic original stream must validate before misuse assertion"
    );
    if !pending {
        evidence
            .lock()
            .map_err(|_| "synthetic evidence poisoned")?
            .observe_fully_read_anthropic_json("{}");
    }
    let prepared = http::Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(terminal))?;
    let restored = attempt
        .restore_client_http_response(
            prepared,
            super::response_restoration::ArgumentNormalizationMode::XaiWholeFloat,
            Some(evidence),
        )
        .await;
    let bytes = restored.into_body().collect().await?.to_bytes();
    let text = std::str::from_utf8(&bytes)?;
    assert!(
        text.contains("response.failed")
            && !text.contains("event: response.completed\n")
            && !text.contains("function_call")
            && !text.contains(FIXTURE_PHONE),
        "foreign JSON witness must permanently revoke xAI completion and discard buffered tools"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_resp7_verified_xai_foreign_json_witness() -> TestResult {
    run_closure_resp7_xai_foreign_json_witness(false).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_resp7_pending_xai_foreign_json_witness() -> TestResult {
    run_closure_resp7_xai_foreign_json_witness(true).await
}

async fn operation_lineage_fixture(mode: &str) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let seen = Arc::new(Mutex::new(Vec::<(String, Value, String)>::new()));
    let captured = seen.clone();
    let upstream = Router::new().fallback(post(move |uri: http::Uri, headers: http::HeaderMap, Json(body): Json<Value>| {
        let captured = captured.clone();
        async move {
            let mut seen = captured.lock().expect("synthetic lineage capture");
            let text = fixture_text(&body).unwrap_or("missing synthetic carrier").to_owned();
            seen.push((uri.to_string(), body, headers.get("chatgpt-account-id").and_then(|v| v.to_str().ok()).unwrap_or("").to_owned()));
            let compact = uri.path().ends_with("/compact");
            let number = seen.len();
            let mut response = json!({"id":format!("resp_lineage_{number}"),"object":"response","status":"completed",
                "output":[{"type":"message","id":format!("msg_lineage_{number}"),"role":"assistant","status":"completed",
                    "content":[{"type":"output_text","text":text,"annotations":[]}]}]});
            if compact {
                response["object"] = json!("response.compaction");
                response["created_at"] = json!(1);
                response["output"].as_array_mut().expect("synthetic output").push(
                    json!({"id":"cmp_lineage","type":"compaction","encrypted_content":"synthetic-lineage-opaque"}));
            }
            Json(response)
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-operation-lineage".into(),
        "Synthetic operation lineage".into(),
        json!({"base_url":if mode == "prefix" {format!("http://{address}/synthetic-prefix")} else {format!("http://{address}")},"api_key":"synthetic-lineage-key","api_format":"responses"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let proxy = ProxyServer::new(ProxyConfig::default(), db, None).with_restoration_gateway(
        RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024),
    );
    let mut router = proxy.build_router();
    for stage in 1..=3 {
        let compact = stage == 2 && mode != "workspace_same";
        let uri = if compact {
            "/v1/responses/compact?route=synthetic"
        } else {
            "/v1/responses?route=synthetic"
        };
        let mut body = json!({"model":"synthetic-model","stream":false,"input":format!("Synthetic request {FIXTURE_PHONE}")});
        if stage > 1 {
            body["previous_response_id"] = json!(format!("resp_lineage_{}", stage - 1));
        }
        if stage == 3 && mode != "workspace_same" {
            body["input"] = json!([{"type":"compaction","id":"cmp_lineage","encrypted_content":"synthetic-lineage-opaque"},
                {"role":"user","content":format!("Synthetic request {FIXTURE_PHONE}")}]);
        }
        if (stage == 2 && matches!(mode, "workspace" | "workspace_same"))
            || (stage == 3 && mode == "workspace_reverse")
        {
            let refused = router
                .call(
                    http::Request::builder()
                        .method("POST")
                        .uri(uri)
                        .header("content-type", "application/json")
                        .header("session-id", "synthetic-operation-lineage-session")
                        .header("chatgpt-account-id", "synthetic-workspace-other")
                        .body(Body::from(body.to_string()))?,
                )
                .await?;
            let status = refused.status();
            let _ = refused.into_body().collect().await?;
            let sends = seen
                .lock()
                .map_err(|_| "synthetic lineage capture poisoned")?
                .len();
            assert!(status == http::StatusCode::BAD_REQUEST && sends == stage - 1,
                "changed actual workspace must revoke cross-operation lineage before sending; status={status} sends={sends}");
        }
        if stage > 1 {
            for (negative_uri, session, unknown_opaque) in [
                (
                    if compact {
                        "/v1/responses/compact?route=changed"
                    } else {
                        "/v1/responses?route=changed"
                    },
                    "synthetic-operation-lineage-session",
                    false,
                ),
                (uri, "synthetic-other-lineage-session", false),
                (uri, "synthetic-operation-lineage-session", true),
            ] {
                let mut negative_body = body.clone();
                if unknown_opaque {
                    negative_body["input"] = json!([{"type":"compaction","encrypted_content":"synthetic-unknown-opaque"},
                        {"role":"user","content":format!("Synthetic request {FIXTURE_PHONE}")}]);
                }
                let refused = router
                    .call(
                        http::Request::builder()
                            .method("POST")
                            .uri(negative_uri)
                            .header("content-type", "application/json")
                            .header("session-id", session)
                            .header("chatgpt-account-id", "synthetic-workspace")
                            .body(Body::from(negative_body.to_string()))?,
                    )
                    .await?;
                assert_eq!(refused.status(), http::StatusCode::BAD_REQUEST);
                let _ = refused.into_body().collect().await?;
                assert_eq!(
                    seen.lock()
                        .map_err(|_| "synthetic lineage capture poisoned")?
                        .len(),
                    stage - 1,
                    "changed query/session or unknown opaque must make zero additional sends"
                );
            }
        }
        let request = http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .header("session-id", "synthetic-operation-lineage-session")
            .header("chatgpt-account-id", "synthetic-workspace")
            .body(Body::from(body.to_string()))?;
        let response = router.call(request).await?;
        let status = response.status();
        let bytes = response.into_body().collect().await?.to_bytes();
        let value: Value = serde_json::from_slice(&bytes)?;
        let sends = seen
            .lock()
            .map_err(|_| "synthetic lineage capture poisoned")?
            .len();
        assert!(status == http::StatusCode::OK && sends == stage,
            "approved Responses/compact operation transfer must retain lineage; stage={stage} status={status} sends={sends}");
        assert_eq!(
            value["output"][0]["content"][0]["text"],
            format!("Synthetic request {FIXTURE_PHONE}")
        );
        let captured = seen
            .lock()
            .map_err(|_| "synthetic lineage capture poisoned")?;
        assert_eq!(
            captured[stage - 1].0,
            if mode == "prefix" {
                format!(
                    "/synthetic-prefix{}",
                    uri.strip_prefix("/v1").unwrap_or(uri)
                )
            } else {
                uri.to_owned()
            }
        );
        assert_eq!(captured[stage - 1].2, "synthetic-workspace");
        let outbound = serde_json::to_string(&captured[stage - 1].1)?;
        assert!(
            !outbound.contains(FIXTURE_PHONE) && outbound.contains("rredact-cn_phone-"),
            "every lineage operation must send only tokenized supported text"
        );
    }
    server.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_responses_compact_responses_lineage() -> TestResult {
    operation_lineage_fixture("origin").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_custom_prefix_alias_lineage() -> TestResult {
    operation_lineage_fixture("prefix").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_changed_actual_workspace_refused() -> TestResult {
    operation_lineage_fixture("workspace").await
}

fn assert_history_literal_has_no_authority(
    gateway: &RestorationGateway,
    original: &Value,
    session: &str,
    literal: &str,
) -> TestResult {
    use super::http_restoration::{admit_http_request, RestorationAdmission};
    let mut body = original.clone();
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", session.parse()?);
    let request = match admit_http_request(
        gateway,
        &crate::app_config::AppType::Codex,
        true,
        false,
        &mut body,
        &headers,
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic literal history was not admitted".into()),
    };
    let attempt = request
        .prepare_attempt(
            "synthetic-literal-proof",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            None,
            &mut body,
            |candidate| Ok(candidate.clone()),
        )
        .map_err(|_| "synthetic literal history was not prepared")?;
    assert!(
        attempt.lease.resolve_if_authorized(literal)?.is_none(),
        "literal history must not grant restoration authority"
    );
    assert!(
        !attempt
            .lease
            .authorized_tokens()?
            .as_slice()
            .contains(&literal),
        "literal history must not enter the authorized proof"
    );
    Ok(())
}

async fn fresh_readable_history_fixture(
    protocol: &'static str,
    custom: bool,
    unsupported_reasoning: bool,
) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = Router::new()
        .fallback(post(matrix_upstream))
        .with_state(MatrixFixture {
            protocol,
            compact: false,
            streaming: false,
            complete: true,
            seen: seen.clone(),
        });
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-fresh-history".into(),
        "Synthetic fresh history".into(),
        json!({"base_url":format!("http://{address}"),"api_key":"synthetic-fresh-key",
            "api_format":if protocol == "chat" {"chat_completions"} else {"responses"}}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let proxy = ProxyServer::new(ProxyConfig::default(), db, None)
        .with_restoration_gateway(gateway.clone());
    let mut router = proxy.build_router();
    let call = if custom {
        json!({"type":"custom_tool_call","call_id":"synthetic_fresh_call","name":"synthetic_tool",
            "input":format!("synthetic inert text {FIXTURE_PHONE}"),"status":"completed"})
    } else {
        json!({"type":"function_call","call_id":"synthetic_fresh_call","name":"synthetic_lookup",
            "arguments":format!("{{\"phone\":\"{FIXTURE_PHONE}\"}}"),"status":"completed"})
    };
    let output_type = if custom {
        "custom_tool_call_output"
    } else {
        "function_call_output"
    };
    let mut body = json!({"model":"synthetic-model","stream":false,"input":[
        {"role":"user","content":format!("Synthetic user {FIXTURE_PHONE}")},
        {"type":"reasoning","id":"rs_synthetic_fresh","summary":[{"type":"summary_text","text":format!("Synthetic reasoning {FIXTURE_PHONE}")}],"content":null,"encrypted_content":null},
        call, {"type":output_type,"call_id":"synthetic_fresh_call","output":format!("Synthetic result {FIXTURE_PHONE}")},
        {"role":"user","content":"Synthetic continue"}]});
    if unsupported_reasoning {
        body["input"][2]["reasoning_content"] =
            json!(format!("Synthetic private reasoning {FIXTURE_PHONE}"));
    }
    let mut negative_bodies = Vec::new();
    let mut orphan = body.clone();
    orphan["input"]
        .as_array_mut()
        .expect("synthetic input")
        .remove(2);
    negative_bodies.push(orphan);
    let mut missing_result = body.clone();
    missing_result["input"]
        .as_array_mut()
        .expect("synthetic input")
        .remove(3);
    negative_bodies.push(missing_result);
    let mut reversed = body.clone();
    reversed["input"]
        .as_array_mut()
        .expect("synthetic input")
        .swap(2, 3);
    negative_bodies.push(reversed);
    let mut duplicate = body.clone();
    duplicate["input"]
        .as_array_mut()
        .expect("synthetic input")
        .insert(3, body["input"][2].clone());
    negative_bodies.push(duplicate);
    let mut stale = body.clone();
    stale["previous_response_id"] = json!("resp_synthetic_expired");
    negative_bodies.push(stale);
    let mut opaque = body.clone();
    opaque["input"][1]["encrypted_content"] = json!("synthetic-unknown-opaque");
    negative_bodies.push(opaque);
    let mut mismatched = body.clone();
    mismatched["input"][3]["type"] = json!(if custom {
        "function_call_output"
    } else {
        "custom_tool_call_output"
    });
    negative_bodies.push(mismatched);
    let mut incomplete = body.clone();
    incomplete["input"][2]
        .as_object_mut()
        .expect("synthetic call")
        .remove(if custom { "input" } else { "arguments" });
    negative_bodies.push(incomplete);
    let foreign = super::tokenization::DlpTokenStore::new();
    let mut foreign_lease = foreign.admit("codex", Some("synthetic-old-generation"), 1)?;
    let old_token = foreign_lease.prepare_with_budget(
        &[super::tokenization::TokenFragment {
            raw: "synthetic-old-generation-secret",
            whole_span_categories: &[super::tokenization::TokenCategory::Custom],
        }],
        &[],
        super::payload_budget::PayloadBudget::with_limit(1024 * 1024),
        |tokens| Ok(tokens[0].as_str().to_owned()),
    )?;
    let mut old = body.clone();
    old["input"][0]["content"] = json!(old_token);
    negative_bodies.push(old);
    let mut literal_sends = 0;
    for (case_index, negative) in negative_bodies.into_iter().enumerate() {
        let literal_case = case_index == 8 && !unsupported_reasoning;
        if literal_case {
            assert_history_literal_has_no_authority(
                &gateway,
                &negative,
                "synthetic-new-generation-history",
                &old_token,
            )?;
        }
        let response = router
            .call(
                http::Request::builder()
                    .method("POST")
                    .uri("/v1/responses")
                    .header("content-type", "application/json")
                    .header("session-id", "synthetic-new-generation-history")
                    .body(Body::from(negative.to_string()))?,
            )
            .await?;
        assert_eq!(
            response.status(),
            if literal_case {
                http::StatusCode::OK
            } else {
                http::StatusCode::BAD_REQUEST
            },
            "fresh history outcome; case_index={case_index}"
        );
        let bytes = response.into_body().collect().await?.to_bytes();
        literal_sends += usize::from(literal_case);
        let captured = seen
            .lock()
            .map_err(|_| "synthetic fresh capture poisoned")?;
        assert_eq!(
            captured.len(),
            literal_sends,
            "malformed/incomplete history must send nothing; case_index={case_index}"
        );
        if literal_case {
            let outbound = captured
                .last()
                .ok_or("synthetic literal send missing")?
                .to_string();
            assert!(
                outbound.contains(&old_token) && !outbound.contains(FIXTURE_PHONE),
                "unknown literal must survive while fresh PII stays tokenized"
            );
            assert!(
                !std::str::from_utf8(&bytes)?.contains("synthetic-old-generation-secret"),
                "unknown literal must not resolve its foreign mapping"
            );
        }
    }
    let response = router
        .call(
            http::Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .header("content-type", "application/json")
                .header("session-id", "synthetic-new-generation-history")
                .body(Body::from(body.to_string()))?,
        )
        .await?;
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    let sends = seen
        .lock()
        .map_err(|_| "synthetic fresh capture poisoned")?
        .len();
    if unsupported_reasoning {
        assert!(status == http::StatusCode::BAD_REQUEST && sends == 0,
            "unsupported call reasoning must not bypass fresh-input reinspection; status={status} sends={sends}");
        server.abort();
        return Ok(());
    }
    assert!(status == http::StatusCode::OK && sends == literal_sends + 1,
        "complete original readable history must be reinspected as fresh input; status={status} sends={sends}");
    let response: Value = serde_json::from_slice(&bytes)?;
    let serialized = serde_json::to_string(&response)?;
    assert!(serialized.contains(FIXTURE_PHONE) && !serialized.contains("rredact-cn_phone-"));
    let captured = seen
        .lock()
        .map_err(|_| "synthetic fresh capture poisoned")?;
    let outbound = serde_json::to_string(&captured[literal_sends])?;
    assert!(
        !outbound.contains(FIXTURE_PHONE) && outbound.contains("rredact-cn_phone-"),
        "all supported fresh history text must be tokenized before the only send"
    );
    assert!(outbound.contains("synthetic_fresh_call") && outbound.contains("Synthetic result"));
    server.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_fresh_function_history() -> TestResult {
    fresh_readable_history_fixture("native", false, false).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_fresh_custom_history() -> TestResult {
    fresh_readable_history_fixture("native", true, false).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_fresh_chat_history() -> TestResult {
    fresh_readable_history_fixture("chat", false, false).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_changed_workspace_same_operation() -> TestResult {
    operation_lineage_fixture("workspace_same").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_changed_workspace_reverse_transfer() -> TestResult {
    operation_lineage_fixture("workspace_reverse").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_original_history_ignores_cached_completion() -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = Router::new()
        .fallback(post(matrix_upstream))
        .with_state(MatrixFixture {
            protocol: "chat",
            compact: false,
            streaming: false,
            complete: true,
            seen: seen.clone(),
        });
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-cache-provenance".into(),
        "Synthetic cache provenance".into(),
        json!({"base_url":format!("http://{address}"),"api_key":"synthetic-cache-key","api_format":"chat_completions"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let proxy_state = state(db);
    let arguments = format!("{{\"phone\":\"{FIXTURE_PHONE}\"}}");
    let cached = json!({"id":"resp_synthetic_cached","output":[{"type":"function_call","call_id":"synthetic_provenance_call",
        "name":"synthetic_lookup","arguments":arguments,"reasoning_content":"synthetic-cache-only-reasoning"}]});
    assert_eq!(
        proxy_state
            .codex_chat_history
            .record_response(&cached)
            .await,
        1
    );
    let body = json!({"model":"synthetic-model","stream":false,"input":[
        {"type":"function_call","call_id":"synthetic_provenance_call","name":"synthetic_lookup","arguments":arguments},
        {"type":"function_call_output","call_id":"synthetic_provenance_call","output":"synthetic original result"}]});
    let mut legacy_probe = body.clone();
    assert_eq!(
        proxy_state
            .codex_chat_history
            .enrich_request(&mut legacy_probe)
            .await,
        1,
        "control must prove a real matching cache contribution is available"
    );
    assert!(legacy_probe
        .to_string()
        .contains("synthetic-cache-only-reasoning"));
    for negative in [
        json!({"model":"synthetic-model","stream":false,"input":[body["input"][1].clone()]}),
        json!({"model":"synthetic-model","stream":false,"input":[
            {"type":"function_call","call_id":"synthetic_provenance_call","name":"synthetic_lookup"},body["input"][1].clone()]}),
    ] {
        let response = handlers::handle_responses(
            State(proxy_state.clone()),
            http::Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .header("content-type", "application/json")
                .header("session-id", "synthetic-new-cache-generation")
                .body(Body::from(negative.to_string()))?,
        )
        .await?;
        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
        let _ = response.into_body().collect().await?;
        assert_eq!(
            seen.lock()
                .map_err(|_| "synthetic cache capture poisoned")?
                .len(),
            0
        );
    }
    let response = handlers::handle_responses(
        State(proxy_state),
        http::Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("content-type", "application/json")
            .header("session-id", "synthetic-new-cache-generation")
            .body(Body::from(body.to_string()))?,
    )
    .await?;
    assert_eq!(response.status(), http::StatusCode::OK);
    let bytes = response.into_body().collect().await?.to_bytes();
    assert!(std::str::from_utf8(&bytes)?.contains(FIXTURE_PHONE));
    let captured = seen
        .lock()
        .map_err(|_| "synthetic cache capture poisoned")?;
    assert_eq!(captured.len(), 1);
    let outbound = captured[0].to_string();
    assert!(
        !outbound.contains("synthetic-cache-only-reasoning")
            && !outbound.contains(FIXTURE_PHONE)
            && outbound.contains("rredact-cn_phone-"),
        "original complete history must not inherit cached data or completion authority"
    );
    server.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_unsupported_call_reasoning_refused() -> TestResult {
    fresh_readable_history_fixture("native", false, true).await
}

async fn fresh_history_boundary_fixture(boundary: &str) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = Router::new()
        .fallback(post(matrix_upstream))
        .with_state(MatrixFixture {
            protocol: "native",
            compact: false,
            streaming: false,
            complete: true,
            seen: seen.clone(),
        });
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-history-boundary".into(),
        "Synthetic history boundary".into(),
        json!({"base_url":format!("http://{address}"),"api_key":"synthetic-boundary-key","api_format":"responses"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let proxy = ProxyServer::new(ProxyConfig::default(), db, None)
        .with_restoration_gateway(gateway.clone());
    let mut router = proxy.build_router();
    let seed = json!({"model":"synthetic-model","stream":false,"input":[
        {"role":"user","content":format!("Synthetic seed {FIXTURE_PHONE}")} ]});
    let response = router
        .call(
            http::Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .header("content-type", "application/json")
                .header("session-id", "synthetic-boundary-session")
                .body(Body::from(seed.to_string()))?,
        )
        .await?;
    assert_eq!(response.status(), http::StatusCode::OK);
    let bytes = response.into_body().collect().await?.to_bytes();
    let completed: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(completed["id"], "resp_matrix");
    let token = {
        let captured = seen
            .lock()
            .map_err(|_| "synthetic boundary capture poisoned")?;
        assert_eq!(captured.len(), 1);
        let text = fixture_text(&captured[0]).ok_or("synthetic seed text missing")?;
        let mut token = None;
        super::tokenization::visit_token_references(text, |range| {
            token = Some(text[range].to_owned());
            Ok(())
        })?;
        token.ok_or("synthetic seed token missing")?
    };
    let body = json!({"model":"synthetic-model","stream":false,"input":[
        {"role":"user","content":format!("Synthetic new input {FIXTURE_PHONE}")},
        {"type":"function_call","call_id":"synthetic_new_boundary_call","name":"synthetic_lookup","arguments":"{}"},
        {"type":"function_call_output","call_id":"synthetic_new_boundary_call","output":"Synthetic inert result"}],
        "metadata":{"synthetic":"token-free"}});
    let mut negatives = Vec::new();
    match boundary {
        "token_key" => {
            let mut value = body.clone();
            value["metadata"]["synthetic"] = json!(token);
            negatives.push(("value", value));
            let mut call_id = body.clone();
            call_id["input"][1]["call_id"] = json!(token);
            call_id["input"][2]["call_id"] = json!(token);
            negatives.push(("call_id", call_id));
            let mut key = body.clone();
            key["metadata"] = json!({token.clone():"synthetic"});
            negatives.push(("key", key));
        }
        "escaped_argument_key" => {
            assert!(token.starts_with("rredact-cn_phone-"));
            let mut invalid = body.clone();
            invalid["input"][1]["arguments"] =
                json!(format!("{{\"\\u0072{}\":\"synthetic\"}}", &token[1..]));
            negatives.push(("escaped_unknown_argument_key", invalid.clone()));
        }
        "message_type" => {
            for invalid_type in [Value::Null, json!(42), json!({"synthetic":"message"})] {
                let mut invalid = body.clone();
                invalid["input"][0]["type"] = invalid_type;
                invalid["input"][0]["content"] = json!("Synthetic nonsensitive malformed message");
                negatives.push(("message_type", invalid));
            }
        }
        "reasoning_shape" => {
            let mut invalid = body.clone();
            invalid["input"]
                .as_array_mut()
                .ok_or("synthetic input missing")?
                .insert(
                    1,
                    json!({"type":"reasoning","summary":[{"type":"summary_text","text":42}]}),
                );
            negatives.push(("reasoning_shape", invalid));
        }
        "result_shape" => {
            let mut invalid = body.clone();
            invalid["input"][2]["output"] = json!([null]);
            negatives.push(("result_shape", invalid));
        }
        "message_shape" => {
            let mut invalid = body.clone();
            invalid["input"][0]["content"] = json!([{"type":"input_text","text":42}]);
            negatives.push(("message_shape", invalid));
        }
        "mixed_reasoning_shape" => {
            let mut invalid = body.clone();
            invalid["input"].as_array_mut().ok_or("synthetic input missing")?.insert(1,
                json!({"type":"reasoning","summary":[],"content":[{"type":"reasoning_text","text":false}]}));
            negatives.push(("mixed_reasoning_shape", invalid));
        }
        _ => return Err("unknown synthetic boundary".into()),
    }
    let mut literal_sends = 0;
    for (case_index, (case, negative)) in negatives.into_iter().enumerate() {
        let literal_case =
            boundary == "escaped_argument_key" && case == "escaped_unknown_argument_key";
        let session = if literal_case {
            "synthetic-unleased-boundary-session"
        } else {
            "synthetic-boundary-session"
        };
        if literal_case {
            assert_history_literal_has_no_authority(&gateway, &negative, session, &token)?;
        }
        let response = router
            .call(
                http::Request::builder()
                    .method("POST")
                    .uri("/v1/responses")
                    .header("content-type", "application/json")
                    .header("session-id", session)
                    .body(Body::from(negative.to_string()))?,
            )
            .await?;
        let status = response.status();
        let _ = response.into_body().collect().await?;
        literal_sends += usize::from(literal_case);
        let captured = seen
            .lock()
            .map_err(|_| "synthetic boundary capture poisoned")?;
        let expected_status = if literal_case {
            http::StatusCode::OK
        } else {
            http::StatusCode::BAD_REQUEST
        };
        assert!(
            status == expected_status && captured.len() == 1 + literal_sends,
            "history authority boundary; case_index={case_index} case={case} status={status}"
        );
        if literal_case {
            assert!(
                captured.last().ok_or("synthetic escaped send missing")?["input"][1]["arguments"]
                    == negative["input"][1]["arguments"],
                "escaped unknown argument key must preserve encoded bytes"
            );
        }
    }
    if boundary == "escaped_argument_key" {
        use super::tokenization::{DlpTokenStore, TokenCategory, TokenFragment};
        let mut authorized = DlpTokenStore::new().admit("codex", None, 1)?;
        let reference = authorized.prepare(
            &[TokenFragment {
                raw: "synthetic-authorized-key-secret",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            |tokens| Ok(tokens[0].clone()),
        )?;
        let mut unsupported = body.clone();
        unsupported["input"][1]["arguments"] =
            json!(format!("{{\"\\u0072{}\":\"synthetic\"}}", &reference[1..]));
        assert!(
            super::request_preparation::prepare_codex_request_with_budget(
                &mut authorized,
                &unsupported,
                &Default::default(),
                &[],
                &[],
                super::payload_budget::PayloadBudget::default(),
                |_, _, _| Ok(()),
            )
            .is_err(),
            "actual authorized reference in an immutable decoded key must be refused"
        );
    }
    let response = router
        .call(
            http::Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .header("content-type", "application/json")
                .header("session-id", "synthetic-boundary-session")
                .body(Body::from(body.to_string()))?,
        )
        .await?;
    assert_eq!(response.status(), http::StatusCode::OK);
    let _ = response.into_body().collect().await?;
    assert_eq!(
        seen.lock()
            .map_err(|_| "synthetic boundary capture poisoned")?
            .len(),
        2 + literal_sends
    );
    server.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_old_token_object_key_refused() -> TestResult {
    fresh_history_boundary_fixture("token_key").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_malformed_reasoning_part_refused() -> TestResult {
    fresh_history_boundary_fixture("reasoning_shape").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_malformed_result_part_refused() -> TestResult {
    fresh_history_boundary_fixture("result_shape").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_malformed_message_part_refused() -> TestResult {
    fresh_history_boundary_fixture("message_shape").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_mixed_reasoning_shape_refused() -> TestResult {
    fresh_history_boundary_fixture("mixed_reasoning_shape").await
}

closure_snapshot_test!(
    reversible_closure_resp24_item_identity_before,
    "matrix_item_identity_before"
);

closure_snapshot_test!(
    reversible_closure_resp24_item_identity_after,
    "matrix_item_identity_after"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_kind_before,
    "matrix_part_kind_before"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_kind_after,
    "matrix_part_kind_after"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_omitted_before,
    "matrix_part_omitted_before"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_omitted_after,
    "matrix_part_omitted_after"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_empty_done_before,
    "matrix_part_empty_done_before"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_empty_done_after,
    "matrix_part_empty_done_after"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_empty_final_before,
    "matrix_part_empty_final_before"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_empty_final_after,
    "matrix_part_empty_final_after"
);

closure_snapshot_test!(
    reversible_closure_resp24_call_identity_before,
    "matrix_call_identity_before"
);

closure_snapshot_test!(
    reversible_closure_resp24_call_identity_after,
    "matrix_call_identity_after"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_empty_added_before,
    "matrix_ok_part_empty_added_before"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_empty_added_after,
    "matrix_ok_part_empty_added_after"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_empty_equal_before,
    "matrix_ok_part_empty_equal_before"
);

closure_snapshot_test!(
    reversible_closure_resp24_part_empty_equal_after,
    "matrix_ok_part_empty_equal_after"
);

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_escaped_argument_key_reference_refused() -> TestResult {
    fresh_history_boundary_fixture("escaped_argument_key").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req3_present_nonstring_message_type_refused() -> TestResult {
    fresh_history_boundary_fixture("message_type").await
}

struct LoopbackCodexAdapter(std::net::SocketAddr);
impl crate::proxy::providers::ProviderAdapter for LoopbackCodexAdapter {
    fn name(&self) -> &'static str {
        crate::proxy::providers::CodexAdapter::new().name()
    }
    fn extract_base_url(&self, _: &Provider) -> Result<String, crate::proxy::ProxyError> {
        assert!(self.0.ip().is_loopback());
        Ok(format!("http://{}", self.0))
    }
    fn extract_auth(&self, provider: &Provider) -> Option<crate::proxy::providers::AuthInfo> {
        crate::proxy::providers::CodexAdapter::new().extract_auth(provider)
    }
    fn build_url(&self, base: &str, endpoint: &str) -> String {
        crate::proxy::providers::CodexAdapter::new().build_url(base, endpoint)
    }
    fn get_auth_headers(
        &self,
        auth: &crate::proxy::providers::AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, crate::proxy::ProxyError> {
        crate::proxy::providers::CodexAdapter::new().get_auth_headers(auth)
    }
}

async fn official_duplicate_authorization_fixture(
    initial_compact: bool,
    next_compact: bool,
    derived_key_change: bool,
) -> TestResult {
    let _ = rustls::crypto::ring::default_provider().install_default();
    use super::http_restoration::{admit_http_request, RestorationAdmission};
    use crate::{app_config::AppType, proxy::forwarder::RequestForwarder};
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let vectors_match = Arc::new(Mutex::new(Vec::<bool>::new()));
    let capture = seen.clone();
    let vectors = vectors_match.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let adapter = LoopbackCodexAdapter(listener.local_addr()?);
    let upstream = Router::new().fallback(post(move |uri: http::Uri, headers: http::HeaderMap, Json(body): Json<Value>| {
        let capture = capture.clone(); let vectors = vectors.clone();
        async move {
            let values: Vec<_> = headers.get_all(http::header::AUTHORIZATION).iter().map(|v| v.as_bytes()).collect();
            let matches = values == [b"Bearer synthetic-first".as_slice(), b"Bearer synthetic-second".as_slice()];
            let matches = if derived_key_change {
                values == [b"Bearer synthetic-derived-a".as_slice(), b"Bearer synthetic-incoming-a".as_slice(), b"Bearer synthetic-incoming-b".as_slice()]
            } else { matches };
            vectors.lock().expect("synthetic vector capture").push(matches);
            let text = fixture_text(&body).unwrap_or("Synthetic inert response").to_owned();
            let number = {
                let mut seen = capture.lock().expect("synthetic vector capture");
                seen.push(body); seen.len()
            };
            let mut response = json!({"id":format!("resp_vector_{number}"),"object":"response","status":"completed",
                "model":"synthetic-model","output":[{"type":"message","id":format!("msg_vector_{number}"),
                    "role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]}]});
            if uri.path().ends_with("/compact") {
                response["object"] = json!("response.compaction"); response["created_at"] = json!(1);
                response["output"].as_array_mut().expect("synthetic output").push(
                    json!({"id":format!("cmp_vector_{number}"),"type":"compaction","encrypted_content":"synthetic-vector-opaque"}));
            }
            Json(response).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let proxy_state = state(Arc::new(Database::memory()?));
    let mut provider = Provider::with_id(
        crate::database::CODEX_OFFICIAL_PROVIDER_ID.into(),
        "Synthetic official passthrough".into(),
        json!({"api_format":"responses"}),
        None,
    );
    provider.category = Some("official".into());
    assert!(crate::proxy::providers::is_codex_official_provider(
        &provider
    ));
    assert!(provider
        .meta
        .as_ref()
        .is_none_or(|meta| meta.managed_account_id_for("codex_oauth").is_none()));
    for stage in 0..3 {
        let compact = if stage == 0 {
            initial_compact
        } else {
            next_compact
        };
        if derived_key_change {
            provider.settings_config["api_key"] = json!(if stage == 2 {
                "synthetic-derived-b"
            } else {
                "synthetic-derived-a"
            });
        }
        let mut body = if stage == 0 {
            json!({"model":"synthetic-model","stream":false,"input":FIXTURE_PHONE})
        } else {
            json!({"model":"synthetic-model","stream":false,"input":"Synthetic inert continuation","previous_response_id":"resp_vector_1"})
        };
        let mut headers = http::HeaderMap::new();
        headers.insert("content-type", "application/json".parse()?);
        headers.insert("session-id", "synthetic-auth-vector-session".parse()?);
        headers.insert("chatgpt-account-id", "synthetic-account".parse()?);
        headers.append(
            http::header::AUTHORIZATION,
            "Bearer synthetic-first".parse()?,
        );
        headers.append(
            http::header::AUTHORIZATION,
            if stage == 2 {
                "Bearer synthetic-changed"
            } else {
                "Bearer synthetic-second"
            }
            .parse()?,
        );
        if derived_key_change {
            headers.remove(http::header::AUTHORIZATION);
            headers.insert("x-api-key", "synthetic-discarded-key".parse()?);
            headers.append(
                http::header::AUTHORIZATION,
                "Bearer synthetic-incoming-a".parse()?,
            );
            headers.append(
                http::header::AUTHORIZATION,
                "Bearer synthetic-incoming-b".parse()?,
            );
            assert_eq!(
                headers
                    .iter()
                    .find(|(name, _)| matches!(
                        name.as_str(),
                        "authorization" | "x-api-key" | "x-goog-api-key"
                    ))
                    .map(|(name, _)| name.as_str()),
                Some("x-api-key"),
                "fixture must reach the derived-then-incoming authentication boundary"
            );
        }
        let request = match admit_http_request(
            &proxy_state.restoration_gateway,
            &AppType::Codex,
            true,
            compact,
            &mut body,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic vector request not admitted".into()),
        };
        let forwarder = RequestForwarder::new(
            proxy_state.provider_router.clone(),
            5,
            proxy_state.status.clone(),
            proxy_state.current_providers.clone(),
            proxy_state.gemini_shadow.clone(),
            proxy_state.codex_chat_history.clone(),
            proxy_state.failover_manager.clone(),
            None,
            provider.id.clone(),
            "synthetic-auth-vector-session".into(),
            true,
            5,
            5,
            Default::default(),
            Default::default(),
            Default::default(),
            0,
        )
        .with_dlp_request(Some(request));
        let result = forwarder
            .forward_with_adapter_for_test(
                &provider,
                if compact {
                    "/responses/compact"
                } else {
                    "/responses"
                },
                &body,
                &headers,
                &adapter,
            )
            .await;
        if stage == 2 {
            let status = match result {
                Ok(result) => {
                    let _ = result.response.bytes_with_limit(1024 * 1024).await?;
                    http::StatusCode::OK
                }
                Err(error) => error.into_response().status(),
            };
            let sends = seen
                .lock()
                .map_err(|_| "synthetic vector capture poisoned")?
                .len();
            if sends == 3 {
                assert!(
                    !vectors_match
                        .lock()
                        .map_err(|_| "synthetic vector capture poisoned")?[2],
                    "the changed vector must reach the real listener before counting a binding RED"
                );
            }
            assert!(status == http::StatusCode::BAD_REQUEST && sends == 2,
                "changed actual credential vector must refuse historical continuation before another send; derived={derived_key_change} status={status} sends={sends}");
        } else {
            let mut result = result.map_err(|_| "synthetic unchanged vector send refused")?;
            let attempt = result
                .dlp_attempt
                .take()
                .ok_or("synthetic vector attempt missing")?;
            let bytes = result.response.bytes_with_limit(1024 * 1024).await?;
            let response = http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(Body::from(bytes))?;
            let response = attempt
                .restore_client_http_response(
                    response,
                    super::response_restoration::ArgumentNormalizationMode::Exact,
                    None,
                )
                .await;
            assert_eq!(
                response.status(),
                http::StatusCode::OK,
                "unchanged actual vector must retain compatibility"
            );
            let bytes = response.into_body().collect().await?.to_bytes();
            let completed: Value = serde_json::from_slice(&bytes)?;
            assert_eq!(completed["id"], format!("resp_vector_{}", stage + 1));
            if stage == 0 {
                assert!(std::str::from_utf8(&bytes)?.contains(FIXTURE_PHONE));
            }
            let vectors = vectors_match
                .lock()
                .map_err(|_| "synthetic vector capture poisoned")?;
            assert!(
                vectors.len() == stage + 1 && vectors.iter().all(|matches| *matches),
                "actual listener must receive both original Authorization values unchanged"
            );
        }
    }
    server.abort();
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_duplicate_authorization_same_operation() -> TestResult {
    official_duplicate_authorization_fixture(false, false, false).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_duplicate_authorization_cross_operation() -> TestResult {
    official_duplicate_authorization_fixture(false, true, false).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_duplicate_authorization_reverse_operation() -> TestResult {
    official_duplicate_authorization_fixture(true, false, false).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_derived_authorization_same_operation() -> TestResult {
    official_duplicate_authorization_fixture(false, false, true).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_derived_authorization_cross_operation() -> TestResult {
    official_duplicate_authorization_fixture(false, true, true).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_req2_derived_authorization_reverse_operation() -> TestResult {
    official_duplicate_authorization_fixture(true, false, true).await
}

async fn core_failure_http_fixture(mode: &'static str) -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        tokenization::{DlpTokenStore, TokenError, TokenStoreLimits},
    };
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    let mut config = runtime::SecurityRuntimeConfig::default();
    if mode == "redaction" {
        config.custom_rules.push(runtime::CompiledCustomRule {
            id: "synthetic-missing-keyword".into(),
            name: "Synthetic missing snapshot".into(),
            rule_type: "keyword".into(),
            pattern: "synthetic-no-match".into(),
            regex: None,
            risk_level: super::types::RiskLevel::Medium,
            action: super::types::AuditAction::Redact,
            enabled: true,
        });
    }
    runtime::set_security_runtime_for_test(config);
    let store = match mode {
        "collision" => DlpTokenStore::with_test_digest([0x31; 10]),
        "lease_capacity" => DlpTokenStore::with_test_limits(TokenStoreLimits {
            namespaces: 0,
            ..Default::default()
        }),
        "mapping_capacity" => DlpTokenStore::with_test_limits(TokenStoreLimits {
            entries_per_namespace: 0,
            ..Default::default()
        }),
        _ => DlpTokenStore::new(),
    };
    let sends = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured = sends.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let app = Router::new().fallback(post(move || {
        let captured = captured.clone();
        async move {
            captured.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Json(json!({"id":"resp_core_unexpected","status":"completed","output":[]}))
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "closure-core-failure".into(),
        "Synthetic core failure".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-core-credential","api_format":"responses"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let mut proxy_state = state(db);
    proxy_state.restoration_gateway = RestorationGateway::with_test_store_and_rules(
        store.clone(),
        &["chinese_phone_number"],
        PayloadBudget::with_limit(64 * 1024 * 1024),
    );
    if mode == "mapping_reserve" {
        store.fail_next_mapping_reserve_for_test();
    }
    let _core_fault = (mode == "entropy").then(|| {
        super::tokenization::identity_test_support::install(|phase, unlocked| {
            assert!(unlocked, "core entropy must remain outside the store lock");
            if phase == super::tokenization::identity_test_support::Phase::Entropy {
                Err(TokenError::Unavailable)
            } else {
                Ok(())
            }
        })
    });
    let _audit_fault = super::identity::test_support::fail_audit_fingerprint();
    let input = match mode {
        "collision" => json!("13800138000 13900139000"),
        "unsupported" => {
            json!([{"type":"function_call","call_id":"call_core","name":"synthetic_lookup","arguments":"{}","reasoning_content":FIXTURE_PHONE},
            {"type":"function_call_output","call_id":"call_core","output":"Synthetic inert result"}])
        }
        _ => json!(FIXTURE_PHONE),
    };
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-core-session")
        .body(Body::from(
            json!({"model":"synthetic-model","stream":false,"input":input}).to_string(),
        ))?;
    let response = match handlers::handle_responses(State(proxy_state), request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    let status = response.status();
    let wire = response.into_body().collect().await?.to_bytes();
    let value: Value = serde_json::from_slice(&wire)?;
    server.abort();
    assert_eq!(
        sends.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "core failure must precede all actual HTTP sends; case={mode}"
    );
    let expected = match mode {
        "entropy" => "dlp_unavailable",
        "collision" => "dlp_token_collision",
        "redaction" => "dlp_redaction_failed",
        "unsupported" => "dlp_restore_unsupported",
        _ => "dlp_capacity_exceeded",
    };
    assert!(
        status.is_client_error() || status.is_server_error(),
        "core failure cannot succeed; case={mode}"
    );
    assert!(
        value.pointer("/error/code").and_then(Value::as_str) == Some(expected),
        "fixed core failure code must survive auxiliary degradation; case={mode}"
    );
    let text = std::str::from_utf8(&wire)?;
    assert!(
        !text.contains(FIXTURE_PHONE)
            && !text.contains("rredact-cn_phone-")
            && !text.contains("synthetic-core-credential")
    );
    assert_eq!(
        store.stats()?.entries,
        0,
        "failed core preparation must not commit a mapping"
    );
    Ok(())
}
macro_rules! core_failure_http_test {
    ($name:ident, $mode:literal) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            core_failure_http_fixture($mode).await
        }
    };
}
core_failure_http_test!(reversible_closure_req4_core_entropy_zero_send, "entropy");
core_failure_http_test!(
    reversible_closure_req4_core_lease_capacity_zero_send,
    "lease_capacity"
);
core_failure_http_test!(
    reversible_closure_req4_core_mapping_capacity_zero_send,
    "mapping_capacity"
);
core_failure_http_test!(
    reversible_closure_req4_core_mapping_reserve_zero_send,
    "mapping_reserve"
);
core_failure_http_test!(
    reversible_closure_req4_core_collision_zero_send,
    "collision"
);
core_failure_http_test!(
    reversible_closure_req4_core_redaction_zero_send,
    "redaction"
);
core_failure_http_test!(
    reversible_closure_req4_core_unsupported_carrier_zero_send,
    "unsupported"
);

// Exhaust the shared payload budget only after a real upstream has sent a tool
// prefix. This proves client buffering behavior, not dependency allocation bounds.
async fn run_closure_client_budget(xai: bool, exhaust: bool) -> TestResult {
    use super::{payload_budget::PayloadBudget, tokenization::DlpTokenStore};
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let prefix_sent = Arc::new(tokio::sync::Notify::new());
    let release_tail = Arc::new(tokio::sync::Notify::new());
    let capture = seen.clone();
    let reached = prefix_sent.clone();
    let release = release_tail.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let app = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let capture = capture.clone();
        let reached = reached.clone();
        let release = release.clone();
        async move {
            capture.lock().expect("synthetic budget capture").push(body.clone());
            if body.get("previous_response_id").is_some() {
                return Json(json!({"id":"resp_client_budget_followup","object":"response","status":"completed",
                    "output":[{"type":"message","id":"msg_client_budget_followup","role":"assistant","status":"completed",
                        "content":[{"type":"output_text","text":"synthetic continuation","annotations":[]}]}]})).into_response();
            }
            let arguments = json!({"phone":fixture_text(&body).expect("synthetic tokenized carrier")}).to_string();
            let item = json!({"type":"function_call","id":"fc_client_budget","call_id":"call_client_budget",
                "name":"synthetic_lookup","arguments":arguments,"status":"completed"});
            let events = [
                json!({"type":"response.output_item.added","output_index":0,"item":{
                    "type":"function_call","id":"fc_client_budget","call_id":"call_client_budget",
                    "name":"synthetic_lookup","arguments":"","status":"in_progress"}}),
                json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_client_budget","delta":arguments}),
                json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_client_budget","arguments":arguments}),
                json!({"type":"response.output_item.done","output_index":0,"item":item}),
            ];
            let prefix: String = events.iter().map(|event| format!("event: {}\ndata: {event}\n\n", event["type"].as_str().expect("synthetic event"))).collect();
            let tail = format!("event: response.completed\ndata: {}\n\n", json!({"type":"response.completed",
                "response":{"id":"resp_client_budget","object":"response","status":"completed","output":[item]}}));
            let stream = async_stream::stream! {
                yield Ok::<_, std::io::Error>(bytes::Bytes::from(prefix));
                reached.notify_one();
                release.notified().await;
                yield Ok(bytes::Bytes::from(tail));
            };
            ([("content-type", "text/event-stream")], Body::from_stream(stream)).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let db = Arc::new(Database::memory()?);
    let settings = if xai {
        json!({"auth":{"OPENAI_API_KEY":"synthetic-client-budget-credential"},"config":format!(
            "model_provider = \"custom\"\n[model_providers.custom]\nname = \"xai\"\nbase_url = \"http://{addr}/api.x.ai/v1\"\nwire_api = \"responses\"\n")})
    } else {
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-client-budget-credential"})
    };
    let provider = Provider::with_id(
        "closure-client-budget".into(),
        "Synthetic client budget".into(),
        settings,
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let mut proxy_state = state(db);
    proxy_state.restoration_gateway = RestorationGateway::with_test_store_and_rules(
        DlpTokenStore::new(),
        &["chinese_phone_number"],
        budget.clone(),
    );
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-client-budget-session")
        .body(Body::from(
            json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":true,
            "tools":[{"type":"function","name":"synthetic_lookup","parameters":{"type":"object"}}]})
            .to_string(),
        ))?;
    let response = handlers::handle_responses(State(proxy_state.clone()), request).await?;
    assert_eq!(response.status(), http::StatusCode::OK);
    let mut collecting = Box::pin(response.into_body().collect());
    tokio::select! {
        result = &mut collecting => { let _ = result; panic!("tool prefix cannot finish before original terminal"); }
        _ = prefix_sent.notified() => {}
        _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => panic!("synthetic tool prefix was not sent"),
    }
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut collecting)
            .await
            .is_err(),
        "completed tool item must remain buffered before terminal and EOF"
    );
    let held = if exhaust {
        Some(budget.reserve(budget.limit() - budget.used())?)
    } else {
        None
    };
    release_tail.notify_one();
    let bytes = tokio::time::timeout(std::time::Duration::from_secs(10), collecting)
        .await??
        .to_bytes();
    drop(held);
    let text = std::str::from_utf8(&bytes)?;
    let mut recorder = FinalClientRecorder::default();
    recorder.observe_sse(text)?;
    assert_eq!(
        recorder.submissions.len(),
        usize::from(!exhaust),
        "response-stage budget failure discards every buffered tool; xai={xai} exhaust={exhaust}"
    );
    assert_eq!(recorder.completed, !exhaust);
    assert_eq!(recorder.failed, exhaust);
    assert!(!text.contains("rredact-cn_phone-"));
    if !exhaust {
        let arguments: Value = serde_json::from_str(
            recorder.submissions[0]["arguments"]
                .as_str()
                .ok_or("missing synthetic arguments")?,
        )?;
        assert!(
            arguments["phone"] == FIXTURE_PHONE,
            "recorded tool restores the synthetic phone"
        );
    } else {
        assert!(!text.contains(FIXTURE_PHONE));
    }
    {
        let captured = seen
            .lock()
            .map_err(|_| "synthetic budget capture poisoned")?;
        assert_eq!(
            captured.len(),
            1,
            "response budget fails after exactly one actual send"
        );
        assert!(captured[0].to_string().contains("rredact-cn_phone-"));
        assert!(!captured[0].to_string().contains(FIXTURE_PHONE));
    }
    let followup = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-client-budget-session")
        .body(Body::from(
            json!({"model":"synthetic-model","input":"synthetic continuation","stream":false,
            "previous_response_id":"resp_client_budget"})
            .to_string(),
        ))?;
    let followup = match handlers::handle_responses(State(proxy_state), followup).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    let followup_status = followup.status();
    let _ = followup.into_body().collect().await?;
    server.abort();
    assert_eq!(
        followup_status,
        if exhaust {
            http::StatusCode::BAD_REQUEST
        } else {
            http::StatusCode::OK
        }
    );
    assert_eq!(
        seen.lock()
            .map_err(|_| "synthetic budget capture poisoned")?
            .len(),
        if exhaust { 1 } else { 2 },
        "failed buffered response publishes no reusable context even after budget recovery"
    );
    assert_eq!(
        budget.used(),
        0,
        "response and cancellation owners release their own payload charges"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_client_native_budget_failure() -> TestResult {
    run_closure_client_budget(false, true).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_client_native_budget_success_control() -> TestResult {
    run_closure_client_budget(false, false).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_client_xai_budget_failure() -> TestResult {
    run_closure_client_budget(true, true).await
}
#[tokio::test]
#[serial_test::serial]
async fn reversible_closure_client_xai_budget_success_control() -> TestResult {
    run_closure_client_budget(true, false).await
}

// These are nonexecuting semantic counterexamples, not model/task acceptance.
// Usage and observation projections below are explicit mocks of side consumers;
// the request audit and network send use the real forwarder boundary.
async fn run_closure_task_counterexample(mode: &'static str) -> TestResult {
    use super::{
        http_restoration::{admit_http_request, RestorationAdmission},
        runtime::CompiledCustomRule,
    };
    use crate::proxy::{
        forwarder::RequestForwarder,
        tool_observation::{
            parsers::{parse_response, ParseContext},
            types::{ExecutionScope, ObservationStage},
        },
        usage::parser::TokenUsage,
    };
    const FIRST: &str = "synthetic'\"\\\nprivate-first";
    const SECOND: &str = "synthetic'\"\\\nprivate-second";
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    let pattern = format!("(?:{}|{})", regex::escape(FIRST), regex::escape(SECOND));
    let mut config = runtime::SecurityRuntimeConfig::default();
    config.custom_rules.push(CompiledCustomRule {
        id: "synthetic-task-secret".into(),
        name: "Synthetic task secret".into(),
        rule_type: "regex".into(),
        regex: Some(regex::Regex::new(&pattern)?),
        pattern,
        risk_level: super::RiskLevel::High,
        action: super::AuditAction::RedactRestore,
        enabled: true,
    });
    runtime::set_security_runtime_for_test(config);
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let capture = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let app = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let capture = capture.clone();
        async move {
            capture.lock().expect("synthetic task capture").push(body.clone());
            let source = body["input"].as_str().expect("synthetic task input");
            let (first, second) = source.strip_prefix("first=").and_then(|value| value.split_once("; second="))
                .expect("synthetic task tokens");
            assert!(first.starts_with("rredact-") && second.starts_with("rredact-") && first != second);
            let value = match mode {
                "literal" => first.to_owned(),
                "shell_quote" => format!("printf '%s' '{first}'"),
                "nested_json" => format!("{{\"value\":\"{first}\"}}"),
                "patch" => format!("@@ -1 +1 @@\n-old\n+{first}\n"),
                "partial" => first[..20].to_owned(),
                "encoded" => first.as_bytes().iter().map(|byte| format!("%{byte:02X}")).collect(),
                "missing" => "synthetic omitted value".to_owned(),
                "wrong_valid" => second.to_owned(),
                _ => panic!("unknown synthetic task case"),
            };
            let arguments = json!({"value":value}).to_string();
            let item = json!({"type":"function_call","id":"fc_task_counterexample","call_id":"call_task_counterexample",
                "name":"synthetic_nonexecuting_tool","arguments":arguments,"status":"completed"});
            let events = [
                json!({"type":"response.output_item.added","output_index":0,"item":{
                    "type":"function_call","id":"fc_task_counterexample","call_id":"call_task_counterexample",
                    "name":"synthetic_nonexecuting_tool","arguments":"","status":"in_progress"}}),
                json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_task_counterexample","delta":arguments}),
                json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_task_counterexample","arguments":arguments}),
                json!({"type":"response.output_item.done","output_index":0,"item":item}),
                json!({"type":"response.completed","response":{"id":"resp_task_counterexample","object":"response","status":"completed",
                    "model":"synthetic-model","output":[item],"usage":{"input_tokens":2,"output_tokens":3}}}),
            ];
            let wire: String = events.iter().map(|event| format!("event: {}\ndata: {event}\n\n", event["type"].as_str().expect("synthetic event"))).collect();
            ([("content-type", "text/event-stream")], wire).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let db = Arc::new(Database::memory()?);
    let proxy_state = state(db);
    let provider = Provider::with_id(
        "closure-task-counterexample".into(),
        "Synthetic task counterexample".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-task-credential"}),
        None,
    );
    let mut body = json!({"model":"synthetic-model","input":format!("first={FIRST}; second={SECOND}"),"stream":true});
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", "synthetic-task-session".parse()?);
    let request = match admit_http_request(
        &proxy_state.restoration_gateway,
        &crate::app_config::AppType::Codex,
        true,
        false,
        &mut body,
        &headers,
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic task request not admitted".into()),
    };
    let audit = request.audit_observer_for_test();
    assert!(audit().is_empty());
    let forwarder = RequestForwarder::new(
        proxy_state.provider_router.clone(),
        5,
        proxy_state.status.clone(),
        proxy_state.current_providers.clone(),
        proxy_state.gemini_shadow.clone(),
        proxy_state.codex_chat_history.clone(),
        proxy_state.failover_manager.clone(),
        None,
        provider.id.clone(),
        "synthetic-task-session".into(),
        true,
        5,
        5,
        Default::default(),
        Default::default(),
        Default::default(),
        0,
    )
    .with_dlp_request(Some(request));
    let mut result = forwarder
        .forward_with_retry(
            &crate::app_config::AppType::Codex,
            http::Method::POST,
            "/v1/responses",
            body,
            headers,
            Default::default(),
            vec![provider],
        )
        .await
        .map_err(|_| "synthetic task actual send failed")?;
    let attempt = result
        .dlp_attempt
        .take()
        .ok_or("missing synthetic task attempt")?;
    let upstream = result
        .response
        .bytes_with_limit(1024 * 1024)
        .await
        .map_err(|_| "synthetic task body failed")?;
    let prepared = http::Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(upstream))?;
    let partial = {
        let captured = seen.lock().map_err(|_| "synthetic task capture poisoned")?;
        let first = captured[0]["input"]
            .as_str()
            .and_then(|text| text.strip_prefix("first="))
            .and_then(|text| text.split_once("; second="))
            .map(|(first, _)| first)
            .ok_or("synthetic task reference missing")?;
        first[..20].to_owned()
    };
    if mode == "partial" {
        assert!(
            attempt.lease.resolve_if_authorized(&partial)?.is_none(),
            "partial reference must not resolve an authorized full mapping"
        );
        assert_history_literal_has_no_authority(
            &proxy_state.restoration_gateway,
            &json!({"input":partial}),
            "synthetic-partial-literal-proof",
            &partial,
        )?;
    }
    let response = attempt
        .restore_client_http_response(
            prepared,
            super::response_restoration::ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    let wire = std::str::from_utf8(&bytes)?;
    let mut recorder = FinalClientRecorder::default();
    recorder.observe_sse(wire)?;
    let protocol_complete = true;
    assert_eq!(
        recorder.completed, protocol_complete,
        "protocol outcome only; case={mode}"
    );
    assert_eq!(recorder.failed, !protocol_complete);
    assert_eq!(recorder.submissions.len(), usize::from(protocol_complete));
    let mock_cache = CodexChatHistoryStore::default();
    let mut mock_usage = Vec::new();
    let mut mock_observations = Vec::new();
    let mut task_correct = false;
    if protocol_complete {
        let item = &recorder.submissions[0];
        let arguments: Value = serde_json::from_str(
            item["arguments"]
                .as_str()
                .ok_or("missing recorded task arguments")?,
        )?;
        let value = arguments["value"]
            .as_str()
            .ok_or("missing recorded task value")?;
        if mode == "partial" {
            assert!(
                value == partial
                    && !value.contains("private-first")
                    && !value.contains("private-second"),
                "partial literal must retain exact bytes without restoring either mapping"
            );
        }
        let expected = match mode {
            "shell_quote" => format!("printf '%s' '{}'", FIRST.replace('\'', "'\\''")),
            "nested_json" => json!({"value":FIRST}).to_string(),
            "patch" => format!("@@ -1 +1 @@\n-old\n+{}\n", FIRST.replace('\n', "\\n")),
            _ => FIRST.to_owned(),
        };
        task_correct = value == expected;
        if mode == "nested_json" {
            assert!(
                serde_json::from_str::<Value>(value).is_err(),
                "outer JSON cannot validate the embedded JSON string"
            );
        }
        if mode == "patch" {
            assert!(
                value.lines().count() > 3,
                "literal restoration changes patch line structure"
            );
        }
        if mode == "wrong_valid" {
            assert!(
                value == SECOND,
                "authorized but semantically wrong token restores the other mapping"
            );
        }
        if matches!(mode, "literal" | "shell_quote" | "nested_json" | "patch") {
            assert!(
                value.contains(FIRST),
                "exact insertion is not shell/JSON/patch escaping"
            );
        }
        let completed: Value = wire
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .find(|event| event["type"] == "response.completed")
            .ok_or("missing synthetic task completion")?["response"]
            .clone();
        assert_eq!(mock_cache.record_response(&completed).await, 1);
        let usage = TokenUsage::from_codex_response_auto(&completed)
            .ok_or("missing synthetic task usage")?;
        assert_eq!((usage.input_tokens, usage.output_tokens), (2, 3));
        mock_usage.push(serde_json::to_value(usage)?);
        let parsed = parse_response(
            "openai_responses",
            &completed,
            &ParseContext {
                scope_id: "synthetic-task-scope".into(),
                client: Some("codex".into()),
                execution_scope: ExecutionScope::Client,
            },
        );
        assert!(parsed.recognized && parsed.observations.len() == 1);
        assert_eq!(
            parsed.observations[0].stage,
            ObservationStage::Proposed,
            "completion observation is a proposal, never execution authorization"
        );
        assert!(parsed.observations[0].validate().is_ok());
        let exposes_custom_plaintext = matches!(
            mode,
            "literal" | "shell_quote" | "nested_json" | "patch" | "wrong_valid"
        );
        let observation_input = parsed.observations[0]
            .input
            .value
            .as_ref()
            .ok_or("missing proposed input")?
            .to_string();
        assert_eq!(
            observation_input.contains("private-first")
                || observation_input.contains("private-second"),
            exposes_custom_plaintext,
            "in-memory proposed observations are raw, not persistence-safe; case={mode}"
        );
        // Inspect the existing summary projection in memory without persisting it.
        let summary = crate::services::tool_risk::sanitize::summarize_command(value);
        assert_eq!(summary.contains("private-first") || summary.contains("private-second"), exposes_custom_plaintext,
            "custom DLP patterns are not covered by the independent built-in summary sanitizer; case={mode}");
        mock_observations.push(summary);
    }
    assert_eq!(
        task_correct,
        mode == "literal",
        "protocol completion does not imply task correctness; case={mode}"
    );
    assert_eq!(mock_usage.len(), usize::from(protocol_complete));
    assert_eq!(mock_observations.len(), usize::from(protocol_complete));
    let events = audit();
    assert_eq!(
        events.len(),
        2,
        "the two request findings are independent of response/task success"
    );
    for event in events {
        assert_eq!(event.log.action_taken, "redact_restore");
        let safe = serde_json::to_string(&event.log)?;
        assert!(
            !safe.contains("private-first")
                && !safe.contains("private-second")
                && !safe.contains("rredact-")
        );
    }
    let safe_usage = serde_json::to_string(&mock_usage)?;
    assert!(
        !safe_usage.contains("private-first")
            && !safe_usage.contains("private-second")
            && !safe_usage.contains("rredact-")
    );
    // Custom DLP rules are not a guarantee of arbitrary tool-summary sanitization.
    // The fixture retains summaries only in local memory, never logs or persists them.
    let capture = seen.lock().map_err(|_| "synthetic task capture poisoned")?;
    assert_eq!(capture.len(), 1);
    let sent = capture[0].to_string();
    assert!(
        !sent.contains("private-first")
            && !sent.contains("private-second")
            && sent.contains("rredact-")
    );
    if !protocol_complete {
        assert!(
            wire.contains("dlp_restore_failed")
                && !wire.contains("private-first")
                && !wire.contains("private-second")
        );
    }
    // Exercise mock cache lookup without printing its payload. A completed but wrong
    // task can still enter history; no incomplete/failed response enters this mock.
    let mut probe = json!({"previous_response_id":"resp_task_counterexample","input":[{
        "type":"function_call_output","call_id":"call_task_counterexample","output":"synthetic inert result"}]});
    let enriched = mock_cache.enrich_request(&mut probe).await;
    assert_eq!(enriched > 0, protocol_complete);
    let cached = probe.to_string();
    assert_eq!(cached.contains("private-first") || cached.contains("private-second"),
        matches!(mode, "literal" | "shell_quote" | "nested_json" | "patch" | "wrong_valid"),
        "the local mock cache retains completed input regardless of semantic task outcome; case={mode}");
    Ok(())
}

macro_rules! closure_task_test {
    ($name:ident, $mode:literal) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            run_closure_task_counterexample($mode).await
        }
    };
}
closure_task_test!(reversible_closure_task_literal_control, "literal");
closure_task_test!(
    reversible_closure_task_shell_quote_counterexample,
    "shell_quote"
);
closure_task_test!(
    reversible_closure_task_nested_json_counterexample,
    "nested_json"
);
closure_task_test!(reversible_closure_task_patch_counterexample, "patch");
closure_task_test!(reversible_closure_task_partial_reference_refused, "partial");
closure_task_test!(
    reversible_closure_task_encoded_reference_counterexample,
    "encoded"
);
closure_task_test!(
    reversible_closure_task_missing_reference_counterexample,
    "missing"
);
closure_task_test!(
    reversible_closure_task_wrong_valid_reference_counterexample,
    "wrong_valid"
);

// Actual HTTP fixture for the independent native xAI JSON wire allocation.
async fn run_reversible_wire_xai_json(mode: &'static str) -> TestResult {
    use super::{payload_budget::PayloadBudget, tokenization::DlpTokenStore};
    use handlers::xai_json_wire_test_support::{self, Event};
    #[derive(Default)]
    struct BoundaryState {
        entered: usize,
        active: bool,
        full: bool,
        encoded: usize,
        released: usize,
        recovered: bool,
        before: usize,
        charge: usize,
        retained: Vec<bytes::Bytes>,
    }
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    let mut config = runtime::SecurityRuntimeConfig::default();
    if mode == "legacy" {
        config
            .builtin_action_overrides
            .insert("chinese_phone_number".into(), super::AuditAction::Redact);
    }
    runtime::set_security_runtime_for_test(config);
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    assert!(
        crate::proxy::forwarder::final_send_binding_test_support::is_active(),
        "xAI JSON fixture requires explicit no-proxy transport before network setup"
    );
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(6);
    let listener = tokio::time::timeout_at(deadline, tokio::net::TcpListener::bind("127.0.0.1:0"))
        .await
        .map_err(|_| "synthetic xAI bind deadline exceeded")??;
    let address = listener.local_addr()?;
    assert!(address.ip().is_loopback());
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let captured = seen.clone();
    // Enable the real nonexecuting observation queue so failure cannot pass merely
    // because auxiliary collection is disabled. Restore its runtime with RAII.
    struct ToolRuntimeReset(crate::services::tool_risk::runtime::ToolRiskRuntime);
    impl Drop for ToolRuntimeReset {
        fn drop(&mut self) {
            crate::services::tool_risk::runtime::set_tool_risk_runtime_for_test(self.0.clone());
        }
    }
    let _tool_reset =
        ToolRuntimeReset((*crate::services::tool_risk::runtime::get_tool_risk_runtime()).clone());
    crate::services::tool_risk::runtime::set_tool_risk_runtime_for_test(
        crate::services::tool_risk::runtime::ToolRiskRuntime {
            enabled: true,
            policy_version: 917,
            ..Default::default()
        },
    );
    let (sender, mut observation_queue) = tokio::time::timeout_at(
        deadline,
        crate::proxy::tool_observation::channel::test_channel(),
    )
    .await
    .map_err(|_| "synthetic xAI observation lock deadline exceeded")?;
    // Saturate the real shared queue without closing its process-wide receiver.
    // The handler must still restore and deliver the response, without a retry.
    let _queue_occupant = if mode == "observation_full" {
        use crate::proxy::tool_observation::channel::{
            OutputQuality, ResponseMetadata, Transport, DEFAULT_QUEUE_CAPACITY,
        };
        let mut tap = sender
            .start(
                ResponseMetadata {
                    protocol: "openai_responses",
                    client: "codex",
                    transport: Transport::Json,
                    quality: OutputQuality::Native,
                    definitions: Arc::new(Vec::new()),
                },
                917,
            )
            .ok_or("synthetic queue reservation unavailable")?;
        let before = observation_queue.counters.snapshot().delivered;
        for _ in 0..DEFAULT_QUEUE_CAPACITY {
            tap.push(&bytes::Bytes::from_static(b" "));
        }
        assert_eq!(
            observation_queue.counters.snapshot().delivered - before,
            DEFAULT_QUEUE_CAPACITY as u64,
            "the actual observation queue must be full before forwarding"
        );
        Some(tap)
    } else {
        None
    };
    let observations_before = observation_queue.counters.snapshot();
    let response_id = format!("resp_wire_xai_json_{mode}");
    let upstream_id = response_id.clone();
    let app = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let captured = captured.clone();
        let response_id = upstream_id.clone();
        async move {
            let initial = {
                let mut captured = captured.lock().expect("synthetic wire capture");
                captured.push(body.clone());
                captured.len() == 1
            };
            if !initial {
                return Json(json!({"id":format!("{response_id}_followup"),"object":"response","status":"completed",
                    "output":[{"id":"msg_wire_followup","type":"message","role":"assistant","status":"completed",
                        "content":[{"type":"output_text","text":"synthetic continuation","annotations":[]}]}]}));
            }
            let text = fixture_text(&body).unwrap_or("synthetic ordinary carrier");
            Json(json!({"id":response_id,"object":"response","status":"completed","model":"synthetic-model",
                "output":[{"id":"fc_wire_xai","type":"function_call","status":"completed","call_id":"call_wire_xai",
                    "name":"synthetic_ns__lookup","arguments":format!("{{\"phone\":{},\"count\":3.0,\"zero\":-0.0,\"signed_zero_text\":\"-0.0\",\"fraction\":1.25}}",json!(text))}]}))
        }
    }));
    let server =
        FinalSendBindingServer::new(tokio::spawn(
            async move { axum::serve(listener, app).await },
        ));
    let db = Arc::new(Database::memory()?);
    let url = format!("http://{address}/api.x.ai/v1");
    let provider = Provider::with_id(
        format!("synthetic-wire-xai-{mode}"),
        "Synthetic xAI wire boundary".into(),
        json!({"base_url":url,"api_key":"synthetic-wire-credential","api_format":"responses",
            "config":format!("model_provider = \"synthetic\"\n[model_providers.synthetic]\nwire_api = \"responses\"\nbase_url = \"{url}\"\n")}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let foreign = budget.reserve(47)?;
    let gateway = RestorationGateway::with_test_store_and_rules(
        DlpTokenStore::new(),
        if mode == "legacy" {
            &[]
        } else {
            &["chinese_phone_number"]
        },
        budget.clone(),
    );
    let mut proxy_state = state(db);
    proxy_state.restoration_gateway = gateway;
    let boundary = Arc::new(Mutex::new(BoundaryState::default()));
    let observation = boundary.clone();
    let injected_budget = budget.clone();
    let _probe = xai_json_wire_test_support::install(move |event| {
        let mut state = observation.lock().expect("synthetic wire boundary");
        match event {
            Event::Before { active } => {
                state.entered += 1;
                state.active = active;
                state.before = injected_budget.used();
                if mode == "refuse" {
                    let fill = injected_budget
                        .reserve(injected_budget.limit() - injected_budget.used())
                        .expect("synthetic boundary occupancy");
                    state.full = injected_budget.used() == injected_budget.limit();
                    return Some(Box::new(fill));
                }
            }
            Event::Encoded(bytes) => {
                state.encoded += 1;
                state.charge = injected_budget.used() - state.before;
                if matches!(mode, "lifetime" | "cancel") && state.entered == 1 {
                    assert!(
                        state.charge >= bytes.len() && state.charge > 0,
                        "independent wire bytes must own a real nonempty reservation"
                    );
                    let before = injected_budget.used();
                    state.retained.push(bytes.clone());
                    state.retained.push(bytes.clone());
                    assert_eq!(
                        injected_budget.used(),
                        before,
                        "Bytes clones must share the same reservation without double charging"
                    );
                }
            }
            Event::Released => {
                state.released += 1;
                state.recovered = injected_budget.used() < injected_budget.limit();
            }
        }
        None
    });
    let body = json!({"model":"synthetic-model","stream":false,
        "input":if mode == "noactive" { "synthetic ordinary carrier" } else { FIXTURE_PHONE },
        "tools":[{"type":"namespace","name":"synthetic_ns","tools":[
            {"type":"function","name":"lookup","parameters":{"type":"object","properties":{"phone":{"type":"string"}}}}]}]});
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", format!("synthetic-wire-{mode}"))
        .body(Body::from(body.to_string()))?;
    let response = tokio::time::timeout_at(
        deadline,
        handlers::handle_responses(State(proxy_state.clone()), request),
    )
    .await??;
    let status = response.status();
    let wire = tokio::time::timeout_at(deadline, response.into_body().collect())
        .await??
        .to_bytes();
    {
        let captured = seen.lock().map_err(|_| "synthetic capture poisoned")?;
        assert_eq!(
            captured.len(),
            1,
            "original upstream send must already have completed exactly once"
        );
        assert!(
            captured[0]["tools"][0]["name"] == "synthetic_ns__lookup",
            "upstream tool namespace normalization must remain unchanged"
        );
        assert!(
            !captured[0].to_string().contains(FIXTURE_PHONE),
            "fixture plaintext must not reach upstream"
        );
        if !matches!(mode, "legacy" | "noactive") {
            assert!(
                captured[0].to_string().contains("rredact-cn_phone-"),
                "active request must reach real upstream tokenized"
            );
        }
    }
    {
        let observed = boundary.lock().map_err(|_| "synthetic boundary poisoned")?;
        assert_eq!(
            observed.entered, 1,
            "target serialization must be reached after raw completion evidence"
        );
        assert_eq!(
            observed.released, 1,
            "injected occupancy must end before final wrapper"
        );
        assert!(
            observed.recovered,
            "final wrapper must run with budget recovered"
        );
        if mode == "refuse" {
            assert!(
                observed.active && observed.full,
                "only the admitted active target boundary exhausts real shared budget"
            );
        }
    }
    assert_eq!(status, if mode == "refuse" { http::StatusCode::BAD_GATEWAY } else { http::StatusCode::OK },
        "target xAI JSON wire serialization must fail closed when its independent budget reservation is refused");
    let value: Value = serde_json::from_slice(&wire)?;
    let observations_after = observation_queue.counters.snapshot();
    let observations = observations_after.delivered - observations_before.delivered;
    if mode == "observation_full" {
        assert_eq!(
            observations, 0,
            "a saturated queue must not accept response bytes"
        );
        assert!(
            observations_after.dropped_queue_full > observations_before.dropped_queue_full,
            "the real handler must attempt observation and encounter the full queue"
        );
    } else if mode == "refuse" {
        assert_eq!(
            observations, 0,
            "wire serialization failure must precede tool observation/publication"
        );
    } else {
        assert!(
            observations > 0,
            "success control must exercise the real nonexecuting observation queue"
        );
    }
    drain_xai_json_observations(&mut observation_queue, deadline).await?;
    let mut recorder = FinalClientRecorder::default();
    recorder.observe_json(&value);
    let text = std::str::from_utf8(&wire)?;
    assert!(
        !text.contains("rredact-cn_phone-"),
        "final client must not expose restoration references"
    );
    if mode == "refuse" {
        assert_eq!(value["error"]["code"], "dlp_restore_failed");
        assert!(
            !text.contains(FIXTURE_PHONE),
            "fixed failure must not expose fixture plaintext"
        );
        assert!(
            recorder.submissions.is_empty() && !recorder.completed,
            "refused wire must publish no client tools or completion"
        );
    } else {
        assert!(
            recorder.completed && recorder.submissions.len() == 1,
            "success delivers exactly one complete tool without executing it"
        );
        let item = &recorder.submissions[0];
        assert!(
            item["name"] == "lookup" && item["namespace"] == "synthetic_ns",
            "delivered tool identity must preserve the native namespace"
        );
        let arguments = item["arguments"]
            .as_str()
            .ok_or("missing synthetic arguments")?;
        let args: Value = serde_json::from_str(arguments)?;
        assert_eq!(args["count"], 3);
        assert_eq!(args["fraction"], 1.25);
        let mut compatibility = json!({"type":"function_call",
            "arguments":r#"{"count":3.0,"zero":-0.0,"fraction":1.25,"signed_zero_text":"-0.0"}"#});
        crate::proxy::providers::transform_codex_responses_xai_sanitize::normalize_xai_function_call_integer_arguments(&mut compatibility);
        let normalized: Value = serde_json::from_str(
            compatibility["arguments"]
                .as_str()
                .ok_or("missing synthetic normalized arguments")?,
        )?;
        for key in ["count", "zero", "fraction", "signed_zero_text"] {
            assert!(
                args[key] == normalized[key],
                "wire encoding must preserve the existing xAI numeric and string normalization"
            );
        }
        assert_eq!(args["signed_zero_text"], "-0.0");
        if !matches!(mode, "legacy" | "noactive") {
            assert!(
                args["phone"]
                    .as_str()
                    .is_some_and(|text| text.contains(FIXTURE_PHONE)),
                "active success must restore supported text"
            );
        }
    }
    drop(wire);
    {
        let mut state = boundary.lock().map_err(|_| "synthetic boundary poisoned")?;
        if matches!(mode, "legacy" | "noactive") {
            assert!(
                !state.active && state.charge == 0,
                "ordinary no-token and legacy redact traffic must acquire no response wire owner"
            );
        } else if mode == "refuse" {
            assert_eq!(
                state.encoded, 0,
                "leased serialization failure must not fall back to uncharged bytes"
            );
        } else {
            assert!(
                state.charge > 0,
                "active successful serialization must charge its independent buffer"
            );
        }
        if matches!(mode, "lifetime" | "cancel") {
            assert_eq!(state.retained.len(), 2);
            assert_eq!(
                budget.used(),
                foreign.bytes() + state.charge,
                "original upstream and final wrapper owners must release while wire clones remain"
            );
            drop(state.retained.pop());
            assert_eq!(
                budget.used(),
                foreign.bytes() + state.charge,
                "dropping one wire clone must not release the shared owner"
            );
            let last = state.retained.pop().ok_or("missing synthetic wire clone")?;
            if mode == "cancel" {
                let unread_body = Body::from(last);
                assert_eq!(budget.used(), foreign.bytes() + state.charge);
                drop(unread_body);
            } else {
                drop(last);
            }
            assert_eq!(budget.used(), foreign.bytes(),
                "final clone drop or unread transport body cancellation must release the wire reservation");
        }
    }
    if !matches!(mode, "legacy" | "noactive") {
        let followup = http::Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("content-type", "application/json")
            .header("session-id", format!("synthetic-wire-{mode}"))
            .body(Body::from(
                json!({"model":"synthetic-model","input":"synthetic continuation","stream":false,
                "previous_response_id":response_id})
                .to_string(),
            ))?;
        let response = tokio::time::timeout_at(
            deadline,
            handlers::handle_responses(State(proxy_state.clone()), followup),
        )
        .await?;
        let response = response.unwrap_or_else(IntoResponse::into_response);
        let followup_status = response.status();
        let followup_wire = tokio::time::timeout_at(deadline, response.into_body().collect())
            .await??
            .to_bytes();
        let sends = seen.lock().map_err(|_| "synthetic capture poisoned")?.len();
        if mode == "refuse" {
            assert_eq!(followup_status, http::StatusCode::BAD_REQUEST);
            let error: Value = serde_json::from_slice(&followup_wire)?;
            assert_eq!(error["error"]["code"], "dlp_context_unavailable");
            assert_eq!(sends, 1, "budget recovery must not authorize an additional upstream send for refused context");
        } else {
            assert_eq!(followup_status, http::StatusCode::OK);
            assert_eq!(
                sends, 2,
                "successful completed context must authorize one continuation"
            );
        }
    }
    drain_xai_json_observations(&mut observation_queue, deadline).await?;
    server.finish_at(deadline).await?;
    drop(proxy_state);
    assert_eq!(
        budget.used(),
        foreign.bytes(),
        "all request/response owners must release independently of unrelated occupancy"
    );
    drop(foreign);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_wire_xai_json_serialization_budget_failure() -> TestResult {
    run_reversible_wire_xai_json("refuse").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_wire_xai_json_observation_queue_full_preserves_forwarding() -> TestResult {
    run_reversible_wire_xai_json("observation_full").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_wire_xai_json_success_namespace_whole_float_and_context() -> TestResult {
    run_reversible_wire_xai_json("success").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_wire_xai_json_clone_and_final_drop_release() -> TestResult {
    run_reversible_wire_xai_json("lifetime").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_wire_xai_json_unread_body_cancellation_releases() -> TestResult {
    run_reversible_wire_xai_json("cancel").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_wire_xai_json_no_active_tokens_bypass_owner() -> TestResult {
    run_reversible_wire_xai_json("noactive").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_wire_xai_json_legacy_redact_bypasses_owner() -> TestResult {
    run_reversible_wire_xai_json("legacy").await
}

#[tokio::test]
async fn reversible_context_checkpoints_are_fixed_and_preserve_refusals() -> TestResult {
    const CHILD: &str = "VARYAI_SYNTHETIC_CONTEXT_CHECKPOINT_TEST";
    if legacy_diagnostic_observer::parent(
        CHILD,
        "proxy::security::reversible_integration_tests::reversible_context_checkpoints_are_fixed_and_preserve_refusals",
    ).await? { return Ok(()); }
    use super::{
        http_restoration::{
            admit_http_request, validate_unleased_enriched_body, DlpForwardError,
            RestorationAdmission,
        },
        payload_budget::PayloadBudget,
        tokenization::{DlpTokenStore, TokenCategory},
    };
    use crate::app_config::AppType;
    const CHECKPOINTS: [(&str, &str); 20] = [
        (
            "reference_scan",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "reference_scan"
            ),
        ),
        (
            "unleased_reference",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "unleased_reference"
            ),
        ),
        (
            "gateway_state",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "gateway_state"
            ),
        ),
        (
            "gateway_missing",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "gateway_missing"
            ),
        ),
        (
            "runtime_disabled",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "runtime_disabled"
            ),
        ),
        (
            "namespace_admission",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "namespace_admission"
            ),
        ),
        (
            "history_extract",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "history_extract"
            ),
        ),
        (
            "history_classify",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "history_classify"
            ),
        ),
        (
            "admission_audit",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "admission_audit"
            ),
        ),
        (
            "admission_prepare",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "admission_prepare"
            ),
        ),
        (
            "attempt_fork",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "attempt_fork"
            ),
        ),
        (
            "attempt_binding",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "attempt_binding"
            ),
        ),
        (
            "attempt_history",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "attempt_history"
            ),
        ),
        (
            "context_resume",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "context_resume"
            ),
        ),
        (
            "output_call_ids",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "output_call_ids"
            ),
        ),
        (
            "token_resume",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "token_resume"
            ),
        ),
        (
            "attempt_audit",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "attempt_audit"
            ),
        ),
        (
            "request_prepare",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "request_prepare"
            ),
        ),
        (
            "request_transform",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "request_transform"
            ),
        ),
        (
            "final_validation",
            concat!(
                "[DLP] context failure code=dlp_context_unavailable checkpoint=",
                "final_validation"
            ),
        ),
    ];
    // Only a fixed checkpoint index, a saturating count and a failure bit survive logging.
    static RECORDS: Mutex<(usize, Option<usize>, bool)> = Mutex::new((0, None, false));
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata
                .target()
                .ends_with("::security::restore_diagnostics")
        }
        fn log(&self, record: &log::Record<'_>) {
            if !self.enabled(record.metadata()) {
                return;
            }
            if let Ok(mut records) = RECORDS.lock() {
                records.0 = records.0.saturating_add(1);
                let index = CHECKPOINTS.iter().position(|(_, expected)| {
                    legacy_diagnostic_observer::exact(*record.args(), expected)
                });
                records.1 = index;
                records.2 |= index.is_none();
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Logger = Logger;
    log::set_logger(&LOGGER).map_err(|_| "isolated context logger unavailable")?;
    log::set_max_level(log::LevelFilter::Warn);
    runtime::set_security_runtime_for_test(Default::default());
    let store = DlpTokenStore::new();
    // A complete synthetic reference originating outside this gateway has no authority.
    let mut unrelated = DlpTokenStore::new().admit("codex", Some("synthetic-unrelated"), 1)?;
    let references = unrelated.prepare(
        &[super::tokenization::TokenFragment {
            raw: "synthetic-private-document-value",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    let reference = references.first().ok_or("synthetic reference missing")?;
    let gateway = RestorationGateway::with_test_store_and_rules(
        store,
        &["chinese_phone_number"],
        PayloadBudget::with_limit(64 * 1024 * 1024),
    );
    let mut headers = http::HeaderMap::new();
    headers.insert(
        "session-id",
        "synthetic-context-checkpoint-session".parse()?,
    );
    let check = |checkpoint: &str| -> TestResult {
        let mut records = RECORDS
            .lock()
            .map_err(|_| "synthetic log capture poisoned")?;
        let expected = CHECKPOINTS
            .iter()
            .position(|(label, _)| *label == checkpoint);
        assert!(
            expected.is_some() && records.0 == 1 && records.1 == expected && !records.2,
            "context log must contain only the expected fixed checkpoint once"
        );
        *records = (0, None, false);
        Ok(())
    };
    let no_context_log = || -> TestResult {
        let records = RECORDS
            .lock()
            .map_err(|_| "synthetic log capture poisoned")?;
        assert!(
            records.0 == 0 && records.1.is_none() && !records.2,
            "literal text and non-context failures must not emit context diagnostics"
        );
        Ok(())
    };
    let response_contract = |error: DlpForwardError| {
        assert!(
            error == DlpForwardError::ContextUnavailable,
            "context error identity changed"
        );
        assert_eq!(error.status_code(), http::StatusCode::BAD_REQUEST);
        assert_eq!(error.code(), "dlp_context_unavailable");
        assert_eq!(error.error_type(), "invalid_request_error");
        assert_eq!(
            error.message(),
            "Reversible DLP session or cursor context is unavailable."
        );
    };
    let mut token_only = json!({"model":"synthetic-model", "input":[{"role":"user","content":format!("Synthetic document sample {reference}")}]});
    let admitted = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut token_only,
        &headers,
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic reference history was not admitted".into()),
    };
    assert!(
        RECORDS
            .lock()
            .map_err(|_| "synthetic log capture poisoned")?
            .0
            == 0,
        "successful admission must not emit a failure log"
    );
    let transformed = std::cell::Cell::new(false);
    let result = admitted.prepare_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
        &mut token_only,
        |prepared| {
            transformed.set(true);
            assert!(
                prepared["input"][0]["content"] == format!("Synthetic document sample {reference}"),
                "foreign reference must remain literal before transformation"
            );
            Err(DlpForwardError::Blocked)
        },
    );
    assert!(
        transformed.get(),
        "foreign literal must reach the transform"
    );
    let error = result
        .err()
        .ok_or("synthetic policy refusal was accepted")?;
    assert!(
        error == DlpForwardError::Blocked,
        "real policy block must not become a context failure"
    );
    assert_eq!(error.status_code(), http::StatusCode::BAD_REQUEST);
    assert_eq!(error.code(), "dlp_blocked");
    assert_eq!(error.error_type(), "invalid_request_error");
    assert_eq!(error.message(), "Request blocked by local security policy.");
    no_context_log()?;

    // The same data reference must not conceal a preceding call-context refusal.
    let mut with_call = json!({"model":"synthetic-model", "previous_response_id":"synthetic-missing-response", "input":[
        {"type":"function_call","call_id":"synthetic-missing-call","name":"synthetic-tool","arguments":"{}"},
        {"type":"function_call_output","call_id":"synthetic-missing-call","output":format!("Synthetic document sample {reference}")}
    ]});
    let admitted = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut with_call,
        &headers,
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic call history was not admitted".into()),
    };
    let result = admitted.prepare_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
        &mut with_call,
        |_| Err(DlpForwardError::Blocked),
    );
    response_contract(result.err().ok_or("missing call context was accepted")?);
    check("context_resume")?;

    let mut orphan = json!({"model":"synthetic-model", "input":[
        {"type":"function_call_output","call_id":"synthetic-orphan-call","output":format!("Synthetic document sample {reference}")}
    ]});
    let admitted = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut orphan,
        &headers,
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic orphan history was not admitted".into()),
    };
    let result = admitted.prepare_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
        &mut orphan,
        |_| Err(DlpForwardError::Blocked),
    );
    response_contract(result.err().ok_or("orphan output was accepted")?);
    check("output_call_ids")?;

    validate_unleased_enriched_body(&json!({"input":reference}))
        .map_err(|_| "synthetic unleased literal validation unexpectedly refused")?;
    no_context_log()?;
    let unleased = admit_http_request(
        &RestorationGateway::disabled(),
        &AppType::Codex,
        true,
        false,
        &mut json!({"input":reference}),
        &headers,
    )?;
    assert!(
        matches!(unleased, RestorationAdmission::Passthrough),
        "a foreign literal must not require a gateway"
    );
    no_context_log()?;

    // Nonmatches remain literal, while malformed encoded tool JSON still fails closed.
    let malformed_text = ["rredact-", "custom-", "broken"].concat();
    let mut malformed = json!({"input": malformed_text});
    let literal = admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut malformed,
        &headers,
    )?;
    assert!(
        matches!(literal, RestorationAdmission::Admitted(_)),
        "malformed-looking literal must be admitted without authority"
    );
    drop(literal);
    no_context_log()?;
    let mut invalid_encoded = json!({"input":[{
        "type":"function_call", "call_id":"synthetic-invalid-json", "name":"synthetic-tool",
        "arguments":r#"{"value":"\q"}"#
    }]});
    let response = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut invalid_encoded,
        &headers,
    )? {
        RestorationAdmission::Intercepted(response) => response,
        _ => return Err("invalid encoded tool JSON was accepted".into()),
    };
    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await?.to_bytes();
    let payload: Value = serde_json::from_slice(&bytes)?;
    assert!(
        payload
            == json!({"error":{
                "message":"Reversible DLP request preparation failed.", "type":"invalid_request_error",
                "code":"dlp_redaction_failed", "param":null
            }}),
        "encoded JSON refusal contract changed"
    );
    no_context_log()?;
    let mut invalid_history = json!({"input":"synthetic", "previous_response_id":42});
    let response = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut invalid_history,
        &headers,
    )? {
        RestorationAdmission::Intercepted(response) => response,
        _ => return Err("synthetic invalid cursor was accepted".into()),
    };
    assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
    check("history_extract")?;
    let mut disabled = (*runtime::get_security_runtime()).clone();
    disabled.enabled = false;
    runtime::set_security_runtime_for_test(disabled);
    let disabled_admission = admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut json!({"input":reference}),
        &headers,
    )?;
    assert!(
        matches!(disabled_admission, RestorationAdmission::Passthrough),
        "foreign literal must not require an enabled restoration runtime"
    );
    no_context_log()?;
    runtime::set_security_runtime_for_test(Default::default());

    for final_check in [false, true] {
        let mut source = json!({"model":"synthetic-model", "input":"synthetic harmless text"});
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            &mut source,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic transform request was not admitted".into()),
        };
        let result = request.prepare_attempt(
            "synthetic-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-principal"),
            &mut source,
            |prepared| {
                if final_check {
                    let mut final_body = prepared.clone();
                    final_body["input"] = json!([{
                        "type":"function_call", "call_id":"synthetic-invalid-final-json",
                        "name":"synthetic-tool", "arguments":"{invalid"
                    }]);
                    Ok(final_body)
                } else {
                    Err(DlpForwardError::ContextUnavailable)
                }
            },
        );
        let error = result.err().ok_or("synthetic transform refusal missing")?;
        if final_check {
            assert!(
                error == DlpForwardError::RedactionFailed,
                "invalid final tool JSON must retain its real refusal"
            );
            no_context_log()?;
        } else {
            response_contract(error);
            check("request_transform")?;
        }
    }

    use super::restore_diagnostics::ContextCheckpoint as C;
    for (checkpoint, label) in [
        (C::ReferenceScan, "reference_scan"),
        (C::UnleasedReference, "unleased_reference"),
        (C::GatewayState, "gateway_state"),
        (C::GatewayMissing, "gateway_missing"),
        (C::RuntimeDisabled, "runtime_disabled"),
        (C::NamespaceAdmission, "namespace_admission"),
        (C::HistoryExtract, "history_extract"),
        (C::HistoryClassify, "history_classify"),
        (C::AdmissionAudit, "admission_audit"),
        (C::AdmissionPrepare, "admission_prepare"),
        (C::AttemptFork, "attempt_fork"),
        (C::AttemptBinding, "attempt_binding"),
        (C::AttemptHistory, "attempt_history"),
        (C::ContextResume, "context_resume"),
        (C::OutputCallIds, "output_call_ids"),
        (C::TokenResume, "token_resume"),
        (C::AttemptAudit, "attempt_audit"),
        (C::RequestPrepare, "request_prepare"),
        (C::RequestTransform, "request_transform"),
        (C::FinalValidation, "final_validation"),
    ] {
        response_contract(DlpForwardError::ContextUnavailable.at_context_checkpoint(checkpoint));
        check(label)?;
    }
    for error in [
        DlpForwardError::PolicyConflict,
        DlpForwardError::RestoreUnsupported,
        DlpForwardError::RedactionFailed,
        DlpForwardError::Unavailable,
        DlpForwardError::CapacityExceeded,
        DlpForwardError::TokenCollision,
        DlpForwardError::Blocked,
        DlpForwardError::RestoreFailed,
        DlpForwardError::RestoreFailedAt(super::restore_diagnostics::RestoreStage::TokenRestore),
    ] {
        assert!(
            error.at_context_checkpoint(C::TokenResume) == error,
            "checkpoint replaced non-context error"
        );
    }

    let mut harmless = json!({"model":"synthetic-model","input":"synthetic harmless text"});
    let admitted = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut harmless,
        &headers,
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic harmless request was not admitted".into()),
    };
    let result = admitted.prepare_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
        &mut harmless,
        |_| Err(DlpForwardError::Blocked),
    );
    assert!(
        matches!(result, Err(DlpForwardError::Blocked)),
        "non-context error was replaced"
    );
    assert!(
        RECORDS
            .lock()
            .map_err(|_| "synthetic log capture poisoned")?
            .0
            == 0,
        "non-context error must not emit a context failure log"
    );
    std::process::exit(87);
}

#[cfg(test)]
mod text_fallback_http_tests {
    use super::super::{
        http_restoration::{
            admit_http_request, prepare_text_projection_http_for_test, RestorationAdmission,
            RestorationGateway,
        },
        payload_budget::PayloadBudget,
        response_restoration::{ArgumentNormalizationMode, TEXT_PROJECTION_NOTICE},
        tokenization::{DlpRequestLease, DlpTokenStore, TokenCategory, TokenFragment},
    };
    use super::TestResult;
    use axum::{body::Body, extract::State, response::Response, routing::post, Router};
    use bytes::Bytes;
    use http_body_util::{BodyExt, Empty};
    use serde_json::{json, Value};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::{
        sync::Notify,
        time::{timeout, Duration},
    };

    const SESSION: &str = "synthetic-http-projection";
    const ORIGINAL: &str = "synthetic-http-source-value";

    struct Fixture {
        store: DlpTokenStore,
        lease: DlpRequestLease,
        budget: PayloadBudget,
        reference: String,
    }

    fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(32 * 1024 * 1024);
        let mut lease = store.admit("codex", Some(SESSION), 1)?;
        lease.prepare_with_budget(
            &[TokenFragment {
                raw: ORIGINAL,
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            budget.clone(),
            |_| Ok(()),
        )?;
        let reference = lease.authorized_tokens()?.as_slice()[0].to_owned();
        Ok(Fixture {
            store,
            lease,
            budget,
            reference,
        })
    }

    fn native(text: &str) -> Value {
        json!({"id":"resp_http_projection", "object":"response", "status":"completed", "model":"synthetic-model",
            "output":[{"id":"msg_http_projection", "type":"message", "role":"assistant", "status":"completed",
                "content":[{"type":"output_text", "text":text, "annotations":[]}]}]})
    }

    fn wire(fixture: &Fixture, sse: bool) -> Vec<u8> {
        wire_with_reference(&fixture.reference, sse)
    }

    fn wire_with_reference(reference: &str, sse: bool) -> Vec<u8> {
        let text = format!("{reference} rredact-custom-damaged é");
        let mut response = native(&text);
        if !sse {
            response["output"][0]["content"]
                .as_array_mut()
                .into_iter()
                .for_each(|parts| {
                    parts.push(json!({"type":"output_text", "text":reference, "annotations":[]}));
                });
            return response.to_string().into_bytes();
        }
        let part = response["output"][0]["content"][0].clone();
        let item = response["output"][0].clone();
        [
            json!({"type":"response.output_text.delta", "output_index":0, "content_index":0, "item_id":"msg_http_projection", "delta":text}),
            json!({"type":"response.output_text.done", "output_index":0, "content_index":0, "item_id":"msg_http_projection", "text":text}),
            json!({"type":"response.content_part.done", "output_index":0, "content_index":0, "item_id":"msg_http_projection", "part":part}),
            json!({"type":"response.output_item.done", "output_index":0, "item":item}),
            json!({"type":"response.completed", "response":response}),
        ].iter().map(|event| format!("event: {}\ndata: {event}\n\n", event["type"].as_str().unwrap_or("")))
            .collect::<String>().into_bytes()
    }

    fn response(body: Body, sse: bool) -> Result<Response, http::Error> {
        Response::builder()
            .header(
                "content-type",
                if sse {
                    "text/event-stream; charset=utf-8"
                } else {
                    "application/json; charset=utf-8"
                },
            )
            .header("content-encoding", "identity")
            .header("etag", "synthetic-stale")
            .header("digest", "synthetic-stale")
            .header("content-digest", "synthetic-stale")
            .header("repr-digest", "synthetic-stale")
            .header("content-md5", "synthetic-stale")
            .header("x-synthetic-preserved", "yes")
            .body(body)
    }

    fn no_authority(fixture: &Fixture) -> TestResult {
        let mut observer = fixture.store.admit("codex", Some(SESSION), 1)?;
        for provider in ["projection-fixture", "synthetic-http"] {
            let binding = fixture.lease.bind_actual_attempt(
                provider,
                "openai_responses",
                "http://127.0.0.1:1/responses",
                None,
            )?;
            assert!(
                observer
                    .resume_completed_context(&binding, Some("resp_http_projection"), &[], &[])
                    .is_err(),
                "projection published response authority"
            );
            assert!(
                observer
                    .resume_completed_tokens(&binding, &[&fixture.reference])
                    .is_err(),
                "projection published token authority"
            );
        }
        assert!(
            observer.authorized_tokens()?.as_slice().is_empty(),
            "observer gained authority"
        );
        Ok(())
    }

    fn clean_headers(response: &Response, sse: bool) {
        for name in [
            "content-encoding",
            "transfer-encoding",
            "etag",
            "digest",
            "content-digest",
            "repr-digest",
            "content-md5",
        ] {
            assert!(
                !response.headers().contains_key(name),
                "stale entity header retained: {name}"
            );
        }
        assert!(response.headers()["x-synthetic-preserved"] == "yes");
        assert_eq!(response.headers().contains_key("content-length"), !sse);
        assert!(response.headers()["content-type"]
            .to_str()
            .is_ok_and(|value| value.starts_with(if sse {
                "text/event-stream"
            } else {
                "application/json"
            })));
    }

    #[tokio::test]
    async fn text_fallback_loopback_finish_confirms_task_resource_release() -> TestResult {
        struct Release(Arc<AtomicUsize>);
        impl Drop for Release {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let released = Arc::new(AtomicUsize::new(0));
        let task_release = released.clone();
        let (ready, received) = tokio::sync::oneshot::channel();
        let task = super::FinalSendBindingServer::new(tokio::spawn(async move {
            let _release = Release(task_release);
            let _ = ready.send(());
            std::future::pending::<std::io::Result<()>>().await
        }));
        timeout(Duration::from_secs(2), received).await??;
        task.finish().await?;
        assert!(
            released.load(Ordering::SeqCst) == 1,
            "cleanup success must confirm task-owned resources were released"
        );
        Ok(())
    }

    #[tokio::test]
    async fn text_fallback_loopback_finish_reports_task_failure() -> TestResult {
        let handle = tokio::spawn(async {
            panic!("synthetic loopback task failure");
        });
        let finished = handle.abort_handle();
        let task = super::FinalSendBindingServer::new(handle);
        timeout(Duration::from_secs(2), async {
            while !finished.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        assert!(
            task.finish().await.is_err(),
            "failed task must not report cleanup success"
        );
        Ok(())
    }
    #[derive(Clone)]
    struct Upstream {
        wire: Bytes,
        sse: bool,
        attempts: Arc<AtomicUsize>,
        reads: Arc<AtomicUsize>,
        ready: Arc<Notify>,
        eof: Arc<Notify>,
    }

    async fn upstream(State(state): State<Upstream>) -> Response {
        state.attempts.fetch_add(1, Ordering::SeqCst);
        let sse = state.sse;
        let body = Body::from_stream(async_stream::stream! {
            // Byte-sized network chunks exercise UTF-8 and frame boundaries independently.
            for offset in 0..state.wire.len() {
                state.reads.fetch_add(1, Ordering::SeqCst);
                yield Ok::<_, std::io::Error>(state.wire.slice(offset..offset + 1));
            }
            state.ready.notify_one();
            state.eof.notified().await;
        });
        response(body, sse).unwrap_or_else(|_| Response::new(Body::empty()))
    }

    #[tokio::test]
    async fn text_fallback_http_loopback_native_json_sse_wait_for_eof_once_without_authority(
    ) -> TestResult {
        for sse in [false, true] {
            let fixture = fixture()?;
            let payload = wire(&fixture, sse);
            let state = Upstream {
                wire: Bytes::from(payload.clone()),
                sse,
                attempts: Arc::default(),
                reads: Arc::default(),
                ready: Arc::default(),
                eof: Arc::default(),
            };
            let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
            let listener =
                tokio::time::timeout_at(deadline, tokio::net::TcpListener::bind("127.0.0.1:0"))
                    .await??;
            let address = listener.local_addr()?;
            let app = Router::new()
                .route("/responses", post(upstream))
                .with_state(state.clone());
            let server = super::FinalSendBindingServer::new(tokio::spawn(async move {
                axum::serve(listener, app).await
            }));
            let socket = tokio::time::timeout_at(deadline, tokio::net::TcpStream::connect(address))
                .await??;
            let (mut client, connection) = tokio::time::timeout_at(
                deadline,
                hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(socket)),
            )
            .await??;
            let connection = super::FinalSendBindingServer::new(tokio::spawn(async move {
                connection.await.map_err(std::io::Error::other)
            }));
            let incoming = tokio::time::timeout_at(
                deadline,
                client.send_request(
                    http::Request::builder()
                        .method("POST")
                        .uri("/responses")
                        .header("host", address.to_string())
                        .body(Empty::<Bytes>::new())?,
                ),
            )
            .await??;
            let (parts, body) = incoming.into_parts();
            let mut native = Response::from_parts(parts, Body::new(body));
            native.headers_mut().insert("content-length", "7".parse()?);
            native
                .headers_mut()
                .insert("transfer-encoding", "chunked".parse()?);
            let mut pending = Box::pin(prepare_text_projection_http_for_test(
                native,
                fixture.lease.fork_attempt_extracted()?,
                fixture.budget.clone(),
            ));
            tokio::select! {
                ready = tokio::time::timeout_at(deadline, state.ready.notified()) => { ready?; },
                _ = &mut pending => return Err("projection returned before entity EOF".into()),
            }
            assert!(
                timeout(Duration::from_millis(20), pending.as_mut())
                    .await
                    .is_err(),
                "projection exposed output before EOF"
            );
            assert_eq!(state.reads.load(Ordering::SeqCst), payload.len());
            no_authority(&fixture)?;
            state.eof.notify_one();
            let projected = tokio::time::timeout_at(deadline, pending)
                .await?
                .map_err(|_| "synthetic projection refused")?;
            clean_headers(&projected, sse);
            assert!(fixture.budget.used() > 0, "body lost output owners");
            let length = projected.headers().get("content-length").map(|value| {
                value
                    .to_str()
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
            });
            let output = tokio::time::timeout_at(deadline, projected.into_body().collect())
                .await??
                .to_bytes();
            let raw = std::str::from_utf8(&output).map_err(|_| "projected entity is not UTF-8")?;
            assert!(
                !raw.contains("rredact-") && !raw.contains(&fixture.reference),
                "projection leaked a reference"
            );
            if sse {
                assert!(
                    !raw.contains(ORIGINAL),
                    "damaged field was only partially replaced"
                );
                let events: Vec<Value> = raw
                    .lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                    .map(serde_json::from_str)
                    .collect::<Result<_, _>>()
                    .map_err(|_| "invalid projected SSE")?;
                assert_eq!(events.len(), 5);
                assert!(
                    events[0]["delta"] == TEXT_PROJECTION_NOTICE
                        && events[1]["text"] == TEXT_PROJECTION_NOTICE
                );
                assert!(events[4]["type"] == "response.completed");
            } else {
                assert_eq!(length, Some(Some(output.len())));
                let value: Value =
                    serde_json::from_slice(&output).map_err(|_| "invalid projected JSON")?;
                assert!(value["output"][0]["content"][0]["text"] == TEXT_PROJECTION_NOTICE);
                assert!(value["output"][0]["content"][1]["text"] == ORIGINAL);
            }
            drop(output);
            assert_eq!(fixture.budget.used(), 0);
            assert_eq!(
                state.attempts.load(Ordering::SeqCst),
                1,
                "projection retried upstream"
            );
            assert_eq!(
                state.reads.load(Ordering::SeqCst),
                payload.len(),
                "projection reread upstream"
            );
            no_authority(&fixture)?;
            drop(client);
            connection.finish_at(deadline).await?;
            server.finish_at(deadline).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn text_fallback_http_output_owners_live_through_body_drop() -> TestResult {
        for sse in [false, true] {
            let fixture = fixture()?;
            let projected = prepare_text_projection_http_for_test(
                response(Body::from(wire(&fixture, sse)), sse)?,
                fixture.lease.fork_attempt_extracted()?,
                fixture.budget.clone(),
            )
            .await
            .map_err(|_| "synthetic projection refused")?;
            assert!(fixture.budget.used() > 0);
            let mut body = projected.into_body();
            let first = body.frame().await.ok_or("missing projected frame")??;
            drop(body);
            assert!(
                fixture.budget.used() > 0,
                "emitted bytes lost their reservation"
            );
            drop(first);
            assert_eq!(fixture.budget.used(), 0);
            no_authority(&fixture)?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn text_fallback_http_capacity_cancel_truncation_io_and_utf8_refuse() -> TestResult {
        for sse in [false, true] {
            let fixture = fixture()?;
            for variant in 0..7 {
                let mut payload = wire(&fixture, sse);
                match variant {
                    0 => {
                        payload.pop();
                    }
                    1 => payload.extend_from_slice(&[0xff]),
                    2 => payload.extend_from_slice(&[0xe2, 0x82]),
                    3 => payload.extend_from_slice(b"trailing"),
                    4 if sse => payload.extend_from_slice(b"data: {}\n\n"),
                    4 => {
                        payload = wire(&fixture, true);
                    }
                    _ => {}
                }
                let reads = Arc::new(AtomicUsize::new(0));
                let counter = reads.clone();
                let body = Body::from_stream(async_stream::stream! {
                    counter.fetch_add(1, Ordering::SeqCst);
                    yield Ok::<_, std::io::Error>(Bytes::from(payload));
                    if variant == 5 {
                        counter.fetch_add(1, Ordering::SeqCst);
                        yield Err(std::io::Error::other("Synthetic upstream failure."));
                    }
                });
                let budget = if variant == 6 {
                    PayloadBudget::with_limit(1)
                } else {
                    fixture.budget.clone()
                };
                let result = prepare_text_projection_http_for_test(
                    response(body, sse)?,
                    fixture.lease.fork_attempt_extracted()?,
                    budget.clone(),
                )
                .await;
                assert!(
                    result.is_err(),
                    "invalid synthetic entity accepted; mode={sse} case={variant}"
                );
                assert_eq!(
                    reads.load(Ordering::SeqCst),
                    if variant == 5 { 2 } else { 1 }
                );
                assert_eq!(budget.used(), 0);
                no_authority(&fixture)?;
            }
            let payload = wire(&fixture, sse);
            let body = Body::from_stream(async_stream::stream! {
                yield Ok::<_, std::io::Error>(Bytes::from(payload));
                futures::future::pending::<()>().await;
            });
            let mut pending = Box::pin(prepare_text_projection_http_for_test(
                response(body, sse)?,
                fixture.lease.fork_attempt_extracted()?,
                fixture.budget.clone(),
            ));
            assert!(timeout(Duration::from_millis(20), pending.as_mut())
                .await
                .is_err());
            assert!(fixture.budget.used() > 0);
            no_authority(&fixture)?;
            drop(pending);
            assert_eq!(fixture.budget.used(), 0);
            no_authority(&fixture)?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn text_fallback_http_encoded_status_and_non_native_refuse_before_body_read() -> TestResult
    {
        for variant in 0..8 {
            let fixture = fixture()?;
            let reads = Arc::new(AtomicUsize::new(0));
            let counter = reads.clone();
            let body = Body::from_stream(async_stream::stream! {
                counter.fetch_add(1, Ordering::SeqCst);
                yield Ok::<_, std::io::Error>(Bytes::from_static(b"synthetic"));
            });
            let mut native = response(body, false)?;
            match variant {
                0 => {
                    *native.status_mut() = http::StatusCode::BAD_REQUEST;
                }
                1 => {
                    *native.status_mut() = http::StatusCode::SERVICE_UNAVAILABLE;
                }
                2 => {
                    native
                        .headers_mut()
                        .insert("content-encoding", "gzip".parse()?);
                }
                3 => {
                    native
                        .headers_mut()
                        .append("content-encoding", "br".parse()?);
                }
                4 => {
                    native
                        .headers_mut()
                        .insert("content-encoding", "identity, gzip".parse()?);
                }
                5 => {
                    native
                        .headers_mut()
                        .insert("content-type", "application/octet-stream".parse()?);
                }
                6 => {
                    native.headers_mut().remove("content-type");
                }
                _ => {
                    native
                        .headers_mut()
                        .insert("content-type", "application/json-invalid".parse()?);
                }
            }
            assert!(prepare_text_projection_http_for_test(
                native,
                fixture.lease.fork_attempt_extracted()?,
                fixture.budget.clone()
            )
            .await
            .is_err());
            assert_eq!(reads.load(Ordering::SeqCst), 0, "refusal consumed the body");
            assert_eq!(fixture.budget.used(), 0);
            no_authority(&fixture)?;
        }
        let fixture = fixture()?;
        for value in [
            json!({"id":"synthetic", "object":"chat.completion", "choices":[]}),
            json!({"type":"message", "content":[]}),
        ] {
            assert!(prepare_text_projection_http_for_test(
                response(Body::from(value.to_string()), false)?,
                fixture.lease.fork_attempt_extracted()?,
                fixture.budget.clone()
            )
            .await
            .is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn text_fallback_http_sse_byte_chunks_crlf_and_frame_limit() -> TestResult {
        for crlf in [false, true] {
            let fixture = fixture()?;
            let payload = String::from_utf8(wire(&fixture, true))
                .map_err(|_| "invalid synthetic SSE fixture")?;
            let payload = if crlf {
                payload.replace('\n', "\r\n")
            } else {
                payload
            };
            let reads = Arc::new(AtomicUsize::new(0));
            let counter = reads.clone();
            let payload = Bytes::from(payload);
            let expected = payload.len();
            let body = Body::from_stream(async_stream::stream! {
                for offset in 0..payload.len() {
                    counter.fetch_add(1, Ordering::SeqCst);
                    yield Ok::<_, std::io::Error>(payload.slice(offset..offset + 1));
                }
            });
            let projected = prepare_text_projection_http_for_test(
                response(body, true)?,
                fixture.lease.fork_attempt_extracted()?,
                fixture.budget.clone(),
            )
            .await
            .map_err(|_| "synthetic fragmented SSE refused")?;
            let output = projected.into_body().collect().await?.to_bytes();
            let raw = std::str::from_utf8(&output).map_err(|_| "invalid projected SSE")?;
            assert!(raw.contains(TEXT_PROJECTION_NOTICE) && !raw.contains("rredact-"));
            assert_eq!(reads.load(Ordering::SeqCst), expected);
            drop(output);
            assert_eq!(fixture.budget.used(), 0);
            no_authority(&fixture)?;
        }
        let fixture = fixture()?;
        let oversized = Body::from(vec![b':'; 1024 * 1024 + 1]);
        assert!(
            prepare_text_projection_http_for_test(
                response(oversized, true)?,
                fixture.lease.fork_attempt_extracted()?,
                fixture.budget.clone()
            )
            .await
            .is_err(),
            "oversized frame accepted"
        );
        assert_eq!(fixture.budget.used(), 0);
        no_authority(&fixture)?;
        Ok(())
    }

    async fn production_refusal_case(case_index: usize) -> TestResult {
        for sse in [false, true] {
            let store = DlpTokenStore::new();
            let budget = PayloadBudget::with_limit(32 * 1024 * 1024);
            let gateway = RestorationGateway::with_test_store_and_rules(
                store.clone(),
                &["chinese_phone_number"],
                budget.clone(),
            );
            let mut request_body = json!({"model":"synthetic-model", "input":super::FIXTURE_PHONE});
            let mut headers = http::HeaderMap::new();
            headers.insert("session-id", "synthetic-strict-projection".parse()?);
            let request = match admit_http_request(
                &gateway,
                &crate::app_config::AppType::Codex,
                true,
                false,
                &mut request_body,
                &headers,
            )? {
                RestorationAdmission::Admitted(request) => request,
                _ => return Err("synthetic strict request was not admitted".into()),
            };
            let attempt = request
                .prepare_attempt(
                    "synthetic-http",
                    "openai_responses",
                    "http://127.0.0.1:1/responses",
                    Some("synthetic-strict-principal"),
                    &mut request_body,
                    |prepared| Ok(prepared.clone()),
                )
                .map_err(|_| "synthetic strict attempt was not prepared")?;
            let reference = attempt.lease.authorized_tokens()?.as_slice()[0].to_owned();
            let binding = attempt.binding.clone();
            let literal = "rredact-custom-damaged";
            let text = if case_index == 0 {
                format!("{reference} {literal}")
            } else {
                literal.to_owned()
            };
            let mut part = json!({"type":"refusal", "refusal":text});
            match case_index {
                0 => {}
                1 => part["metadata"] = json!({"synthetic-immutable":reference}),
                2 => part = json!({"type":"synthetic_unsupported", "text":reference}),
                _ => return Err("invalid synthetic refusal case".into()),
            }
            let mut source = native("");
            source["output"][0]["content"] = json!([part]);
            let mut events = Vec::new();
            if case_index != 2 {
                events.extend([
                    json!({"type":"response.refusal.delta", "output_index":0, "content_index":0, "item_id":"msg_http_projection", "delta":text}),
                    json!({"type":"response.refusal.done", "output_index":0, "content_index":0, "item_id":"msg_http_projection", "refusal":text}),
                ]);
            }
            events.extend([
                json!({"type":"response.content_part.done", "output_index":0, "content_index":0, "item_id":"msg_http_projection", "part":part}),
                json!({"type":"response.output_item.done", "output_index":0, "item":source["output"][0]}),
                json!({"type":"response.completed", "response":source}),
            ]);
            let payload = if sse {
                events
                    .iter()
                    .map(|event| {
                        format!(
                            "event: {}\ndata: {event}\n\n",
                            event["type"].as_str().unwrap_or("")
                        )
                    })
                    .collect::<String>()
                    .into_bytes()
            } else {
                source.to_string().into_bytes()
            };
            let reads = Arc::new(AtomicUsize::new(0));
            let counter = reads.clone();
            let body = Body::from_stream(async_stream::stream! {
                counter.fetch_add(1, Ordering::SeqCst);
                yield Ok::<_, std::io::Error>(Bytes::from(payload));
            });
            let ordinary = attempt
                .restore_client_http_response(
                    response(body, sse)?,
                    ArgumentNormalizationMode::Exact,
                    None,
                )
                .await;
            let status = ordinary.status();
            let bytes = ordinary.into_body().collect().await?.to_bytes();
            let raw = std::str::from_utf8(&bytes).map_err(|_| "invalid strict response entity")?;
            assert_eq!(reads.load(Ordering::SeqCst), 1, "case={case_index}");
            assert!(!raw.contains(TEXT_PROJECTION_NOTICE), "case={case_index}");
            let mut observer = store.admit_published_policy(
                "codex",
                Some("synthetic-strict-projection"),
                binding.policy_revision,
            )?;
            if case_index == 0 {
                assert_eq!(status, http::StatusCode::OK);
                let expected = format!("{} {literal}", super::FIXTURE_PHONE);
                assert!(
                    !raw.contains(&reference),
                    "authorized reference was not restored"
                );
                if sse {
                    let actual: Vec<Value> = raw
                        .lines()
                        .filter_map(|line| line.strip_prefix("data: "))
                        .map(serde_json::from_str)
                        .collect::<Result<_, _>>()?;
                    events[0]["delta"] = json!(expected);
                    events[1]["refusal"] = json!(expected);
                    events[2]["part"]["refusal"] = json!(expected);
                    events[3]["item"]["content"][0]["refusal"] = json!(expected);
                    events[4]["response"]["output"][0]["content"][0]["refusal"] = json!(expected);
                    assert!(actual == events, "refusal snapshots changed literal bytes");
                } else {
                    source["output"][0]["content"][0]["refusal"] = json!(expected);
                    assert!(
                        serde_json::from_slice::<Value>(&bytes)? == source,
                        "refusal JSON changed literal bytes"
                    );
                }
                observer.resume_completed_context(
                    &binding,
                    Some("resp_http_projection"),
                    &[],
                    &[],
                )?;
                assert!(
                    observer.authorized_tokens()?.as_slice() == [reference.as_str()],
                    "literal refusal expanded completion authority"
                );
                assert!(observer.resolve(literal).is_err());
                assert!(observer
                    .resume_completed_tokens(&binding, &[literal])
                    .is_err());
            } else {
                assert!(
                    !raw.contains(&reference) && !raw.contains(super::FIXTURE_PHONE),
                    "production path emitted a current reference or private data; case={case_index}"
                );
                if sse {
                    assert!(
                        raw.contains("response.failed") && !raw.contains("response.completed"),
                        "authorized immutable/unsupported carrier completed; case={case_index}"
                    );
                } else {
                    assert_eq!(status, http::StatusCode::BAD_GATEWAY, "case={case_index}");
                }
                assert!(
                    observer
                        .resume_completed_context(&binding, Some("resp_http_projection"), &[], &[])
                        .is_err(),
                    "refused carrier published completion authority; case={case_index}"
                );
                assert!(observer.authorized_tokens()?.as_slice().is_empty());
            }
            drop(bytes);
            drop(request);
            assert_eq!(budget.used(), 0, "case={case_index}");
        }
        Ok(())
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn text_fallback_http_production_literal_refusal_preserves_bytes_without_extra_authority(
    ) -> TestResult {
        let _reset = super::RuntimeReset((*super::runtime::get_security_runtime()).clone());
        super::runtime::set_security_runtime_for_test(Default::default());
        production_refusal_case(0).await
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn text_fallback_http_production_refusal_path_stays_strict() -> TestResult {
        let _reset = super::RuntimeReset((*super::runtime::get_security_runtime()).clone());
        super::runtime::set_security_runtime_for_test(Default::default());
        for case_index in 1..=2 {
            production_refusal_case(case_index).await?;
        }
        Ok(())
    }
}

async fn run_output_text_exact_http(
    protocol: &'static str,
    streaming: bool,
    scenario: &'static str,
) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let store = super::tokenization::DlpTokenStore::new();
    let revision = runtime::admit_security_runtime()?.restoration_policy_revision;
    let mut observer = store.admit_published_policy(
        "codex",
        Some("synthetic-output-text-exact-session"),
        revision,
    )?;
    let mut unrelated = super::tokenization::DlpTokenStore::new().admit(
        "codex",
        Some("synthetic-exact-unrelated"),
        revision,
    )?;
    let foreign = unrelated.prepare_with_budget(
        &[super::tokenization::TokenFragment {
            raw: "synthetic-exact-unrelated-value",
            whole_span_categories: &[super::tokenization::TokenCategory::Custom],
        }],
        &[],
        super::payload_budget::PayloadBudget::with_limit(1024 * 1024),
        |tokens| Ok(tokens[0].as_str().to_owned()),
    )?;
    let foreign_literal = foreign.clone();
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let capture = seen.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let upstream = Router::new().fallback(post(move |Json(body): Json<Value>| {
        let capture = capture.clone();
        let foreign = foreign.clone();
        async move {
            if let Ok(mut seen) = capture.lock() {
                seen.push(body.clone());
            }
            let reference = fixture_text(&body).unwrap_or("missing synthetic carrier");
            let pieces = match scenario {
                "tail" => vec![reference.to_owned(), " ends with rredact-cn_phone-123".to_owned()],
                "foreign_literal" => vec![
                    "Literal rredact-".to_owned(),
                    "cn_phone-doc_hint remains; then ".to_owned(),
                    foreign,
                    "; authorized ".to_owned(),
                    reference[..12.min(reference.len())].to_owned(),
                    reference[12.min(reference.len())..].to_owned(),
                ],
                _ => vec![
                    "Literal rredact-".to_owned(),
                    "cn_phone-doc_hint remains; VARY_ph_sample and vary_zz_sample stay; then ".to_owned(),
                    reference[..12.min(reference.len())].to_owned(),
                    reference[12.min(reference.len())..].to_owned(),
                    " done".to_owned(),
                ],
            };
            let text = pieces.concat();
            let complete = scenario != "incomplete_source";
            let arguments = "{\"label\":\"synthetic-inert\"}";
            let tool_name = if scenario == "authorized_immutable" {
                reference
            } else {
                "synthetic_lookup"
            };
            let tool = json!({"id":"fc_exact_http","type":"function_call","call_id":"call_exact_http",
                "name":tool_name,"arguments":arguments,"status":"completed"});
            let part = json!({"type":"output_text","text":text,"annotations":[]});
            let message = json!({"id":"msg_exact_http","type":"message","role":"assistant",
                "status":"completed","content":[part]});
            let native = json!({"id":"resp_exact_http","object":"response","model":"synthetic-model",
                "status":if complete {"completed"} else {"incomplete"},"output":[tool,message]});
            if !streaming {
                return Json(native).into_response();
            }
            let mut events = Vec::new();
            if protocol == "chat" {
                events.push(json!({"id":"chat_exact_http","object":"chat.completion.chunk","model":"synthetic-model",
                    "choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,
                        "id":"call_exact_http","type":"function","function":{"name":tool_name,"arguments":arguments}}]},"finish_reason":null}]}));
                for piece in pieces {
                    events.push(json!({"id":"chat_exact_http","object":"chat.completion.chunk","model":"synthetic-model",
                        "choices":[{"index":0,"delta":{"content":piece},"finish_reason":null}]}));
                }
                events.push(json!({"id":"chat_exact_http","object":"chat.completion.chunk","model":"synthetic-model",
                    "choices":[{"index":0,"delta":{},"finish_reason":if complete {"tool_calls"} else {"length"}}]}));
            } else {
                events.push(json!({"type":"response.created","response":{"id":"resp_exact_http","object":"response",
                    "status":"in_progress","model":"synthetic-model","output":[]}}));
                let mut pending_tool = tool.clone();
                pending_tool["status"] = json!("in_progress");
                pending_tool["arguments"] = json!("");
                events.push(json!({"type":"response.output_item.added","output_index":0,"item":pending_tool}));
                events.push(json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_exact_http","delta":arguments}));
                events.push(json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_exact_http","arguments":arguments}));
                events.push(json!({"type":"response.output_item.done","output_index":0,"item":tool}));
                events.push(json!({"type":"response.output_item.added","output_index":1,
                    "item":{"id":"msg_exact_http","type":"message","role":"assistant","status":"in_progress","content":[]}}));
                events.push(json!({"type":"response.content_part.added","output_index":1,"item_id":"msg_exact_http","content_index":0,
                    "part":{"type":"output_text","text":"","annotations":[]}}));
                for piece in pieces {
                    events.push(json!({"type":"response.output_text.delta","output_index":1,"item_id":"msg_exact_http","content_index":0,"delta":piece}));
                }
                events.push(json!({"type":"response.output_text.done","output_index":1,"item_id":"msg_exact_http","content_index":0,"text":text}));
                events.push(json!({"type":"response.content_part.done","output_index":1,"item_id":"msg_exact_http","content_index":0,"part":part}));
                events.push(json!({"type":"response.output_item.done","output_index":1,"item":message}));
                events.push(json!({"type":"response.completed","response":native}));
            }
            let mut wire = String::new();
            for event in events {
                if let Some(kind) = event["type"].as_str() {
                    wire.push_str(&format!("event: {kind}\n"));
                }
                wire.push_str(&format!("data: {event}\n\n"));
            }
            wire.push_str("data: [DONE]\n\n");
            ([("content-type", "text/event-stream")], wire).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "output-text-exact-http".into(),
        "Synthetic exact output text".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-exact-credential",
            "api_format":if protocol == "chat" {"chat_completions"} else {"responses"}}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let budget = super::payload_budget::PayloadBudget::with_limit(64 * 1024 * 1024);
    let proxy = ProxyServer::new(ProxyConfig::default(), db, None).with_restoration_gateway(
        RestorationGateway::with_test_store_and_rules(
            store.clone(),
            &["chinese_phone_number"],
            budget.clone(),
        ),
    );
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-output-text-exact-session")
        .body(Body::from(
            json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":streaming,
            "tools":[{"type":"function","name":"synthetic_lookup","parameters":{"type":"object"}}]})
            .to_string(),
        ))?;
    let actual_attempt = Arc::new(Mutex::new(None));
    let observed_attempt = actual_attempt.clone();
    let hook =
        crate::proxy::forwarder::final_send_binding_test_support::install(move |url, headers| {
            use sha2::{Digest, Sha256};
            let mut digest = Sha256::new();
            for name in [
                "authorization",
                "x-api-key",
                "x-goog-api-key",
                "chatgpt-account-id",
            ] {
                for value in headers.get_all(name) {
                    digest.update((name.len() as u64).to_be_bytes());
                    digest.update(name.as_bytes());
                    digest.update((value.as_bytes().len() as u64).to_be_bytes());
                    digest.update(value.as_bytes());
                }
            }
            if let Ok(mut observed) = observed_attempt.lock() {
                *observed = Some((url.clone(), format!("{:x}", digest.finalize())));
            }
        });
    let response = proxy.build_router().call(request).await?;
    let status = response.status();
    let collected = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        response.into_body().collect(),
    )
    .await;
    server.abort();
    let bytes = collected??.to_bytes();
    let wire = std::str::from_utf8(&bytes)?;
    let authorized_reference = {
        let captured = seen.lock().map_err(|_| "synthetic capture poisoned")?;
        assert_eq!(
            captured.len(),
            1,
            "exact fixture must make one real loopback attempt"
        );
        let outbound = fixture_text(&captured[0]).ok_or("missing synthetic upstream text")?;
        assert!(
            outbound.contains("rredact-cn_phone-") && !outbound.contains(FIXTURE_PHONE),
            "original lease must tokenize the synthetic upstream request"
        );
        outbound.to_owned()
    };
    let mut recorder = FinalClientRecorder::default();
    if streaming {
        recorder.observe_sse(wire)?;
    } else {
        recorder.observe_json(&serde_json::from_slice::<Value>(&bytes)?);
    }
    assert_eq!(
        hook.transport_counts(),
        (1, 0),
        "fixture must use reqwest once"
    );
    let (target_url, principal) = actual_attempt
        .lock()
        .map_err(|_| "synthetic attempt capture poisoned")?
        .take()
        .ok_or("missing synthetic actual attempt")?;
    let binding = observer.bind_actual_operation_attempt_extracted(
        &provider.id,
        if protocol == "chat" {
            "openai_chat"
        } else {
            "openai_responses"
        },
        &target_url,
        Some(&principal),
        crate::proxy::providers::CodexAdapter::resolve_dlp_operation_descriptor(
            &format!("http://{addr}"),
            "/v1/responses",
            if protocol == "chat" {
                "openai_chat"
            } else {
                "openai_responses"
            },
            &target_url,
            false,
        )
        .as_ref(),
    )?;
    assert!(observer.resolve(&foreign_literal).is_err());
    assert!(
        observer
            .resume_completed_tokens(&binding, &[&foreign_literal])
            .is_err(),
        "literal response must not publish foreign token authority"
    );
    assert!(observer.authorized_tokens()?.as_slice().is_empty());
    if matches!(scenario, "authorized_immutable" | "incomplete_source") {
        assert!(
            recorder.failed && !recorder.completed && recorder.submissions.is_empty(),
            "failed original evidence or current immutable reference must keep the tool barrier closed"
        );
        assert!(
            !wire.contains("[DONE]") && !wire.contains(FIXTURE_PHONE),
            "failed output must not expose plaintext or a success terminal"
        );
        if scenario == "authorized_immutable" {
            assert!(
                wire.contains("reason=token_context"),
                "current authorized reference in a tool name must be rejected"
            );
        }
        drop(bytes);
        drop(proxy);
        // Early stream refusal finishes usage asynchronously, retaining history owners until then.
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while budget.used() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| "synthetic refusal cleanup retained payload owners")?;
        assert_eq!(budget.used(), 0);
        return Ok(());
    }
    assert!(
        status == http::StatusCode::OK && recorder.completed && !recorder.failed,
        "nonmatching ordinary output text must retain verified completion"
    );
    assert_eq!(
        recorder.submissions.len(),
        1,
        "verified completion must release one inert tool fixture"
    );
    assert!(
        recorder.submissions[0]["arguments"] == "{\"label\":\"synthetic-inert\"}",
        "ordinary output text matching must not change tool arguments"
    );
    observer.resume_completed_tokens(&binding, &[&authorized_reference])?;
    assert!(
        observer.authorized_tokens()?.as_slice() == [authorized_reference.as_str()],
        "verified output must publish only the original authorized reference"
    );
    assert!(observer.resolve(&foreign_literal).is_err());
    assert!(
        !wire.contains("synthetic-exact-unrelated-value"),
        "foreign literal was restored without authorization"
    );
    let expected = match scenario {
        "tail" => format!("{FIXTURE_PHONE} ends with rredact-cn_phone-123"),
        "foreign_literal" => format!("Literal rredact-cn_phone-doc_hint remains; then {foreign_literal}; authorized {FIXTURE_PHONE}"),
        _ => format!("Literal rredact-cn_phone-doc_hint remains; VARY_ph_sample and vary_zz_sample stay; then {FIXTURE_PHONE} done"),
    };
    if !streaming {
        let response: Value = serde_json::from_slice(&bytes)?;
        let message = response["output"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["type"] == "message"))
            .ok_or("missing synthetic message snapshot")?;
        assert!(
            message["content"][0]["text"] == expected,
            "native JSON must preserve every nonmatching byte and restore the authorized reference"
        );
        drop(bytes);
        drop(proxy);
        assert_eq!(budget.used(), 0);
        return Ok(());
    }
    let mut deltas = String::new();
    let mut snapshots = 0;
    for data in wire.lines().filter_map(|line| line.strip_prefix("data: ")) {
        if data == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(data)?;
        match event["type"].as_str() {
            Some("response.output_text.delta") => deltas.push_str(
                event["delta"]
                    .as_str()
                    .ok_or("missing synthetic text delta")?,
            ),
            Some("response.output_text.done") => {
                assert!(
                    event["text"] == expected,
                    "output-text done snapshot must preserve exact text"
                );
                snapshots += 1;
            }
            Some("response.content_part.done") => {
                assert!(
                    event["part"]["text"] == expected,
                    "content-part done snapshot must preserve exact text"
                );
                snapshots += 1;
            }
            Some("response.output_item.done") if event["item"]["type"] == "message" => {
                assert!(
                    event["item"]["content"][0]["text"] == expected,
                    "message done snapshot must preserve exact text"
                );
                snapshots += 1;
            }
            Some("response.completed") => {
                let message = event["response"]["output"]
                    .as_array()
                    .and_then(|items| items.iter().find(|item| item["type"] == "message"))
                    .ok_or("missing synthetic completed message")?;
                assert!(
                    message["content"][0]["text"] == expected,
                    "completed snapshot must preserve exact text"
                );
                snapshots += 1;
                if protocol == "chat" {
                    assert!(
                        event["response"]["id"] != "chat_exact_http",
                        "fixture must traverse the real Chat-to-Responses converter"
                    );
                }
            }
            _ => {}
        }
    }
    assert!(
        deltas == expected,
        "cross-delta ordinary output text must not truncate or replace nonmatches"
    );
    assert_eq!(
        snapshots, 4,
        "all converted/native completion snapshots must be exercised"
    );
    drop(bytes);
    drop(proxy);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn output_text_exact_chat_explanation_cross_deltas_preserves_snapshots_and_completion(
) -> TestResult {
    run_output_text_exact_http("chat", true, "explanation").await
}

#[tokio::test]
#[serial_test::serial]
async fn output_text_exact_chat_incomplete_field_end_tail_is_preserved() -> TestResult {
    run_output_text_exact_http("chat", true, "tail").await
}

#[tokio::test]
#[serial_test::serial]
async fn output_text_exact_chat_foreign_literal_after_malformed_prefix_preserves_completion(
) -> TestResult {
    run_output_text_exact_http("chat", true, "foreign_literal").await
}

#[tokio::test]
#[serial_test::serial]
async fn output_text_exact_chat_current_authorized_immutable_name_keeps_tool_barrier() -> TestResult
{
    run_output_text_exact_http("chat", true, "authorized_immutable").await
}

#[tokio::test]
#[serial_test::serial]
async fn output_text_exact_chat_incomplete_original_evidence_keeps_tool_barrier() -> TestResult {
    run_output_text_exact_http("chat", true, "incomplete_source").await
}

#[tokio::test]
#[serial_test::serial]
async fn output_text_exact_native_json_preserves_nonmatches_and_authorized_reference() -> TestResult
{
    run_output_text_exact_http("native", false, "explanation").await
}

#[tokio::test]
#[serial_test::serial]
async fn output_text_exact_native_sse_preserves_nonmatches_and_verified_snapshots() -> TestResult {
    run_output_text_exact_http("native", true, "explanation").await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_upstream_error_reads_bounded_body_without_fail_over() -> TestResult {
    use super::http_restoration::{admit_http_request, RestorationAdmission};
    use crate::{
        app_config::AppType,
        proxy::{
            error::ProxyError,
            error_mapper::{get_error_message, map_proxy_error_to_status},
            forwarder::{upstream_error_test_support, RequestForwarder},
            hyper_client::ProxyResponse,
            types::{CopilotOptimizerConfig, OptimizerConfig, RectifierConfig},
        },
    };
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    for upstream_status in [401, 429, 503] {
        let transport = upstream_error_test_support::install(ProxyResponse::buffered(
            http::StatusCode::from_u16(upstream_status)?,
            http::HeaderMap::new(),
            bytes::Bytes::from_static(b"synthetic-safe-reason"),
        ));
        let db = Arc::new(Database::memory()?);
        let provider = Provider::with_id(
            "synthetic-first".into(),
            "Synthetic first".into(),
            json!({"base_url":"http://127.0.0.1:1","api_key":"synthetic-credential"}),
            None,
        );
        let mut backup = provider.clone();
        backup.id = "synthetic-backup".into();
        let proxy_state = state(db);
        let mut body = json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":true});
        let request = match admit_http_request(
            &proxy_state.restoration_gateway,
            &AppType::Codex,
            true,
            false,
            &mut body,
            &Default::default(),
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic request must have a restoration lease".into()),
        };
        let forwarder = RequestForwarder::new(
            proxy_state.provider_router.clone(),
            5,
            proxy_state.status.clone(),
            proxy_state.current_providers.clone(),
            proxy_state.gemini_shadow.clone(),
            proxy_state.codex_chat_history.clone(),
            proxy_state.failover_manager.clone(),
            None,
            provider.id.clone(),
            "synthetic-error-session".into(),
            true,
            5,
            5,
            RectifierConfig::default(),
            OptimizerConfig::default(),
            CopilotOptimizerConfig::default(),
            1,
        )
        .with_dlp_request(Some(request));
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            forwarder.forward_with_retry(
                &AppType::Codex,
                http::Method::POST,
                "/v1/responses",
                body,
                Default::default(),
                Default::default(),
                vec![provider, backup],
            ),
        )
        .await;
        let Err(failure) = result.map_err(|_| "synthetic_error_deadline")? else {
            return Err("synthetic_error_accepted".into());
        };
        assert!(
            failure
                .provider
                .as_ref()
                .is_some_and(|p| p.id == "synthetic-first"),
            "attempted_provider_identity_lost"
        );
        assert!(
            matches!(&failure.error, ProxyError::SafeUpstream(_)),
            "unsafe_error_carrier"
        );
        assert_eq!(map_proxy_error_to_status(&failure.error), upstream_status);
        assert!(
            get_error_message(&failure.error)
                == format!("upstream_error HTTP {upstream_status} upstream_error"),
            "diagnostic_category_changed"
        );
        assert_eq!(
            transport.attempts(),
            1,
            "prepared errors must retain the no-retry and no-failover barrier"
        );
    }
    Ok(())
}

#[tokio::test]
async fn reversible_http_upstream_error_fixed_logging() -> TestResult {
    const CHILD: &str = "VARYAI_SYNTHETIC_UPSTREAM_ERROR_TEST";
    if legacy_diagnostic_observer::parent(
        CHILD,
        "proxy::security::reversible_integration_tests::reversible_http_upstream_error_fixed_logging",
    ).await? { return Ok(()); }
    use std::sync::atomic::{AtomicBool, Ordering};
    static FORBIDDEN: AtomicBool = AtomicBool::new(false);
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            if legacy_diagnostic_observer::forbidden(
                *record.args(),
                &[
                    FIXTURE_PHONE,
                    "rredact-cn_phone-",
                    "synthetic-upstream-private-marker",
                    "synthetic-private-provider-marker",
                    "Synthetic private provider label",
                    "synthetic-model",
                    "synthetic-error-credential",
                    "dlp_restore_failed",
                    "restoration failure",
                ],
            ) {
                FORBIDDEN.store(true, Ordering::SeqCst);
                if let (Some(file), Some(line)) = (record.file(), record.line()) {
                    eprintln!("DIAGNOSTIC_SOURCE {file}:{line}");
                }
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Logger = Logger;
    log::set_logger(&LOGGER).map_err(|_| "isolated upstream logger unavailable")?;
    log::set_max_level(log::LevelFilter::Trace);
    runtime::set_security_runtime_for_test(Default::default());
    let mut completed = 0;
    for status in [400, 401, 429, 503] {
        check_upstream_error_echo(http::StatusCode::from_u16(status)?, true, "responses").await?;
        completed += 1;
    }
    // Silence at Warn is valid; success still requires all real fixture assertions.
    assert!(
        completed == 4,
        "upstream error fixture must complete every scenario"
    );
    if FORBIDDEN.load(Ordering::SeqCst) {
        std::process::exit(86);
    }
    std::process::exit(87);
}

async fn image_history_http_fixture(custom: bool) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    assert!(
        crate::proxy::forwarder::final_send_binding_test_support::is_active(),
        "image history requires an explicit no-proxy transport before any network setup"
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = Router::new()
        .fallback(post(matrix_upstream))
        .with_state(MatrixFixture {
            protocol: "native",
            compact: false,
            streaming: false,
            complete: true,
            seen: seen.clone(),
        });
    let server = FinalSendBindingServer::new(tokio::spawn(async move {
        axum::serve(listener, upstream).await
    }));
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-image-history".into(),
        "Synthetic image history".into(),
        json!({"base_url":format!("http://{address}"),"api_key":"synthetic-image-key","api_format":"responses"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let proxy = ProxyServer::new(ProxyConfig::default(), db, None)
        .with_restoration_gateway(gateway.clone());
    let mut router = proxy.build_router();
    let image = json!({"type":"input_image","image_url":"data:image/png;base64,synthetic-opaque-not-decoded","detail":"auto"});
    let message = json!({"type":"message","role":"user","content":[
        {"type":"input_text","text":format!("Synthetic image question {FIXTURE_PHONE}")},image.clone()]});
    let call = if custom {
        json!({"type":"custom_tool_call","call_id":"synthetic_image_call","name":"synthetic_lookup","input":format!("Synthetic input {FIXTURE_PHONE}")})
    } else {
        json!({"type":"function_call","call_id":"synthetic_image_call","name":"synthetic_lookup","arguments":format!("{{\"phone\":\"{FIXTURE_PHONE}\"}}")})
    };
    let output = json!({"type":if custom {"custom_tool_call_output"} else {"function_call_output"},
        "call_id":"synthetic_image_call","output":format!("Synthetic result {FIXTURE_PHONE}")});
    let paired = json!({"model":"synthetic-model","stream":false,"input":[message.clone(),call,output,
        {"type":"function_call","call_id":"synthetic_second_image_call","name":"synthetic_lookup","arguments":format!("{{\"phone\":\"{FIXTURE_PHONE}\"}}")},
        {"type":"function_call_output","call_id":"synthetic_second_image_call","output":format!("Synthetic second result {FIXTURE_PHONE}")} ]});
    let mut continued = paired.clone();
    continued["input"]
        .as_array_mut()
        .ok_or("synthetic input missing")?
        .push(json!({"role":"user","content":"Synthetic continue"}));
    for body in [
        json!({"model":"synthetic-model","stream":false,"input":[message]}),
        paired.clone(),
        continued,
    ] {
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            router.call(
                http::Request::builder()
                    .method("POST")
                    .uri("/v1/responses")
                    .header("content-type", "application/json")
                    .header("session-id", "synthetic-image-session")
                    .body(Body::from(body.to_string()))?,
            ),
        )
        .await
        .map_err(|_| "synthetic image router deadline exceeded")??;
        assert!(
            response.status() == http::StatusCode::OK,
            "supported user image history must reach upstream before and after paired tool results"
        );
        let bytes = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            response.into_body().collect(),
        )
        .await
        .map_err(|_| "synthetic image response deadline exceeded")??
        .to_bytes();
        assert!(
            std::str::from_utf8(&bytes)?.contains(FIXTURE_PHONE),
            "image history must preserve authorized response restoration"
        );
    }
    {
        let captured = seen
            .lock()
            .map_err(|_| "synthetic image capture poisoned")?;
        assert!(
            captured.len() == 3,
            "each valid image history request must send exactly once"
        );
        for body in captured.iter() {
            assert!(
                body["input"][0]["content"][1] == image,
                "opaque image fields must reach upstream unchanged"
            );
            let wire = body.to_string();
            assert!(
                !wire.contains(FIXTURE_PHONE) && wire.contains("rredact-cn_phone-"),
                "all readable image history text and tool input must remain inspected"
            );
        }
    }
    let foreign = super::tokenization::DlpTokenStore::new();
    let mut lease = foreign.admit("codex", Some("synthetic-foreign-image"), 1)?;
    let reference = lease.prepare_with_budget(
        &[super::tokenization::TokenFragment {
            raw: "synthetic-foreign-image-secret",
            whole_span_categories: &[super::tokenization::TokenCategory::Custom],
        }],
        &[],
        super::payload_budget::PayloadBudget::with_limit(1024 * 1024),
        |tokens| Ok(tokens[0].as_str().to_owned()),
    )?;
    let mut negatives = Vec::new();
    for location in 0..6 {
        let mut body = paired.clone();
        match location {
            0 => body["input"][0]["content"][0]["text"] = json!(reference),
            1 => body["input"][0]["content"][1]["image_url"] = json!(reference),
            2 => {
                body["input"][1][if custom { "input" } else { "arguments" }] = json!(if custom {
                    reference.clone()
                } else {
                    json!({"type":"input_image","image_url":"synthetic","value":reference})
                        .to_string()
                })
            }
            3 => body["input"][2]["output"] = json!(reference),
            4 => body["metadata"] = json!({reference.clone(): "synthetic"}),
            _ => {
                body["input"][0]["content"] =
                    json!([{"type":"refusal","refusal":reference},image.clone()])
            }
        }
        negatives.push(body);
    }
    for mutation in 0..9 {
        let mut body = paired.clone();
        match mutation {
            0 => body["input"][0]["content"][1]["text"] = json!("Synthetic hidden text"),
            1 => body["input"][2]["output"] = json!([image.clone()]),
            2 => body["input"]
                .as_array_mut()
                .ok_or("synthetic input missing")?
                .push(json!({"type":"reasoning","summary":[image.clone()]})),
            3 => body["input"][0]["content"][1]["image_url"] = Value::Null,
            4 => body["input"][0]["content"][1]["type"] = json!("input_file"),
            5 => {
                body["input"]
                    .as_array_mut()
                    .ok_or("synthetic input missing")?
                    .pop();
            }
            6 => body["input"][0]["encrypted_content"] = json!("synthetic-opaque-context"),
            7 => body["previous_response_id"] = json!("resp_synthetic_image_expired"),
            _ => body["input"]
                .as_array_mut()
                .ok_or("synthetic input missing")?
                .push(paired["input"][1].clone()),
        }
        negatives.push(body);
    }
    let mut expected_sends = 3;
    for (case_index, body) in negatives.into_iter().enumerate() {
        let literal_case = case_index < 6;
        for session in [
            "synthetic-unleased-image-session",
            "synthetic-image-session",
        ] {
            if literal_case {
                assert_history_literal_has_no_authority(&gateway, &body, session, &reference)?;
            }
            let response = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                router.call(
                    http::Request::builder()
                        .method("POST")
                        .uri("/v1/responses")
                        .header("content-type", "application/json")
                        .header("session-id", session)
                        .body(Body::from(body.to_string()))?,
                ),
            )
            .await
            .map_err(|_| "synthetic image router deadline exceeded")??;
            assert_eq!(
                response.status(),
                if literal_case {
                    http::StatusCode::OK
                } else {
                    http::StatusCode::BAD_REQUEST
                },
                "image history outcome; case_index={case_index} session={session}"
            );
            let bytes = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                response.into_body().collect(),
            )
            .await
            .map_err(|_| "synthetic image response deadline exceeded")??
            .to_bytes();
            expected_sends += usize::from(literal_case);
            let captured = seen
                .lock()
                .map_err(|_| "synthetic image capture poisoned")?;
            assert_eq!(captured.len(), expected_sends,
                "rejected image history must send nothing; case_index={case_index} session={session}");
            if literal_case {
                let outbound = captured
                    .last()
                    .ok_or("synthetic literal image send missing")?;
                let preserved = match case_index {
                    0 => {
                        outbound["input"][0]["content"][0]["text"]
                            == body["input"][0]["content"][0]["text"]
                    }
                    1 => outbound["input"][0]["content"][1] == body["input"][0]["content"][1],
                    2 => {
                        outbound["input"][1][if custom { "input" } else { "arguments" }]
                            == body["input"][1][if custom { "input" } else { "arguments" }]
                    }
                    3 => outbound["input"][2]["output"] == body["input"][2]["output"],
                    4 => outbound["metadata"] == body["metadata"],
                    _ => outbound["input"][0]["content"][0] == body["input"][0]["content"][0],
                };
                assert!(preserved && !outbound.to_string().contains(FIXTURE_PHONE),
                    "literal bytes must survive independently of fresh PII inspection; case_index={case_index}");
                assert!(
                    !std::str::from_utf8(&bytes)?.contains("synthetic-foreign-image-secret"),
                    "image literal must not resolve a foreign mapping; case_index={case_index}"
                );
            } else {
                let error: Value = serde_json::from_slice(&bytes)?;
                assert!(error["error"]["code"] == "dlp_context_unavailable",
                    "invalid image history must retain the fixed context error; case_index={case_index}");
            }
        }
    }
    assert!(
        transport.transport_counts() == (expected_sends, 0),
        "only valid image history and literal cases may reach the no-proxy transport"
    );
    server.finish().await?;
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn image_history_http_function_before_results_and_continue() -> TestResult {
    image_history_http_fixture(false).await
}

#[tokio::test]
#[serial_test::serial]
async fn image_history_http_custom_before_results_and_continue() -> TestResult {
    image_history_http_fixture(true).await
}

async fn empty_lease_handler_json_fixture(protocol: &'static str) -> TestResult {
    use super::{payload_budget::PayloadBudget, tokenization::DlpTokenStore};
    use crate::proxy::providers::{transform_codex_anthropic, transform_codex_chat};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    const SESSION: &str = "synthetic-empty-handler-session";
    const OUTPUT: &str = "Synthetic delivered output";
    const REASONING: &str = "Synthetic delivered reasoning";
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    assert!(crate::proxy::forwarder::final_send_binding_test_support::is_active());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let hits = Arc::new(AtomicUsize::new(0));
    let captured = hits.clone();
    let raw_response = move |complete: bool| match protocol {
        "chat" => json!({"id":"chat_synthetic_empty_handler","object":"chat.completion",
            "model":"synthetic-model","choices":[{"index":0,"message":{"role":"assistant",
                "content":OUTPUT,"reasoning_content":REASONING},
                "finish_reason":if complete {Some("stop")} else {None}}]}),
        "anthropic" => {
            json!({"id":"msg_synthetic_empty_handler","type":"message","role":"assistant",
            "model":"synthetic-model","content":[{"type":"text","text":OUTPUT}],
            "stop_reason":if complete {Some("end_turn")} else {None},
            "usage":{"input_tokens":0,"output_tokens":0}})
        }
        _ => json!({"id":"resp_synthetic_empty_handler","object":"response",
            "status":if complete {"completed"} else {"incomplete"},"model":"synthetic-model",
            "output":[{"id":"msg_synthetic_empty_handler","type":"message","role":"assistant",
                "status":"completed","content":[{"type":"output_text","text":OUTPUT,"annotations":[]}]}]}),
    };
    let upstream = Router::new().fallback(move || {
        let complete = captured.fetch_add(1, Ordering::SeqCst) != 0;
        async move { Json(raw_response(complete)) }
    });
    let server = FinalSendBindingServer::new(tokio::spawn(async move {
        axum::serve(listener, upstream).await
    }));
    let db = Arc::new(Database::memory()?);
    let settings = if protocol == "xai" {
        json!({"auth":{"OPENAI_API_KEY":"synthetic-empty-handler-key"},"config":format!(
            "model_provider = \"custom\"\n[model_providers.custom]\nname = \"xai\"\nbase_url = \"http://{address}/api.x.ai/v1\"\nwire_api = \"responses\"\n")})
    } else {
        json!({"base_url":format!("http://{address}"),"api_key":"synthetic-empty-handler-key",
            "api_format":if protocol == "chat" {"chat_completions"} else {"anthropic"}})
    };
    let provider = Provider::with_id(
        "synthetic-empty-handler-provider".into(),
        "Synthetic empty handler".into(),
        settings,
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let store = DlpTokenStore::new();
    let observer = store.admit_published_policy(
        "codex",
        Some(SESSION),
        runtime::admit_security_runtime()?.restoration_policy_revision,
    )?;
    let proxy = ProxyServer::new(ProxyConfig::default(), db, None).with_restoration_gateway(
        RestorationGateway::with_test_store_and_rules(
            store,
            &["chinese_phone_number"],
            PayloadBudget::with_limit(64 * 1024 * 1024),
        ),
    );
    let mut router = proxy.build_router();
    for complete in [false, true] {
        let raw = raw_response(complete);
        let expected = match protocol {
            "chat" => transform_codex_chat::chat_completion_to_response(raw)?,
            "anthropic" => transform_codex_anthropic::anthropic_response_to_responses(raw)?,
            _ => raw,
        };
        let replay = json!({"input":expected["output"]});
        assert!(
            observer
                .history_admission_candidate_projection(&replay)
                .is_none(),
            "no response proof may precede final delivery"
        );
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            router.call(
                http::Request::builder()
                    .method("POST")
                    .uri("/v1/responses")
                    .header("content-type", "application/json")
                    .header("session-id", SESSION)
                    .body(Body::from(
                        json!({"model":"synthetic-model","stream":false,
                    "input":"Synthetic inert prompt"})
                        .to_string(),
                    ))?,
            ),
        )
        .await
        .map_err(|_| "synthetic empty handler router deadline")??;
        let status = response.status();
        let bytes = tokio::time::timeout(Duration::from_secs(5), response.into_body().collect())
            .await
            .map_err(|_| "synthetic empty handler body deadline")??
            .to_bytes();
        let delivered: Value = serde_json::from_slice(&bytes)?;
        if complete {
            assert!(
                status == http::StatusCode::OK,
                "a valid original completion must succeed with empty token authority"
            );
            assert!(
                delivered["output"] == expected["output"],
                "the handler must preserve the converted delivered items"
            );
            assert!(
                observer
                    .history_admission_candidate_projection(&replay)
                    .is_some(),
                "only final successful delivery may publish complete typed-item proof"
            );
        } else {
            assert!(
                status == http::StatusCode::BAD_GATEWAY,
                "missing original completion must not be repaired into authority"
            );
            assert!(
                observer
                    .history_admission_candidate_projection(&replay)
                    .is_none(),
                "rejected original completion must publish no proof"
            );
        }
    }
    assert!(
        hits.load(Ordering::SeqCst) == 2,
        "each handler request must send once"
    );
    assert!(
        transport.transport_counts() == (2, 0),
        "the handler must use explicit no-proxy transport"
    );
    server.finish().await?;
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_empty_lease_handler_chat_json_proof_boundary() -> TestResult {
    empty_lease_handler_json_fixture("chat").await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_empty_lease_handler_anthropic_json_proof_boundary() -> TestResult {
    empty_lease_handler_json_fixture("anthropic").await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_empty_lease_handler_xai_json_proof_boundary() -> TestResult {
    empty_lease_handler_json_fixture("xai").await
}

async fn empty_lease_handler_sse_fixture(protocol: &'static str) -> TestResult {
    use super::{payload_budget::PayloadBudget, tokenization::DlpTokenStore};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    const OUTPUT: &str = "Synthetic SSE delivered output";
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    assert!(crate::proxy::forwarder::final_send_binding_test_support::is_active());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let hits = Arc::new(AtomicUsize::new(0));
    let captured = hits.clone();
    let upstream = Router::new().fallback(move || {
        let mode = captured.fetch_add(1, Ordering::SeqCst);
        async move {
            let events = match protocol {
                "chat" => {
                    let mut events = vec![json!({"id":"chat_synthetic_empty_sse","object":"chat.completion.chunk",
                        "model":"synthetic-model","choices":[{"index":0,"delta":{"role":"assistant","content":OUTPUT},"finish_reason":null}]})];
                    if mode != 2 {
                        events.push(json!({"id":"chat_synthetic_empty_sse","object":"chat.completion.chunk",
                            "model":"synthetic-model","choices":[{"index":0,"delta":{},
                            "finish_reason":if mode == 1 {None} else {Some("stop")}}]}));
                    }
                    events
                }
                "anthropic" => {
                    let mut events = vec![
                        json!({"type":"message_start","message":{"id":"msg_synthetic_empty_sse","type":"message",
                            "role":"assistant","model":"synthetic-model","content":[],"usage":{"input_tokens":0,"output_tokens":0}}}),
                        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":OUTPUT}}),
                        json!({"type":"content_block_stop","index":0}),
                    ];
                    if mode != 2 {
                        events.push(json!({"type":"message_delta","delta":{"stop_reason":if mode == 1 {None} else {Some("end_turn")}},"usage":{"output_tokens":0}}));
                        events.push(json!({"type":"message_stop"}));
                    }
                    events
                }
                _ => {
                    let response = json!({"id":"resp_synthetic_empty_sse","object":"response","model":"synthetic-model",
                        "status":if mode == 1 {"incomplete"} else {"completed"},"output":[{
                            "id":"msg_synthetic_empty_sse","type":"message","role":"assistant","status":"completed",
                            "content":[{"type":"output_text","text":OUTPUT,"annotations":[]}]}]});
                    if mode == 2 { vec![] } else { vec![json!({"type":"response.completed","response":response})] }
                }
            };
            let mut wire = String::new();
            for event in events {
                if let Some(kind) = event["type"].as_str() { wire.push_str(&format!("event: {kind}\n")); }
                wire.push_str(&format!("data: {event}\n\n"));
            }
            if protocol != "anthropic" && mode != 2 { wire.push_str("data: [DONE]\n\n"); }
            if mode == 3 { wire.push_str("data: {"); }
            ([("content-type", "text/event-stream")], wire)
        }
    });
    let server = FinalSendBindingServer::new(tokio::spawn(async move {
        axum::serve(listener, upstream).await
    }));
    let db = Arc::new(Database::memory()?);
    let settings = if protocol == "xai" {
        json!({"auth":{"OPENAI_API_KEY":"synthetic-empty-sse-key"},"config":format!(
            "model_provider = \"custom\"\n[model_providers.custom]\nname = \"xai\"\nbase_url = \"http://{address}/api.x.ai/v1\"\nwire_api = \"responses\"\n")})
    } else {
        json!({"base_url":format!("http://{address}"),"api_key":"synthetic-empty-sse-key",
            "api_format":if protocol == "chat" {"chat_completions"} else {"anthropic"}})
    };
    let provider = Provider::with_id(
        "synthetic-empty-sse-provider".into(),
        "Synthetic empty SSE".into(),
        settings,
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let store = DlpTokenStore::new();
    let proxy = ProxyServer::new(ProxyConfig::default(), db, None).with_restoration_gateway(
        RestorationGateway::with_test_store_and_rules(
            store.clone(),
            &["chinese_phone_number"],
            PayloadBudget::with_limit(64 * 1024 * 1024),
        ),
    );
    let mut router = proxy.build_router();
    let mut delivered_output = Value::Null;
    for mode in 0..5 {
        let session = format!("synthetic-empty-sse-session-{mode}");
        let observer = store.admit_published_policy(
            "codex",
            Some(&session),
            runtime::admit_security_runtime()?.restoration_policy_revision,
        )?;
        let response = tokio::time::timeout(Duration::from_secs(5), router.call(
            http::Request::builder().method("POST").uri("/v1/responses")
                .header("content-type", "application/json").header("session-id", &session)
                .body(Body::from(json!({"model":"synthetic-model","stream":true,"input":"Synthetic inert prompt"}).to_string()))?,
        )).await.map_err(|_| "synthetic empty SSE router deadline")??;
        assert!(
            response.status() == http::StatusCode::OK,
            "SSE must reach the final wrapper"
        );
        let mut body = response.into_body();
        let mut completed = false;
        let mut failed = false;
        let mut terminal_output = Value::Null;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while let Some(frame) = tokio::time::timeout_at(deadline, body.frame())
            .await
            .map_err(|_| "synthetic empty SSE body deadline")?
        {
            let frame = frame?;
            if let Some(data) = frame.data_ref() {
                for raw in std::str::from_utf8(data)?
                    .lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                {
                    if raw == "[DONE]" {
                        continue;
                    }
                    let event: Value = serde_json::from_str(raw)?;
                    failed |= event["type"] == "response.failed";
                    if event["type"] == "response.completed" {
                        completed = true;
                        terminal_output = event["response"]["output"].clone();
                    }
                }
            }
            if mode == 4 && completed {
                break;
            }
        }
        if mode == 0 {
            assert!(
                completed && !failed,
                "valid original SSE must complete with empty authority"
            );
            delivered_output = terminal_output;
            let replay = json!({"input":delivered_output});
            assert!(
                observer
                    .history_admission_candidate_projection(&replay)
                    .is_some(),
                "fully released valid SSE must publish exact delivered proof"
            );
        } else {
            let replay = json!({"input":if mode == 4 {&terminal_output} else {&delivered_output}});
            assert!(
                observer
                    .history_admission_candidate_projection(&replay)
                    .is_none(),
                "failed or suspended terminal delivery must publish no proof"
            );
            if mode == 4 {
                assert!(
                    completed && !failed,
                    "cancellation control must stop after a real terminal frame"
                );
                drop(body);
                assert!(
                    observer
                        .history_admission_candidate_projection(&replay)
                        .is_none(),
                    "dropping after terminal yield must not publish proof"
                );
            } else {
                assert!(
                    !completed && failed,
                    "missing original evidence must not become synthetic completion authority"
                );
            }
        }
    }
    assert!(
        hits.load(Ordering::SeqCst) == 5,
        "each synthetic SSE request must send once"
    );
    assert!(
        transport.transport_counts() == (5, 0),
        "SSE must use explicit no-proxy transport"
    );
    server.finish().await?;
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_empty_lease_handler_chat_sse_proof_boundary() -> TestResult {
    empty_lease_handler_sse_fixture("chat").await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_empty_lease_handler_anthropic_sse_proof_boundary() -> TestResult {
    empty_lease_handler_sse_fixture("anthropic").await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_empty_lease_handler_xai_sse_proof_boundary() -> TestResult {
    empty_lease_handler_sse_fixture("xai").await
}

#[derive(Clone, Copy)]
enum FinalSendBindingTransport {
    Reqwest,
    Hyper,
}

#[derive(Clone, Copy)]
enum FinalSendBindingChange {
    Exact,
    Authorization,
    Account,
    Query,
    Operation,
}

struct FinalSendBindingAdapter {
    loopback: LoopbackCodexAdapter,
    transport: FinalSendBindingTransport,
}

impl crate::proxy::providers::ProviderAdapter for FinalSendBindingAdapter {
    fn name(&self) -> &'static str {
        match self.transport {
            FinalSendBindingTransport::Reqwest => "Codex",
            FinalSendBindingTransport::Hyper => "SyntheticFinalBinding",
        }
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, crate::proxy::ProxyError> {
        self.loopback.extract_base_url(provider)
    }

    fn extract_auth(&self, provider: &Provider) -> Option<crate::proxy::providers::AuthInfo> {
        self.loopback.extract_auth(provider)
    }

    fn build_url(&self, base: &str, endpoint: &str) -> String {
        self.loopback.build_url(base, endpoint)
    }

    fn get_auth_headers(
        &self,
        auth: &crate::proxy::providers::AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, crate::proxy::ProxyError> {
        self.loopback.get_auth_headers(auth)
    }
}

struct FinalSendBindingServer {
    cancel: Option<tokio::sync::oneshot::Sender<tokio::time::Instant>>,
    completed: Option<tokio::sync::oneshot::Receiver<Result<(), &'static str>>>,
}

async fn supervise_loopback_cleanup(
    completion: impl std::future::Future<Output = Result<(), &'static str>>,
    deadline: tokio::time::Instant,
    sent: tokio::sync::oneshot::Sender<Result<(), &'static str>>,
) {
    tokio::pin!(completion);
    match tokio::time::timeout_at(deadline, &mut completion).await {
        Ok(result) => {
            let _ = sent.send(result);
        }
        Err(_) => {
            let _ = sent.send(Err("synthetic loopback cleanup deadline exceeded"));
            // A timeout is a failure report, not evidence of completed cleanup.
            // Retain reaping responsibility while this Tokio runtime remains live.
            let _ = completion.await;
        }
    }
}

async fn observe_loopback_fixture_ready(
    ready: tokio::sync::oneshot::Receiver<()>,
    fixture: tokio::task::JoinHandle<std::io::Result<()>>,
    deadline: tokio::time::Instant,
) -> Result<FinalSendBindingServer, &'static str> {
    let guard = FinalSendBindingServer::new(fixture);
    tokio::time::timeout_at(deadline, ready)
        .await
        .map_err(|_| "synthetic fixture readiness deadline exceeded")?
        .map_err(|_| "synthetic fixture readiness unavailable")?;
    Ok(guard)
}

impl FinalSendBindingServer {
    fn new(task: tokio::task::JoinHandle<std::io::Result<()>>) -> Self {
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        let (sent, received) = tokio::sync::oneshot::channel();
        // While the runtime stays live, retain the task across fixture failure
        // and cancellation; deadline failures never masquerade as completed cleanup.
        tokio::spawn(async move {
            let deadline = cancelled.await.unwrap_or_else(|_| {
                tokio::time::Instant::now() + std::time::Duration::from_secs(5)
            });
            task.abort();
            supervise_loopback_cleanup(
                async move {
                    match task.await {
                        Ok(Ok(())) => Ok(()),
                        Err(error) if error.is_cancelled() => Ok(()),
                        _ => Err("synthetic loopback task failed"),
                    }
                },
                deadline,
                sent,
            )
            .await;
        });
        Self {
            cancel: Some(cancel),
            completed: Some(received),
        }
    }
    fn cleanup_receipt(
        &mut self,
    ) -> Option<tokio::sync::oneshot::Receiver<Result<(), &'static str>>> {
        self.completed.take()
    }
    fn request_cleanup(&mut self, deadline: tokio::time::Instant) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(deadline);
        }
    }
    async fn finish(self) -> Result<(), &'static str> {
        self.finish_at(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await
    }
    async fn finish_at(mut self, deadline: tokio::time::Instant) -> Result<(), &'static str> {
        let receipt = self
            .cleanup_receipt()
            .ok_or("synthetic loopback receipt missing")?;
        self.request_cleanup(deadline);
        tokio::time::timeout_at(deadline, receipt)
            .await
            .map_err(|_| "synthetic loopback acknowledgement deadline exceeded")?
            .map_err(|_| "synthetic loopback supervisor unavailable")?
    }
}

impl Drop for FinalSendBindingServer {
    fn drop(&mut self) {
        self.request_cleanup(tokio::time::Instant::now() + std::time::Duration::from_secs(5));
    }
}

async fn loopback_cleanup_abnormal_fixture(mode: u8) -> TestResult {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let retained = Dropped(dropped.clone());
    let task = tokio::spawn(async move {
        let _retained = retained;
        std::future::pending::<std::io::Result<()>>().await
    });
    let mut server = FinalSendBindingServer::new(task);
    let receipt = server
        .cleanup_receipt()
        .ok_or("synthetic cleanup receipt missing")?;
    let (started, ready) = tokio::sync::oneshot::channel();
    let (observed, result) = tokio::sync::oneshot::channel();
    struct Outcome(tokio::sync::oneshot::Sender<u8>);
    let outcome = Outcome(observed);
    let fixture = tokio::spawn(async move {
        let _server = server;
        let _ = started.send(());
        match mode {
            0 => {
                let _ = outcome.0.send(0);
                Err(std::io::Error::other("synthetic early fixture error"))
            }
            1 => {
                let _ = outcome.0.send(1);
                panic!("synthetic loopback fixture interruption")
            }
            _ => {
                let _outcome = outcome;
                std::future::pending().await
            }
        }
    });
    let fixture = observe_loopback_fixture_ready(
        ready,
        fixture,
        tokio::time::Instant::now() + Duration::from_secs(1),
    )
    .await?;
    if mode == 2 {
        fixture.finish().await?;
        assert!(tokio::time::timeout(Duration::from_secs(1), result)
            .await?
            .is_err());
    } else {
        assert!(tokio::time::timeout(Duration::from_secs(1), result).await?? == mode);
        // Panic and early-error results are failures, but still acknowledge that
        // the supervisor actually joined the fixture before this check returns.
        assert!(fixture.finish().await.is_err());
    }
    let reaped = tokio::time::timeout(Duration::from_secs(1), receipt).await;
    assert!(
        matches!(reaped, Ok(Ok(Ok(())))),
        "abnormal cleanup must acknowledge bounded task reaping"
    );
    assert!(
        dropped.load(Ordering::SeqCst),
        "task-owned state must be destroyed before cleanup acknowledgement"
    );
    Ok(())
}

#[tokio::test]
async fn reasoning_history_loopback_cleanup_early_error() -> TestResult {
    loopback_cleanup_abnormal_fixture(0).await
}
#[tokio::test]
async fn reasoning_history_loopback_cleanup_panic() -> TestResult {
    loopback_cleanup_abnormal_fixture(1).await
}
#[tokio::test]
async fn reasoning_history_loopback_cleanup_cancellation() -> TestResult {
    loopback_cleanup_abnormal_fixture(2).await
}

#[tokio::test]
async fn reasoning_history_loopback_cleanup_timeout_retains_reaping_owner() -> TestResult {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    struct Probe(Arc<AtomicBool>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let probe = Probe(dropped.clone());
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let (sent, receipt) = tokio::sync::oneshot::channel();
    let supervisor = tokio::spawn(supervise_loopback_cleanup(
        async move {
            let _probe = probe;
            let _ = released.await;
            Ok(())
        },
        tokio::time::Instant::now() + Duration::from_millis(5),
        sent,
    ));
    let verdict = tokio::time::timeout(Duration::from_secs(1), receipt).await??;
    let abandoned_before_release = dropped.load(Ordering::SeqCst);
    let _ = release.send(());
    tokio::time::timeout(Duration::from_secs(1), supervisor).await??;
    assert!(verdict == Err("synthetic loopback cleanup deadline exceeded"));
    assert!(
        !abandoned_before_release,
        "deadline failure must retain the pending reaping owner"
    );
    assert!(dropped.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn reasoning_history_loopback_cleanup_readiness_timeout_reaps_fixture() -> TestResult {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    struct Probe(Arc<AtomicBool>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let probe = Probe(dropped.clone());
    let (started, ready) = tokio::sync::oneshot::channel::<()>();
    let (finished, completion) = tokio::sync::oneshot::channel();
    let fixture = tokio::spawn(async move {
        let _probe = probe;
        let _started = started;
        struct Finished(tokio::sync::oneshot::Sender<()>);
        impl Drop for Finished {
            fn drop(&mut self) {}
        }
        let _finished = Finished(finished);
        std::future::pending::<std::io::Result<()>>().await
    });
    let abort = fixture.abort_handle();
    let result = observe_loopback_fixture_ready(
        ready,
        fixture,
        tokio::time::Instant::now() + Duration::from_millis(5),
    )
    .await;
    let observed = tokio::time::timeout(Duration::from_millis(50), completion).await;
    let reaped_before_fallback = dropped.load(Ordering::SeqCst);
    abort.abort();
    tokio::task::yield_now().await;
    assert!(result.is_err());
    assert!(
        observed.is_ok() && reaped_before_fallback,
        "readiness timeout must cancel and reap its fixture"
    );
    Ok(())
}

async fn final_send_binding_http_fixture(
    transport: FinalSendBindingTransport,
    change: FinalSendBindingChange,
) -> TestResult {
    use super::http_restoration::{admit_http_request, DlpForwardError, RestorationAdmission};
    use crate::{
        app_config::AppType,
        proxy::{
            error::ProxyError,
            forwarder::{final_send_binding_test_support, RequestForwarder},
        },
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    let _ = rustls::crypto::ring::default_provider().install_default();
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "synthetic final binding listener failed")?;
    let address = listener
        .local_addr()
        .map_err(|_| "synthetic final binding address unavailable")?;
    let hits = Arc::new(AtomicUsize::new(0));
    let valid_requests = Arc::new(AtomicUsize::new(0));
    let captured_hits = hits.clone();
    let captured_valid = valid_requests.clone();
    let upstream = Router::new().fallback(move |request: http::Request<Body>| {
        let captured_hits = captured_hits.clone();
        let captured_valid = captured_valid.clone();
        async move {
            // Count every upstream request before parsing, including unexpected routes/bodies.
            captured_hits.fetch_add(1, Ordering::SeqCst);
            let exact_headers = request
                .headers()
                .get(http::header::AUTHORIZATION)
                .is_some_and(|value| value == "Bearer synthetic-final-binding-principal-a")
                && request
                    .headers()
                    .get("chatgpt-account-id")
                    .is_some_and(|value| value == "synthetic-final-binding-account-a");
            let exact_target = request
                .uri()
                .path_and_query()
                .is_some_and(|target| target.as_str() == "/v1/responses?operation=synthetic-a");
            let tokenized = match axum::body::to_bytes(request.into_body(), 64 * 1024).await {
                Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .is_some_and(|body| {
                        body["input"].as_str().is_some_and(|text| {
                            text.contains("rredact-cn_phone-") && !text.contains(FIXTURE_PHONE)
                        })
                    }),
                Err(_) => false,
            };
            if exact_headers && exact_target && tokenized {
                captured_valid.fetch_add(1, Ordering::SeqCst);
            }
            Json(json!({
                "id":"resp_final_binding", "object":"response", "status":"completed",
                "model":"synthetic-model", "output":[{
                    "id":"msg_final_binding", "type":"message", "role":"assistant",
                    "status":"completed", "content":[{
                        "type":"output_text", "text":"Synthetic inert final binding response",
                        "annotations":[]
                    }]
                }]
            }))
        }
    });
    let server = FinalSendBindingServer::new(tokio::spawn(async move {
        axum::serve(listener, upstream).await
    }));
    let proxy_state = state(Arc::new(
        Database::memory().map_err(|_| "synthetic final binding database failed")?,
    ));
    let provider = Provider::with_id(
        "synthetic-final-binding-provider".into(),
        "Synthetic final binding".into(),
        json!({"base_url":format!("http://{address}"),
            "api_key":"synthetic-final-binding-principal-a"}),
        None,
    );
    let adapter = FinalSendBindingAdapter {
        loopback: LoopbackCodexAdapter(address),
        transport,
    };
    // A working exact send on the same listener brackets every negative attempt.
    let stages: &[FinalSendBindingChange] = if matches!(change, FinalSendBindingChange::Exact) {
        &[FinalSendBindingChange::Exact]
    } else {
        &[
            FinalSendBindingChange::Exact,
            change,
            FinalSendBindingChange::Exact,
        ]
    };
    for &stage in stages {
        hits.store(0, Ordering::SeqCst);
        valid_requests.store(0, Ordering::SeqCst);
        let mut body = json!({"model":"synthetic-model", "input":FIXTURE_PHONE, "stream":false});
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "session-id",
            http::HeaderValue::from_static("synthetic-final-binding-session"),
        );
        headers.insert(
            "chatgpt-account-id",
            http::HeaderValue::from_static("synthetic-final-binding-account-a"),
        );
        let request = match admit_http_request(
            &proxy_state.restoration_gateway,
            &AppType::Codex,
            true,
            false,
            &mut body,
            &headers,
        )
        .map_err(|_| "synthetic final binding admission failed")?
        {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic final binding request not admitted".into()),
        };
        let observe_audit = request.audit_observer_for_test();
        let forwarder = RequestForwarder::new(
            proxy_state.provider_router.clone(),
            5,
            proxy_state.status.clone(),
            proxy_state.current_providers.clone(),
            proxy_state.gemini_shadow.clone(),
            proxy_state.codex_chat_history.clone(),
            proxy_state.failover_manager.clone(),
            None,
            provider.id.clone(),
            "synthetic-final-binding-session".into(),
            true,
            5,
            5,
            Default::default(),
            Default::default(),
            Default::default(),
            0,
        )
        .with_dlp_request(Some(request));
        let reached = Arc::new(AtomicUsize::new(0));
        let observed_reached = reached.clone();
        let hook = final_send_binding_test_support::install(move |target, headers| {
            observed_reached.fetch_add(1, Ordering::SeqCst);
            match stage {
                FinalSendBindingChange::Exact => {}
                FinalSendBindingChange::Authorization => {
                    headers.insert(
                        http::header::AUTHORIZATION,
                        http::HeaderValue::from_static(
                            "Bearer synthetic-final-binding-principal-b",
                        ),
                    );
                }
                FinalSendBindingChange::Account => {
                    headers.insert(
                        "chatgpt-account-id",
                        http::HeaderValue::from_static("synthetic-final-binding-account-b"),
                    );
                }
                FinalSendBindingChange::Query => {
                    target.push_str("&synthetic_final_scope=changed");
                }
                FinalSendBindingChange::Operation => {
                    *target =
                        format!("http://{address}/v1/responses/compact?operation=synthetic-a");
                }
            }
        });
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            forwarder.forward_with_adapter_for_test(
                &provider,
                "/responses?operation=synthetic-a",
                &body,
                &headers,
                &adapter,
            ),
        )
        .await
        .map_err(|_| "synthetic final binding forwarding timed out")?;
        assert!(
            reached.load(Ordering::SeqCst) == 1,
            "fixture must reach the actual post-preparation send binding hook exactly once"
        );
        let counts = hook.transport_counts();
        if matches!(stage, FinalSendBindingChange::Exact) {
            assert!(
                match transport {
                    FinalSendBindingTransport::Reqwest => counts == (1, 0),
                    FinalSendBindingTransport::Hyper => counts == (0, 1),
                },
                "exact binding must invoke the requested real transport exactly once"
            );
            let result = result.map_err(|_| "synthetic exact final binding send refused")?;
            assert!(
                result.response.status() == http::StatusCode::OK && result.dlp_attempt.is_some(),
                "exact final binding must retain the prepared attempt and upstream success"
            );
            result
                .response
                .bytes_with_limit(64 * 1024)
                .await
                .map_err(|_| "synthetic final binding response read failed")?;
            assert!(
                hits.load(Ordering::SeqCst) == 1 && valid_requests.load(Ordering::SeqCst) == 1,
                "exact binding must send the tokenized request and original principal exactly once"
            );
            assert!(
                observe_audit().len() == 1,
                "an exact binding send must record exactly one permitted request action"
            );
        } else {
            assert!(
                counts == (0, 0),
                "a changed final actual binding must not invoke either real transport"
            );
            assert!(
                hits.load(Ordering::SeqCst) == 0,
                "a changed final actual binding must send no request to the loopback upstream"
            );
            assert!(
                matches!(
                    result,
                    Err(ProxyError::DlpForward(DlpForwardError::ContextUnavailable))
                ),
                "a late binding mismatch must return only the fixed context refusal"
            );
            assert!(
                observe_audit().is_empty(),
                "a refused final binding must not record a permitted request action"
            );
        }
    }
    server.finish().await?;
    Ok(())
}

macro_rules! final_send_binding_http_test {
    ($name:ident, $transport:ident, $change:ident) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            final_send_binding_http_fixture(
                FinalSendBindingTransport::$transport,
                FinalSendBindingChange::$change,
            )
            .await
        }
    };
}

final_send_binding_http_test!(
    reversible_http_final_send_binding_reqwest_exact_attempt_sends,
    Reqwest,
    Exact
);
final_send_binding_http_test!(
    reversible_http_final_send_binding_hyper_exact_attempt_sends,
    Hyper,
    Exact
);
final_send_binding_http_test!(
    reversible_http_final_send_binding_reqwest_late_authorization_change_sends_nothing,
    Reqwest,
    Authorization
);
final_send_binding_http_test!(
    reversible_http_final_send_binding_hyper_late_authorization_change_sends_nothing,
    Hyper,
    Authorization
);
final_send_binding_http_test!(
    reversible_http_final_send_binding_reqwest_late_account_change_sends_nothing,
    Reqwest,
    Account
);
final_send_binding_http_test!(
    reversible_http_final_send_binding_hyper_late_account_change_sends_nothing,
    Hyper,
    Account
);
final_send_binding_http_test!(
    reversible_http_final_send_binding_reqwest_late_query_change_sends_nothing,
    Reqwest,
    Query
);
final_send_binding_http_test!(
    reversible_http_final_send_binding_hyper_late_query_change_sends_nothing,
    Hyper,
    Query
);
final_send_binding_http_test!(
    reversible_http_final_send_binding_reqwest_late_operation_change_sends_nothing,
    Reqwest,
    Operation
);
final_send_binding_http_test!(
    reversible_http_final_send_binding_hyper_late_operation_change_sends_nothing,
    Hyper,
    Operation
);

#[derive(Clone, Copy, Debug)]
enum ChatHistoryLoopbackStage {
    InitialDelivery,
    ExactReplay,
    OverrideContent,
    OverrideReasoning,
    OverrideSibling,
    CachedReasoning,
    LateAccount,
    LateQuery,
    LatePolicy,
    RedirectReplay,
}

async fn delivered_chat_history_transport_loopback_fixture(
    transport: FinalSendBindingTransport,
) -> TestResult {
    delivered_chat_history_transport_boundary_fixture(transport, true, false, None, None).await
}

async fn delivered_chat_history_transport_boundary_fixture(
    transport: FinalSendBindingTransport,
    active_authority: bool,
    late_policy: bool,
    observe_diagnostics: Option<&'static std::sync::atomic::AtomicBool>,
    redirect_status: Option<http::StatusCode>,
) -> TestResult {
    use super::{
        completion_json::JsonCompletionProtocol,
        http_restoration::{admit_http_request, DlpForwardError, RestorationAdmission},
        response_restoration::ArgumentNormalizationMode,
    };
    use crate::{
        app_config::AppType,
        provider::{LocalProxyRequestOverrides, ProviderMeta},
        proxy::{
            error::ProxyError,
            forwarder::{final_send_binding_test_support, RequestForwarder},
            providers::transform_codex_chat::chat_completion_to_response,
        },
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    const OUTPUT: &str = "Synthetic delivered answer rredact-custom- literal";
    const REASONING: &str = "Synthetic delivered thought rredact-custom- literal";
    const SESSION: &str = "synthetic-chat-history-loopback-session";
    const SAFE_INPUT: &str = "Synthetic inert history prompt";
    let prompt = if active_authority {
        FIXTURE_PHONE
    } else {
        SAFE_INPUT
    };

    let _ = rustls::crypto::ring::default_provider().install_default();
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "synthetic Chat history listener failed")?;
    let address = listener
        .local_addr()
        .map_err(|_| "synthetic Chat history address unavailable")?;
    let hits = Arc::new(AtomicUsize::new(0));
    let valid_requests = Arc::new(AtomicUsize::new(0));
    let redirect_next = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let redirected_hits = Arc::new(AtomicUsize::new(0));
    let redirected_body_bytes = Arc::new(AtomicUsize::new(0));
    let captured_hits = hits.clone();
    let captured_valid = valid_requests.clone();
    let captured_redirect = redirect_next.clone();
    let captured_redirected_hits = redirected_hits.clone();
    let captured_redirected_bytes = redirected_body_bytes.clone();
    let upstream = Router::new().fallback(move |request: http::Request<Body>| {
        let captured_hits = captured_hits.clone();
        let captured_valid = captured_valid.clone();
        let captured_redirect = captured_redirect.clone();
        let captured_redirected_hits = captured_redirected_hits.clone();
        let captured_redirected_bytes = captured_redirected_bytes.clone();
        async move {
            if request.uri().path() == "/synthetic-unbound-target" {
                captured_redirected_hits.fetch_add(1, Ordering::SeqCst);
                let size = axum::body::to_bytes(request.into_body(), 64 * 1024)
                    .await
                    .map_or(0, |body| body.len());
                captured_redirected_bytes.fetch_add(size, Ordering::SeqCst);
                return http::StatusCode::BAD_REQUEST.into_response();
            }
            if captured_redirect.swap(false, Ordering::SeqCst) {
                captured_hits.fetch_add(1, Ordering::SeqCst);
                let status = redirect_status.unwrap_or(http::StatusCode::TEMPORARY_REDIRECT);
                return (
                    status,
                    [(
                        http::header::LOCATION,
                        "/synthetic-unbound-target?operation=synthetic-changed",
                    )],
                )
                    .into_response();
            }
            // Keep only counts, never request text or credential diagnostics.
            captured_hits.fetch_add(1, Ordering::SeqCst);
            let valid_target = request.uri().path_and_query().is_some_and(|target| {
                target.as_str() == "/v1/chat/completions?operation=synthetic-history"
            });
            let valid_headers = request
                .headers()
                .get(http::header::AUTHORIZATION)
                .is_some_and(|value| {
                    value == concat!("Bearer ", "synthetic-chat-history-principal")
                })
                && request
                    .headers()
                    .get("chatgpt-account-id")
                    .is_some_and(|value| value == "synthetic-chat-history-account");
            let valid_body = match axum::body::to_bytes(request.into_body(), 64 * 1024).await {
                Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .is_some_and(|body| {
                        let tokenized_user = |message: &Value| {
                            message["role"] == "user"
                                && message["content"].as_str().is_some_and(|text| {
                                    if active_authority {
                                        text.contains("rredact-cn_phone-")
                                            && !text.contains(FIXTURE_PHONE)
                                    } else {
                                        text == SAFE_INPUT
                                    }
                                })
                        };
                        body["model"] == "synthetic-chat-history-upstream"
                            && body["prompt_cache_key"] == "synthetic-explicit-history-key"
                            && body["stream"] == false
                            && body.get("input").is_none()
                            && body["messages"].as_array().is_some_and(|messages| {
                                match messages.as_slice() {
                                    [user] => tokenized_user(user),
                                    [assistant, user] => {
                                        assistant["role"] == "assistant"
                                            && assistant["content"] == OUTPUT
                                            && assistant["reasoning_content"] == REASONING
                                            && tokenized_user(user)
                                    }
                                    [assistant, user, call, result] => {
                                        assistant["role"] == "assistant"
                                            && assistant["content"] == OUTPUT
                                            && assistant["reasoning_content"] == REASONING
                                            && tokenized_user(user)
                                            && call["role"] == "assistant"
                                            && call.get("reasoning_content").is_none()
                                            && call["tool_calls"][0]["id"]
                                                == "call_synthetic_unproven_cache"
                                            && call["tool_calls"][0]["function"]["arguments"]
                                                == "{}"
                                            && result["role"] == "tool"
                                            && result["tool_call_id"]
                                                == "call_synthetic_unproven_cache"
                                            && result["content"] == "Synthetic inert result"
                                    }
                                    [assistant, user, sibling] => {
                                        assistant["role"] == "assistant"
                                            && assistant["content"] == OUTPUT
                                            && assistant["reasoning_content"] == REASONING
                                            && user["role"] == "user"
                                            && user["content"] == "Synthetic inert continuation"
                                            && sibling["role"] == "assistant"
                                            && sibling["content"] == OUTPUT
                                    }
                                    _ => false,
                                }
                            })
                    }),
                Err(_) => false,
            };
            if valid_target && valid_headers && valid_body {
                captured_valid.fetch_add(1, Ordering::SeqCst);
            }
            Json(json!({
                "id":"chat_synthetic_delivered_history", "object":"chat.completion",
                "model":"synthetic-chat-history-upstream", "created":1,
                "choices":[{"index":0,"message":{"role":"assistant",
                    "content":OUTPUT,"reasoning_content":REASONING},"finish_reason":"stop"}]
            }))
            .into_response()
        }
    });
    let server = FinalSendBindingServer::new(tokio::spawn(async move {
        axum::serve(listener, upstream).await
    }));
    let proxy_state = state(Arc::new(
        Database::memory().map_err(|_| "synthetic Chat history database failed")?,
    ));
    let mut provider = Provider::with_id(
        "synthetic-chat-history-provider".into(),
        "Synthetic Chat history".into(),
        json!({"base_url":format!("http://{address}"),
            "api_key":"synthetic-chat-history-principal", "api_format":"chat_completions",
            "model":"synthetic-chat-history-upstream"}),
        None,
    );
    provider.meta = Some(ProviderMeta {
        prompt_cache_routing: Some("enabled".into()),
        ..Default::default()
    });
    let adapter = FinalSendBindingAdapter {
        loopback: LoopbackCodexAdapter(address),
        transport,
    };
    let mut delivered_output = Value::Null;
    // New-policy delivery is a fresh source, never a revival of the revoked proof.
    let stages: &[ChatHistoryLoopbackStage] = if redirect_status.is_some() {
        &[
            ChatHistoryLoopbackStage::InitialDelivery,
            ChatHistoryLoopbackStage::ExactReplay,
            ChatHistoryLoopbackStage::RedirectReplay,
            ChatHistoryLoopbackStage::ExactReplay,
        ]
    } else if late_policy {
        &[
            ChatHistoryLoopbackStage::InitialDelivery,
            ChatHistoryLoopbackStage::ExactReplay,
            ChatHistoryLoopbackStage::LatePolicy,
            ChatHistoryLoopbackStage::InitialDelivery,
            ChatHistoryLoopbackStage::ExactReplay,
        ]
    } else {
        &[
            ChatHistoryLoopbackStage::InitialDelivery,
            ChatHistoryLoopbackStage::ExactReplay,
            ChatHistoryLoopbackStage::OverrideContent,
            ChatHistoryLoopbackStage::OverrideReasoning,
            ChatHistoryLoopbackStage::OverrideSibling,
            ChatHistoryLoopbackStage::CachedReasoning,
            ChatHistoryLoopbackStage::LateAccount,
            ChatHistoryLoopbackStage::LateQuery,
            ChatHistoryLoopbackStage::ExactReplay,
        ]
    };
    for &stage in stages {
        hits.store(0, Ordering::SeqCst);
        valid_requests.store(0, Ordering::SeqCst);
        let initial = matches!(stage, ChatHistoryLoopbackStage::InitialDelivery);
        let redirecting = matches!(stage, ChatHistoryLoopbackStage::RedirectReplay);
        redirect_next.store(redirecting, Ordering::SeqCst);
        let permitted = initial
            || redirecting
            || matches!(
                stage,
                ChatHistoryLoopbackStage::ExactReplay
                    | ChatHistoryLoopbackStage::OverrideSibling
                    | ChatHistoryLoopbackStage::CachedReasoning
            );
        let mut body = json!({"model":"synthetic-chat-history-local-alias", "stream":false,
            "prompt_cache_key":" \u{2003}synthetic-explicit-history-key\t ",
            "input":if initial { json!(prompt) } else { delivered_output.clone() }});
        if !initial {
            body["input"]
                .as_array_mut()
                .ok_or("synthetic delivered output missing")?
                .push(json!({"role":"user","content":prompt}));
        }
        let mut stage_provider = provider.clone();
        if matches!(
            stage,
            ChatHistoryLoopbackStage::OverrideContent
                | ChatHistoryLoopbackStage::OverrideReasoning
                | ChatHistoryLoopbackStage::OverrideSibling
        ) {
            let mut messages = json!([
                {"role":"assistant","content":OUTPUT,"reasoning_content":REASONING},
                {"role":"user","content":"Synthetic inert continuation"}
            ]);
            match stage {
                ChatHistoryLoopbackStage::OverrideContent => {
                    messages[0]["content"] =
                        json!("Synthetic changed answer rredact-custom- literal");
                }
                ChatHistoryLoopbackStage::OverrideReasoning => {
                    messages[0]["reasoning_content"] =
                        json!("Synthetic changed thought rredact-custom- literal");
                }
                _ => messages
                    .as_array_mut()
                    .ok_or("synthetic override messages missing")?
                    .push(json!({"role":"assistant","content":OUTPUT})),
            }
            stage_provider
                .meta
                .as_mut()
                .ok_or("synthetic Chat metadata missing")?
                .local_proxy_request_overrides = Some(LocalProxyRequestOverrides {
                body: Some(json!({"messages":messages})),
                ..Default::default()
            });
        }
        if matches!(stage, ChatHistoryLoopbackStage::CachedReasoning) {
            // Independent readable history must not import an unrelated cached reasoning field.
            let cached = json!({"id":"resp_synthetic_unproven_cache","output":[{
                "id":"fc_synthetic_unproven_cache","type":"function_call",
                "call_id":"call_synthetic_unproven_cache","name":"synthetic_lookup",
                "arguments":"{}","reasoning_content":REASONING
            }]});
            assert!(
                proxy_state
                    .codex_chat_history
                    .record_response(&cached)
                    .await
                    == 1,
                "synthetic unproven cache entry must be present"
            );
            let input = body["input"]
                .as_array_mut()
                .ok_or("synthetic cache replay input missing")?;
            input.push(
                json!({"type":"function_call","call_id":"call_synthetic_unproven_cache",
                "name":"synthetic_lookup","arguments":"{}"}),
            );
            input.push(json!({"type":"function_call_output",
                "call_id":"call_synthetic_unproven_cache","output":"Synthetic inert result"}));
            let mut probe = body.clone();
            let (enriched, _owners) = proxy_state
                .codex_chat_history
                .enrich_request_with_payload_owners(&mut probe)
                .await
                .map_err(|_| "synthetic cache probe failed")?;
            assert!(
                enriched > 0
                    && probe["input"]
                        .as_array()
                        .is_some_and(|items| items.iter().any(|item| {
                            item["type"] == "function_call"
                                && item["reasoning_content"] == REASONING
                        })),
                "raw cache lookup must find reasoning that independent readable history must not import"
            );
        }
        let mut headers = http::HeaderMap::new();
        headers.insert("session-id", http::HeaderValue::from_static(SESSION));
        headers.insert(
            "chatgpt-account-id",
            http::HeaderValue::from_static("synthetic-chat-history-account"),
        );
        let request = match admit_http_request(
            &proxy_state.restoration_gateway,
            &AppType::Codex,
            true,
            false,
            &mut body,
            &headers,
        )
        .map_err(|_| "synthetic Chat history admission failed")?
        {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic Chat history request not admitted".into()),
        };
        let observe_audit = request.audit_observer_for_test();
        let forwarder = RequestForwarder::new(
            proxy_state.provider_router.clone(),
            5,
            proxy_state.status.clone(),
            proxy_state.current_providers.clone(),
            proxy_state.gemini_shadow.clone(),
            proxy_state.codex_chat_history.clone(),
            proxy_state.failover_manager.clone(),
            None,
            provider.id.clone(),
            SESSION.into(),
            true,
            5,
            5,
            Default::default(),
            Default::default(),
            Default::default(),
            0,
        )
        .with_dlp_request(Some(request));
        let reached = Arc::new(AtomicUsize::new(0));
        let observed_reached = reached.clone();
        let revision = runtime::admit_security_runtime()?.restoration_policy_revision;
        let mut changed_policy = (*runtime::get_security_runtime()).clone();
        changed_policy
            .builtin_action_overrides
            .insert("chinese_id_card".into(), super::types::AuditAction::Audit);
        let hook = final_send_binding_test_support::install(move |target, headers| {
            observed_reached.fetch_add(1, Ordering::SeqCst);
            match stage {
                ChatHistoryLoopbackStage::LateAccount => {
                    headers.insert(
                        "chatgpt-account-id",
                        http::HeaderValue::from_static("synthetic-chat-history-other-account"),
                    );
                }
                ChatHistoryLoopbackStage::LateQuery => {
                    target.push_str("&synthetic_late_scope=changed");
                }
                ChatHistoryLoopbackStage::LatePolicy => {
                    runtime::set_security_runtime_for_test(changed_policy.clone());
                }
                _ => {}
            }
        });
        if let Some(observe) = observe_diagnostics {
            // Empty initial traffic is neither leased nor proof-bearing yet.
            observe.store(active_authority || !initial, Ordering::SeqCst);
        }
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            forwarder.forward_with_adapter_for_test(
                &stage_provider,
                "/responses?operation=synthetic-history",
                &body,
                &headers,
                &adapter,
            ),
        )
        .await
        .map_err(|_| "synthetic Chat history forwarding timed out")?;
        if let Some(observe) = observe_diagnostics {
            observe.store(false, Ordering::SeqCst);
        }
        if matches!(stage, ChatHistoryLoopbackStage::LatePolicy) {
            assert!(
                runtime::admit_security_runtime()?.restoration_policy_revision > revision,
                "final send hook must publish a semantic policy revision before transport"
            );
        }
        let late_binding = matches!(
            stage,
            ChatHistoryLoopbackStage::LateAccount
                | ChatHistoryLoopbackStage::LateQuery
                | ChatHistoryLoopbackStage::LatePolicy
        );
        assert!(
            reached.load(Ordering::SeqCst) == usize::from(permitted || late_binding),
            "only prepared exact history may reach the actual late send-binding hook: {stage:?}"
        );
        if !permitted {
            assert!(
                hook.transport_counts() == (0, 0) && hits.load(Ordering::SeqCst) == 0,
                "unproven text or changed final authority must invoke neither transport nor listener"
            );
            assert!(
                matches!(
                    result,
                    Err(ProxyError::DlpForward(DlpForwardError::ContextUnavailable))
                ),
                "unproven Chat history must return only the fixed context refusal"
            );
            assert!(
                observe_audit()
                    .iter()
                    .all(|event| event.log.action_taken == "block"),
                "a refused history replay must not record a permitted request action"
            );
            continue;
        }
        assert!(
            match transport {
                FinalSendBindingTransport::Reqwest => hook.transport_counts() == (1, 0),
                FinalSendBindingTransport::Hyper => hook.transport_counts() == (0, 1),
            },
            "exact Chat history must reach the selected production transport once"
        );
        if redirecting {
            assert!(
                hits.load(Ordering::SeqCst) == 1
                    && redirected_hits.load(Ordering::SeqCst) == 0
                    && redirected_body_bytes.load(Ordering::SeqCst) == 0,
                "bound history must never resend its body to a redirected target"
            );
            assert!(
                matches!(result, Err(ProxyError::DlpForward(DlpForwardError::UpstreamStatus(status)))
                    if Some(status) == redirect_status),
                "bound redirect must return only a fixed upstream-status refusal"
            );
            continue;
        }
        let mut result = result.map_err(|_| "synthetic exact Chat history send refused")?;
        assert!(
            result.response.status() == http::StatusCode::OK
                && result.outbound_model.as_deref() == Some("synthetic-chat-history-upstream")
                && hits.load(Ordering::SeqCst) == 1
                && valid_requests.load(Ordering::SeqCst) == 1,
            "loopback must accept the exact converted history after model and explicit cache-key edits: {stage:?}; status={} hits={} valid={}", result.response.status().as_u16(), hits.load(Ordering::SeqCst), valid_requests.load(Ordering::SeqCst)
        );
        assert!(
            observe_audit().len() == usize::from(active_authority),
            "exact history must audit actual findings without inventing an empty-authority event"
        );
        let attempt = result
            .dlp_attempt
            .take()
            .ok_or("synthetic Chat history attempt missing")?;
        assert!(
            attempt.has_restoration_lease() == active_authority,
            "loopback fixture must exercise the requested token-authority branch"
        );
        let bytes = result
            .response
            .bytes_with_limit(64 * 1024)
            .await
            .map_err(|_| "synthetic Chat response read failed")?;
        if initial {
            // Use actual received Chat bytes and the real final-delivery wrapper, not a proof seed.
            let raw =
                std::str::from_utf8(&bytes).map_err(|_| "synthetic Chat response must be UTF-8")?;
            attempt
                .verify_upstream_json_completion(JsonCompletionProtocol::ChatCompletions, raw)
                .map_err(|_| "synthetic original Chat completion was not verified")?;
            let chat: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "synthetic Chat response must encode JSON")?;
            let converted = chat_completion_to_response(chat)
                .map_err(|_| "synthetic Chat response conversion failed")?;
            let proof_observer = attempt.lease.fork_attempt()?;
            let delivered_binding = attempt.binding.clone();
            let unpublished_replay = json!({"input":converted["output"]});
            assert!(
                proof_observer
                    .history_projection(&delivered_binding, &unpublished_replay)
                    .is_none(),
                "conversion before final delivery must not publish a history proof"
            );
            let response = attempt
                .restore_client_http_response(
                    http::Response::builder()
                        .header("content-type", "application/json")
                        .body(Body::from(converted.to_string()))?,
                    ArgumentNormalizationMode::Exact,
                    None,
                )
                .await;
            assert!(
                response.status() == http::StatusCode::OK,
                "actual Chat delivery must permit inert malformed synthetic output"
            );
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|_| "synthetic delivered Chat wrapper collection failed")?
                .to_bytes();
            let delivered: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "synthetic delivered Chat wrapper must encode JSON")?;
            assert!(
                delivered["status"] == "completed"
                    && delivered["output"]
                        .as_array()
                        .is_some_and(|items| items.len() == 2)
                    && delivered["output"][0]["type"] == "reasoning"
                    && delivered["output"][0]["summary"][0]["text"] == REASONING
                    && delivered["output"][1]["type"] == "message"
                    && delivered["output"][1]["content"][0]["text"] == OUTPUT,
                "only actually delivered typed reasoning and ordinary output may be replayed"
            );
            let delivered_replay = json!({"input":delivered["output"]});
            let projection = proof_observer
                .history_projection(&delivered_binding, &delivered_replay)
                .ok_or("actual loopback delivery must publish a nonempty history proof")?;
            use super::history_provenance::DlpHistoryLeafField as Field;
            assert!(
                projection
                    .source_for_responses_leaf(0, Field::ReasoningSummary, 0)
                    .is_some()
                    && projection
                        .source_for_responses_leaf(1, Field::OutputText, 0)
                        .is_some(),
                "empty and active authority must prove actual delivered reasoning and ordinary output"
            );
            delivered_output = delivered["output"].clone();
        }
    }
    server.finish().await?;
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_delivered_chat_history_reqwest_production_loopback() -> TestResult {
    delivered_chat_history_transport_loopback_fixture(FinalSendBindingTransport::Reqwest).await
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_delivered_chat_history_hyper_production_loopback() -> TestResult {
    delivered_chat_history_transport_loopback_fixture(FinalSendBindingTransport::Hyper).await
}

mod delivered_chat_history_reqwest_failover {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use super::super::{
        completion_json::JsonCompletionProtocol,
        history_provenance::DlpHistoryLeafField,
        http_restoration::{
            admit_http_request, source_copy_test_support, DlpForwardError, RestorationAdmission,
        },
        response_restoration::ArgumentNormalizationMode,
    };
    use crate::{
        app_config::AppType,
        proxy::{
            error::ProxyError,
            forwarder::{final_send_binding_test_support, RequestForwarder},
            providers::transform_codex_chat::chat_completion_to_response,
        },
    };

    const OUTPUT: &str = "Synthetic failover answer rredact-custom- literal";
    const REASONING: &str = "Synthetic failover thought rredact-custom- literal";
    const SAFE_INPUT: &str = "Synthetic inert failover prompt";
    const SESSION: &str = "synthetic-delivered-history-failover-session";

    fn spawn_upstream(
        listener: tokio::net::TcpListener,
        hits: Arc<AtomicUsize>,
        valid: Arc<AtomicUsize>,
        disconnect: Arc<AtomicBool>,
        deadline: tokio::time::Instant,
    ) -> FinalSendBindingServer {
        // The supervised task owns every connection; no detached connection tasks survive it.
        FinalSendBindingServer::new(tokio::spawn(async move {
            loop {
                let (socket, _) = tokio::time::timeout_at(deadline, listener.accept())
                    .await
                    .map_err(|_| std::io::Error::other("synthetic failover accept deadline"))??;
                hits.fetch_add(1, Ordering::SeqCst);
                let valid = valid.clone();
                let disconnect = disconnect.clone();
                let service = hyper::service::service_fn(
                    move |request: http::Request<hyper::body::Incoming>| {
                        let valid = valid.clone();
                        let disconnect = disconnect.clone();
                        async move {
                            let valid_target = request.method() == http::Method::POST
                                && request.uri().path_and_query().is_some_and(|target| {
                                    target.as_str()
                                        == "/v1/chat/completions?operation=synthetic-failover"
                                });
                            let valid_headers = request
                                .headers()
                                .get(http::header::AUTHORIZATION)
                                .is_some_and(|value| {
                                    value == concat!("Bearer ", "synthetic-failover-principal")
                                })
                                && request
                                    .headers()
                                    .get("chatgpt-account-id")
                                    .is_some_and(|value| value == "synthetic-failover-account");
                            let bytes =
                                axum::body::to_bytes(Body::new(request.into_body()), 64 * 1024)
                                    .await
                                    .map_err(|_| {
                                        std::io::Error::other(
                                            "synthetic failover request incomplete",
                                        )
                                    })?;
                            let valid_body = serde_json::from_slice::<Value>(&bytes)
                                .ok()
                                .is_some_and(|body| {
                                    body["model"] == "synthetic-failover-upstream"
                                        && body["stream"] == false
                                        && body.get("input").is_none()
                                        && body["messages"].as_array().is_some_and(|messages| {
                                            match messages.as_slice() {
                                                [user] => {
                                                    user["role"] == "user"
                                                        && user["content"] == SAFE_INPUT
                                                }
                                                [assistant, user] => {
                                                    assistant["role"] == "assistant"
                                                        && assistant["content"] == OUTPUT
                                                        && assistant["reasoning_content"]
                                                            == REASONING
                                                        && user["role"] == "user"
                                                        && user["content"].as_str().is_some_and(
                                                            |text| {
                                                                text.contains("rredact-cn_phone-")
                                                                    && !text.contains(FIXTURE_PHONE)
                                                            },
                                                        )
                                                }
                                                _ => false,
                                            }
                                        })
                                });
                            if !(valid_target && valid_headers && valid_body) {
                                return Err(std::io::Error::other(
                                    "synthetic failover request rejected",
                                ));
                            }
                            valid.fetch_add(1, Ordering::SeqCst);
                            if disconnect.load(Ordering::SeqCst) {
                                // A complete valid request is received before closing without HTTP headers.
                                return Err(std::io::Error::other(
                                    "synthetic failover disconnect before response",
                                ));
                            }
                            let mut response = Json(json!({
                                "id":"chat_synthetic_failover", "object":"chat.completion",
                                "model":"synthetic-failover-upstream", "created":1,
                                "choices":[{"index":0,"message":{"role":"assistant",
                                    "content":OUTPUT,"reasoning_content":REASONING},
                                    "finish_reason":"stop"}]
                            }))
                            .into_response();
                            response.headers_mut().insert(
                                http::header::CONNECTION,
                                http::HeaderValue::from_static("close"),
                            );
                            Ok::<_, std::io::Error>(response)
                        }
                    },
                );
                // A service error intentionally closes the socket, rather than sending a leased 503.
                let _ = tokio::time::timeout_at(
                    deadline,
                    hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(socket), service),
                )
                .await
                .map_err(|_| std::io::Error::other("synthetic failover connection deadline"))?;
            }
        }))
    }

    async fn fixture(replay_old_proof: bool) -> TestResult {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
        let work_deadline = deadline - Duration::from_secs(1);
        let _ = rustls::crypto::ring::default_provider().install_default();
        let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
        runtime::set_security_runtime_for_test(Default::default());
        let proxy_state = state(Arc::new(
            Database::memory().map_err(|_| "synthetic failover database failed")?,
        ));
        let listener_a =
            tokio::time::timeout_at(work_deadline, tokio::net::TcpListener::bind("127.0.0.1:0"))
                .await
                .map_err(|_| "synthetic failover listener deadline")?
                .map_err(|_| "synthetic failover listener failed")?;
        let listener_b =
            tokio::time::timeout_at(work_deadline, tokio::net::TcpListener::bind("127.0.0.1:0"))
                .await
                .map_err(|_| "synthetic failover listener deadline")?
                .map_err(|_| "synthetic failover listener failed")?;
        let address_a = listener_a
            .local_addr()
            .map_err(|_| "synthetic failover address unavailable")?;
        let address_b = listener_b
            .local_addr()
            .map_err(|_| "synthetic failover address unavailable")?;
        let provider_a = Provider::with_id(
            "synthetic-history-failover-a".into(),
            "Synthetic history failover".into(),
            json!({"base_url":format!("http://{address_a}"),
                "api_key":"synthetic-failover-principal", "api_format":"chat_completions",
                "model":"synthetic-failover-upstream"}),
            None,
        );
        let mut provider_b = provider_a.clone();
        provider_b.id = "synthetic-history-failover-b".into();
        provider_b.settings_config["base_url"] = json!(format!("http://{address_b}"));
        let hits_a = Arc::new(AtomicUsize::new(0));
        let hits_b = Arc::new(AtomicUsize::new(0));
        let valid_a = Arc::new(AtomicUsize::new(0));
        let valid_b = Arc::new(AtomicUsize::new(0));
        let disconnect_a = Arc::new(AtomicBool::new(false));
        let server_a = spawn_upstream(
            listener_a,
            hits_a.clone(),
            valid_a.clone(),
            disconnect_a.clone(),
            deadline,
        );
        let server_b = spawn_upstream(
            listener_b,
            hits_b.clone(),
            valid_b.clone(),
            Arc::new(AtomicBool::new(false)),
            deadline,
        );
        // All fallible work returns through the shared-deadline cleanup below, including timeouts.
        let outcome = tokio::time::timeout_at(work_deadline, async {
            let mut headers = http::HeaderMap::new();
            headers.insert("session-id", http::HeaderValue::from_static(SESSION));
            headers.insert(
                "chatgpt-account-id",
                http::HeaderValue::from_static("synthetic-failover-account"),
            );
            let make_forwarder = |request| {
                RequestForwarder::new(
                    proxy_state.provider_router.clone(),
                    5,
                    proxy_state.status.clone(),
                    proxy_state.current_providers.clone(),
                    proxy_state.gemini_shadow.clone(),
                    proxy_state.codex_chat_history.clone(),
                    proxy_state.failover_manager.clone(),
                    None,
                    provider_a.id.clone(),
                    SESSION.into(),
                    true,
                    5,
                    5,
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    1,
                )
                .with_dlp_request(Some(request))
            };
            let mut body = json!({"model":"synthetic-failover-local-alias",
                "input":SAFE_INPUT, "stream":false});
            let request = match admit_http_request(
                &proxy_state.restoration_gateway,
                &AppType::Codex,
                true,
                false,
                &mut body,
                &headers,
            )
            .map_err(|_| "synthetic failover delivery admission failed")?
            {
                RestorationAdmission::Admitted(request) => request,
                _ => return Err("synthetic failover delivery not admitted".into()),
            };
            // Reuse the existing no-proxy transport guard without changing the final target.
            let transport = final_send_binding_test_support::install(|_, _| {});
            let mut delivered = make_forwarder(request)
                .forward_with_retry(
                    &AppType::Codex,
                    http::Method::POST,
                    "/responses?operation=synthetic-failover",
                    body,
                    headers.clone(),
                    Default::default(),
                    vec![provider_a.clone()],
                )
                .await
                .map_err(|_| "synthetic failover initial delivery failed")?;
            if !(delivered.response.status() == http::StatusCode::OK
                && transport.transport_counts() == (1, 0)
                && hits_a.load(Ordering::SeqCst) == 1
                && valid_a.load(Ordering::SeqCst) == 1
                && hits_b.load(Ordering::SeqCst) == 0)
            {
                return Err("synthetic failover initial delivery invalid".into());
            }
            let attempt = delivered
                .dlp_attempt
                .take()
                .ok_or("synthetic failover delivery attempt missing")?;
            if attempt.has_restoration_lease() {
                return Err("synthetic failover initial delivery must be safe".into());
            }
            let bytes = delivered
                .response
                .bytes_with_limit(64 * 1024)
                .await
                .map_err(|_| "synthetic failover initial response incomplete")?;
            let raw = std::str::from_utf8(&bytes)
                .map_err(|_| "synthetic failover initial response invalid")?;
            attempt
                .verify_upstream_json_completion(JsonCompletionProtocol::ChatCompletions, raw)
                .map_err(|_| "synthetic failover completion unverified")?;
            let chat = serde_json::from_slice(&bytes)
                .map_err(|_| "synthetic failover response invalid")?;
            let converted = chat_completion_to_response(chat)
                .map_err(|_| "synthetic failover conversion failed")?;
            let observer = attempt
                .lease
                .fork_attempt()
                .map_err(|_| "synthetic failover proof observer unavailable")?;
            let binding = attempt.binding.clone();
            let replay = json!({"input":converted["output"]});
            if observer.history_projection(&binding, &replay).is_some() {
                return Err("synthetic failover conversion published proof prematurely".into());
            }
            // Only fully collected final client delivery may publish the proof replayed below.
            let response = attempt
                .restore_client_http_response(
                    http::Response::builder()
                        .header("content-type", "application/json")
                        .body(Body::from(converted.to_string()))
                        .map_err(|_| "synthetic failover wrapper construction failed")?,
                    ArgumentNormalizationMode::Exact,
                    None,
                )
                .await;
            if response.status() != http::StatusCode::OK {
                return Err("synthetic failover final delivery refused".into());
            }
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|_| "synthetic failover final delivery incomplete")?
                .to_bytes();
            let delivered: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "synthetic failover final delivery invalid")?;
            let replay = json!({"input":delivered["output"]});
            let projection = observer
                .history_projection(&binding, &replay)
                .ok_or("synthetic failover delivered proof missing")?;
            if !(delivered["status"] == "completed"
                && delivered["output"][0]["summary"][0]["text"] == REASONING
                && delivered["output"][1]["content"][0]["text"] == OUTPUT
                && projection
                    .source_for_responses_leaf(0, DlpHistoryLeafField::ReasoningSummary, 0)
                    .is_some()
                && projection
                    .source_for_responses_leaf(1, DlpHistoryLeafField::OutputText, 0)
                    .is_some())
            {
                return Err("synthetic failover delivered proof invalid".into());
            }
            drop(transport);
            hits_a.store(0, Ordering::SeqCst);
            valid_a.store(0, Ordering::SeqCst);
            disconnect_a.store(true, Ordering::SeqCst);
            let mut body = json!({"model":"synthetic-failover-local-alias",
                "input":SAFE_INPUT, "stream":false});
            if replay_old_proof {
                body["previous_response_id"] = delivered["id"].clone();
                body["input"] = delivered["output"].clone();
                body["input"]
                    .as_array_mut()
                    .ok_or("synthetic failover replay output missing")?
                    .push(json!({"role":"user", "content":FIXTURE_PHONE}));
            }
            let request = match admit_http_request(
                &proxy_state.restoration_gateway,
                &AppType::Codex,
                true,
                false,
                &mut body,
                &headers,
            )
            .map_err(|_| "synthetic failover replay admission failed")?
            {
                RestorationAdmission::Admitted(request) => request,
                _ => return Err("synthetic failover replay not admitted".into()),
            };
            let transport = final_send_binding_test_support::install(|_, _| {});
            let source = source_copy_test_support::install();
            let result = make_forwarder(request)
                .forward_with_retry(
                    &AppType::Codex,
                    http::Method::POST,
                    "/responses?operation=synthetic-failover",
                    body,
                    headers,
                    Default::default(),
                    vec![provider_a.clone(), provider_b.clone()],
                )
                .await;
            if !(source.snapshot().attempts == 2
                && hits_a.load(Ordering::SeqCst) == 1
                && valid_a.load(Ordering::SeqCst) == 1
                && provider_a.name == provider_b.name
                && provider_a.id != provider_b.id)
            {
                return Err(
                    "synthetic failover must prepare both providers after one complete send".into(),
                );
            }
            if replay_old_proof {
                if !(matches!(&result,
                    Err(failure) if failure.provider.is_none()
                        && matches!(failure.error,
                            ProxyError::DlpForward(DlpForwardError::ContextUnavailable)))
                    && transport.transport_counts() == (1, 0)
                    && hits_b.load(Ordering::SeqCst) == 0
                    && valid_b.load(Ordering::SeqCst) == 0)
                {
                    return Err(format!(
                        "synthetic old proof boundary mismatch: refusal={} transports={:?} backup_hits={} backup_valid={}",
                        result.is_err(), transport.transport_counts(), hits_b.load(Ordering::SeqCst), valid_b.load(Ordering::SeqCst)
                    ).into());
                }
            } else {
                let result = result.map_err(|_| "synthetic safe failover refused")?;
                if !(result.provider.id == provider_b.id
                    && result.response.status() == http::StatusCode::OK
                    && transport.transport_counts() == (2, 0)
                    && hits_b.load(Ordering::SeqCst) == 1
                    && valid_b.load(Ordering::SeqCst) == 1)
                {
                    return Err("synthetic safe failover must reach the backup once".into());
                }
                let bytes = result
                    .response
                    .bytes_with_limit(64 * 1024)
                    .await
                    .map_err(|_| "synthetic safe backup response incomplete")?;
                let response: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| "synthetic safe backup response invalid")?;
                if response["choices"][0]["message"]["content"] != OUTPUT {
                    return Err("synthetic safe backup response unexpected".into());
                }
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        })
        .await;
        // Preserve cleanup responsibility even when admission, forwarding or collection failed.
        let (cleanup_a, cleanup_b) =
            tokio::join!(server_a.finish_at(deadline), server_b.finish_at(deadline),);
        cleanup_a?;
        cleanup_b?;
        outcome.map_err(|_| "synthetic failover shared work deadline exceeded")??;
        Ok(())
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn reversible_http_delivered_chat_history_reqwest_retry_rejects_old_provider_proof(
    ) -> TestResult {
        fixture(true).await
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn reversible_http_delivered_chat_history_reqwest_retry_safe_input_reaches_backup(
    ) -> TestResult {
        fixture(false).await
    }
}

macro_rules! reasoning_history_late_policy_loopback_test {
    ($name:ident, $transport:ident, $active_authority:expr) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            delivered_chat_history_transport_boundary_fixture(
                FinalSendBindingTransport::$transport,
                $active_authority,
                true,
                None,
                None,
            )
            .await
        }
    };
}

reasoning_history_late_policy_loopback_test!(
    reasoning_history_late_policy_reqwest_active_authority,
    Reqwest,
    true
);
reasoning_history_late_policy_loopback_test!(
    reasoning_history_late_policy_hyper_active_authority,
    Hyper,
    true
);
reasoning_history_late_policy_loopback_test!(
    reasoning_history_late_policy_reqwest_empty_authority,
    Reqwest,
    false
);
reasoning_history_late_policy_loopback_test!(
    reasoning_history_late_policy_hyper_empty_authority,
    Hyper,
    false
);

mod history_diagnostic_marker_observer {
    use std::fmt::{self, Write};

    const MARKERS: [&[u8]; 3] = [b"[CacheTrace]", b"hash=", b"_hash="];

    // Retain only fixed-marker progress and booleans, never formatted log contents.
    #[derive(Default)]
    pub(super) struct MarkerProbe {
        active_prefixes: [usize; MARKERS.len()],
        pub(super) matched: bool,
    }
    impl Write for MarkerProbe {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            for byte in text.bytes() {
                for (index, marker) in MARKERS.iter().enumerate() {
                    // Track every matching prefix length, not just the longest:
                    // a mismatch can leave a shorter overlapping prefix alive.
                    let previous = self.active_prefixes[index];
                    let mut next = usize::from(byte == marker[0]);
                    for (prefix_len, expected) in marker.iter().enumerate().skip(1) {
                        if previous & (1usize << (prefix_len - 1)) != 0 && byte == *expected {
                            if prefix_len + 1 == marker.len() {
                                self.matched = true;
                            } else {
                                next |= 1usize << prefix_len;
                            }
                        }
                    }
                    self.active_prefixes[index] = next;
                    if marker.len() == 1 && next != 0 {
                        self.matched = true;
                    }
                }
            }
            Ok(())
        }
    }

    #[test]
    pub(super) fn reasoning_history_diagnostic_marker_observer() -> super::TestResult {
        for marker in MARKERS {
            let marker = std::str::from_utf8(marker)?;
            let mut probe = MarkerProbe::default();
            for byte in marker.as_bytes() {
                probe.write_str(std::str::from_utf8(std::slice::from_ref(byte))?)?;
            }
            assert!(
                probe.matched,
                "streaming diagnostic marker observer must detect split fields"
            );
            // Only fixed field-name bytes enter this self-check. A partial prefix
            // followed by a complete marker must survive every formatting split.
            for prefix_len in 1..marker.len() {
                for split in 0..=marker.len() {
                    let mut probe = MarkerProbe::default();
                    probe.write_str(&marker[..prefix_len])?;
                    assert!(
                        !probe.matched,
                        "partial diagnostic field must not count as a complete match"
                    );
                    probe.write_str(&marker[..split])?;
                    probe.write_str(&marker[split..])?;
                    assert!(
                        probe.matched,
                        "streaming observer must recover overlapping field prefixes across splits"
                    );
                }
            }
        }
        let mut benign = MarkerProbe::default();
        benign.write_str("synthetic forwarding metadata")?;
        assert!(
            !benign.matched,
            "marker observer must not reject fixed benign metadata"
        );
        Ok(())
    }
}

type HistoryDiagnosticChildResult = Result<std::process::ExitStatus, &'static str>;

struct HistoryDiagnosticChild {
    process: Option<tokio::process::Child>,
    cleanup: Option<tokio::task::JoinHandle<(HistoryDiagnosticChildResult, tokio::process::Child)>>,
}

impl HistoryDiagnosticChild {
    fn new(process: tokio::process::Child) -> Self {
        Self {
            process: Some(process),
            cleanup: None,
        }
    }
}

async fn cleanup_history_diagnostic_child(
    child: &mut tokio::process::Child,
    deadline: tokio::time::Instant,
) -> Result<(), &'static str> {
    let killed = child.start_kill();
    finish_history_diagnostic_cleanup(killed, child.wait(), deadline).await
}

async fn finish_history_diagnostic_cleanup<T>(
    killed: std::io::Result<()>,
    reaped: impl std::future::Future<Output = std::io::Result<T>>,
    deadline: tokio::time::Instant,
) -> Result<(), &'static str> {
    match tokio::time::timeout_at(deadline, reaped).await {
        Ok(Ok(_)) if killed.is_ok() => Ok(()),
        Ok(Ok(_)) => Err("isolated diagnostic fixture kill failed; output discarded"),
        Ok(Err(_)) => Err("isolated diagnostic fixture reap failed; output discarded"),
        Err(_) => Err("isolated diagnostic fixture cleanup timed out; output discarded"),
    }
}

fn history_diagnostic_cleanup_task_error(_: tokio::task::JoinError) -> &'static str {
    "isolated diagnostic fixture cleanup task failed; output discarded"
}

async fn wait_history_diagnostic_child(
    child: &mut HistoryDiagnosticChild,
    observation_timeout: std::time::Duration,
    cleanup_timeout: std::time::Duration,
) -> HistoryDiagnosticChildResult {
    let mut process = child
        .process
        .take()
        .ok_or("isolated diagnostic fixture already observed; output discarded")?;
    let (cancel_on_drop, mut cancelled) = tokio::sync::oneshot::channel::<()>();
    let observation_deadline = tokio::time::Instant::now() + observation_timeout;
    let total_deadline = observation_deadline + cleanup_timeout;
    child.cleanup = Some(tokio::spawn(async move {
        let result = async {
            let observation_error = tokio::select! {
                _ = &mut cancelled => "isolated diagnostic fixture observation cancelled; output discarded",
                result = tokio::time::timeout_at(observation_deadline, process.wait()) => {
                    match result {
                        Ok(Ok(status)) => return Ok(status),
                        Ok(Err(_)) => "isolated diagnostic fixture observation failed; output discarded",
                        Err(_) => "isolated diagnostic fixture observation timed out; output discarded",
                    }
                }
            };
            let cleanup_deadline = total_deadline
                .min(tokio::time::Instant::now() + cleanup_timeout);
            cleanup_history_diagnostic_child(&mut process, cleanup_deadline).await?;
            Err(observation_error)
        }
        .await;
        (result, process)
    }));
    let (result, process) = match child.cleanup.as_mut() {
        Some(cleanup) => cleanup
            .await
            .map_err(history_diagnostic_cleanup_task_error)?,
        None => return Err("isolated diagnostic fixture cleanup unavailable; output discarded"),
    };
    drop(cancel_on_drop);
    child.cleanup.take();
    child.process = Some(process);
    result
}

#[tokio::test]
async fn reasoning_history_diagnostic_child_cleanup_failures() -> TestResult {
    use std::{
        future, io,
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };
    for kill_fails in [false, true] {
        for reap_fails in [false, true] {
            let reaped = AtomicBool::new(false);
            let killed = if kill_fails {
                Err(io::Error::other("synthetic kill failure"))
            } else {
                Ok(())
            };
            let reap = async {
                reaped.store(true, Ordering::SeqCst);
                if reap_fails {
                    Err(io::Error::other("synthetic reap failure"))
                } else {
                    Ok(())
                }
            };
            let result = finish_history_diagnostic_cleanup(
                killed,
                reap,
                tokio::time::Instant::now() + Duration::from_millis(200),
            )
            .await;
            assert!(
                reaped.load(Ordering::SeqCst),
                "reap must be attempted even after kill failure"
            );
            let expected = if reap_fails {
                Err("isolated diagnostic fixture reap failed; output discarded")
            } else if kill_fails {
                Err("isolated diagnostic fixture kill failed; output discarded")
            } else {
                Ok(())
            };
            assert_eq!(result, expected);
        }
        let killed = if kill_fails {
            Err(io::Error::other("synthetic kill failure"))
        } else {
            Ok(())
        };
        let started = std::time::Instant::now();
        let result = finish_history_diagnostic_cleanup(
            killed,
            future::pending::<io::Result<()>>(),
            tokio::time::Instant::now() + Duration::from_millis(20),
        )
        .await;
        assert_eq!(
            result,
            Err("isolated diagnostic fixture cleanup timed out; output discarded")
        );
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "pending reap must remain bounded"
        );
    }
    let task = tokio::spawn(future::pending::<()>());
    task.abort();
    let error = task
        .await
        .err()
        .ok_or("synthetic cancelled task must fail")?;
    assert_eq!(
        history_diagnostic_cleanup_task_error(error),
        "isolated diagnostic fixture cleanup task failed; output discarded"
    );
    Ok(())
}

#[tokio::test]
async fn reasoning_history_diagnostic_child_deadline() -> TestResult {
    const CHILD: &str = "VARYAI_SYNTHETIC_HISTORY_DEADLINE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        std::process::exit(87);
    }
    let process = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "proxy::security::reversible_integration_tests::reasoning_history_diagnostic_child_deadline",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut child = HistoryDiagnosticChild::new(process);
    let started = std::time::Instant::now();
    let result = wait_history_diagnostic_child(
        &mut child,
        std::time::Duration::from_millis(40),
        std::time::Duration::from_millis(200),
    )
    .await;
    assert!(
        result.is_err(),
        "observation deadline must reject a still-running child"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_millis(300),
        "observation and cleanup must share a bounded total duration"
    );
    let process = child
        .process
        .as_mut()
        .ok_or("missing reaped fixture child")?;
    assert!(
        process.try_wait()?.is_some(),
        "timed-out child must be reaped before returning"
    );
    Ok(())
}

#[tokio::test]
async fn reasoning_history_diagnostic_child_cancellation() -> TestResult {
    const CHILD: &str = "VARYAI_SYNTHETIC_HISTORY_CANCEL_CHILD";
    if std::env::var_os(CHILD).is_some() {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        std::process::exit(87);
    }
    let process = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "proxy::security::reversible_integration_tests::reasoning_history_diagnostic_child_cancellation",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut child = HistoryDiagnosticChild::new(process);
    let cancelled = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        wait_history_diagnostic_child(
            &mut child,
            std::time::Duration::from_secs(1),
            std::time::Duration::from_millis(200),
        ),
    )
    .await;
    assert!(
        cancelled.is_err(),
        "synthetic observation must be cancelled"
    );
    let cleanup = child
        .cleanup
        .as_mut()
        .ok_or("missing fixture cleanup task")?;
    let (result, mut process) =
        tokio::time::timeout(std::time::Duration::from_millis(250), cleanup).await??;
    assert_eq!(
        result.err(),
        Some("isolated diagnostic fixture observation cancelled; output discarded")
    );
    assert!(
        process.try_wait()?.is_some(),
        "cancelled observation must terminate and reap its child"
    );
    child.cleanup.take();
    Ok(())
}

#[tokio::test]
async fn reasoning_history_diagnostic_child_success_and_cleanup_control() -> TestResult {
    const CHILD: &str = "VARYAI_SYNTHETIC_HISTORY_CONTROL_CHILD";
    if let Some(mode) = std::env::var_os(CHILD) {
        if mode == "slow" {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        std::process::exit(87);
    }
    for mode in ["fast", "slow"] {
        let process = tokio::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "proxy::security::reversible_integration_tests::reasoning_history_diagnostic_child_success_and_cleanup_control",
                "--test-threads=1",
            ])
            .env(CHILD, mode)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut child = HistoryDiagnosticChild::new(process);
        if mode == "fast" {
            let status = wait_history_diagnostic_child(
                &mut child,
                std::time::Duration::from_secs(2),
                std::time::Duration::from_millis(200),
            )
            .await?;
            assert_eq!(
                status.code(),
                Some(87),
                "successful child verdict must be preserved"
            );
        } else {
            let process = child.process.as_mut().ok_or("missing fixture child")?;
            cleanup_history_diagnostic_child(
                process,
                tokio::time::Instant::now() + std::time::Duration::from_millis(200),
            )
            .await?;
        }
        let process = child
            .process
            .as_mut()
            .ok_or("missing reaped fixture child")?;
        assert!(
            process.try_wait()?.is_some(),
            "control child must be reaped"
        );
    }
    Ok(())
}

async fn history_forwarding_diagnostic_fixture(
    transport: FinalSendBindingTransport,
    active_authority: bool,
    test_name: &'static str,
) -> TestResult {
    use std::{
        fmt,
        process::Stdio,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    const CHILD: &str = "VARYAI_SYNTHETIC_HISTORY_LOG_TEST";
    const FORBIDDEN_DIAGNOSTIC_EXIT: i32 = 86;
    const OBSERVER_SUCCESS_EXIT: i32 = 87;
    if std::env::var_os(CHILD).is_none() {
        let process = tokio::process::Command::new(std::env::current_exe()?)
            .args(["--exact", test_name, "--test-threads=1"])
            .env(CHILD, "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut child = HistoryDiagnosticChild::new(process);
        let status = wait_history_diagnostic_child(
            &mut child,
            std::time::Duration::from_secs(55),
            std::time::Duration::from_secs(5),
        )
        .await?;
        if status.code() == Some(FORBIDDEN_DIAGNOSTIC_EXIT) {
            panic!("isolated observer detected a forbidden diagnostic field; output discarded");
        }
        assert!(
            status.code() == Some(OBSERVER_SUCCESS_EXIT),
            "isolated diagnostic fixture failed before its observation verdict; output discarded"
        );
        return Ok(());
    }

    static OBSERVING: AtomicBool = AtomicBool::new(false);
    static FORBIDDEN: AtomicBool = AtomicBool::new(false);
    static OBSERVED: AtomicUsize = AtomicUsize::new(0);
    use history_diagnostic_marker_observer::MarkerProbe;

    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            OBSERVING.load(Ordering::SeqCst) && metadata.target().ends_with("::proxy::forwarder")
        }
        fn log(&self, record: &log::Record<'_>) {
            if !self.enabled(record.metadata()) {
                return;
            }
            OBSERVED.fetch_add(1, Ordering::SeqCst);
            let mut probe = MarkerProbe::default();
            if fmt::write(&mut probe, *record.args()).is_err() || probe.matched {
                FORBIDDEN.store(true, Ordering::SeqCst);
            }
        }
        fn flush(&self) {}
    }

    history_diagnostic_marker_observer::reasoning_history_diagnostic_marker_observer()?;

    static LOGGER: Logger = Logger;
    log::set_logger(&LOGGER).map_err(|_| "isolated leased diagnostic logger unavailable")?;
    log::set_max_level(log::LevelFilter::Debug);
    // Verify the observer independently; a protected transport may be entirely silent.
    OBSERVING.store(true, Ordering::SeqCst);
    log::debug!(target: "cc_switch_lib::proxy::forwarder", "synthetic diagnostic observer control");
    OBSERVING.store(false, Ordering::SeqCst);
    assert_eq!(
        OBSERVED.swap(0, Ordering::SeqCst),
        1,
        "fixed control must reach the diagnostic observer exactly once"
    );
    assert!(
        !FORBIDDEN.load(Ordering::SeqCst),
        "fixed observer control must not contain a forbidden field"
    );
    delivered_chat_history_transport_boundary_fixture(
        transport,
        active_authority,
        false,
        Some(&OBSERVING),
        None,
    )
    .await?;
    if FORBIDDEN.load(Ordering::SeqCst) {
        // Distinguish an observed contract violation from any fixture/harness failure.
        std::process::exit(FORBIDDEN_DIAGNOSTIC_EXIT);
    }
    // A zero-tests subprocess must not masquerade as an observed success.
    std::process::exit(OBSERVER_SUCCESS_EXIT);
}

macro_rules! reasoning_history_diagnostic_test {
    ($name:ident, $transport:ident, $active_authority:expr) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            history_forwarding_diagnostic_fixture(
                FinalSendBindingTransport::$transport,
                $active_authority,
                concat!(
                    "proxy::security::reversible_integration_tests::",
                    stringify!($name)
                ),
            )
            .await
        }
    };
}

reasoning_history_diagnostic_test!(
    reasoning_history_leased_forwarding_reqwest_omits_content_hash_diagnostics,
    Reqwest,
    true
);
reasoning_history_diagnostic_test!(
    reasoning_history_leased_forwarding_hyper_omits_content_hash_diagnostics,
    Hyper,
    true
);
reasoning_history_diagnostic_test!(
    reasoning_history_leased_forwarding_reqwest_empty_authority_omits_content_hash_diagnostics,
    Reqwest,
    false
);
reasoning_history_diagnostic_test!(
    reasoning_history_leased_forwarding_hyper_empty_authority_omits_content_hash_diagnostics,
    Hyper,
    false
);

macro_rules! reasoning_history_redirect_loopback_test {
    ($name:ident, $transport:ident, $active:expr, $status:ident) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            delivered_chat_history_transport_boundary_fixture(
                FinalSendBindingTransport::$transport,
                $active,
                false,
                None,
                Some(http::StatusCode::$status),
            )
            .await
        }
    };
}

reasoning_history_redirect_loopback_test!(
    reasoning_history_redirect_reqwest_active_307,
    Reqwest,
    true,
    TEMPORARY_REDIRECT
);
reasoning_history_redirect_loopback_test!(
    reasoning_history_redirect_reqwest_empty_307,
    Reqwest,
    false,
    TEMPORARY_REDIRECT
);
reasoning_history_redirect_loopback_test!(
    reasoning_history_redirect_reqwest_active_308,
    Reqwest,
    true,
    PERMANENT_REDIRECT
);
reasoning_history_redirect_loopback_test!(
    reasoning_history_redirect_reqwest_empty_308,
    Reqwest,
    false,
    PERMANENT_REDIRECT
);
reasoning_history_redirect_loopback_test!(
    reasoning_history_redirect_hyper_active_307,
    Hyper,
    true,
    TEMPORARY_REDIRECT
);
reasoning_history_redirect_loopback_test!(
    reasoning_history_redirect_hyper_empty_307,
    Hyper,
    false,
    TEMPORARY_REDIRECT
);
reasoning_history_redirect_loopback_test!(
    reasoning_history_redirect_hyper_active_308,
    Hyper,
    true,
    PERMANENT_REDIRECT
);
reasoning_history_redirect_loopback_test!(
    reasoning_history_redirect_hyper_empty_308,
    Hyper,
    false,
    PERMANENT_REDIRECT
);

mod legacy_diagnostic_observer {
    use std::fmt;

    struct ExactProbe<'a> {
        expected: &'a [u8],
        offset: usize,
        failed: bool,
    }

    impl fmt::Write for ExactProbe<'_> {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            if self.failed {
                return Err(fmt::Error);
            }
            for byte in text.bytes() {
                if self.expected.get(self.offset) != Some(&byte) {
                    self.failed = true;
                    return Err(fmt::Error);
                }
                self.offset += 1;
            }
            Ok(())
        }
    }

    pub(super) fn exact(args: fmt::Arguments<'_>, expected: &str) -> bool {
        let mut probe = ExactProbe {
            expected: expected.as_bytes(),
            offset: 0,
            failed: false,
        };
        fmt::write(&mut probe, args).is_ok()
            && !probe.failed
            && probe.offset == probe.expected.len()
    }

    struct ForbiddenProbe<'a, const N: usize> {
        markers: &'a [&'a str; N],
        prefixes: [usize; N],
        failed: bool,
    }

    impl<const N: usize> fmt::Write for ForbiddenProbe<'_, N> {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            if self.failed {
                self.failed = true;
                return Err(fmt::Error);
            }
            for byte in text.bytes() {
                for (index, marker) in self.markers.iter().enumerate() {
                    let marker = marker.as_bytes();
                    if marker.is_empty() || marker.len() > usize::BITS as usize {
                        self.failed = true;
                        return Err(fmt::Error);
                    }
                    let previous = self.prefixes[index];
                    let mut next = usize::from(byte == marker[0]);
                    if marker.len() == 1 && next != 0 {
                        self.failed = true;
                        return Err(fmt::Error);
                    }
                    for (prefix, expected) in marker.iter().enumerate().skip(1) {
                        if previous & (1usize << (prefix - 1)) != 0 && byte == *expected {
                            if prefix + 1 == marker.len() {
                                self.failed = true;
                                return Err(fmt::Error);
                            }
                            next |= 1usize << prefix;
                        }
                    }
                    self.prefixes[index] = next;
                }
            }
            Ok(())
        }
    }

    pub(super) fn forbidden<const N: usize>(args: fmt::Arguments<'_>, markers: &[&str; N]) -> bool {
        let mut probe = ForbiddenProbe {
            markers,
            prefixes: [0; N],
            failed: false,
        };
        fmt::write(&mut probe, args).is_err() || probe.failed
    }

    pub(super) fn verdict(code: Option<i32>) -> Result<(), &'static str> {
        match code {
            Some(87) => Ok(()),
            Some(86) => Err("isolated observer detected forbidden diagnostics; output discarded"),
            _ => Err("isolated fixture ended without an observation verdict; output discarded"),
        }
    }

    pub(super) async fn parent(
        child_flag: &str,
        test_name: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if std::env::var_os(child_flag).is_some() {
            return Ok(false);
        }
        let process = tokio::process::Command::new(std::env::current_exe()?)
            .args(["--exact", test_name, "--test-threads=1"])
            .env(child_flag, "1")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut child = super::HistoryDiagnosticChild::new(process);
        let status = super::wait_history_diagnostic_child(
            &mut child,
            std::time::Duration::from_secs(55),
            std::time::Duration::from_secs(5),
        )
        .await?;
        verdict(status.code())?;
        Ok(true)
    }

    struct TailAfterMismatch<'a> {
        first: &'a str,
        tail_visited: &'a std::cell::Cell<bool>,
    }

    impl fmt::Display for TailAfterMismatch<'_> {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.first)?;
            self.tail_visited.set(true);
            formatter.write_str("synthetic unneeded diagnostic tail")
        }
    }

    #[test]
    fn legacy_diagnostic_exact_stops_before_unneeded_tail() {
        let visited = std::cell::Cell::new(false);
        let value = TailAfterMismatch {
            first: "synthetic mismatch",
            tail_visited: &visited,
        };
        assert!(!exact(format_args!("{value}"), "fixed checkpoint"));
        assert!(
            !visited.get(),
            "exact observer must stop at the first mismatch"
        );
        assert!(exact(
            format_args!("{}{}", "fixed ", "checkpoint"),
            "fixed checkpoint"
        ));
        assert!(!exact(
            format_args!("fixed checkpoint suffix"),
            "fixed checkpoint"
        ));
        assert!(!exact(format_args!("fixed"), "fixed checkpoint"));
    }

    #[test]
    fn legacy_diagnostic_forbidden_stops_before_unneeded_tail() {
        let visited = std::cell::Cell::new(false);
        let value = TailAfterMismatch {
            first: "synthetic-forbidden-field",
            tail_visited: &visited,
        };
        assert!(forbidden(
            format_args!("{value}"),
            &["synthetic-forbidden-field"]
        ));
        assert!(
            !visited.get(),
            "forbidden observer must stop before retaining a tail"
        );
        for split in 0..="abab".len() {
            assert!(forbidden(
                format_args!("aba{}{}", &"abab"[..split], &"abab"[split..]),
                &["abab"]
            ));
        }
        assert!(!forbidden(
            format_args!("synthetic safe metadata"),
            &["abab"]
        ));
    }

    #[test]
    fn legacy_diagnostic_verdict_rejects_zero_tests_and_failures() {
        for code in [Some(0), Some(1), Some(86), None] {
            assert!(
                verdict(code).is_err(),
                "only an explicit observation verdict may pass"
            );
        }
        assert!(
            verdict(Some(87)).is_ok(),
            "explicit observation success must pass"
        );
    }

    #[tokio::test]
    async fn legacy_diagnostic_parent_rejects_actual_zero_test_child() -> super::TestResult {
        assert!(parent("VARYAI_SYNTHETIC_LEGACY_ZERO_TEST_CHILD",
            "proxy::security::reversible_integration_tests::legacy_diagnostic_observer::nonexistent_synthetic_test")
            .await.is_err(), "a successful zero-tests subprocess must not pass");
        Ok(())
    }

    #[tokio::test]
    async fn legacy_diagnostic_parent_rejects_actual_fixture_error() -> super::TestResult {
        const CHILD: &str = "VARYAI_SYNTHETIC_LEGACY_ERROR_CHILD";
        if std::env::var_os(CHILD).is_some() {
            return Err("synthetic isolated fixture failure".into());
        }
        assert!(parent(CHILD,
            "proxy::security::reversible_integration_tests::legacy_diagnostic_observer::legacy_diagnostic_parent_rejects_actual_fixture_error")
            .await.is_err(), "a fixture failure must not pass as a verdict");
        Ok(())
    }

    #[tokio::test]
    async fn legacy_diagnostic_parent_accepts_actual_explicit_success() -> super::TestResult {
        const CHILD: &str = "VARYAI_SYNTHETIC_LEGACY_SUCCESS_CHILD";
        if std::env::var_os(CHILD).is_some() {
            std::process::exit(87);
        }
        assert!(parent(CHILD,
            "proxy::security::reversible_integration_tests::legacy_diagnostic_observer::legacy_diagnostic_parent_accepts_actual_explicit_success")
            .await?, "the explicit successful observation must reach the parent");
        Ok(())
    }

    #[test]
    fn legacy_diagnostic_format_error_cannot_clear_observation_failure() {
        struct IgnoresWriteError;
        impl fmt::Display for IgnoresWriteError {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                let _ = formatter.write_str("synthetic-forbidden-field");
                formatter.write_str("fixed checkpoint")
            }
        }
        assert!(
            !exact(format_args!("{}", IgnoresWriteError), "fixed checkpoint"),
            "ignored formatter errors must not clear an exact mismatch"
        );
        assert!(
            forbidden(
                format_args!("{}", IgnoresWriteError),
                &["synthetic-forbidden-field"]
            ),
            "ignored formatter errors must not clear a forbidden observation"
        );
    }
}

async fn drain_xai_json_observations(
    queue: &mut crate::proxy::tool_observation::channel::ObservationReceiver,
    deadline: tokio::time::Instant,
) -> Result<usize, &'static str> {
    let mut drained = 0;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err("synthetic xAI observation drain deadline exceeded");
        }
        let quiet_deadline =
            deadline.min(tokio::time::Instant::now() + std::time::Duration::from_millis(10));
        match tokio::time::timeout_at(quiet_deadline, queue.recv()).await {
            Ok(Some(piece)) => {
                drop(piece);
                drained += 1;
                tokio::task::yield_now().await;
            }
            Ok(None) => return Ok(drained),
            Err(_) if tokio::time::Instant::now() >= deadline => {
                return Err("synthetic xAI observation drain deadline exceeded");
            }
            Err(_) => return Ok(drained),
        }
    }
}

#[cfg(test)]
mod xai_json_fixture_boundary_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::time::{timeout, Duration, Instant};

    #[tokio::test]
    async fn xai_json_observation_drain_releases_queued_state() -> TestResult {
        use crate::proxy::tool_observation::channel::{
            create_channel, OutputQuality, ResponseMetadata, Transport,
        };
        let (sender, mut queue) = create_channel(2);
        let mut tap = sender
            .start(
                ResponseMetadata {
                    protocol: "openai_responses",
                    client: "codex",
                    transport: Transport::Json,
                    quality: OutputQuality::Native,
                    definitions: Arc::new(Vec::new()),
                },
                917,
            )
            .ok_or("synthetic observation reservation unavailable")?;
        let retained = Arc::downgrade(&tap.state);
        tap.push(&bytes::Bytes::from_static(b"synthetic observation piece"));
        drop(tap);
        let drained = timeout(
            Duration::from_millis(200),
            drain_xai_json_observations(&mut queue, Instant::now() + Duration::from_secs(1)),
        )
        .await??;
        assert!(
            drained == 1 && retained.upgrade().is_none(),
            "draining must release the last queued response-state owner"
        );
        Ok(())
    }

    #[tokio::test]
    async fn xai_json_observation_drain_accepts_live_quiet_queue() -> TestResult {
        let (_sender, mut queue) = crate::proxy::tool_observation::channel::create_channel(2);
        let drained = timeout(
            Duration::from_millis(200),
            drain_xai_json_observations(&mut queue, Instant::now() + Duration::from_secs(1)),
        )
        .await??;
        assert!(
            drained == 0,
            "a live quiet queue must finish without inventing a piece"
        );
        Ok(())
    }

    #[tokio::test]
    async fn xai_json_observation_drain_rejects_total_deadline() -> TestResult {
        let (_sender, mut queue) = crate::proxy::tool_observation::channel::create_channel(2);
        let past = drain_xai_json_observations(&mut queue, Instant::now()).await;
        let imminent = timeout(
            Duration::from_millis(200),
            drain_xai_json_observations(&mut queue, Instant::now() + Duration::from_millis(1)),
        )
        .await?;
        assert!(
            past.is_err() && imminent.is_err(),
            "an exhausted shared deadline must not masquerade as a quiet queue"
        );
        Ok(())
    }

    #[tokio::test]
    async fn xai_json_closed_observation_queue_terminates() -> TestResult {
        let (sender, mut queue) = crate::proxy::tool_observation::channel::create_channel(2);
        drop(sender);
        let result = timeout(
            Duration::from_millis(40),
            drain_xai_json_observations(&mut queue, Instant::now() + Duration::from_secs(1)),
        )
        .await;
        assert!(
            matches!(result, Ok(Ok(0))),
            "closed observation queues must terminate instead of spinning or timing out"
        );
        Ok(())
    }

    #[tokio::test]
    async fn xai_json_finish_confirms_task_resource_release() -> TestResult {
        struct Release(Arc<AtomicUsize>);
        impl Drop for Release {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let released = Arc::new(AtomicUsize::new(0));
        let task_release = released.clone();
        let deadline = Instant::now() + Duration::from_secs(2);
        let (ready, received) = tokio::sync::oneshot::channel();
        let task = FinalSendBindingServer::new(tokio::spawn(async move {
            let _release = Release(task_release);
            let _ = ready.send(());
            std::future::pending::<std::io::Result<()>>().await
        }));
        tokio::time::timeout_at(deadline, received).await??;
        task.finish_at(deadline).await?;
        assert!(
            released.load(Ordering::SeqCst) == 1,
            "successful xAI fixture cleanup must confirm task-owned resource release"
        );
        Ok(())
    }

    #[tokio::test]
    async fn xai_json_finish_reports_task_failure() -> TestResult {
        let handle = tokio::spawn(async {
            panic!("synthetic xAI fixture task failure");
        });
        let finished = handle.abort_handle();
        let task = FinalSendBindingServer::new(handle);
        let deadline = Instant::now() + Duration::from_secs(2);
        tokio::time::timeout_at(deadline, async {
            while !finished.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        assert!(
            task.finish_at(deadline).await.is_err(),
            "a failed xAI fixture task must not report cleanup success"
        );
        Ok(())
    }
}

async fn converter_warning_diagnostic_fixture(
    private_finish: bool,
    test_name: &'static str,
) -> TestResult {
    use crate::proxy::{
        providers::{
            streaming_codex_chat::create_responses_sse_stream_from_chat_observed,
            transform_codex_chat::CodexToolContext,
        },
        security::{
            http_restoration::{admit_http_request, RestorationAdmission},
            response_restoration::ArgumentNormalizationMode,
        },
    };
    use std::{
        process::Stdio,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    const CHILD: &str = "VARYAI_SYNTHETIC_CONVERTER_DIAGNOSTIC";
    if std::env::var_os(CHILD).is_none() {
        let process = tokio::process::Command::new(std::env::current_exe()?)
            .args(["--exact", test_name, "--test-threads=1"])
            .env(CHILD, "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut child = HistoryDiagnosticChild::new(process);
        let status = wait_history_diagnostic_child(
            &mut child,
            std::time::Duration::from_secs(55),
            std::time::Duration::from_secs(5),
        )
        .await?;
        if status.code() == Some(86) {
            panic!("converter diagnostic contract failed; child output discarded");
        }
        assert!(
            status.code() == Some(87),
            "converter fixture failed before explicit verdict; child output discarded"
        );
        return Ok(());
    }
    static OBSERVED: AtomicUsize = AtomicUsize::new(0);
    static VIOLATION: AtomicBool = AtomicBool::new(false);
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata
                .target()
                .ends_with("::proxy::providers::streaming_codex_chat")
        }
        fn log(&self, record: &log::Record<'_>) {
            if !self.enabled(record.metadata()) {
                return;
            }
            OBSERVED.fetch_add(1, Ordering::SeqCst);
            if record.level() != log::Level::Warn || !legacy_diagnostic_observer::exact(*record.args(),
                "[Codex] dropped streaming tool call: chat_index=0 call_id_empty=false args_bytes=2 finish_reason_present=true tools_total=1") {
                VIOLATION.store(true, Ordering::SeqCst);
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Logger = Logger;
    log::set_logger(&LOGGER).map_err(|_| "synthetic converter observer unavailable")?;
    log::set_max_level(log::LevelFilter::Debug);
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    for active in [false, true] {
        let gateway =
            RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
        let mut body =
            json!({"input":if active {FIXTURE_PHONE} else {"synthetic safe"},"stream":true});
        let admitted = match admit_http_request(
            &gateway,
            &crate::app_config::AppType::Codex,
            true,
            false,
            &mut body,
            &http::HeaderMap::new(),
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic converter admission unavailable".into()),
        };
        let (attempt, _owner) = admitted.prepare_owned_attempt_for_proxy(
            "synthetic-converter",
            "openai_chat",
            "http://127.0.0.1:1/v1/chat/completions",
            Some("synthetic-converter-account"),
            &mut body,
            crate::proxy::providers::transform_codex_chat::responses_to_chat_completions,
        )?;
        let lease = attempt.lease.fork_attempt_extracted()?;
        let binding = attempt.binding.clone();
        let tracker = attempt
            .upstream_sse_evidence_tracker()
            .ok_or("synthetic tracker unavailable")?;
        let observer = tracker.clone();
        let chunk = json!({"id":"synthetic-converter-response",
            "model":if private_finish {"synthetic-model"} else {"SYNTHETIC_PRIVATE_MODEL"},
            "choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"synthetic-call",
                "type":"function","function":{"arguments":"{}"}}]},
                "finish_reason":if private_finish {"SYNTHETIC_PRIVATE_FINISH"} else {"tool_calls"}}]});
        let input = format!("data: {chunk}\n\ndata: [DONE]\n\n");
        let converted = create_responses_sse_stream_from_chat_observed(
            futures::stream::iter([Ok::<_, std::io::Error>(bytes::Bytes::from(input))]),
            CodexToolContext::default(),
            move |fact| {
                if let Ok(mut tracker) = observer.lock() {
                    tracker.observe(&fact);
                }
            },
        );
        let response = http::Response::builder()
            .header("content-type", "text/event-stream")
            .body(Body::from_stream(converted))?;
        let restored = attempt
            .restore_client_http_response(
                response,
                ArgumentNormalizationMode::ChatCanonical,
                Some(tracker),
            )
            .await;
        let bytes = restored
            .into_body()
            .collect()
            .await
            .map_err(|_| "synthetic converter body failure")?
            .to_bytes();
        let wire = std::str::from_utf8(&bytes).map_err(|_| "synthetic converter UTF-8 failure")?;
        let mut recorder = FinalClientRecorder::default();
        recorder
            .observe_sse(wire)
            .map_err(|_| "synthetic converter event failure")?;
        assert!(
            recorder.failed && !recorder.completed && recorder.submissions.is_empty(),
            "invalid converter completion must release neither success nor executable tools"
        );
        let replay = json!({"input":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"synthetic safe"}]}]});
        assert!(
            lease.history_projection(&binding, &replay).is_none(),
            "failed converter must not publish history proof"
        );
    }
    assert!(
        OBSERVED.load(Ordering::SeqCst) == 2,
        "both authority branches must reach the converter warning"
    );
    std::process::exit(if VIOLATION.load(Ordering::SeqCst) {
        86
    } else {
        87
    });
}

#[tokio::test]
#[serial_test::serial]
async fn history_converter_warning_omits_private_model() -> TestResult {
    converter_warning_diagnostic_fixture(false, "proxy::security::reversible_integration_tests::history_converter_warning_omits_private_model").await
}

#[tokio::test]
#[serial_test::serial]
async fn history_converter_warning_omits_private_finish_reason() -> TestResult {
    converter_warning_diagnostic_fixture(true, "proxy::security::reversible_integration_tests::history_converter_warning_omits_private_finish_reason").await
}

#[tokio::test]
#[serial_test::serial]
async fn history_provenance_failure_diagnostics_are_silent() -> TestResult {
    const CHILD: &str = "VARYAI_SYNTHETIC_PROVENANCE_FAILURE_DIAGNOSTIC";
    const TEST: &str = "proxy::security::reversible_integration_tests::history_provenance_failure_diagnostics_are_silent";
    if legacy_diagnostic_observer::parent(CHILD, TEST).await? {
        return Ok(());
    }
    use super::{
        token_store_owners::test_support::{install, Kind, Phase},
        tokenization::{DlpTokenStore, TokenError},
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    static FORBIDDEN: AtomicBool = AtomicBool::new(false);
    static CONTROL: AtomicBool = AtomicBool::new(false);
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.target().contains("::proxy::security")
        }
        fn log(&self, record: &log::Record<'_>) {
            if !self.enabled(record.metadata()) {
                return;
            }
            if record.target() == "cc_switch_lib::proxy::security::synthetic_history_control"
                && legacy_diagnostic_observer::exact(
                    *record.args(),
                    "synthetic history diagnostic observer control",
                )
            {
                CONTROL.store(true, Ordering::SeqCst);
            } else {
                // These optional failure paths have no diagnostic contract beyond silence.
                FORBIDDEN.store(true, Ordering::SeqCst);
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Logger = Logger;
    log::set_logger(&LOGGER).map_err(|_| "synthetic history failure logger unavailable")?;
    log::set_max_level(log::LevelFilter::Trace);
    log::warn!(target: "cc_switch_lib::proxy::security::synthetic_history_control",
        "synthetic history diagnostic observer control");
    assert!(
        CONTROL.load(Ordering::SeqCst),
        "synthetic failure observer must receive its control"
    );
    let store = DlpTokenStore::new();
    let lease = store.admit("codex", Some("synthetic-failure-diagnostic-session"), 1)?;
    let binding = lease.bind_actual_attempt_extracted(
        "synthetic-failure-diagnostic-provider",
        "responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-failure-diagnostic-principal"),
    )?;
    let response = json!({"output":[{"type":"reasoning","summary":[
        {"type":"summary_text","text":"synthetic failure diagnostic text"}]}]});
    let replay = json!({"input":response["output"]});
    let warmup = lease
        .capture_delivered_history(&binding, &response)
        .ok_or("synthetic failure diagnostic warmup unavailable")?;
    drop(warmup);
    let baseline = store.history_bytes_for_test()?;
    let mandatory = store.allocation_ledger()?.used();
    let guard = install(|observation| {
        if observation.kind == Kind::Shared && observation.phase == Phase::BeforeAllocation {
            return Err(TokenError::Unavailable);
        }
        Ok(())
    });
    let allocation_miss = lease.capture_delivered_history(&binding, &response);
    drop(guard);
    assert!(
        allocation_miss.is_none(),
        "optional allocation failure must discard the complete capture"
    );
    let mut invalid = response.clone();
    invalid["output"][0]["summary"][0]["type"] = json!("synthetic_unknown");
    assert!(
        lease
            .capture_delivered_history(&binding, &invalid)
            .is_none(),
        "unsupported capture must remain optional"
    );
    let mut oversized = response.clone();
    oversized["output"][0]["summary"][0]["text"] = json!("s".repeat(256 * 1024));
    assert!(
        lease
            .capture_delivered_history(&binding, &oversized)
            .is_none(),
        "over-budget capture must be discarded"
    );
    assert!(
        store.history_bytes_for_test()? == baseline,
        "failed captures must release optional charges"
    );
    let pending = lease
        .capture_delivered_history(&binding, &response)
        .ok_or("synthetic failure diagnostic capture unavailable")?;
    let mut changed = binding.clone();
    changed.policy_revision += 1;
    assert!(
        !lease.publish_delivered_history(&changed, pending),
        "wrong live binding must reject publication"
    );
    assert!(
        lease.history_projection(&binding, &replay).is_none(),
        "failed publication must leave no partial proof"
    );
    assert!(
        store.history_bytes_for_test()? == baseline,
        "rejected publication must release its charge"
    );
    let pending = lease
        .capture_delivered_history(&binding, &response)
        .ok_or("synthetic failure diagnostic recovery unavailable")?;
    assert!(
        lease.publish_delivered_history(&binding, pending),
        "valid capture must recover after optional misses"
    );
    assert!(
        lease.history_projection(&binding, &replay).is_some(),
        "exact history control must acquire proof"
    );
    let mut mismatch = replay.clone();
    mismatch["input"][0]["summary"][0]["text"] = json!("synthetic failure diagnostic texu");
    assert!(
        lease.history_projection(&binding, &mismatch).is_none(),
        "single-byte mismatch must not acquire proof"
    );
    assert!(
        lease.history_projection(&changed, &replay).is_none(),
        "changed binding must not acquire old proof"
    );
    assert!(
        store.allocation_ledger()?.used() == mandatory,
        "optional failures must preserve mandatory accounting"
    );
    drop(lease);
    store.expire_idle_at(std::time::Instant::now() + std::time::Duration::from_secs(7200))?;
    assert!(
        store.history_bytes_for_test()? < 4096,
        "failure fixture must release every optional response charge"
    );
    if FORBIDDEN.load(Ordering::SeqCst) {
        std::process::exit(86);
    }
    std::process::exit(87);
}

pub(super) async fn history_client_diagnostic_parent(
    child_flag: &str,
    test_name: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    legacy_diagnostic_observer::parent(child_flag, test_name).await
}

pub(super) fn history_client_diagnostic_exact(
    args: std::fmt::Arguments<'_>,
    expected: &'static str,
) -> bool {
    legacy_diagnostic_observer::exact(args, expected)
}

#[tokio::test]
#[serial_test::serial]
async fn history_provenance_extended_failure_diagnostics_are_silent() -> TestResult {
    const CHILD: &str = "VARYAI_SYNTHETIC_PROVENANCE_EXTENDED_FAILURE_DIAGNOSTIC";
    const TEST: &str = "proxy::security::reversible_integration_tests::history_provenance_extended_failure_diagnostics_are_silent";
    if legacy_diagnostic_observer::parent(CHILD, TEST).await? {
        return Ok(());
    }
    use std::sync::atomic::{AtomicBool, Ordering};
    static FORBIDDEN: AtomicBool = AtomicBool::new(false);
    static CONTROL: AtomicBool = AtomicBool::new(false);
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.target().contains("::proxy::security")
        }
        fn log(&self, record: &log::Record<'_>) {
            if !self.enabled(record.metadata()) {
                return;
            }
            if record.target() == "cc_switch_lib::proxy::security::synthetic_history_control"
                && legacy_diagnostic_observer::exact(
                    *record.args(),
                    "synthetic history diagnostic observer control",
                )
            {
                CONTROL.store(true, Ordering::SeqCst);
            } else {
                // Observe presence only; never retain or format a diagnostic payload.
                FORBIDDEN.store(true, Ordering::SeqCst);
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Logger = Logger;
    log::set_logger(&LOGGER).map_err(|_| "synthetic extended failure logger unavailable")?;
    log::set_max_level(log::LevelFilter::Trace);
    log::warn!(target: "cc_switch_lib::proxy::security::synthetic_history_control",
        "synthetic history diagnostic observer control");
    assert!(
        CONTROL.load(Ordering::SeqCst),
        "synthetic extended observer must receive its control"
    );
    extended_history_failure_diagnostics::run()?;
    if FORBIDDEN.load(Ordering::SeqCst) {
        std::process::exit(86);
    }
    std::process::exit(87);
}

mod extended_history_failure_diagnostics {
    use super::super::{
        completed_context::ActualAttemptBinding,
        history_provenance::{
            global_history_allocation, DlpHistoryLeafField as Field, DlpHistoryMapping,
            DlpHistoryProjection, DlpHistorySeparator as Sep, DlpHistoryTarget as Target,
            HistoryNamespace, GLOBAL_BYTES,
        },
        token_store_owners::{
            test_support::{install, Guard, Kind, Observation, Phase},
            AllocationCharge, StoreAllocationLedger,
        },
        tokenization::{DlpRequestLease, DlpTokenStore, TokenError},
    };
    use super::{json, TestResult, Value};
    use std::{
        cell::Cell,
        rc::Rc,
        time::{Duration, Instant},
    };

    const NAMESPACE: usize = 0x0003;
    const SHARED: usize = 0x000c;
    const VECTOR: usize = 0x0030;
    const MOVE: usize = 0x0040;
    const TEXT: usize = 0x0380;
    const CONTENT: Target = Target::ChatContent { message_index: 0 };
    const REASONING: Target = Target::ChatReasoning { message_index: 0 };

    fn event_bit(event: Observation) -> usize {
        let index = match (event.kind, event.phase) {
            (Kind::Namespace, Phase::BeforeAllocation) => 0,
            (Kind::Namespace, Phase::AfterAllocation) => 1,
            (Kind::Shared, Phase::BeforeAllocation) => 2,
            (Kind::Shared, Phase::AfterAllocation) => 3,
            (Kind::Vector, Phase::BeforeAllocation) => 4,
            (Kind::Vector, Phase::AfterAllocation) => 5,
            (Kind::Vector, Phase::BeforeMove) => 6,
            (Kind::Text, Phase::BeforeAllocation) => 7,
            (Kind::Text, Phase::AfterAllocation) => 8,
            (Kind::Text, Phase::BeforeCopy) => 9,
            _ => return 0,
        };
        1 << index
    }

    #[derive(Clone, Default)]
    struct Probe {
        count: Rc<Cell<usize>>,
        mask: Rc<Cell<usize>>,
        hit: Rc<Cell<bool>>,
    }
    impl Probe {
        fn install(failure: Option<usize>) -> (Guard, Self) {
            let probe = Self::default();
            let observed = probe.clone();
            let guard = install(move |event| {
                let bit = event_bit(event);
                if bit != 0 {
                    let ordinal = observed.count.get();
                    observed.count.set(ordinal + 1);
                    observed.mask.set(observed.mask.get() | bit);
                    if failure == Some(ordinal) {
                        observed.hit.set(true);
                        return Err(TokenError::Unavailable);
                    }
                }
                Ok(())
            });
            (guard, probe)
        }
        fn assert_hit(&self, failure: usize) {
            assert!(self.hit.get(), "synthetic allocation hook must be reached");
            assert!(
                self.count.get() == failure + 1,
                "synthetic allocation failure must stop the complete operation"
            );
        }
    }

    fn delivered() -> Value {
        json!({"output":[
            {"type":"message","role":"assistant","content":[
                {"type":"output_text","text":"synthetic output"}]},
            {"type":"reasoning","summary":[
                {"type":"summary_text","text":"synthetic first"},
                {"type":"summary_text","text":"synthetic second"}],
                "content":[{"type":"reasoning_text","text":"synthetic detail"}]}
        ]})
    }

    fn outbound() -> Value {
        json!({"messages":[{"role":"assistant",
            "content":"synthetic output","reasoning_content":"synthetic first",
            "metadata":{"synthetic":["synthetic nested",{"synthetic":true}]},
            "tool_calls":[{"id":"synthetic-call","type":"function",
                "function":{"name":"synthetic_tool","arguments":"{}"}}]}]})
    }

    struct Fixture {
        store: DlpTokenStore,
        lease: DlpRequestLease,
        binding: ActualAttemptBinding,
        response: Value,
        request: Value,
        optional: usize,
        mandatory: AllocationCharge,
        global_before: AllocationCharge,
    }
    impl Fixture {
        fn new() -> Result<Self, TokenError> {
            let global_before = global_history_allocation()?.used();
            let store = DlpTokenStore::new();
            let lease = store.admit("codex", Some("synthetic-extended-diagnostics"), 1)?;
            let binding = lease.bind_actual_attempt_extracted(
                "synthetic-provider",
                "responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-principal"),
            )?;
            let response = delivered();
            let request = json!({"input":response["output"]});
            let warmup = lease
                .capture_delivered_history(&binding, &response)
                .ok_or(TokenError::Unavailable)?;
            drop(warmup);
            let optional = store.history_bytes_for_test()?;
            let mandatory = store.allocation_ledger()?.used();
            Ok(Self {
                store,
                lease,
                binding,
                response,
                request,
                optional,
                mandatory,
                global_before,
            })
        }
        fn unchanged(&self, optional: usize) -> TestResult {
            assert!(
                self.store.history_bytes_for_test()? == optional,
                "synthetic optional owners must release every temporary charge"
            );
            assert!(
                self.store.allocation_ledger()?.used() == self.mandatory,
                "synthetic optional failures must preserve mandatory accounting"
            );
            Ok(())
        }
        fn publish(&self) -> TestResult {
            let pending = self
                .lease
                .capture_delivered_history(&self.binding, &self.response)
                .ok_or("synthetic extended capture unavailable")?;
            assert!(
                self.lease.publish_delivered_history(&self.binding, pending),
                "synthetic optional capture must recover"
            );
            Ok(())
        }
        fn finish(self) -> TestResult {
            drop(self.lease);
            self.store
                .expire_idle_at(Instant::now() + Duration::from_secs(7200))?;
            assert!(
                global_history_allocation()?.used() == self.global_before,
                "synthetic teardown must release all namespace and response owners"
            );
            Ok(())
        }
    }

    fn mapping(projection: &DlpHistoryProjection<'_>) -> Result<DlpHistoryMapping, TokenError> {
        let output = projection
            .source_for_responses_leaf(0, Field::OutputText, 0)
            .ok_or(TokenError::Unavailable)?;
        let reasoning = projection
            .source_for_responses_leaf(1, Field::ReasoningSummary, 0)
            .ok_or(TokenError::Unavailable)?;
        let mut mapping = projection.mapping().ok_or(TokenError::Unavailable)?;
        assert!(mapping.record(CONTENT, &[output], Sep::None));
        assert!(mapping.record(REASONING, &[reasoning], Sep::None));
        Ok(mapping)
    }

    fn bootstrap_failures() -> TestResult {
        let fixture = Fixture::new()?;
        let local = StoreAllocationLedger::new(0, GLOBAL_BYTES)?;
        let baseline = local.used();
        let (guard, measured) = Probe::install(None);
        let namespace = HistoryNamespace::new(local.clone());
        drop(guard);
        assert!(
            namespace.is_some(),
            "synthetic local bootstrap must succeed"
        );
        assert!(measured.mask.get() == (NAMESPACE | SHARED));
        drop(namespace);
        assert!(local.used() == baseline);
        for failure in 0..measured.count.get() {
            let (guard, probe) = Probe::install(Some(failure));
            let namespace = HistoryNamespace::new(local.clone());
            drop(guard);
            probe.assert_hit(failure);
            assert!(
                namespace.is_none(),
                "failed bootstrap must discard its owner"
            );
            assert!(
                local.used() == baseline,
                "failed bootstrap must release its permits"
            );
            assert!(fixture
                .lease
                .history_projection(&fixture.binding, &fixture.request)
                .is_none());
            fixture.unchanged(fixture.optional)?;
        }
        let namespace = HistoryNamespace::new(local.clone())
            .ok_or("synthetic local bootstrap recovery unavailable")?;
        let pending = namespace
            .get()?
            .capture(&fixture.binding, &fixture.response)
            .ok_or("synthetic local capture recovery unavailable")?;
        assert!(namespace
            .get()?
            .projection(
                fixture.store.clone(),
                Some(&fixture.binding),
                &fixture.request
            )
            .is_none());
        drop(pending);
        drop(namespace);
        assert!(local.used() == baseline);
        fixture.finish()
    }

    fn capture_failures() -> TestResult {
        let fixture = Fixture::new()?;
        let (guard, measured) = Probe::install(None);
        let pending = fixture
            .lease
            .capture_delivered_history(&fixture.binding, &fixture.response);
        drop(guard);
        assert!(pending.is_some());
        assert!(measured.mask.get() == (NAMESPACE | SHARED | VECTOR | TEXT));
        drop(pending);
        fixture.unchanged(fixture.optional)?;
        // Warming the namespace isolates both RecordPin phases from namespace sharing.
        for failure in 0..measured.count.get() {
            let (guard, probe) = Probe::install(Some(failure));
            let pending = fixture
                .lease
                .capture_delivered_history(&fixture.binding, &fixture.response);
            drop(guard);
            probe.assert_hit(failure);
            assert!(
                pending.is_none(),
                "one capture failure must discard every typed item"
            );
            assert!(fixture
                .lease
                .history_projection(&fixture.binding, &fixture.request)
                .is_none());
            assert!(fixture
                .lease
                .history_admission_candidate_projection(&fixture.request)
                .is_none());
            fixture.unchanged(fixture.optional)?;
        }
        fixture.publish()?;
        let projection = fixture
            .lease
            .history_projection(&fixture.binding, &fixture.request)
            .ok_or("synthetic capture recovery proof unavailable")?;
        for (index, field, part) in [
            (0, Field::OutputText, 0),
            (1, Field::ReasoningSummary, 0),
            (1, Field::ReasoningSummary, 1),
            (1, Field::ReasoningContent, 0),
        ] {
            assert!(projection
                .source_for_responses_leaf(index, field, part)
                .is_some());
        }
        drop(projection);
        assert!(fixture.store.allocation_ledger()?.used() == fixture.mandatory);
        fixture.finish()
    }

    fn projection_exhaustion() -> TestResult {
        let fixture = Fixture::new()?;
        let local = StoreAllocationLedger::new(0, GLOBAL_BYTES)?;
        let empty = local.used();
        let owner = HistoryNamespace::new(local.clone())
            .ok_or("synthetic projection namespace unavailable")?;
        let namespace = owner.get()?;
        let pending = namespace
            .capture(&fixture.binding, &fixture.response)
            .ok_or("synthetic projection capture unavailable")?;
        assert!(namespace.publish(pending).is_ok());
        let baseline = local.used();
        // Leave less than one projection permit in the private global ledger.
        let reservation = local.reserve(AllocationCharge {
            plaintext: 0,
            metadata: local.limits_for_test().metadata - baseline.metadata - 2047,
        })?;
        assert!(local.used().metadata + 2047 == local.limits_for_test().metadata);
        for binding in [Some(&fixture.binding), None] {
            assert!(
                namespace
                    .projection(fixture.store.clone(), binding, &fixture.request)
                    .is_none(),
                "exhausted optional global metadata must not yield a projection"
            );
        }
        drop(reservation);
        assert!(local.used() == baseline);

        // Bound the loop above the namespace's maximum possible 2048-byte pins.
        let mut held = Vec::new();
        let mut exhausted = false;
        for _ in 0..512 {
            match namespace.projection(
                fixture.store.clone(),
                Some(&fixture.binding),
                &fixture.request,
            ) {
                Some(projection) => held.push(projection),
                None => {
                    exhausted = true;
                    break;
                }
            }
        }
        assert!(
            exhausted && !held.is_empty(),
            "synthetic namespace permits must exhaust"
        );
        assert!(local.used().metadata < local.limits_for_test().metadata);
        assert!(namespace
            .projection(fixture.store.clone(), None, &fixture.request)
            .is_none());
        drop(held);
        assert!(
            local.used() == baseline,
            "failed lookup must release both permit planes"
        );
        let recovered = namespace
            .projection(
                fixture.store.clone(),
                Some(&fixture.binding),
                &fixture.request,
            )
            .ok_or("synthetic projection recovery unavailable")?;
        assert!(recovered
            .source_for_responses_leaf(1, Field::ReasoningSummary, 0)
            .is_some());
        drop(recovered);
        let mut items = vec![Value::Null; 4096];
        items[4095] = fixture.response["output"][1].clone();
        let mut boundary = json!({"input":items});
        let permitted = namespace
            .projection(fixture.store.clone(), Some(&fixture.binding), &boundary)
            .ok_or("synthetic projection boundary unavailable")?;
        assert!(permitted
            .source_for_responses_leaf(4095, Field::ReasoningSummary, 0)
            .is_some());
        drop(permitted);
        boundary["input"]
            .as_array_mut()
            .ok_or("synthetic projection boundary input unavailable")?
            .push(Value::Null);
        for binding in [Some(&fixture.binding), None] {
            assert!(namespace
                .projection(fixture.store.clone(), binding, &boundary)
                .is_none());
        }
        assert!(local.used() == baseline);
        drop(owner);
        assert!(local.used() == empty);
        fixture.unchanged(fixture.optional)?;
        fixture.finish()
    }

    fn mapping_failures() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.publish()?;
        let projection = fixture
            .lease
            .history_projection(&fixture.binding, &fixture.request)
            .ok_or("synthetic mapping projection unavailable")?;
        let baseline = fixture.store.history_bytes_for_test()?;
        // A metadata owner has a fallible local ledger, not a fallible projection copy.
        for is_mapping in [false, true] {
            let (guard, measured) = Probe::install(None);
            let available = if is_mapping {
                projection.mapping().is_some()
            } else {
                projection.trace().is_some()
            };
            drop(guard);
            assert!(available && measured.mask.get() == NAMESPACE);
            fixture.unchanged(baseline)?;
            for failure in 0..measured.count.get() {
                let (guard, probe) = Probe::install(Some(failure));
                let available = if is_mapping {
                    projection.mapping().is_some()
                } else {
                    projection.trace().is_some()
                };
                drop(guard);
                probe.assert_hit(failure);
                assert!(!available, "failed metadata bootstrap must yield no owner");
                fixture.unchanged(baseline)?;
            }
        }
        let reasoning = projection
            .source_for_responses_leaf(1, Field::ReasoningSummary, 0)
            .ok_or("synthetic mapping source unavailable")?;
        let outbound = outbound();
        let mut measured_mapping = mapping(&projection)?;
        let (guard, measured) = Probe::install(None);
        let recorded =
            measured_mapping.record(REASONING, std::slice::from_ref(&reasoning), Sep::None);
        drop(guard);
        assert!(recorded && measured.mask.get() == (NAMESPACE | VECTOR | MOVE));
        drop(measured_mapping);
        fixture.unchanged(baseline)?;
        for failure in 0..measured.count.get() {
            let mut failed = mapping(&projection)?;
            assert!(failed.seal_chat_targets(&outbound));
            for target in [CONTENT, REASONING] {
                assert!(failed.validates_target(
                    &fixture.lease,
                    &fixture.binding,
                    &outbound,
                    target
                ));
            }
            let (guard, probe) = Probe::install(Some(failure));
            let recorded = failed.record(REASONING, std::slice::from_ref(&reasoning), Sep::None);
            drop(guard);
            probe.assert_hit(failure);
            assert!(!recorded && failed.failed() && !failed.valid());
            for target in [CONTENT, REASONING] {
                assert!(!failed.validates_target(
                    &fixture.lease,
                    &fixture.binding,
                    &outbound,
                    target
                ));
            }
            drop(failed);
            fixture.unchanged(baseline)?;
        }
        // Fill all four initial mapping slots so the next target must grow its vector.
        let full_mapping = || {
            let mut full = mapping(&projection)?;
            assert!(full.record(
                Target::ChatReasoning { message_index: 1 },
                std::slice::from_ref(&reasoning),
                Sep::None,
            ));
            assert!(full.record(
                Target::ChatReasoning { message_index: 2 },
                std::slice::from_ref(&reasoning),
                Sep::None,
            ));
            Ok::<_, TokenError>(full)
        };
        let next_trace = || {
            let mut trace = projection.trace().ok_or(TokenError::Unavailable)?;
            assert!(trace.push(&reasoning, Sep::None));
            Ok::<_, TokenError>(trace)
        };
        let next_target = Target::ChatReasoning { message_index: 3 };
        let mut measured_mapping = full_mapping()?;
        let next = next_trace()?;
        let (guard, measured) = Probe::install(None);
        let recorded = measured_mapping.record_trace(next_target, next);
        drop(guard);
        assert!(recorded && measured.mask.get() == (VECTOR | MOVE));
        drop(measured_mapping);
        fixture.unchanged(baseline)?;
        for failure in 0..measured.count.get() {
            let mut failed = full_mapping()?;
            assert!(failed.seal_chat_targets(&outbound));
            assert!(failed.validates_target(&fixture.lease, &fixture.binding, &outbound, CONTENT));
            let next = next_trace()?;
            let (guard, probe) = Probe::install(Some(failure));
            let recorded = failed.record_trace(next_target, next);
            drop(guard);
            probe.assert_hit(failure);
            assert!(!recorded && failed.failed() && !failed.valid());
            for target in [CONTENT, REASONING, next_target] {
                assert!(!failed.validates_target(
                    &fixture.lease,
                    &fixture.binding,
                    &outbound,
                    target
                ));
            }
            drop(failed);
            fixture.unchanged(baseline)?;
        }
        let mut recovered = mapping(&projection)?;
        assert!(recovered.seal_chat_targets(&outbound));
        for target in [CONTENT, REASONING] {
            assert!(recovered.validates_target(
                &fixture.lease,
                &fixture.binding,
                &outbound,
                target
            ));
        }
        drop(recovered);
        drop(reasoning);
        fixture.unchanged(baseline)?;
        drop(projection);
        fixture.finish()
    }

    fn trace_growth_failures() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.publish()?;
        let mut request = fixture.request.clone();
        for _ in 0..512 {
            request["input"]
                .as_array_mut()
                .ok_or("synthetic trace input unavailable")?
                .push(fixture.response["output"][1].clone());
        }
        let projection = fixture
            .lease
            .history_projection(&fixture.binding, &request)
            .ok_or("synthetic trace projection unavailable")?;
        let baseline = fixture.store.history_bytes_for_test()?;
        let trace = || {
            let mut trace = projection.trace().ok_or(TokenError::Unavailable)?;
            for index in 1..=4 {
                let source = projection
                    .source_for_responses_leaf(index, Field::ReasoningSummary, 0)
                    .ok_or(TokenError::Unavailable)?;
                assert!(trace.push(&source, if index == 1 { Sep::None } else { Sep::Newline }));
            }
            Ok::<_, TokenError>(trace)
        };
        let source = projection
            .source_for_responses_leaf(5, Field::ReasoningSummary, 0)
            .ok_or("synthetic growth source unavailable")?;
        let mut measured_trace = trace()?;
        let (guard, measured) = Probe::install(None);
        let pushed = measured_trace.push(&source, Sep::Newline);
        drop(guard);
        assert!(pushed && measured.mask.get() == (VECTOR | MOVE));
        drop(measured_trace);
        fixture.unchanged(baseline)?;
        let outbound = outbound();
        for failure in 0..measured.count.get() {
            let mut failed_mapping = mapping(&projection)?;
            assert!(failed_mapping.seal_chat_targets(&outbound));
            assert!(failed_mapping.validates_target(
                &fixture.lease,
                &fixture.binding,
                &outbound,
                CONTENT
            ));
            let mut failed_trace = trace()?;
            let (guard, probe) = Probe::install(Some(failure));
            let pushed = failed_trace.push(&source, Sep::Newline);
            drop(guard);
            probe.assert_hit(failure);
            assert!(!pushed && failed_trace.failed() && !failed_trace.valid());
            assert!(!failed_mapping.record_trace(REASONING, failed_trace));
            assert!(failed_mapping.failed() && !failed_mapping.valid());
            for target in [CONTENT, REASONING] {
                assert!(!failed_mapping.validates_target(
                    &fixture.lease,
                    &fixture.binding,
                    &outbound,
                    target
                ));
            }
            drop(failed_mapping);
            fixture.unchanged(baseline)?;
        }
        let mut recovered = trace()?;
        assert!(recovered.push(&source, Sep::Newline) && recovered.valid());
        drop(recovered);
        let mut bounded = trace()?;
        let mut exhausted = false;
        for index in 5..514 {
            let source = projection
                .source_for_responses_leaf(index, Field::ReasoningSummary, 0)
                .ok_or("synthetic bounded trace source unavailable")?;
            if !bounded.push(&source, Sep::Newline) {
                exhausted = true;
                break;
            }
        }
        assert!(exhausted && bounded.failed() && !bounded.valid());
        let mut failed_mapping = mapping(&projection)?;
        assert!(failed_mapping.seal_chat_targets(&outbound));
        assert!(failed_mapping.validates_target(
            &fixture.lease,
            &fixture.binding,
            &outbound,
            CONTENT
        ));
        assert!(!failed_mapping.record_trace(REASONING, bounded));
        assert!(failed_mapping.failed() && !failed_mapping.valid());
        for target in [CONTENT, REASONING] {
            assert!(!failed_mapping.validates_target(
                &fixture.lease,
                &fixture.binding,
                &outbound,
                target
            ));
        }
        drop(failed_mapping);
        drop(source);
        fixture.unchanged(baseline)?;
        drop(projection);
        fixture.finish()
    }

    fn snapshot_failures() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.publish()?;
        let projection = fixture
            .lease
            .history_projection(&fixture.binding, &fixture.request)
            .ok_or("synthetic snapshot projection unavailable")?;
        let baseline = fixture.store.history_bytes_for_test()?;
        let outbound = outbound();
        let unchanged = outbound.clone();
        let mut successful = mapping(&projection)?;
        let (guard, measured) = Probe::install(None);
        let sealed = successful.seal_chat_targets(&outbound);
        drop(guard);
        assert!(sealed && measured.mask.get() == (NAMESPACE | VECTOR | TEXT));
        for target in [CONTENT, REASONING] {
            assert!(successful.validates_target(
                &fixture.lease,
                &fixture.binding,
                &outbound,
                target
            ));
        }
        drop(successful);
        fixture.unchanged(baseline)?;
        for failure in 0..measured.count.get() {
            let mut failed = mapping(&projection)?;
            let (guard, probe) = Probe::install(Some(failure));
            let sealed = failed.seal_chat_targets(&outbound);
            drop(guard);
            probe.assert_hit(failure);
            assert!(!sealed && failed.failed() && !failed.valid());
            for target in [CONTENT, REASONING] {
                assert!(!failed.validates_target(
                    &fixture.lease,
                    &fixture.binding,
                    &outbound,
                    target
                ));
            }
            assert!(
                outbound == unchanged,
                "failed snapshot must not mutate the outbound body"
            );
            drop(failed);
            fixture.unchanged(baseline)?;
        }
        for limit in 0..4 {
            let mut oversized = outbound.clone();
            match limit {
                0 => oversized["messages"][0]["metadata"] = json!("s".repeat(256 * 1024)),
                1 => oversized["messages"]
                    .as_array_mut()
                    .ok_or("synthetic snapshot messages unavailable")?
                    .resize(4097, Value::Null),
                2 => {
                    let mut nested = Value::Null;
                    for _ in 0..65 {
                        nested = json!([nested]);
                    }
                    oversized["messages"][0]["metadata"] = nested;
                }
                _ => oversized["messages"][0]["metadata"] = json!(vec![Value::Null; 4096]),
            }
            let mut failed = mapping(&projection)?;
            assert!(!failed.seal_chat_targets(&oversized) && failed.failed());
            for target in [CONTENT, REASONING] {
                assert!(!failed.validates_target(
                    &fixture.lease,
                    &fixture.binding,
                    &oversized,
                    target
                ));
            }
            drop(failed);
            fixture.unchanged(baseline)?;
        }
        let mut recovered = mapping(&projection)?;
        assert!(recovered.seal_chat_targets(&outbound));
        for target in [CONTENT, REASONING] {
            assert!(recovered.validates_target(
                &fixture.lease,
                &fixture.binding,
                &outbound,
                target
            ));
        }
        drop(recovered);
        fixture.unchanged(baseline)?;
        // Three retained responses leave insufficient namespace room for a snapshot.
        fixture.publish()?;
        fixture.publish()?;
        let full = fixture.store.history_bytes_for_test()?;
        let mut failed = mapping(&projection)?;
        assert!(!failed.seal_chat_targets(&outbound) && failed.failed());
        for target in [CONTENT, REASONING] {
            assert!(!failed.validates_target(&fixture.lease, &fixture.binding, &outbound, target));
        }
        drop(failed);
        fixture.unchanged(full)?;
        drop(projection);
        fixture.finish()
    }

    pub(super) fn run() -> TestResult {
        // Initialize the process-global OnceLock before any bootstrap denial.
        global_history_allocation()?;
        bootstrap_failures()?;
        capture_failures()?;
        projection_exhaustion()?;
        mapping_failures()?;
        trace_growth_failures()?;
        snapshot_failures()
    }
}
