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
        outbound.contains("vary_ph_"),
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
        Value::String(text) if text.contains("vary_ph_") || text.contains(FIXTURE_PHONE) => {
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
        outbound.contains("vary_ph_"),
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
            !text.contains("vary_ph_"),
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
            input.contains("vary_ph_"),
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
            assert!(!text.contains("vary_ph_"));
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
    assert!(!text.contains("vary_ph_"));
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
        cached.contains("vary_ph_"),
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
    assert!(!raw.contains("vary_ph_"));
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

async fn upstream_error_echo(Json(body): Json<Value>) -> axum::response::Response {
    let reference = fixture_text(&body).unwrap_or("synthetic-fallback");
    (
        http::StatusCode::BAD_REQUEST,
        Json(
            json!({"error":{"message":format!("synthetic-upstream-private-marker {reference}"),
        "type":"synthetic_upstream_error","code":"synthetic_error"}}),
        ),
    )
        .into_response()
}

#[tokio::test]
#[serial_test::serial]
async fn reversible_http_upstream_error_echo_is_fixed_without_reference_or_diagnostics(
) -> TestResult {
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let upstream = Router::new().fallback(post(upstream_error_echo));
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await });
    let db = Arc::new(Database::memory()?);
    let provider = Provider::with_id(
        "synthetic-private-provider-marker".into(),
        "Synthetic private provider label".into(),
        json!({"base_url":format!("http://{addr}"),"api_key":"synthetic-error-credential"}),
        None,
    );
    db.save_provider("codex", &provider)?;
    db.set_current_provider("codex", &provider.id)?;
    let proxy_state = state(db);
    let status = proxy_state.status.clone();
    let request = http::Request::builder()
        .method("POST")
        .uri("/v1/responses")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":false}).to_string(),
        ))?;
    let response = handlers::handle_responses(State(proxy_state), request).await?;
    let http_status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    let text = std::str::from_utf8(&bytes)?;
    assert!(
        !text.contains("vary_ph_"),
        "leased errors must never expose references"
    );
    for forbidden in [
        FIXTURE_PHONE,
        "synthetic-upstream-private-marker",
        "synthetic-private-provider-marker",
        "Synthetic private provider label",
        "synthetic-model",
    ] {
        assert!(
            !text.contains(forbidden),
            "leased upstream errors must be fixed, non-enriched responses"
        );
    }
    assert_eq!(http_status, http::StatusCode::BAD_GATEWAY);
    let body: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(body["error"]["code"], "dlp_restore_failed");
    let last_error = status.read().await.last_error.clone().unwrap_or_default();
    assert!(!last_error.contains("vary_ph_"));
    assert!(!last_error.contains("synthetic-upstream-private-marker"));
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
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (owner, used) = super::upstream_guard::UpstreamPayloadOwner::test_fixture()
        .map_err(|error| std::io::Error::other(error.code()))?;
    let history = Arc::new(CodexChatHistoryStore::default());
    let cached_call = json!({"type":"function_call", "id":"fc_cancel_fixture",
        "call_id":"call_cancel_fixture","name":"synthetic_lookup",
        "arguments":"{\"marker\":\"synthetic-cancel-value\"}"});
    let (_probe_guard, candidate_state) = super::chat_enrichment::test_support::install();
    let before_copy = used();
    let probe = owner
        .try_clone_json(&cached_call)
        .map_err(|error| std::io::Error::other(error.code()))?;
    assert!(used() > before_copy);
    drop(probe);
    assert_eq!(
        history
            .record_response_with_payload_owner(
                &json!({"id":"resp_cancel_fixture","output":[cached_call]}),
                Some(owner),
            )
            .await,
        1
    );
    let wire_length = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let pending_wire_length = wire_length.clone();
    let entered = Arc::new(tokio::sync::Notify::new());
    let pending_entered = entered.clone();
    let pending_history = history.clone();
    let upstream = Router::new().fallback(post(
        move |headers: http::HeaderMap, Json(body): Json<Value>| {
            let entered = pending_entered.clone();
            let history = pending_history.clone();
            let wire_length = pending_wire_length.clone();
            async move {
                let length = headers
                    .get(http::header::CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or_default();
                wire_length.store(length, std::sync::atomic::Ordering::SeqCst);
                assert!(body.to_string().contains("call_cancel_fixture"));
                // Remove the guarded cache allocation while the request's independent
                // enriched copy is still retained by the actual-send future.
                assert_eq!(
                    history
                        .record_response(&json!({"id":"resp_cancel_fixture","output":[{
                "type":"function_call","id":"fc_cancel_fixture","call_id":"call_cancel_fixture",
                "name":"synthetic_lookup","arguments":"{}"}]}))
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
    let mut proxy_state = state(db);
    proxy_state.codex_chat_history = history.clone();
    proxy_state.restoration_gateway = RestorationGateway::new_default();
    let request = http::Request::builder().method("POST").uri("/v1/responses")
        .header("content-type", "application/json")
        .body(Body::from(json!({"model":"synthetic-model","stream":false,"input":[
            {"type":"function_call_output","call_id":"call_cancel_fixture","output":"synthetic result"}
        ]}).to_string()))?;
    let mut forwarding = Box::pin(handlers::handle_responses(State(proxy_state), request));
    let reached_upstream = tokio::select! {
        _ = entered.notified() => true,
        _ = &mut forwarding => false,
        _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => false,
    };
    if !reached_upstream {
        server.abort();
        return Err("synthetic request did not remain pending at upstream".into());
    }
    let serialized_length = wire_length.load(std::sync::atomic::Ordering::SeqCst);
    let charged_during_send = used();
    let pending_candidate = *candidate_state.lock().expect("synthetic observer");
    assert_eq!(pending_candidate.created, 1);
    assert_eq!(pending_candidate.released, 0);
    assert!(pending_candidate.live_bytes > 0);
    drop(forwarding);
    assert!(
        serialized_length > 0,
        "synthetic request must have an exact wire length"
    );
    assert!(
        charged_during_send == pending_candidate.live_bytes + serialized_length,
        "active send retains exact independent candidate and wire charges, not the retired workspace"
    );
    server.abort();
    let released_candidate = *candidate_state.lock().expect("synthetic observer");
    assert_eq!(released_candidate.released, 1);
    assert_eq!(released_candidate.live_bytes, 0);
    assert_eq!(
        used(),
        0,
        "cancelling the send must release the last enriched payload owner"
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
                .is_some_and(|value| value.contains("vary_ph_")));
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
            "vary_ph_",
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
            json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":false}).to_string(),
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
        "vary_ph_",
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
    assert!(!text.contains(FIXTURE_PHONE) && !text.contains("vary_ph_"));
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
    assert!(!text.contains(FIXTURE_PHONE) && !text.contains("vary_ph_"));
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
            assert!(text.contains("vary_ph_") && !text.contains(FIXTURE_PHONE));
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
            assert!(body["input"]
                .as_str()
                .is_some_and(|text| text.contains("vary_ph_") && !text.contains(FIXTURE_PHONE)));
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
                "vary_ph_"
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
    )
    .await?;
    assert!(body["input"]
        .as_str()
        .is_some_and(|text| text.contains("vary_ph_")));
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
        !initial_text.contains("vary_ph_"),
        "control must not expose references"
    );
    let followup_body = json!({"model":"synthetic-model","stream":false,"input":[{
        "type":"function_call_output","call_id":"call_cache_owner","output":"synthetic tool result"}]});
    assert!(
        !followup_body.to_string().contains("vary_ph_"),
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
        cache_probe.to_string().contains("vary_ph_"),
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
        .is_some_and(|body| body.to_string().contains("vary_ph_"));
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
            assert!(!recorder.submissions[0].to_string().contains("vary_ph_"));
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
        let reference_exposed = text.contains("vary_ph_");
        let before = seen.lock().map_err(|_| "synthetic capture poisoned")?.len();
        assert_eq!(
            before, 1,
            "xAI control must traverse exactly one actual upstream request"
        );
        {
            let captured = seen.lock().map_err(|_| "synthetic capture poisoned")?;
            assert!(
                captured[0].to_string().contains("vary_ph_"),
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
        assert!(!text.contains("vary_ph_"));
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
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    let mut config = runtime::SecurityRuntimeConfig::default();
    config
        .builtin_action_overrides
        .insert("chinese_phone_number".into(), super::AuditAction::Redact);
    runtime::set_security_runtime_for_test(config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
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
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
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
    let before = store.stats()?;
    let mut proxy_state = state(db);
    proxy_state.restoration_gateway =
        RestorationGateway::with_test_store_and_rules(store.clone(), &[], budget.clone());
    let request = http::Request::builder().method("POST").uri("/v1/responses")
        .header("content-type", "application/json")
        .body(Body::from(json!({"model":"synthetic-model","input":FIXTURE_PHONE,"stream":true,
            "tools":[{"type":"namespace","name":"synthetic","tools":[{"type":"function","name":"lookup","parameters":{"type":"object","properties":{}}}]}]}).to_string()))?;
    let response = handlers::handle_responses(State(proxy_state), request).await?;
    assert_eq!(response.status(), http::StatusCode::OK);
    let bytes = response.into_body().collect().await?.to_bytes();
    server.abort();
    let text = std::str::from_utf8(&bytes)?;
    assert!(text.contains("\"namespace\":\"synthetic\"") && text.contains("\"name\":\"lookup\""));
    assert!(
        !text.contains("response.failed")
            && !text.contains("vary_ph_")
            && !text.contains(FIXTURE_PHONE)
    );
    let requests = seen.lock().map_err(|_| "synthetic capture poisoned")?;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].to_string().contains("[REDACTED]"));
    assert!(!requests[0].to_string().contains(FIXTURE_PHONE));
    assert_eq!(
        store.stats()?,
        before,
        "legacy redact must create no token namespace/mappings"
    );
    assert_eq!(
        budget.used(),
        59,
        "legacy must work while the unrelated restoration budget is exhausted"
    );
    drop(foreign);
    assert_eq!(budget.used(), 0);
    Ok(())
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
            capture[0].to_string().contains("vary_ph_"),
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
            wire.contains(FIXTURE_PHONE) && !wire.contains("vary_ph_"),
            "restored client output only"
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
            !wire.contains(FIXTURE_PHONE) && !wire.contains("vary_ph_"),
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
    assert!(control_wire.contains(FIXTURE_PHONE) && !control_wire.contains("vary_ph_"));
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
    let exposed = wire.contains(FIXTURE_PHONE) || wire.contains("vary_ph_");
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
                captured[index].to_string().contains("vary_ph_")
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
                && !wire.contains("vary_ph_"),
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
            text.contains(FIXTURE_PHONE) && !text.contains("vary_ph_"),
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
        assert!(!text.contains(FIXTURE_PHONE) && !text.contains("vary_ph_"));
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
            !outbound.contains(FIXTURE_PHONE) && outbound.contains("vary_ph_"),
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
    let proxy = ProxyServer::new(ProxyConfig::default(), db, None).with_restoration_gateway(
        RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024),
    );
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
        {"type":"reasoning","summary":[{"type":"summary_text","text":format!("Synthetic reasoning {FIXTURE_PHONE}")}]},
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
    for negative in negative_bodies {
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
        assert_eq!(response.status(), http::StatusCode::BAD_REQUEST);
        let _ = response.into_body().collect().await?;
        assert_eq!(
            seen.lock()
                .map_err(|_| "synthetic fresh capture poisoned")?
                .len(),
            0,
            "old dependencies and malformed/incomplete readable history must send nothing"
        );
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
    assert!(status == http::StatusCode::OK && sends == 1,
        "complete original readable history must be reinspected as fresh input; status={status} sends={sends}");
    let response: Value = serde_json::from_slice(&bytes)?;
    let serialized = serde_json::to_string(&response)?;
    assert!(serialized.contains(FIXTURE_PHONE) && !serialized.contains("vary_ph_"));
    let captured = seen
        .lock()
        .map_err(|_| "synthetic fresh capture poisoned")?;
    let outbound = serde_json::to_string(&captured[0])?;
    assert!(
        !outbound.contains(FIXTURE_PHONE) && outbound.contains("vary_ph_"),
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
            && outbound.contains("vary_ph_"),
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
    let proxy = ProxyServer::new(ProxyConfig::default(), db, None).with_restoration_gateway(
        RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024),
    );
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
            assert!(token.starts_with("vary_ph_"));
            let mut invalid = body.clone();
            invalid["input"][1]["arguments"] =
                json!(format!("{{\"\\u0076{}\":\"synthetic\"}}", &token[1..]));
            negatives.push(("escaped_argument_key", invalid));
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
    for (case, negative) in negatives {
        let response = router
            .call(
                http::Request::builder()
                    .method("POST")
                    .uri("/v1/responses")
                    .header("content-type", "application/json")
                    .header("session-id", "synthetic-boundary-session")
                    .body(Body::from(negative.to_string()))?,
            )
            .await?;
        let status = response.status();
        let _ = response.into_body().collect().await?;
        let sends = seen
            .lock()
            .map_err(|_| "synthetic boundary capture poisoned")?
            .len();
        assert!(status == http::StatusCode::BAD_REQUEST && sends == 1,
            "old references and malformed readable history must not gain independent authority; case={case} status={status} sends={sends}");
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
        2
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
            && !text.contains("vary_ph_")
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
    assert!(!text.contains("vary_ph_"));
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
        assert!(captured[0].to_string().contains("vary_ph_"));
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
            assert!(first.starts_with("vary_") && second.starts_with("vary_") && first != second);
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
    let protocol_complete = mode != "partial";
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
                && !safe.contains("vary_")
        );
    }
    let safe_usage = serde_json::to_string(&mock_usage)?;
    assert!(
        !safe_usage.contains("private-first")
            && !safe_usage.contains("private-second")
            && !safe_usage.contains("vary_")
    );
    // Custom DLP rules are not a guarantee of arbitrary tool-summary sanitization.
    // The fixture retains summaries only in local memory, never logs or persists them.
    let capture = seen.lock().map_err(|_| "synthetic task capture poisoned")?;
    assert_eq!(capture.len(), 1);
    let sent = capture[0].to_string();
    assert!(
        !sent.contains("private-first")
            && !sent.contains("private-second")
            && sent.contains("vary_")
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
    struct AbortServer(tokio::task::JoinHandle<std::io::Result<()>>);
    impl Drop for AbortServer {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
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
    let (_sender, mut observation_queue) =
        crate::proxy::tool_observation::channel::test_channel().await;
    let observations_before = observation_queue.counters.snapshot().delivered;
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
    let _server = AbortServer(tokio::spawn(
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
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        handlers::handle_responses(State(proxy_state.clone()), request),
    )
    .await??;
    let status = response.status();
    let wire = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        response.into_body().collect(),
    )
    .await??
    .to_bytes();
    {
        let captured = seen.lock().map_err(|_| "synthetic capture poisoned")?;
        assert_eq!(
            captured.len(),
            1,
            "original upstream send must already have completed exactly once"
        );
        assert_eq!(captured[0]["tools"][0]["name"], "synthetic_ns__lookup");
        assert!(
            !captured[0].to_string().contains(FIXTURE_PHONE),
            "fixture plaintext must not reach upstream"
        );
        if !matches!(mode, "legacy" | "noactive") {
            assert!(
                captured[0].to_string().contains("vary_ph_"),
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
        eprintln!("WIRE_XAI_JSON mode={mode} initial_sends=1 target_entries={} active={} full={} encoded={} released={} recovered={} status={}",
            observed.entered, observed.active, observed.full, observed.encoded, observed.released, observed.recovered, status.as_u16());
    }
    assert_eq!(status, if mode == "refuse" { http::StatusCode::BAD_GATEWAY } else { http::StatusCode::OK },
        "target xAI JSON wire serialization must fail closed when its independent budget reservation is refused");
    let value: Value = serde_json::from_slice(&wire)?;
    let observations = observation_queue.counters.snapshot().delivered - observations_before;
    if mode == "refuse" {
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
    while tokio::time::timeout(
        std::time::Duration::from_millis(10),
        observation_queue.recv(),
    )
    .await
    .is_ok()
    {}
    let mut recorder = FinalClientRecorder::default();
    recorder.observe_json(&value);
    let text = std::str::from_utf8(&wire)?;
    assert!(
        !text.contains("vary_ph_"),
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
        assert_eq!(item["name"], "lookup");
        assert_eq!(item["namespace"], "synthetic_ns");
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
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            handlers::handle_responses(State(proxy_state.clone()), followup),
        )
        .await?;
        let response = response.unwrap_or_else(IntoResponse::into_response);
        let followup_status = response.status();
        let followup_wire = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            response.into_body().collect(),
        )
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
    while tokio::time::timeout(
        std::time::Duration::from_millis(10),
        observation_queue.recv(),
    )
    .await
    .is_ok()
    {}
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
