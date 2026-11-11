//! Native Anthropic HTTP integration with synthetic dynamic upstreams only.
use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use super::{
    http_restoration::RestorationGateway,
    payload_budget::PayloadBudget,
    tokenization::publication_test_support,
    unified_test_support::{ResponsesFixture, TestResult, BUDGET_BYTES, CONTACT},
};
use crate::{
    database::Database,
    provider::Provider,
    proxy::{
        failover_switch::FailoverSwitchManager,
        handlers,
        provider_router::ProviderRouter,
        providers::{codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore},
        server::ProxyState,
        types::{ProxyConfig, ProxyStatus},
    },
};

const DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Value>>>);

fn text_sse(text: &str) -> String {
    let events = [
        json!({"type":"message_start","message":{"id":"msg_synthetic_m3","type":"message","role":"assistant","model":"synthetic-m3-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ];
    events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap_or("synthetic_invalid")
            )
        })
        .collect()
}

async fn upstream(State(capture): State<Capture>, Json(body): Json<Value>) -> Response {
    let text = body["messages"][0]["content"]
        .as_str()
        .unwrap_or("Synthetic missing text");
    if let Ok(mut records) = capture.0.lock() {
        records.push(body.clone());
    } else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if body["stream"] == true {
        ([("content-type", "text/event-stream")], text_sse(text)).into_response()
    } else {
        Json(json!({"id":"msg_synthetic_m3","type":"message","role":"assistant",
            "model":"synthetic-m3-model","content":[{"type":"text","text":text}],
            "stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":2},
            "extension":{"types":[true,null,7]}})).into_response()
    }
}

struct UpstreamTask(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for UpstreamTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn proxy_state(db: Arc<Database>, fixture: &ResponsesFixture) -> ProxyState {
    ProxyState {
        pi_runtime: Arc::default(),
        intent_runtime: Arc::default(),
        db: db.clone(),
        config: Arc::new(RwLock::new(ProxyConfig {
            listen_port: 0,
            ..Default::default()
        })),
        status: Arc::new(RwLock::new(ProxyStatus::default())),
        start_time: Arc::new(RwLock::new(None)),
        current_providers: Arc::default(),
        provider_router: Arc::new(ProviderRouter::new(db.clone())),
        workbuddy_upstream: Arc::default(),
        gemini_shadow: Arc::new(GeminiShadowStore::default()),
        codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
        app_handle: None,
        failover_manager: Arc::new(FailoverSwitchManager::new(db)),
        restoration_gateway: fixture.gateway.clone(),
    }
}

async fn roundtrip(stream: bool) -> TestResult {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = ResponsesFixture::new();
    let state = proxy_state(Arc::new(Database::memory()?), &fixture);
    let capture = Capture::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = Router::new()
        .route("/v1/messages", post(upstream))
        .with_state(capture.clone());
    let _task = UpstreamTask(tokio::spawn(
        async move { axum::serve(listener, router).await },
    ));
    let provider = Provider::with_id(
        "synthetic-m3-provider".into(),
        "Synthetic M3 upstream".into(),
        json!({"base_url":format!("http://{address}"),"api_key":"synthetic-m3-key","api_format":"anthropic"}),
        None,
    );
    state.db.save_provider("claude", &provider)?;
    state.db.set_current_provider("claude", &provider.id)?;
    let body = json!({"model":"synthetic-m3-model","max_tokens":64,"stream":stream,
        "messages":[{"role":"user","content":CONTACT}],"extension":{"types":[true,null,7]}});
    let request = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-m3-session")
        .body(Body::from(serde_json::to_vec(&body)?))?;
    let response = tokio::time::timeout(DEADLINE, handlers::handle_messages(State(state), request))
        .await?
        .map_err(|_| "m3_native_anthropic_handler_failed")?;
    assert!(
        response.status() == StatusCode::OK,
        "m3_direct_anthropic_not_admitted"
    );
    let bytes = tokio::time::timeout(DEADLINE, response.into_body().collect())
        .await??
        .to_bytes();
    let wire = std::str::from_utf8(&bytes)?;
    assert!(
        wire.contains(CONTACT),
        "m3_native_anthropic_response_not_restored"
    );
    assert!(
        !wire.contains("rredact-"),
        "m3_native_anthropic_reference_escaped"
    );
    if stream {
        assert!(
            wire.contains("message_stop"),
            "m3_native_anthropic_terminal_missing"
        );
    } else {
        let delivered: Value = serde_json::from_slice(&bytes)?;
        assert!(
            delivered["extension"] == body["extension"],
            "m3_native_response_types_changed"
        );
    }
    let records = capture.0.lock().map_err(|_| "m3_capture_poisoned")?;
    assert_eq!(records.len(), 1, "m3_native_attempt_count_changed");
    let outbound = records[0]["messages"][0]["content"]
        .as_str()
        .ok_or("m3_native_outbound_text_missing")?;
    assert!(
        !outbound.contains(CONTACT) && outbound.contains("rredact-cn_phone-"),
        "m3_native_sensitive_input_forwarded"
    );
    assert!(
        records[0]["extension"] == body["extension"],
        "m3_native_request_types_changed"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_native_anthropic_json_redacts_upstream_restores_final_body() -> TestResult {
    roundtrip(false).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_native_anthropic_sse_redacts_upstream_restores_final_stream() -> TestResult {
    roundtrip(true).await
}

const HIDDEN_CONTACT: &str = "13800000000";
const SESSION: &str = "synthetic-m3-history-session";

#[derive(Clone, Copy)]
enum ReplyMode {
    History,
    HistoryBatches,
    HistoryClose,
    HistoryWithExtension,
    NativeError,
    ErrorAfterTerminal,
    ImmutableOutput,
    Delayed,
    PendingAfterBytes,
    ReadErrorAfterBytes,
    ExactPublic,
    HttpError,
    SignatureError,
    Redirect,
    UnsupportedMedia,
}
#[derive(Clone)]
struct Script {
    capture: Capture,
    mode: ReplyMode,
    wire_len: Arc<std::sync::atomic::AtomicUsize>,
    release_error: Arc<tokio::sync::Notify>,
    redirect_hits: Arc<std::sync::atomic::AtomicUsize>,
}

const RECTIFIER_ERROR_MESSAGE: &str = "Invalid signature in thinking block.";

fn native_blocks(text: &str) -> Value {
    json!([
        {"type":"text","text":text},
        {"type":"thinking","thinking":"Synthetic immutable reasoning.","signature":"synthetic-m3-signature"},
        {"type":"tool_use","id":"synthetic-m3-call","name":"synthetic_lookup",
         "input":{"contact":text,"types":[true,null,7]}}
    ])
}

fn blocks_sse(blocks: &Value) -> TestResult<String> {
    let mut events = vec![json!({"type":"message_start","message":{
        "id":"msg_synthetic_m3","type":"message","role":"assistant","model":"synthetic-m3-model",
        "content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}})];
    for (index, block) in blocks
        .as_array()
        .ok_or("m3_blocks_missing")?
        .iter()
        .enumerate()
    {
        let mut start = block.clone();
        let delta = match block["type"].as_str() {
            Some("text") => {
                start["text"] = json!("");
                json!({"type":"text_delta","text":block["text"]})
            }
            Some("thinking") => {
                start["thinking"] = json!("");
                start["signature"] = json!("");
                json!({"type":"thinking_delta","thinking":block["thinking"]})
            }
            Some("tool_use") => {
                start["input"] = json!({});
                json!({"type":"input_json_delta","partial_json":serde_json::to_string(&block["input"])?})
            }
            _ => return Err("m3_invalid_scripted_block".into()),
        };
        events.push(json!({"type":"content_block_start","index":index,"content_block":start}));
        events.push(json!({"type":"content_block_delta","index":index,"delta":delta}));
        if block["type"] == "thinking" {
            events.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"signature_delta","signature":block["signature"]}}));
        }
        events.push(json!({"type":"content_block_stop","index":index}));
    }
    events.push(json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":2}}));
    events.push(json!({"type":"message_stop"}));
    Ok(events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap_or("synthetic_invalid")
            )
        })
        .collect())
}

async fn scripted_upstream(State(script): State<Script>, Json(body): Json<Value>) -> Response {
    let (round, first) = match script.capture.0.lock() {
        Ok(mut records) => {
            records.push(body.clone());
            (records.len(), records[0].clone())
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(text) = body["messages"][0]["content"].as_str() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let mut response = match script.mode {
        ReplyMode::Delayed => std::future::pending::<Response>().await,
        ReplyMode::PendingAfterBytes | ReplyMode::ReadErrorAfterBytes => {
            let streaming = body["stream"] == true;
            let wire = if streaming { text_sse(text).into_bytes() } else {
                json!({"id":"msg_synthetic_m3","type":"message","role":"assistant",
                    "model":"synthetic-m3-model","content":[{"type":"text","text":text}],
                    "stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":2}}).to_string().into_bytes()
            };
            script.wire_len.store(wire.len(), std::sync::atomic::Ordering::SeqCst);
            let stream = async_stream::stream! {
                yield Ok::<_, std::io::Error>(bytes::Bytes::from(wire));
                if matches!(script.mode, ReplyMode::ReadErrorAfterBytes) {
                    script.release_error.notified().await;
                    yield Err(std::io::Error::other("Synthetic upstream stream failure."));
                } else {
                    std::future::pending::<()>().await;
                }
            };
            ([("content-type", if streaming {"text/event-stream"} else {"application/json"})], Body::from_stream(stream)).into_response()
        }
        ReplyMode::ExactPublic => {
            let wire = if body["stream"] == true { exact_public_sse() } else { exact_public_json().to_owned() };
            ([("content-type", if body["stream"] == true {"text/event-stream"} else {"application/json"})], wire).into_response()
        }
        ReplyMode::HttpError => (StatusCode::TOO_MANY_REQUESTS, Json(json!({"type":"error","error":{
            "type":"rate_limit_error","message":format!("Synthetic upstream failure {CONTACT} {text} synthetic-m3-key"),
            "extension":[true,null,7]}}))).into_response(),
        ReplyMode::SignatureError => (StatusCode::BAD_REQUEST, Json(json!({"type":"error","error":{
            "type":"invalid_request_error","message":RECTIFIER_ERROR_MESSAGE}}))).into_response(),
        ReplyMode::Redirect => (StatusCode::TEMPORARY_REDIRECT, [("location","/synthetic-m3-redirect-sink")]).into_response(),
        ReplyMode::UnsupportedMedia => Json(json!({"id":"msg_synthetic_m3","type":"message","role":"assistant",
            "model":"synthetic-m3-model","content":[{"type":"text","text":text},
                {"type":"tool_use","id":"synthetic-media-call","name":"synthetic_tool","input":{"text":text}},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"c3ludGhldGlj"}}],
            "stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":2}})).into_response(),
        ReplyMode::ImmutableOutput => {
            if body["stream"] == true {
                let mut blocks = native_blocks(text);
                blocks[1]["thinking"] = json!(text);
                match blocks_sse(&blocks) {
                    Ok(wire)=>([("content-type","text/event-stream")],wire).into_response(),
                    Err(_)=>StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            } else {
                Json(json!({"id":"msg_synthetic_m3","type":"message","role":"assistant",
                    "model":"synthetic-m3-model","content":[{"type":"thinking","thinking":text,"signature":"synthetic-immutable-signature"}],
                    "stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":2}})).into_response()
            }
        }
        ReplyMode::History|ReplyMode::HistoryBatches|ReplyMode::HistoryClose|ReplyMode::HistoryWithExtension=>{
            let selected=if round==1 {Some(text)} else if round==2 && matches!(script.mode,ReplyMode::HistoryBatches) {
                Some("Synthetic public second batch.")
            } else {first["system"].as_str()};
            let Some(selected)=selected else {return StatusCode::BAD_REQUEST.into_response();};
            let mut blocks = native_blocks(selected);
            if matches!(script.mode, ReplyMode::HistoryWithExtension) {
                blocks[2]["input"]["_vendor_extension"] = json!({"_typed":[true,null,7]});
            }
            if body["stream"]==true {
                match blocks_sse(&blocks) {
                    Ok(wire)=>([("content-type","text/event-stream")],wire).into_response(),
                    Err(_)=>StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            } else {
                Json(json!({"id":"msg_synthetic_m3","type":"message","role":"assistant","model":"synthetic-m3-model",
                    "content":blocks,"stop_reason":"tool_use","stop_sequence":null,
                    "usage":{"input_tokens":1,"output_tokens":2}})).into_response()
            }
        }
        ReplyMode::NativeError|ReplyMode::ErrorAfterTerminal=>{
            let prefix=if matches!(script.mode,ReplyMode::ErrorAfterTerminal) {text_sse(text)} else {
                let start=json!({"type":"message_start","message":{"id":"msg_synthetic_error","type":"message","role":"assistant","content":[],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":0}}});
                let block=json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Synthetic partial output must not escape."}});
                format!("event: message_start\ndata: {start}\n\nevent: content_block_start\ndata: {block}\n\n")
            };
            let error=json!({"type":"error","error":{"type":"overloaded_error",
                "message":format!("Synthetic failure {CONTACT} synthetic-m3-key"),"extension":[true,null,7]}});
            ([("content-type","text/event-stream")],format!("{prefix}event: error\ndata: {error}\n\n")).into_response()
        }
    };
    if matches!(script.mode, ReplyMode::HistoryClose) {
        // The allocation sweep opens many isolated Hyper upstreams in one runtime.
        response.headers_mut().insert(
            axum::http::header::CONNECTION,
            axum::http::HeaderValue::from_static("close"),
        );
    }
    response
}

struct HttpFixture {
    runtime: ResponsesFixture,
    state: ProxyState,
    capture: Capture,
    provider: Provider,
    budget: PayloadBudget,
    script: Script,
    _task: UpstreamTask,
}
impl HttpFixture {
    async fn new(mode: ReplyMode) -> TestResult<Self> {
        Self::with_budget_limit(mode, BUDGET_BYTES).await
    }

    async fn with_budget_limit(mode: ReplyMode, limit: usize) -> TestResult<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut runtime = ResponsesFixture::new();
        let budget = PayloadBudget::with_limit(limit);
        runtime.gateway = RestorationGateway::with_test_store_and_rules(
            runtime.store.clone(),
            &["chinese_phone_number"],
            budget.clone(),
        );
        let state = proxy_state(Arc::new(Database::memory()?), &runtime);
        let capture = Capture::default();
        let script = Script {
            capture: capture.clone(),
            mode,
            wire_len: Arc::default(),
            release_error: Arc::default(),
            redirect_hits: Arc::default(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = Router::new()
            .route("/v1/messages", post(scripted_upstream))
            .route("/claude/v1/messages", post(scripted_upstream))
            .route("/synthetic-binding/v1/messages", post(scripted_upstream))
            .route(
                "/synthetic-m3-redirect-sink",
                post(|State(script): State<Script>| async move {
                    script
                        .redirect_hits
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    StatusCode::BAD_GATEWAY
                }),
            )
            .with_state(script.clone());
        let task = UpstreamTask(tokio::spawn(
            async move { axum::serve(listener, router).await },
        ));
        let provider = Provider::with_id(
            "synthetic-m3-provider".into(),
            "Synthetic M3 upstream".into(),
            json!({"base_url":format!("http://{address}"),"api_key":"synthetic-m3-key","api_format":"anthropic"}),
            None,
        );
        state.db.save_provider("claude", &provider)?;
        state.db.set_current_provider("claude", &provider.id)?;
        Ok(Self {
            runtime,
            state,
            capture,
            provider,
            budget,
            script,
            _task: task,
        })
    }
    async fn send(&self, body: &Value) -> TestResult<Response> {
        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("content-type", "application/json")
            .header("x-claude-code-session-id", SESSION)
            .body(Body::from(serde_json::to_vec(body)?))?;
        tokio::time::timeout(
            DEADLINE,
            handlers::handle_messages(State(self.state.clone()), request),
        )
        .await?
        .map_err(|_| "m3_scripted_handler_failed".into())
    }
}
async fn collect_wire(response: Response) -> TestResult<(StatusCode, Vec<u8>, bool)> {
    use futures::StreamExt;
    let status = response.status();
    let mut stream = response.into_body().into_data_stream();
    let mut wire = Vec::new();
    let mut failed = false;
    tokio::time::timeout(DEADLINE, async {
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(chunk) => wire.extend_from_slice(&chunk),
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
    })
    .await?;
    Ok((status, wire, failed))
}
async fn assert_history_roundtrip(stream: bool) -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    let first = json!({"model":"synthetic-m3-model","max_tokens":64,"stream":stream,
        "system":HIDDEN_CONTACT,"messages":[{"role":"user","content":CONTACT}]});
    let (status, wire, failed) = collect_wire(fixture.send(&first).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_history_first_delivery_failed"
    );
    let expected = native_blocks(CONTACT);
    if stream {
        let text = std::str::from_utf8(&wire)?;
        assert!(
            text.contains("synthetic-m3-signature") && text.contains(CONTACT),
            "m3_sse_history_fixture_not_delivered"
        );
    } else {
        let value: Value = serde_json::from_slice(&wire)?;
        assert!(
            value["content"] == expected,
            "m3_native_blocks_or_signature_changed"
        );
    }
    let counts = super::tokenization::publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (counts.0, counts.1, counts.2) == (1, 0, 0),
        "m3_first_delivery_not_published"
    );
    let replay = json!({"model":"synthetic-m3-model","max_tokens":64,"stream":false,"messages":[
        {"role":"user","content":"Synthetic prior request."},{"role":"assistant","content":expected},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-m3-call","content":"Synthetic result."}]}
    ]});
    let (status, wire, failed) = collect_wire(fixture.send(&replay).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_history_second_delivery_failed"
    );
    let delivered: Value = serde_json::from_slice(&wire)?;
    assert!(
        delivered["content"][0]["text"] == HIDDEN_CONTACT,
        "m3_exact_history_unrepeated_dependency_not_imported"
    );
    let records = fixture
        .capture
        .0
        .lock()
        .map_err(|_| "m3_history_capture_poisoned")?;
    assert!(
        records.len() == 2 && records[1]["messages"][1]["content"][1] == expected[1],
        "m3_signed_history_was_modified"
    );
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_json_exact_history_imports_unrepeated_dependency() -> TestResult {
    assert_history_roundtrip(false).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_sse_exact_history_imports_unrepeated_dependency() -> TestResult {
    assert_history_roundtrip(true).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_genuine_sse_error_preserves_safe_native_failure_without_publication(
) -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::NativeError).await?;
    let body = json!({"model":"synthetic-m3-model","max_tokens":64,"stream":true,"messages":[{"role":"user","content":CONTACT}]});
    let (status, wire, failed) = collect_wire(fixture.send(&body).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_genuine_native_error_became_transport_failure"
    );
    let text = std::str::from_utf8(&wire)?;
    assert!(
        text.starts_with("event: error\n") && text.contains("overloaded_error"),
        "m3_native_error_semantics_lost"
    );
    assert!(
        !text.contains(CONTACT)
            && !text.contains("synthetic-m3-key")
            && !text.contains("partial output")
            && !text.contains("message_stop"),
        "m3_native_error_released_private_or_partial_success"
    );
    let counts = super::tokenization::publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (counts.0, counts.1, counts.2) == (0, 0, 0),
        "m3_native_error_published_history"
    );
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_error_after_message_stop_is_rejected_without_publication() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::ErrorAfterTerminal).await?;
    let body = json!({"model":"synthetic-m3-model","max_tokens":64,"stream":true,"messages":[{"role":"user","content":CONTACT}]});
    let (status, wire, failed) = collect_wire(fixture.send(&body).await?).await?;
    // The answer is still refused, but nothing was yielded before the failure, so the
    // client gets one fixed error frame instead of a silently broken stream. The
    // completion that preceded the upstream error must never reach it.
    assert!(
        status == StatusCode::OK && !failed,
        "m3_post_terminal_error_broke_stream"
    );
    assert_native_local_sse_error(&wire)?;
    let counts = super::tokenization::publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (counts.0, counts.1, counts.2) == (0, 0, 0),
        "m3_post_terminal_error_published_history"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_local_finalizer_failure_uses_native_error_and_never_edits_signature(
) -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::ImmutableOutput).await?;
    let body = json!({"model":"synthetic-m3-model","max_tokens":64,"stream":false,
        "messages":[{"role":"user","content":CONTACT}]});
    let (status, wire, failed) = collect_wire(fixture.send(&body).await?).await?;
    // The upstream already answered and the answer carries a local placeholder inside a
    // signed block, so it cannot be delivered: the status is server-class and the client
    // projection stays the generic processing failure with no fabricated DLP cause.
    assert!(
        status == StatusCode::BAD_GATEWAY && !failed,
        "m3_immutable_output_was_not_safely_rejected: {status}"
    );
    let error: Value = serde_json::from_slice(&wire)?;
    assert!(
        error["type"] == "error" && error["error"]["type"] == "api_error",
        "m3_local_finalizer_error_not_anthropic_native"
    );
    assert!(
        error["error"]["code"] == "proxy_processing_failed"
            && error["error"]["message"]
                == "The local proxy could not process the request. Please retry."
            && error["error"].get("param").is_none(),
        "m3_local_finalizer_error_code_or_shape_changed: {text}",
        text = std::str::from_utf8(&wire)?
    );
    let text = std::str::from_utf8(&wire)?;
    assert!(
        !text.contains(CONTACT)
            && !text.contains("rredact-")
            && !text.contains("synthetic-immutable-signature"),
        "m3_local_finalizer_error_leaked_private_output"
    );
    let counts = super::tokenization::publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (counts.0, counts.1, counts.2) == (0, 0, 0),
        "m3_immutable_output_published_history"
    );
    Ok(())
}

const DESKTOP_AUTH: &str = "Bearer synthetic-m3-desktop-gateway";
const DESKTOP_MODEL: &str = "claude-sonnet-fixture";
impl HttpFixture {
    fn configure_desktop(&self) -> TestResult {
        self.configure_desktop_model("synthetic-m3-model")
    }
    fn configure_desktop_model(&self, model: &str) -> TestResult {
        let mut provider = self.provider.clone();
        provider.meta = Some(serde_json::from_value(
            json!({"apiFormat":"anthropic","claudeDesktopMode":"proxy",
            "claudeDesktopModelRoutes":{"claude-sonnet-fixture":{"model":model}}}),
        )?);
        self.state.db.save_provider("claude-desktop", &provider)?;
        self.state
            .db
            .set_current_provider("claude-desktop", &provider.id)?;
        self.state.db.set_setting(
            "claude_desktop_gateway_token",
            "synthetic-m3-desktop-gateway",
        )?;
        self.state
            .db
            .set_setting(crate::claude_desktop_config::ROUTING_ENABLED_KEY, "true")?;
        Ok(())
    }
    async fn send_route(
        &self,
        body: &Value,
        uri: &str,
        authorization: Option<&str>,
    ) -> TestResult<Response> {
        use tower::Service;
        let proxy = crate::proxy::server::ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            self.state.db.clone(),
            None,
        )
        .with_restoration_gateway(self.runtime.gateway.clone());
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .header("x-claude-code-session-id", SESSION);
        if let Some(auth) = authorization {
            builder = builder.header("authorization", auth);
        }
        let request = builder.body(Body::from(serde_json::to_vec(body)?))?;
        Ok(tokio::time::timeout(DEADLINE, proxy.build_router().call(request)).await??)
    }
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_messages_aliases_and_queries_keep_native_json_sse() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for uri in [
        "/v1/messages?beta=synthetic",
        "/claude/v1/messages?beta=synthetic",
        "/claude-desktop/v1/messages?beta=synthetic",
    ] {
        for stream in [false, true] {
            let fixture = HttpFixture::new(ReplyMode::History).await?;
            fixture.configure_desktop()?;
            let desktop = uri.starts_with("/claude-desktop/");
            let body = json!({"model":if desktop {DESKTOP_MODEL} else {"synthetic-m3-model"},"max_tokens":64,
                "stream":stream,"system":HIDDEN_CONTACT,"messages":[{"role":"user","content":CONTACT}]});
            let (status, wire, failed) = collect_wire(
                fixture
                    .send_route(&body, uri, desktop.then_some(DESKTOP_AUTH))
                    .await?,
            )
            .await?;
            assert!(
                status == StatusCode::OK && !failed,
                "m3_native_messages_alias_delivery_failed"
            );
            let text = std::str::from_utf8(&wire)?;
            assert!(
                text.contains(CONTACT)
                    && !text.contains("rredact-")
                    && text.contains("synthetic-m3-signature"),
                "m3_native_alias_restoration_or_signature_changed"
            );
            let records = fixture
                .capture
                .0
                .lock()
                .map_err(|_| "m3_alias_capture_poisoned")?;
            assert!(
                records.len() == 1 && records[0]["model"] == "synthetic-m3-model",
                "m3_alias_model_route_changed"
            );
            assert!(
                !records[0].to_string().contains(CONTACT),
                "m3_alias_forwarded_private_text"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_desktop_auth_and_routing_precede_admission() -> TestResult {
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    fixture.configure_desktop()?;
    let stats = fixture.runtime.store.stats()?;
    let body = json!({"model":DESKTOP_MODEL,"max_tokens":64,"messages":[{"role":"user","content":CONTACT}]});
    for auth in [None, Some("Bearer synthetic-wrong-gateway")] {
        let request = {
            let mut b = Request::builder()
                .method("POST")
                .uri("/claude-desktop/v1/messages");
            if let Some(auth) = auth {
                b = b.header("authorization", auth);
            }
            b.body(Body::from(serde_json::to_vec(&body)?))?
        };
        let result =
            handlers::handle_claude_desktop_messages(State(fixture.state.clone()), request).await;
        assert!(
            matches!(result, Err(crate::proxy::error::ProxyError::AuthError(_))),
            "m3_desktop_invalid_auth_reached_admission"
        );
    }
    fixture
        .state
        .db
        .set_setting(crate::claude_desktop_config::ROUTING_ENABLED_KEY, "false")?;
    let request = Request::builder()
        .method("POST")
        .uri("/claude-desktop/v1/messages")
        .header("authorization", DESKTOP_AUTH)
        .body(Body::from(serde_json::to_vec(&body)?))?;
    let result =
        handlers::handle_claude_desktop_messages(State(fixture.state.clone()), request).await;
    assert!(
        matches!(result, Err(crate::proxy::error::ProxyError::ConfigError(_))),
        "m3_desktop_disabled_route_reached_admission"
    );
    assert!(
        fixture.runtime.store.stats()? == stats && transport.transport_counts() == (0, 0),
        "m3_desktop_auth_failure_allocated_or_forwarded"
    );
    assert!(
        fixture
            .capture
            .0
            .lock()
            .map_err(|_| "m3_auth_capture_poisoned")?
            .is_empty(),
        "m3_desktop_auth_failure_sent_upstream"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_desktop_mapping_owner_precharged_through_send_and_refunded() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    fixture.configure_desktop()?;
    let probe = Arc::new(super::http_restoration::mapping_copy_test_support::install());
    let at_send = Arc::new(Mutex::new(None));
    let observed = at_send.clone();
    let send_probe = probe.clone();
    let _transport =
        crate::proxy::forwarder::final_send_binding_test_support::install(move |_, _| {
            if let Ok(mut slot) = observed.lock() {
                *slot = Some(send_probe.snapshot());
            }
        });
    let body = json!({"model":DESKTOP_MODEL,"max_tokens":64,"system":HIDDEN_CONTACT,"messages":[{"role":"user","content":CONTACT}]});
    let (status, wire, failed) = collect_wire(
        fixture
            .send_route(&body, "/claude-desktop/v1/messages", Some(DESKTOP_AUTH))
            .await?,
    )
    .await?;
    assert!(
        status == StatusCode::OK && !failed && std::str::from_utf8(&wire)?.contains(CONTACT),
        "m3_desktop_mapping_fixture_failed"
    );
    let snapshot = at_send
        .lock()
        .map_err(|_| "m3_mapping_capture_poisoned")?
        .ok_or("m3_desktop_send_unobserved")?;
    assert!(
        snapshot.attempts == 1
            && snapshot.created == 1
            && snapshot.live_charge > 0
            && snapshot.released == 0,
        "m3_desktop_mapping_tree_not_precharged_through_send"
    );
    let after = probe.snapshot();
    assert!(
        after.created == 1 && after.released == 1 && after.live_charge == 0,
        "m3_desktop_mapping_owner_not_refunded"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_history_never_crosses_claude_desktop_namespace() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for source_desktop in [false, true] {
        let fixture = HttpFixture::new(ReplyMode::History).await?;
        fixture.configure_desktop()?;
        let first = json!({"model":if source_desktop {DESKTOP_MODEL} else {"synthetic-m3-model"},"max_tokens":64,
            "system":HIDDEN_CONTACT,"messages":[{"role":"user","content":CONTACT}]});
        let route = |desktop| {
            if desktop {
                "/claude-desktop/v1/messages"
            } else {
                "/claude/v1/messages"
            }
        };
        let (status, wire, failed) = collect_wire(
            fixture
                .send_route(
                    &first,
                    route(source_desktop),
                    source_desktop.then_some(DESKTOP_AUTH),
                )
                .await?,
        )
        .await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m3_namespace_first_delivery_failed"
        );
        let delivered: Value = serde_json::from_slice(&wire)?;
        let replay = json!({"model":if source_desktop {"synthetic-m3-model"} else {DESKTOP_MODEL},"max_tokens":64,
            "messages":[{"role":"user","content":"Synthetic prior request."},{"role":"assistant","content":delivered["content"]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-m3-call","content":"Synthetic result."}]}]});
        let (status, wire, _) = collect_wire(
            fixture
                .send_route(
                    &replay,
                    route(!source_desktop),
                    (!source_desktop).then_some(DESKTOP_AUTH),
                )
                .await?,
        )
        .await?;
        assert!(
            !std::str::from_utf8(&wire)?.contains(HIDDEN_CONTACT),
            "m3_cross_app_history_restored_private_dependency"
        );
        if status == StatusCode::OK {
            let literal: Value = serde_json::from_slice(&wire)?;
            let records = fixture
                .capture
                .0
                .lock()
                .map_err(|_| "m3_namespace_capture_poisoned")?;
            let reference = records[0]["system"]
                .as_str()
                .ok_or("m3_namespace_reference_missing")?;
            assert!(
                literal["content"] == native_blocks(reference),
                "m3_cross_app_history_literal_was_modified"
            );
        }
        let counts = super::tokenization::publication_test_support::counts(&fixture.runtime.store)?;
        // A genuine target-app completion may publish its own exact snapshot;
        // neither that snapshot nor the foreign literal grants source-app dependencies.
        assert!(
            counts.1 == 0
                && counts.2 == 0
                && counts.0 == if status == StatusCode::OK { 2 } else { 1 },
            "m3_cross_app_history_completion_count_or_pending_changed"
        );
        if status == StatusCode::OK {
            let literal: Value = serde_json::from_slice(&wire)?;
            let followup = json!({"model":if source_desktop {"synthetic-m3-model"} else {DESKTOP_MODEL},"max_tokens":64,
                "messages":[{"role":"user","content":"Synthetic target-app prior request."},
                    {"role":"assistant","content":literal["content"]},
                    {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-m3-call","content":"Synthetic next result."}]}]});
            let (_, next, _) = collect_wire(
                fixture
                    .send_route(
                        &followup,
                        route(!source_desktop),
                        (!source_desktop).then_some(DESKTOP_AUTH),
                    )
                    .await?,
            )
            .await?;
            assert!(
                !std::str::from_utf8(&next)?.contains(HIDDEN_CONTACT),
                "m3_cross_app_history_laundered_dependency_on_replay"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_desktop_mimo_normalization_never_rewrites_signed_carriers() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for sensitive in [false, true] {
        let fixture = HttpFixture::new(ReplyMode::History).await?;
        fixture.configure_desktop_model("synthetic-mimo-model")?;
        let immutable = json!({"type":"thinking", "thinking":if sensitive {CONTACT} else {"Synthetic signed reasoning."},
            "signature":"synthetic-m3-input-signature","extension":{"types":[true,null,7]}});
        let body = json!({"model":DESKTOP_MODEL,"max_tokens":64,"messages":[
            {"role":"user","content":CONTACT},
            {"role":"assistant","content":[immutable,{"type":"tool_use","id":"synthetic-m3-input-call",
                "name":"synthetic_lookup","input":{"value":"Synthetic input."}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-m3-input-call","content":"Synthetic result."}]}]});
        let (status, wire, _) = collect_wire(
            fixture
                .send_route(&body, "/claude-desktop/v1/messages", Some(DESKTOP_AUTH))
                .await?,
        )
        .await?;
        let records = fixture
            .capture
            .0
            .lock()
            .map_err(|_| "m3_signed_capture_poisoned")?;
        if status == StatusCode::OK {
            assert!(
                !sensitive
                    && records.len() == 1
                    && records[0]["messages"][1]["content"][0] == immutable,
                "m3_desktop_normalization_rewrote_signed_carrier"
            );
        } else {
            assert!(
                records.is_empty() && !std::str::from_utf8(&wire)?.contains(CONTACT),
                "m3_signed_refusal_forwarded_or_leaked"
            );
        }
        let counts = super::tokenization::publication_test_support::counts(&fixture.runtime.store)?;
        if sensitive || status != StatusCode::OK {
            assert!(
                (counts.0, counts.1, counts.2) == (0, 0, 0),
                "m3_signed_refusal_published"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_desktop_strict_routes_and_unsigned_mimo_normalization_remain() -> TestResult {
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    fixture.configure_desktop_model("synthetic-mimo-model")?;
    let body = json!({"model":DESKTOP_MODEL,"max_tokens":64,"messages":[{"role":"user","content":CONTACT},
        {"role":"assistant","content":[{"type":"tool_use","id":"synthetic-m3-input-call","name":"synthetic_lookup","input":{"value":"Synthetic input."}}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-m3-input-call","content":"Synthetic result."}]}],"extension":{"types":[true,null,7]}});
    let (status, _, failed) = collect_wire(
        fixture
            .send_route(&body, "/claude-desktop/v1/messages", Some(DESKTOP_AUTH))
            .await?,
    )
    .await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_unsigned_mimo_normalization_failed"
    );
    let records = fixture
        .capture
        .0
        .lock()
        .map_err(|_| "m3_mimo_capture_poisoned")?;
    assert!(
        records.len() == 1
            && records[0]["model"] == "synthetic-mimo-model"
            && records[0]["extension"] == body["extension"],
        "m3_desktop_mimo_route_or_unknown_types_changed"
    );
    assert!(
        records[0]["messages"][1]["content"][0]
            == json!({"type":"thinking","thinking":"tool call"})
            && records[0]["messages"][1]["content"][1] == body["messages"][1]["content"][0],
        "m3_unsigned_mimo_normalization_semantics_changed"
    );
    drop(records);
    let before = transport.transport_counts();
    let mut unknown = body;
    unknown["model"] = json!("synthetic-unknown-desktop-route");
    let (status, _, _) = collect_wire(
        fixture
            .send_route(&unknown, "/claude-desktop/v1/messages", Some(DESKTOP_AUTH))
            .await?,
    )
    .await?;
    assert!(
        status != StatusCode::OK && transport.transport_counts() == before,
        "m3_desktop_unknown_route_used_fallback"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_desktop_cancel_during_upstream_wait_refunds_mapping_owner() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::Delayed).await?;
    fixture.configure_desktop()?;
    let probe = super::http_restoration::mapping_copy_test_support::install();
    let body = json!({"model":DESKTOP_MODEL,"max_tokens":64,"messages":[{"role":"user","content":CONTACT}]});
    let mut request =
        Box::pin(fixture.send_route(&body, "/claude-desktop/v1/messages", Some(DESKTOP_AUTH)));
    tokio::time::timeout(DEADLINE, async {
        loop {
            tokio::select! {
                _ = &mut request => return Err("m3_cancel_fixture_completed_before_cancel"),
                _ = tokio::task::yield_now() => {}
            }
            let arrived = !fixture
                .capture
                .0
                .lock()
                .map_err(|_| "m3_cancel_capture_poisoned")?
                .is_empty();
            if arrived {
                return Ok(());
            }
        }
    })
    .await??;
    let waiting = probe.snapshot();
    assert!(
        waiting.created == 1 && waiting.released == 0 && waiting.live_charge > 0,
        "m3_cancel_wait_did_not_retain_mapping_charge"
    );
    drop(request);
    let cancelled = probe.snapshot();
    assert!(
        cancelled.created == 1 && cancelled.released == 1 && cancelled.live_charge == 0,
        "m3_cancel_did_not_refund_mapping_charge"
    );
    let counts = super::tokenization::publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (counts.0, counts.1, counts.2) == (0, 0, 0),
        "m3_cancel_published_completion"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_desktop_actual_send_failure_refunds_mapping_owner() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    fixture.configure_desktop()?;
    fixture._task.0.abort();
    tokio::task::yield_now().await;
    let probe = super::http_restoration::mapping_copy_test_support::install();
    let body = json!({"model":DESKTOP_MODEL,"max_tokens":64,"messages":[{"role":"user","content":CONTACT}]});
    let (status, wire, _) = collect_wire(
        fixture
            .send_route(&body, "/claude-desktop/v1/messages", Some(DESKTOP_AUTH))
            .await?,
    )
    .await?;
    assert!(
        status != StatusCode::OK && !std::str::from_utf8(&wire)?.contains(CONTACT),
        "m3_send_failure_not_safe"
    );
    let failed = probe.snapshot();
    assert!(
        failed.created > 0 && failed.created == failed.released && failed.live_charge == 0,
        "m3_send_failure_did_not_refund_mapping_charge"
    );
    let counts = super::tokenization::publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (counts.0, counts.1, counts.2) == (0, 0, 0),
        "m3_send_failure_published_completion"
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum HistoryMutation {
    Exact,
    EditedText,
    MissingBlock,
    ReorderedBlocks,
    PartialTool,
    DuplicateBlock,
    EditedSignature,
    UnknownTail,
    DuplicateBatch,
    ReorderedBatches,
}

async fn history_matrix(
    first_sse: bool,
    second_sse: bool,
    mutation: HistoryMutation,
) -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::HistoryBatches).await?;
    let first = json!({"model":"synthetic-m3-model","max_tokens":64,"stream":first_sse,
        "system":HIDDEN_CONTACT,"messages":[{"role":"user","content":CONTACT}]});
    let (status, _, failed) = collect_wire(fixture.send(&first).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_history_matrix_first_delivery_failed"
    );
    let first_blocks = native_blocks(CONTACT);
    let second = json!({"model":"synthetic-m3-model","max_tokens":64,"stream":second_sse,
        "messages":[{"role":"user","content":"Synthetic previous request."},
            {"role":"assistant","content":first_blocks},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-m3-call","content":"Synthetic edited tool result."}]}]});
    let (status, _, failed) = collect_wire(fixture.send(&second).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_history_matrix_second_delivery_failed"
    );
    let second_blocks = native_blocks("Synthetic public second batch.");
    let mut assistants = vec![first_blocks, second_blocks];
    match mutation {
        HistoryMutation::Exact => {}
        HistoryMutation::EditedText => assistants[1][0]["text"] = json!("Synthetic edited text."),
        HistoryMutation::MissingBlock => {
            assistants[1]
                .as_array_mut()
                .ok_or("m3_blocks_missing")?
                .remove(1);
        }
        HistoryMutation::ReorderedBlocks => assistants[1]
            .as_array_mut()
            .ok_or("m3_blocks_missing")?
            .swap(0, 2),
        HistoryMutation::PartialTool => {
            assistants[1][2]["input"]
                .as_object_mut()
                .ok_or("m3_tool_input_missing")?
                .shift_remove("types");
        }
        HistoryMutation::DuplicateBlock => {
            let block = assistants[1][0].clone();
            assistants[1]
                .as_array_mut()
                .ok_or("m3_blocks_missing")?
                .push(block);
        }
        HistoryMutation::EditedSignature => {
            assistants[1][1]["signature"] = json!("synthetic-edited-signature")
        }
        HistoryMutation::UnknownTail => {
            assistants.push(json!([{"type":"text","text":"Synthetic unrecognized tail."}]))
        }
        HistoryMutation::DuplicateBatch => assistants.push(assistants[1].clone()),
        HistoryMutation::ReorderedBatches => assistants.swap(0, 1),
    }
    let mut messages = vec![json!({"role":"user","content":"Synthetic previous request."})];
    for blocks in assistants {
        messages.push(json!({"role":"assistant","content":blocks}));
    }
    messages.push(json!({"role":"user","content":"Synthetic next request."}));
    let replay =
        json!({"model":"synthetic-m3-model","max_tokens":64,"stream":false,"messages":messages});
    let (status, wire, failed) = collect_wire(fixture.send(&replay).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_history_matrix_replay_delivery_failed"
    );
    let delivered: Value = serde_json::from_slice(&wire)?;
    if matches!(mutation, HistoryMutation::Exact) {
        assert!(
            delivered["content"][0]["text"] == HIDDEN_CONTACT,
            "m3_cumulative_mixed_history_did_not_import_complete_batches"
        );
    } else {
        let hidden_reference = {
            let records = fixture
                .capture
                .0
                .lock()
                .map_err(|_| "m3_history_capture_poisoned")?;
            records[0]["system"].clone()
        };
        assert!(
            hidden_reference
                .as_str()
                .is_some_and(|text| text.contains("rredact-")),
            "m3_history_matrix_hidden_fixture_not_tokenized"
        );
        assert!(
            delivered["content"][0]["text"] == hidden_reference,
            "m3_invalid_history_imported_hidden_dependency_from_valid_prefix"
        );
        assert!(
            !std::str::from_utf8(&wire)?.contains(HIDDEN_CONTACT),
            "m3_invalid_history_released_unrepeated_hidden_plaintext"
        );
    }
    let counts = super::tokenization::publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        counts.1 == 0 && counts.2 == 0,
        "m3_history_matrix_left_pending_or_pinned_context"
    );
    Ok(())
}

macro_rules! history_matrix_case {
    ($name:ident, $first:expr, $second:expr, $mutation:ident) => {
        #[tokio::test(flavor = "current_thread")]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            history_matrix($first, $second, HistoryMutation::$mutation).await
        }
    };
}
history_matrix_case!(
    m3_history_matrix_json_then_sse_exact_cumulative,
    false,
    true,
    Exact
);
history_matrix_case!(
    m3_history_matrix_sse_then_json_exact_cumulative,
    true,
    false,
    Exact
);
history_matrix_case!(
    m3_history_matrix_edited_text_discards_valid_prefix,
    false,
    true,
    EditedText
);
history_matrix_case!(
    m3_history_matrix_missing_block_discards_valid_prefix,
    true,
    false,
    MissingBlock
);
history_matrix_case!(
    m3_history_matrix_reordered_blocks_discards_valid_prefix,
    false,
    true,
    ReorderedBlocks
);
history_matrix_case!(
    m3_history_matrix_partial_tool_discards_valid_prefix,
    true,
    false,
    PartialTool
);
history_matrix_case!(
    m3_history_matrix_duplicate_block_discards_valid_prefix,
    false,
    true,
    DuplicateBlock
);
history_matrix_case!(
    m3_history_matrix_edited_signature_discards_valid_prefix,
    true,
    false,
    EditedSignature
);
history_matrix_case!(
    m3_history_matrix_unknown_tail_discards_valid_prefix,
    false,
    true,
    UnknownTail
);
history_matrix_case!(
    m3_history_matrix_duplicate_batch_discards_valid_prefix,
    true,
    false,
    DuplicateBatch
);
history_matrix_case!(
    m3_history_matrix_reordered_batches_discards_all_imports,
    false,
    true,
    ReorderedBatches
);

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_history_first_and_edited_tool_results_never_grant_hidden_dependency() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for edited in [false, true] {
        let fixture = HttpFixture::new(ReplyMode::History).await?;
        let first = json!({"model":"synthetic-m3-model","max_tokens":64,"system":HIDDEN_CONTACT,
            "messages":[{"role":"user","content":CONTACT}]});
        let (status, _, failed) = collect_wire(fixture.send(&first).await?).await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m3_tool_result_first_delivery_failed"
        );
        let hidden = {
            let records = fixture
                .capture
                .0
                .lock()
                .map_err(|_| "m3_history_capture_poisoned")?;
            records[0]["system"]
                .as_str()
                .ok_or("m3_hidden_reference_missing")?
                .to_string()
        };
        let result = if edited {
            format!("Synthetic edited result {hidden}")
        } else {
            hidden.clone()
        };
        let body = json!({"model":"synthetic-m3-model","max_tokens":64,"messages":[
            {"role":"user","content":"Synthetic prior request."},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-m3-call","content":result}]}]});
        let (status, wire, failed) = collect_wire(fixture.send(&body).await?).await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m3_first_tool_result_delivery_failed"
        );
        let delivered: Value = serde_json::from_slice(&wire)?;
        assert!(
            delivered["content"][0]["text"] == hidden,
            "m3_tool_result_granted_unproven_history_dependency"
        );
        assert!(
            !std::str::from_utf8(&wire)?.contains(HIDDEN_CONTACT),
            "m3_tool_result_released_unrepeated_hidden_plaintext"
        );
    }
    Ok(())
}

fn lifecycle_input(stream: bool) -> Value {
    json!({"model":"synthetic-m3-model","max_tokens":64,"stream":stream,
        "messages":[{"role":"user","content":CONTACT}]})
}
impl HttpFixture {
    fn assert_no_publication(&self) -> TestResult {
        let (published, pending, pins, _) = publication_test_support::counts(&self.runtime.store)?;
        assert!(
            (published, pending, pins) == (0, 0, 0),
            "m3_unexpected_publication_or_pending_context"
        );
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_json_activates_before_body_return_and_drop_refunds_payload() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    let baseline = fixture.budget.used();
    let response = fixture.send(&lifecycle_input(false)).await?;
    assert!(
        response.status() == StatusCode::OK,
        "m3_json_activation_setup_failed"
    );
    let (published, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (published, pending, pins) == (1, 0, 0),
        "m3_json_not_active_before_body_return"
    );
    assert!(
        fixture.budget.used() > baseline,
        "m3_json_wire_has_no_budget_owner"
    );
    drop(response);
    assert!(
        fixture.budget.used() == baseline,
        "m3_json_drop_retained_payload_budget"
    );
    Ok(())
}

async fn anthropic_sse_delivery_boundary(continue_poll: bool, desktop: bool) -> TestResult {
    use futures::StreamExt;
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    let baseline = fixture.budget.used();
    let mut input = lifecycle_input(true);
    let response = if desktop {
        fixture.configure_desktop()?;
        input["model"] = json!(DESKTOP_MODEL);
        fixture
            .send_route(&input, "/claude-desktop/v1/messages", Some(DESKTOP_AUTH))
            .await?
    } else {
        fixture.send(&input).await?
    };
    assert!(
        response.status() == StatusCode::OK,
        "m3_sse_activation_setup_failed"
    );
    fixture.assert_no_publication()?;
    let mut body = response.into_body().into_data_stream();
    let mut wire = Vec::new();
    loop {
        let chunk = tokio::time::timeout(DEADLINE, body.next())
            .await?
            .ok_or("m3_sse_ended_before_message_stop")??;
        wire.extend_from_slice(&chunk);
        if String::from_utf8_lossy(&wire).contains("event: message_stop\n") {
            break;
        }
    }
    let (published, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (published, pending, pins) == (0, 1, 1),
        "m3_sse_activated_at_final_yield"
    );
    let text = std::str::from_utf8(&wire)?;
    assert!(
        text.contains(CONTACT)
            && text.contains("synthetic-m3-signature")
            && !text.contains("rredact-"),
        "m3_sse_final_body_or_signature_changed"
    );
    assert!(
        fixture.budget.used() > baseline,
        "m3_sse_final_yield_has_no_budget_owner"
    );
    if continue_poll {
        assert!(
            tokio::time::timeout(DEADLINE, body.next()).await?.is_none(),
            "m3_sse_continuation_did_not_end"
        );
        let (published, pending, pins, _) =
            publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (published, pending, pins) == (1, 0, 0),
            "m3_sse_continuation_not_active"
        );
    }
    drop(body);
    if !continue_poll {
        fixture.assert_no_publication()?;
    }
    assert!(
        fixture.budget.used() == baseline,
        "m3_sse_delivery_retained_payload_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_sse_last_yield_stays_pending_until_continuation_poll() -> TestResult {
    for desktop in [false, true] {
        anthropic_sse_delivery_boundary(true, desktop).await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_sse_drop_after_last_yield_discards_history_and_budget() -> TestResult {
    for desktop in [false, true] {
        anthropic_sse_delivery_boundary(false, desktop).await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_sse_unpolled_body_drop_never_publishes_and_refunds_payload() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    let baseline = fixture.budget.used();
    let response = fixture.send(&lifecycle_input(true)).await?;
    assert!(
        response.status() == StatusCode::OK && fixture.budget.used() > baseline,
        "m3_unpolled_sse_fixture_failed"
    );
    fixture.assert_no_publication()?;
    drop(response);
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == baseline,
        "m3_unpolled_sse_retained_budget"
    );
    Ok(())
}

async fn anthropic_original_eof_boundary(streaming: bool, read_error: bool) -> TestResult {
    use futures::StreamExt;
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(if read_error {
        ReplyMode::ReadErrorAfterBytes
    } else {
        ReplyMode::PendingAfterBytes
    })
    .await?;
    let baseline = fixture.budget.used();
    let (_capture_guard, capture) = super::http_restoration::capture_test_support::install();
    let input = lifecycle_input(streaming);
    if streaming {
        let response = fixture.send(&input).await?;
        assert!(
            response.status() == StatusCode::OK,
            "m3_eof_sse_setup_failed"
        );
        let mut body = response.into_body().into_data_stream();
        {
            let next = body.next();
            tokio::pin!(next);
            tokio::select! {
                biased;
                _ = &mut next => return Err("m3_sse_released_before_original_eof".into()),
                notice = tokio::time::timeout(DEADLINE, capture.after_bytes(&fixture.script.wire_len)) => {notice?;}
            }
            assert!(
                futures::poll!(next.as_mut()).is_pending(),
                "m3_message_stop_substituted_for_original_eof"
            );
            fixture.assert_no_publication()?;
            assert!(
                fixture.budget.used() > baseline,
                "m3_pending_sse_has_no_budget_owner"
            );
            if read_error {
                fixture.script.release_error.notify_one();
                match tokio::time::timeout(DEADLINE, &mut next).await? {
                    // A read error may never become a successful end of the stream.
                    Some(Err(_)) => {}
                    // Nothing was yielded before the failure, so one fixed error frame
                    // may end it; that still withholds the completion content.
                    Some(Ok(frame)) => assert_native_local_sse_error(frame.as_ref())?,
                    None => return Err("m3_sse_read_error_became_silent_eof".into()),
                }
            }
        }
        drop(body);
    } else {
        let send = fixture.send_route(&input, "/v1/messages", None);
        tokio::pin!(send);
        tokio::select! {
            biased;
            _ = &mut send => return Err("m3_json_completed_before_original_eof".into()),
            notice = tokio::time::timeout(DEADLINE, capture.after_bytes(&fixture.script.wire_len)) => {notice?;}
        }
        assert!(
            futures::poll!(send.as_mut()).is_pending(),
            "m3_complete_json_substituted_for_original_eof"
        );
        fixture.assert_no_publication()?;
        assert!(
            fixture.budget.used() > baseline,
            "m3_pending_json_has_no_budget_owner"
        );
        if read_error {
            fixture.script.release_error.notify_one();
            let (status, wire, failed) =
                collect_wire(tokio::time::timeout(DEADLINE, &mut send).await??).await?;
            assert!(
                !status.is_success() || failed,
                "m3_json_read_error_became_successful_eof"
            );
            let text = std::str::from_utf8(&wire)?;
            assert!(
                !text.contains(CONTACT)
                    && !text.contains("rredact-")
                    && !text.contains("synthetic-m3-key"),
                "m3_json_read_error_leaked_private_body"
            );
            let error: Value = serde_json::from_slice(&wire)?;
            assert!(
                error["type"] == "error" && error["error"]["type"] == "api_error",
                "m3_json_read_error_not_anthropic_native"
            );
            assert!(
                error["error"]["code"] == "proxy_processing_failed"
                    && error["error"].get("param").is_none(),
                "m3_json_read_error_client_code_or_shape_changed"
            );
        }
    }
    tokio::task::yield_now().await;
    fixture.assert_no_publication()?;
    assert!(
        fixture
            .capture
            .0
            .lock()
            .map_err(|_| "m3_eof_capture_poisoned")?
            .len()
            == 1,
        "m3_eof_upstream_not_exercised"
    );
    assert!(
        fixture.budget.used() == baseline,
        "m3_original_eof_failure_or_cancel_retained_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_json_complete_bytes_without_eof_cancel_refunds_without_publication(
) -> TestResult {
    anthropic_original_eof_boundary(false, false).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_sse_message_stop_without_eof_cancel_refunds_without_publication() -> TestResult
{
    anthropic_original_eof_boundary(true, false).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_json_read_error_after_complete_bytes_never_proves_eof() -> TestResult {
    anthropic_original_eof_boundary(false, true).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_sse_read_error_after_message_stop_never_proves_eof() -> TestResult {
    anthropic_original_eof_boundary(true, true).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_final_target_or_principal_mutation_rejects_before_transport() -> TestResult {
    use crate::proxy::forwarder::final_send_binding_test_support;
    for desktop in [false, true] {
        for streaming in [false, true] {
            for principal in [false, true] {
                let fixture = HttpFixture::new(ReplyMode::History).await?;
                fixture.configure_desktop()?;
                let hook = final_send_binding_test_support::install(move |url, headers| {
                    if principal {
                        headers.insert(
                            "x-api-key",
                            axum::http::HeaderValue::from_static("synthetic-unbound-principal"),
                        );
                    } else {
                        url.push_str("?synthetic=unbound");
                    }
                });
                let mut input = lifecycle_input(streaming);
                if desktop {
                    input["model"] = json!(DESKTOP_MODEL);
                }
                let response = fixture
                    .send_route(
                        &input,
                        if desktop {
                            "/claude-desktop/v1/messages"
                        } else {
                            "/v1/messages"
                        },
                        desktop.then_some(DESKTOP_AUTH),
                    )
                    .await?;
                let (status, wire, failed) = collect_wire(response).await?;
                assert!(!status.is_success() || failed, "m3_final_mutation_accepted");
                let text = std::str::from_utf8(&wire)?;
                assert!(
                    !text.contains(CONTACT)
                        && !text.contains("rredact-")
                        && !text.contains("synthetic-m3-key"),
                    "m3_final_mutation_leaked_private_body"
                );
                assert!(
                    hook.transport_counts() == (0, 0)
                        && fixture
                            .capture
                            .0
                            .lock()
                            .map_err(|_| "m3_binding_capture_poisoned")?
                            .is_empty(),
                    "m3_final_mutation_reached_transport"
                );
                fixture.assert_no_publication()?;
                assert!(
                    fixture.budget.used() == 0,
                    "m3_final_mutation_retained_budget"
                );
            }
        }
    }
    Ok(())
}

fn exact_public_json() -> &'static str {
    "{ \"id\" : \"msg_synthetic_exact\", \"type\":\"message\", \"role\":\"assistant\", \"model\":\"synthetic-m3-model\", \"content\":[{\"type\":\"text\",\"text\":\"Synthetic public output.\"}], \"stop_reason\":\"end_turn\", \"stop_sequence\":null, \"usage\":{\"input_tokens\":1,\"output_tokens\":2}, \"extension\":{\"types\":[true,null,7],\"unknown\":\"Synthetic opaque field.\"} }\n"
}
fn exact_public_sse() -> String {
    format!(
        ": Synthetic original framing.\n\n{}",
        text_sse("Synthetic public output.")
    )
}
fn assert_no_private_wire(wire: &[u8]) -> TestResult {
    let text = std::str::from_utf8(wire)?;
    assert!(
        !text.contains(CONTACT)
            && !text.contains(HIDDEN_CONTACT)
            && !text.contains("rredact-")
            && !text.contains("synthetic-m3-key")
            && !text.contains("synthetic-m3-first-key"),
        "m3_safety_error_released_private_content"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_unmodified_empty_lease_preserves_exact_json_and_sse_bytes() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for streaming in [false, true] {
        let fixture = HttpFixture::new(ReplyMode::ExactPublic).await?;
        let mut input = lifecycle_input(streaming);
        input["messages"][0]["content"] = json!("Synthetic ordinary input.");
        let (status, wire, failed) =
            collect_wire(fixture.send_route(&input, "/v1/messages", None).await?).await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m3_exact_public_delivery_failed"
        );
        let expected = if streaming {
            exact_public_sse()
        } else {
            exact_public_json().to_owned()
        };
        assert!(
            wire == expected.as_bytes(),
            "m3_unmodified_native_original_bytes_changed"
        );
        let counts = publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (counts.0, counts.1, counts.2) == (1, 0, 0),
            "m3_empty_lease_delivery_lifecycle_changed"
        );
        assert!(
            fixture.budget.used() == 0,
            "m3_exact_public_retained_budget"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_http_error_keeps_native_safe_shape_without_completion() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for desktop in [false, true] {
        for streaming in [false, true] {
            let fixture = HttpFixture::new(ReplyMode::HttpError).await?;
            if desktop {
                fixture.configure_desktop()?;
            }
            let mut input = lifecycle_input(streaming);
            if desktop {
                input["model"] = json!(DESKTOP_MODEL);
            }
            let (status, wire, failed) = collect_wire(
                fixture
                    .send_route(
                        &input,
                        if desktop {
                            "/claude-desktop/v1/messages"
                        } else {
                            "/v1/messages"
                        },
                        desktop.then_some(DESKTOP_AUTH),
                    )
                    .await?,
            )
            .await?;
            assert!(
                status == StatusCode::TOO_MANY_REQUESTS && !failed,
                "m3_http_error_status_changed"
            );
            assert_no_private_wire(&wire)?;
            let error: Value = serde_json::from_slice(&wire)?;
            assert!(
                error["type"] == "error"
                    && error["error"]["type"] == "rate_limit_error"
                    && error["error"]["extension"] == json!([true, null, 7]),
                "m3_http_error_native_shape_or_types_changed"
            );
            assert!(
                fixture
                    .capture
                    .0
                    .lock()
                    .map_err(|_| "m3_error_capture_poisoned")?
                    .len()
                    == 1,
                "m3_http_error_retried"
            );
            fixture.assert_no_publication()?;
            assert!(fixture.budget.used() == 0, "m3_http_error_retained_budget");
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_redirect_never_reaches_sink_even_with_empty_lease() -> TestResult {
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for mapped in [false, true] {
        let fixture = HttpFixture::new(ReplyMode::Redirect).await?;
        let mut input = lifecycle_input(false);
        if !mapped {
            input["messages"][0]["content"] = json!("Synthetic ordinary input.");
        }
        let before = transport.transport_counts();
        let (status, wire, failed) =
            collect_wire(fixture.send_route(&input, "/v1/messages", None).await?).await?;
        assert!(!status.is_success() || failed, "m3_redirect_accepted");
        assert_no_private_wire(&wire)?;
        assert!(
            fixture
                .capture
                .0
                .lock()
                .map_err(|_| "m3_redirect_capture_poisoned")?
                .len()
                == 1,
            "m3_redirect_first_send_missing"
        );
        assert!(
            fixture
                .script
                .redirect_hits
                .load(std::sync::atomic::Ordering::SeqCst)
                == 0,
            "m3_redirect_reached_unbound_sink"
        );
        let observed = transport.transport_counts();
        eprintln!(
            "m3_redirect_transport before={before:?} after={observed:?} captures=1 sink_hits=0"
        );
        assert_eq!(
            observed,
            (before.0, before.1 + 1),
            "m3_redirect_sent_additional_request"
        );
        fixture.assert_no_publication()?;
        assert!(fixture.budget.used() == 0, "m3_redirect_retained_budget");
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_conversion_provider_rejected_before_transport() -> TestResult {
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for format in ["openai_chat", "openai_responses"] {
        for desktop in [false, true] {
            let fixture = HttpFixture::new(ReplyMode::History).await?;
            let mut provider = fixture.provider.clone();
            provider.meta = Some(serde_json::from_value(
                json!({"apiFormat":format,"claudeDesktopMode":"proxy",
                "claudeDesktopModelRoutes":{"claude-sonnet-fixture":{"model":"synthetic-m3-model"}}}),
            )?);
            let app = if desktop { "claude-desktop" } else { "claude" };
            if desktop {
                fixture.configure_desktop()?;
            }
            fixture.state.db.save_provider(app, &provider)?;
            let mut input = lifecycle_input(false);
            if desktop {
                input["model"] = json!(DESKTOP_MODEL);
            }
            let before = transport.transport_counts();
            let (status, wire, failed) = collect_wire(
                fixture
                    .send_route(
                        &input,
                        if desktop {
                            "/claude-desktop/v1/messages"
                        } else {
                            "/v1/messages"
                        },
                        desktop.then_some(DESKTOP_AUTH),
                    )
                    .await?,
            )
            .await?;
            assert!(
                !status.is_success() || failed,
                "m3_native_conversion_was_admitted"
            );
            assert_no_private_wire(&wire)?;
            assert!(
                transport.transport_counts() == before
                    && fixture
                        .capture
                        .0
                        .lock()
                        .map_err(|_| "m3_conversion_capture_poisoned")?
                        .is_empty(),
                "m3_native_conversion_reached_transport"
            );
            fixture.assert_no_publication()?;
            assert!(fixture.budget.used() == 0, "m3_conversion_retained_budget");
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_signed_sse_never_releases_partial_privileged_output() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let control = HttpFixture::new(ReplyMode::History).await?;
    let (status, wire, failed) = collect_wire(control.send(&lifecycle_input(true)).await?).await?;
    let wire = std::str::from_utf8(&wire)?;
    assert!(
        status == StatusCode::OK
            && !failed
            && wire.contains("event: message_stop\n")
            && wire.contains("synthetic-m3-signature")
            && wire.contains("synthetic-m3-call"),
        "m3_signed_sse_control_did_not_complete"
    );
    let (published, pending, pins, _) = publication_test_support::counts(&control.runtime.store)?;
    assert!((published, pending, pins) == (1, 0, 0));
    assert!(control.budget.used() == 0);
    drop(control);
    let fixture = HttpFixture::new(ReplyMode::ImmutableOutput).await?;
    let (status, wire, failed) = collect_wire(
        fixture
            .send_route(&lifecycle_input(true), "/v1/messages", None)
            .await?,
    )
    .await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_signed_sse_local_error_not_projected_after_commit"
    );
    assert_native_local_sse_error(&wire)?;
    fixture.assert_no_publication()?;
    assert!(fixture.budget.used() == 0, "m3_signed_sse_retained_budget");
    Ok(())
}

fn assert_native_local_sse_error(wire: &[u8]) -> TestResult {
    assert_no_private_wire(wire)?;
    let data = std::str::from_utf8(wire)?
        .strip_prefix("event: error\ndata: ")
        .and_then(|frame| frame.strip_suffix("\n\n"))
        .ok_or("m3_native_local_sse_error_frame_missing")?;
    assert!(
        !data.contains(['\r', '\n']),
        "m3_native_local_sse_error_extra_lines"
    );
    let error: Value = serde_json::from_str(data)?;
    assert!(
        error
            == json!({"type":"error","error":{
                "type":"api_error", "code":"proxy_processing_failed",
                "message":"The local proxy could not process the request. Please retry."
            }}),
        "m3_native_local_sse_error_invalid_projection"
    );
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_unknown_media_rejects_entire_privileged_tool_batch() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::UnsupportedMedia).await?;
    let (status, wire, failed) = collect_wire(
        fixture
            .send_route(&lifecycle_input(false), "/v1/messages", None)
            .await?,
    )
    .await?;
    assert!(
        !status.is_success() || failed,
        "m3_unknown_media_completion_admitted"
    );
    assert_no_private_wire(&wire)?;
    assert!(
        !std::str::from_utf8(&wire)?.contains("synthetic-media-call"),
        "m3_unknown_media_released_partial_tool"
    );
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m3_unknown_media_retained_budget"
    );
    Ok(())
}

// Consume one complete synthetic request, then close without response headers.
async fn install_failed_first_anthropic_provider(
    fixture: &HttpFixture,
) -> TestResult<(UpstreamTask, Capture)> {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let capture = Capture::default();
    let observed = capture.clone();
    let task = UpstreamTask(tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await?;
        let mut wire = Vec::new();
        loop {
            let mut chunk = [0u8; 4096];
            let count = socket.read(&mut chunk).await?;
            if count == 0 || wire.len() + count > 1024 * 1024 {
                return Err(std::io::Error::other("Synthetic request incomplete."));
            }
            wire.extend_from_slice(&chunk[..count]);
            let Some(end) = wire.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let headers = std::str::from_utf8(&wire[..end])
                .map_err(|_| std::io::Error::other("Synthetic headers invalid."))?;
            let length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .ok_or_else(|| std::io::Error::other("Synthetic length missing."))?;
            if wire.len() < end + 4 + length {
                continue;
            }
            let body: Value = serde_json::from_slice(&wire[end + 4..end + 4 + length])
                .map_err(|_| std::io::Error::other("Synthetic JSON invalid."))?;
            observed
                .0
                .lock()
                .map_err(|_| std::io::Error::other("Synthetic capture unavailable."))?
                .push(body);
            return Ok(());
        }
    }));
    let first = Provider::with_id(
        "synthetic-m3-failed-provider".into(),
        "Synthetic failed transport".into(),
        json!({"base_url":format!("http://{address}"),"api_key":"synthetic-m3-first-key","api_format":"anthropic"}),
        None,
    );
    fixture.state.db.save_provider("claude", &first)?;
    fixture
        .state
        .db
        .add_to_failover_queue("claude", &first.id)?;
    fixture
        .state
        .db
        .add_to_failover_queue("claude", &fixture.provider.id)?;
    let mut config = fixture.state.db.get_proxy_config_for_app("claude").await?;
    config.auto_failover_enabled = true;
    config.max_retries = 1;
    fixture.state.db.update_proxy_config_for_app(config).await?;
    Ok((task, capture))
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_transport_retry_binds_actual_second_provider_json_sse() -> TestResult {
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for streaming in [false, true] {
        let fixture = HttpFixture::new(ReplyMode::History).await?;
        let (task, first) = install_failed_first_anthropic_provider(&fixture).await?;
        let before = transport.transport_counts();
        let (status, wire, failed) = collect_wire(
            fixture
                .send_route(&lifecycle_input(streaming), "/v1/messages", None)
                .await?,
        )
        .await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m3_retry_actual_native_provider_rejected"
        );
        let text = std::str::from_utf8(&wire)?;
        assert!(
            text.contains(CONTACT) && !text.contains("rredact-"),
            "m3_retry_actual_attempt_not_restored"
        );
        let observed = transport.transport_counts();
        let first_count = first
            .0
            .lock()
            .map_err(|_| "m3_retry_first_capture_poisoned")?
            .len();
        let second_count = fixture
            .capture
            .0
            .lock()
            .map_err(|_| "m3_retry_second_capture_poisoned")?
            .len();
        eprintln!("m3_retry_transport before={before:?} after={observed:?} first={first_count} second={second_count} task_finished={}",task.0.is_finished());
        assert!(task.0.is_finished(), "m3_retry_first_task_unfinished");
        assert_eq!(
            observed,
            (before.0, before.1 + 2),
            "m3_retry_real_attempts_missing"
        );
        let first = first
            .0
            .lock()
            .map_err(|_| "m3_retry_first_capture_poisoned")?;
        let second = fixture
            .capture
            .0
            .lock()
            .map_err(|_| "m3_retry_second_capture_poisoned")?;
        assert!(
            first.len() == 1 && second.len() == 1,
            "m3_retry_attempt_count_changed"
        );
        for request in first.iter().chain(second.iter()) {
            let text = request["messages"][0]["content"]
                .as_str()
                .ok_or("m3_retry_text_missing")?;
            assert!(
                !text.contains(CONTACT) && text.contains("rredact-cn_phone-"),
                "m3_retry_forwarded_private_input"
            );
        }
        let counts = publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (counts.0, counts.1, counts.2) == (1, 0, 0),
            "m3_retry_published_failed_attempt"
        );
        assert!(fixture.budget.used() == 0, "m3_retry_retained_budget");
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_retry_to_conversion_stops_before_second_send() -> TestResult {
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    let mut second = fixture.provider.clone();
    second.meta = Some(serde_json::from_value(json!({"apiFormat":"openai_chat"}))?);
    fixture.state.db.save_provider("claude", &second)?;
    let (_task, first) = install_failed_first_anthropic_provider(&fixture).await?;
    let (status, wire, failed) = collect_wire(
        fixture
            .send_route(&lifecycle_input(false), "/v1/messages", None)
            .await?,
    )
    .await?;
    assert!(
        !status.is_success() || failed,
        "m3_retry_conversion_accepted"
    );
    assert_no_private_wire(&wire)?;
    let first_count = first
        .0
        .lock()
        .map_err(|_| "m3_retry_first_capture_poisoned")?
        .len();
    let observed = transport.transport_counts();
    let second_count = fixture
        .capture
        .0
        .lock()
        .map_err(|_| "m3_retry_second_capture_poisoned")?
        .len();
    eprintln!("m3_retry_conversion_transport after={observed:?} first={first_count} second={second_count} task_finished={}",_task.0.is_finished());
    assert!(first_count == 1, "m3_retry_first_capture_missing");
    assert_eq!(observed, (0, 1), "m3_retry_first_attempt_not_exercised");
    assert!(
        fixture
            .capture
            .0
            .lock()
            .map_err(|_| "m3_retry_second_capture_poisoned")?
            .is_empty(),
        "m3_retry_conversion_reached_second_send"
    );
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m3_retry_conversion_retained_budget"
    );
    Ok(())
}

impl HttpFixture {
    async fn send_identity_headers(
        &self,
        body: &Value,
        desktop: bool,
        identity_headers: &[(&str, &str)],
    ) -> TestResult<Response> {
        use tower::Service;
        let proxy = crate::proxy::server::ProxyServer::new(
            ProxyConfig {
                listen_port: 0,
                ..Default::default()
            },
            self.state.db.clone(),
            None,
        )
        .with_restoration_gateway(self.runtime.gateway.clone());
        let mut builder = Request::builder()
            .method("POST")
            .uri(if desktop {
                "/claude-desktop/v1/messages"
            } else {
                "/v1/messages"
            })
            .header("content-type", "application/json");
        if desktop {
            builder = builder.header("authorization", DESKTOP_AUTH);
        }
        for (name, value) in identity_headers {
            builder = builder.header(*name, *value);
        }
        let request = builder.body(Body::from(serde_json::to_vec(body)?))?;
        Ok(tokio::time::timeout(DEADLINE, proxy.build_router().call(request)).await??)
    }
}

fn native_history_replay(content: &Value, desktop: bool) -> Value {
    json!({"model":if desktop {DESKTOP_MODEL} else {"synthetic-m3-model"},"max_tokens":64,"stream":false,
        "messages":[{"role":"user","content":"Synthetic prior request."},
            {"role":"assistant","content":content},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-m3-call","content":"Synthetic result."}]}]})
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_hidden_history_isolated_by_actual_session_provider_target_and_principal(
) -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for desktop in [false, true] {
        for streaming in [false, true] {
            for case in 0..4 {
                let fixture = HttpFixture::new(ReplyMode::History).await?;
                if desktop {
                    fixture.configure_desktop()?;
                }
                let app = if desktop { "claude-desktop" } else { "claude" };
                let route = if desktop {
                    "/claude-desktop/v1/messages"
                } else {
                    "/v1/messages"
                };
                let mut input = lifecycle_input(streaming);
                input["system"] = json!(HIDDEN_CONTACT);
                if desktop {
                    input["model"] = json!(DESKTOP_MODEL);
                }
                let (status, wire, failed) = collect_wire(
                    fixture
                        .send_route(&input, route, desktop.then_some(DESKTOP_AUTH))
                        .await?,
                )
                .await?;
                assert!(
                    status == StatusCode::OK && !failed,
                    "m3_isolation_seed_failed"
                );
                assert!(
                    !std::str::from_utf8(&wire)?.contains(HIDDEN_CONTACT),
                    "m3_isolation_hidden_dependency_delivered"
                );
                let reference = fixture
                    .capture
                    .0
                    .lock()
                    .map_err(|_| "m3_isolation_capture_poisoned")?[0]["system"]
                    .as_str()
                    .ok_or("m3_isolation_reference_missing")?
                    .to_owned();
                let mut provider = fixture
                    .state
                    .db
                    .get_provider_by_id(&fixture.provider.id, app)?
                    .ok_or("m3_isolation_provider_missing")?;
                match case {
                    1 => provider.id = "synthetic-m3-isolated-provider".into(),
                    2 => {
                        let base = provider.settings_config["base_url"]
                            .as_str()
                            .ok_or("m3_isolation_base_missing")?
                            .to_owned();
                        provider.settings_config["base_url"] =
                            json!(format!("{base}/synthetic-binding"));
                    }
                    3 => provider.settings_config["api_key"] = json!("synthetic-m3-isolated-key"),
                    _ => {}
                }
                fixture.state.db.save_provider(app, &provider)?;
                fixture.state.db.set_current_provider(app, &provider.id)?;
                let session = if case == 0 {
                    "synthetic-m3-isolated-session"
                } else {
                    SESSION
                };
                let headers = [("x-claude-code-session-id", session)];
                let replay = native_history_replay(&native_blocks(CONTACT), desktop);
                let (status, wire, failed) = collect_wire(
                    fixture
                        .send_identity_headers(&replay, desktop, &headers)
                        .await?,
                )
                .await?;
                assert!(
                    status == StatusCode::OK && !failed,
                    "m3_isolation_literal_delivery_failed"
                );
                let delivered: Value = serde_json::from_slice(&wire)?;
                assert!(
                    delivered["content"][0]["text"] == reference
                        && !std::str::from_utf8(&wire)?.contains(HIDDEN_CONTACT),
                    "m3_changed_binding_imported_hidden_history"
                );
                let followup = native_history_replay(&delivered["content"], desktop);
                let (_, next, _) = collect_wire(
                    fixture
                        .send_identity_headers(&followup, desktop, &headers)
                        .await?,
                )
                .await?;
                assert!(
                    !std::str::from_utf8(&next)?.contains(HIDDEN_CONTACT),
                    "m3_changed_binding_laundered_hidden_history"
                );
                let (_, pending, pins, _) =
                    publication_test_support::counts(&fixture.runtime.store)?;
                assert!(
                    (pending, pins) == (0, 0) && fixture.budget.used() == 0,
                    "m3_isolation_retained_owners"
                );
                assert!(
                    fixture
                        .capture
                        .0
                        .lock()
                        .map_err(|_| "m3_isolation_capture_poisoned")?
                        .len()
                        == 3,
                    "m3_isolation_replay_not_exercised"
                );
            }
        }
    }
    Ok(())
}

async fn assert_anthropic_current_only_identity(case: usize) -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for desktop in [false, true] {
        for streaming in [false, true] {
            let fixture = HttpFixture::new(ReplyMode::History).await?;
            if desktop {
                fixture.configure_desktop()?;
            }
            let mut input = lifecycle_input(streaming);
            input["system"] = json!(HIDDEN_CONTACT);
            if desktop {
                input["model"] = json!(DESKTOP_MODEL);
            }
            let mut headers = vec![("session-id", SESSION)];
            match case {
                1 => {
                    headers.extend([
                        ("x-claude-code-session-id", "synthetic invalid identity"),
                        ("claude-code-session-id", SESSION),
                    ]);
                    input["metadata"] = json!({"session_id":SESSION});
                }
                2 => headers.extend([
                    ("x-claude-code-session-id", SESSION),
                    ("x-claude-code-session-id", SESSION),
                ]),
                3 => {
                    input["metadata"] = json!({"session_id":7,"user_id":format!("user_synthetic_session_{SESSION}")})
                }
                _ => {}
            }
            let (status, wire, failed) = collect_wire(
                fixture
                    .send_identity_headers(&input, desktop, &headers)
                    .await?,
            )
            .await?;
            assert!(
                status == StatusCode::OK && !failed,
                "m3_current_only_identity_delivery_failed"
            );
            let text = std::str::from_utf8(&wire)?;
            assert!(
                text.contains(CONTACT)
                    && !text.contains(HIDDEN_CONTACT)
                    && !text.contains("rredact-"),
                "m3_current_only_identity_restoration_missing"
            );
            // A current-only delivery may complete in an anonymous namespace; replay below
            // proves that its hidden dependency is not visible to a later valid identity.
            let (_, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
            assert!(
                (pending, pins) == (0, 0),
                "m3_current_only_delivery_retained_owners"
            );
            let reference = fixture
                .capture
                .0
                .lock()
                .map_err(|_| "m3_identity_capture_poisoned")?[0]["system"]
                .as_str()
                .ok_or("m3_identity_reference_missing")?
                .to_owned();
            let replay = native_history_replay(&native_blocks(CONTACT), desktop);
            let valid = [("x-claude-code-session-id", SESSION)];
            let (status, wire, failed) = collect_wire(
                fixture
                    .send_identity_headers(&replay, desktop, &valid)
                    .await?,
            )
            .await?;
            assert!(
                status == StatusCode::OK && !failed,
                "m3_identity_literal_delivery_failed"
            );
            let delivered: Value = serde_json::from_slice(&wire)?;
            assert!(
                delivered["content"][0]["text"] == reference
                    && !std::str::from_utf8(&wire)?.contains(HIDDEN_CONTACT),
                "m3_invalid_identity_granted_historical_privilege"
            );
            let (_, next, _) = collect_wire(
                fixture
                    .send_identity_headers(
                        &native_history_replay(&delivered["content"], desktop),
                        desktop,
                        &valid,
                    )
                    .await?,
            )
            .await?;
            assert!(
                !std::str::from_utf8(&next)?.contains(HIDDEN_CONTACT),
                "m3_invalid_identity_laundered_hidden_dependency"
            );
            let (_, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
            assert!(
                (pending, pins) == (0, 0) && fixture.budget.used() == 0,
                "m3_identity_retained_owners"
            );
        }
    }
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_missing_identity_restores_current_without_history_privilege() -> TestResult {
    assert_anthropic_current_only_identity(0).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_invalid_priority_identity_never_falls_back() -> TestResult {
    assert_anthropic_current_only_identity(1).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_duplicate_identity_never_grants_continuity() -> TestResult {
    assert_anthropic_current_only_identity(2).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_invalid_metadata_identity_never_falls_back_to_user_id() -> TestResult {
    assert_anthropic_current_only_identity(3).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_http_each_payload_buffer_capacity_failure_is_atomic_and_refunded(
) -> TestResult {
    use super::payload_buffer::test_support::{self, Action, Phase};
    use std::{cell::Cell, rc::Rc};
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for desktop in [false, true] {
        for streaming in [false, true] {
            let fixture = HttpFixture::new(ReplyMode::HistoryClose).await?;
            if desktop {
                fixture.configure_desktop()?;
            }
            let mut input = lifecycle_input(streaming);
            if desktop {
                input["model"] = json!(DESKTOP_MODEL);
            }
            let count = Rc::new(Cell::new(0usize));
            let observed = count.clone();
            let hook = test_support::install(move |event| {
                if event.phase == Phase::CapacityObserved {
                    observed.set(observed.get() + 1);
                }
                Action::Continue
            });
            let route = if desktop {
                "/claude-desktop/v1/messages"
            } else {
                "/v1/messages"
            };
            let (status, _, failed) = collect_wire(
                fixture
                    .send_route(&input, route, desktop.then_some(DESKTOP_AUTH))
                    .await?,
            )
            .await?;
            drop(hook);
            assert!(
                status == StatusCode::OK && !failed,
                "m3_capacity_sweep_baseline_failed"
            );
            let allocations = count.get();
            assert!(allocations > 0, "m3_capacity_sweep_zero_allocations");
            drop(fixture);
            for fail_at in 1..=allocations {
                let fixture = HttpFixture::new(ReplyMode::HistoryClose).await?;
                if desktop {
                    fixture.configure_desktop()?;
                }
                let count = Rc::new(Cell::new(0usize));
                let fired = Rc::new(Cell::new(false));
                let observed = count.clone();
                let injected = fired.clone();
                let hook = test_support::install(move |event| {
                    if event.phase == Phase::CapacityObserved {
                        observed.set(observed.get() + 1);
                        if observed.get() == fail_at {
                            injected.set(true);
                            return Action::ReportCapacity(usize::MAX);
                        }
                    }
                    Action::Continue
                });
                let (status, wire, failed) = collect_wire(
                    fixture
                        .send_route(&input, route, desktop.then_some(DESKTOP_AUTH))
                        .await?,
                )
                .await?;
                drop(hook);
                assert!(fired.get(), "m3_capacity_sweep_injection_not_reached");
                if status.is_success() && !failed {
                    assert!(streaming, "m3_capacity_sweep_json_failure_ignored");
                    assert_native_local_sse_error(&wire)?;
                } else {
                    assert_no_private_wire(&wire)?;
                }
                fixture.assert_no_publication()?;
                assert!(
                    fixture.budget.used() == 0,
                    "m3_capacity_sweep_retained_budget"
                );
            }
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_admission_budget_exhaustion_never_sends_or_publishes() -> TestResult {
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for desktop in [false, true] {
        for streaming in [false, true] {
            let fixture = HttpFixture::new(ReplyMode::History).await?;
            if desktop {
                fixture.configure_desktop()?;
            }
            let mut input = lifecycle_input(streaming);
            if desktop {
                input["model"] = json!(DESKTOP_MODEL);
            }
            let saturation = fixture.budget.reserve(fixture.budget.limit())?;
            let before = transport.transport_counts();
            let (status, wire, failed) = collect_wire(
                fixture
                    .send_route(
                        &input,
                        if desktop {
                            "/claude-desktop/v1/messages"
                        } else {
                            "/v1/messages"
                        },
                        desktop.then_some(DESKTOP_AUTH),
                    )
                    .await?,
            )
            .await?;
            assert!(
                !status.is_success() || failed,
                "m3_exhausted_admission_accepted"
            );
            assert_no_private_wire(&wire)?;
            assert!(
                transport.transport_counts() == before
                    && fixture
                        .capture
                        .0
                        .lock()
                        .map_err(|_| "m3_admission_capture_poisoned")?
                        .is_empty(),
                "m3_exhausted_admission_reached_transport"
            );
            fixture.assert_no_publication()?;
            drop(saturation);
            assert!(
                fixture.budget.used() == 0,
                "m3_exhausted_admission_retained_budget"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_upstream_signature_error_is_sanitized_without_rectifier_resend() -> TestResult
{
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let config = crate::proxy::types::RectifierConfig::default();
    assert!(
        crate::proxy::thinking_rectifier::should_rectify_thinking_signature(
            Some(RECTIFIER_ERROR_MESSAGE),
            &config
        ),
        "m3_signature_error_fixture_does_not_match_rectifier"
    );
    for desktop in [false, true] {
        for streaming in [false, true] {
            let fixture = HttpFixture::new(ReplyMode::SignatureError).await?;
            fixture.state.db.set_rectifier_config(&config)?;
            if desktop {
                fixture.configure_desktop()?;
            }
            let blocks = native_blocks(CONTACT);
            let mut input = native_history_replay(&blocks, desktop);
            input["stream"] = json!(streaming);
            let route = if desktop {
                "/claude-desktop/v1/messages"
            } else {
                "/v1/messages"
            };
            let before = transport.transport_counts();
            let (status, wire, failed) = collect_wire(
                fixture
                    .send_route(&input, route, desktop.then_some(DESKTOP_AUTH))
                    .await?,
            )
            .await?;

            assert!(
                status == StatusCode::BAD_REQUEST && !failed,
                "m3_rectifier_original_status_changed"
            );
            assert_no_private_wire(&wire)?;
            let error: Value = serde_json::from_slice(&wire)?;
            // Error shape survives; overlapping private request strings must not.
            assert!(
                error["type"] == "error" && error["error"].is_object(),
                "m3_rectifier_native_error_envelope_lost"
            );
            assert!(
                error["error"].get("code").is_none(),
                "m3_rectifier_error_became_fallback_or_local_error"
            );
            for field in ["type", "message"] {
                assert!(
                    error["error"][field]
                        .as_str()
                        .is_some_and(|value| value.contains("[REDACTED]")),
                    "m3_rectifier_error_overlap_not_redacted field={field}"
                );
            }
            assert!(
                !std::str::from_utf8(&wire)?.contains("synthetic-m3-signature"),
                "m3_rectifier_error_released_signature"
            );

            let records = fixture
                .capture
                .0
                .lock()
                .map_err(|_| "m3_rectifier_capture_poisoned")?;
            assert_eq!(
                records.len(),
                1,
                "m3_rectifier_upstream_capture_count_changed"
            );
            assert!(
                records[0]["messages"][1]["content"][1] == blocks[1],
                "m3_rectifier_rewrote_signed_block"
            );
            assert_eq!(
                transport.transport_counts(),
                (before.0, before.1 + 1),
                "m3_rectifier_attempt_count_changed"
            );
            assert!(
                !fixture._task.0.is_finished(),
                "m3_rectifier_upstream_task_terminated_early"
            );
            fixture.assert_no_publication()?;
            assert!(fixture.budget.used() == 0, "m3_rectifier_retained_budget");
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_immutable_sensitive_request_rejected_before_send() -> TestResult {
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for desktop in [false, true] {
        for streaming in [false, true] {
            let fixture = HttpFixture::new(ReplyMode::SignatureError).await?;
            if desktop {
                fixture.configure_desktop()?;
            }
            let mut blocks = native_blocks(CONTACT);
            blocks[1]["thinking"] = json!(HIDDEN_CONTACT);
            let mut input = native_history_replay(&blocks, desktop);
            input["stream"] = json!(streaming);
            let route = if desktop {
                "/claude-desktop/v1/messages"
            } else {
                "/v1/messages"
            };
            let before = transport.transport_counts();
            let (status, wire, failed) = collect_wire(
                fixture
                    .send_route(&input, route, desktop.then_some(DESKTOP_AUTH))
                    .await?,
            )
            .await?;

            assert!(
                !status.is_success() && !failed,
                "m3_immutable_sensitive_request_was_not_safely_rejected"
            );
            assert_no_private_wire(&wire)?;
            let text = std::str::from_utf8(&wire)?;
            assert!(
                !text.contains("synthetic-m3-signature"),
                "m3_immutable_error_released_signature"
            );
            // This path cannot attribute a cause, so it keeps the accepted generic
            // projection; the signed-unit guard below attributes its own cause.
            assert!(
                fixture
                    .capture
                    .0
                    .lock()
                    .map_err(|_| "m3_immutable_capture_poisoned")?
                    .is_empty(),
                "m3_immutable_sensitive_request_reached_upstream"
            );
            assert_eq!(
                transport.transport_counts(),
                before,
                "m3_immutable_sensitive_request_reached_transport"
            );
            fixture.assert_no_publication()?;
            assert!(
                fixture.budget.used() == 0,
                "m3_immutable_sensitive_request_retained_budget"
            );
        }
    }
    Ok(())
}

async fn seed_outbound_history(fixture: &HttpFixture) -> TestResult<Value> {
    let mut input = lifecycle_input(false);
    input["system"] = json!(HIDDEN_CONTACT);
    let (status, wire, failed) = collect_wire(fixture.send(&input).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_outbound_history_seed_failed"
    );
    let delivered: Value = serde_json::from_slice(&wire)?;
    let (published, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (published, pending, pins) == (1, 0, 0),
        "m3_outbound_history_seed_not_published"
    );
    Ok(delivered["content"].clone())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_outbound_history_preserves_native_tool_input_extensions() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::HistoryWithExtension).await?;
    let blocks = seed_outbound_history(&fixture).await?;
    let extension = json!({"_typed":[true,null,7]});
    assert!(
        blocks[2]["input"]["_vendor_extension"] == extension,
        "m3_outbound_history_seed_extension_missing"
    );
    let (status, wire, failed) =
        collect_wire(fixture.send(&native_history_replay(&blocks, false)).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_outbound_history_replay_failed"
    );
    let delivered: Value = serde_json::from_slice(&wire)?;
    assert!(
        delivered["content"][0]["text"] == HIDDEN_CONTACT,
        "m3_outbound_history_import_not_exercised"
    );
    let records = fixture
        .capture
        .0
        .lock()
        .map_err(|_| "m3_outbound_history_capture_poisoned")?;
    assert!(records.len() == 2, "m3_outbound_history_capture_missing");
    let sent = &records[1]["messages"][1]["content"];
    assert!(
        sent[1] == blocks[1],
        "m3_outbound_history_signature_changed"
    );
    assert!(
        sent[2]["input"]["_vendor_extension"] == extension,
        "m3_outbound_history_unknown_field_filtered"
    );
    assert!(
        fixture.budget.used() == 0,
        "m3_outbound_history_replay_retained_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_outbound_history_rejects_override_before_transport() -> TestResult {
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    let blocks = seed_outbound_history(&fixture).await?;
    let replay = native_history_replay(&blocks, false);
    let mut replacement = replay["messages"].clone();
    // Use only public synthetic text so ordinary redaction is not the rejection mechanism.
    replacement[1]["content"] = json!([
        {"type":"text","text":"Synthetic replaced assistant."},
        {"type":"thinking","thinking":"Synthetic immutable reasoning.","signature":"synthetic-replaced-signature"},
        {"type":"tool_use","id":"synthetic-m3-call","name":"synthetic_lookup","input":{}}
    ]);
    let mut provider = fixture.provider.clone();
    provider.meta = Some(serde_json::from_value(json!({
        "apiFormat":"anthropic", "localProxyRequestOverrides":{"body":{"messages":replacement}}
    }))?);
    fixture.state.db.save_provider("claude", &provider)?;
    let before = transport.transport_counts();
    let (status, wire, failed) =
        collect_wire(fixture.send_route(&replay, "/v1/messages", None).await?).await?;
    assert!(
        transport.transport_counts() == before,
        "m3_outbound_history_override_reached_transport"
    );
    assert!(
        !status.is_success() && !failed,
        "m3_outbound_history_override_not_rejected"
    );
    assert_no_private_wire(&wire)?;
    let records = fixture
        .capture
        .0
        .lock()
        .map_err(|_| "m3_outbound_history_capture_poisoned")?;
    assert!(
        records.len() == 1,
        "m3_outbound_history_override_reached_upstream"
    );
    let (published, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (published, pending, pins) == (1, 0, 0),
        "m3_outbound_history_override_published"
    );
    assert!(
        fixture.budget.used() == 0,
        "m3_outbound_history_override_retained_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_desktop_resolver_capacity_rejects_before_mapping_and_send() -> TestResult {
    const LIMIT: usize = 1024 * 1024;
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let mut body = lifecycle_input(false);
    body["model"] = json!(DESKTOP_MODEL);
    let route = "/claude-desktop/v1/messages";
    let control = HttpFixture::with_budget_limit(ReplyMode::History, LIMIT).await?;
    control.configure_desktop()?;
    let (status, _, failed) =
        collect_wire(control.send_route(&body, route, Some(DESKTOP_AUTH)).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m3_desktop_resolver_control_failed"
    );
    assert!(
        control.budget.used() == 0,
        "m3_desktop_resolver_control_retained_budget"
    );
    drop(control);

    let fixture = HttpFixture::with_budget_limit(ReplyMode::History, LIMIT).await?;
    fixture.configure_desktop()?;
    let mut provider = fixture.provider.clone();
    provider.meta = Some(serde_json::from_value(json!({
        "apiFormat":"anthropic", "claudeDesktopMode":"proxy", "claudeDesktopModelRoutes":{
            "claude-sonnet-fixture":{"model":"synthetic-m3-model"},
            "claude-haiku-fixture":{"model":"synthetic-unused-model", "labelOverride":"x".repeat(2 * LIMIT)}
        }
    }))?);
    assert!(
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.claude_desktop_model_routes.get("claude-haiku-fixture"))
            .and_then(|route| route.label_override.as_deref())
            .is_some_and(|label| label.len() > LIMIT),
        "m3_desktop_resolver_oversized_label_fixture_missing"
    );
    fixture
        .state
        .db
        .save_provider("claude-desktop", &provider)?;
    let mapping = super::http_restoration::mapping_copy_test_support::install();
    let before = transport.transport_counts();
    let (status, wire, failed) =
        collect_wire(fixture.send_route(&body, route, Some(DESKTOP_AUTH)).await?).await?;
    assert!(
        transport.transport_counts() == before,
        "m3_desktop_resolver_unbudgeted_route_reached_transport"
    );
    assert!(
        mapping.snapshot().attempts == 0,
        "m3_desktop_resolver_entered_mapping_after_budget_denial"
    );
    assert!(
        !status.is_success() && !failed,
        "m3_desktop_resolver_capacity_not_rejected"
    );
    assert_no_private_wire(&wire)?;
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m3_desktop_resolver_denial_retained_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_desktop_resolver_unknown_route_withholds_request_model() -> TestResult {
    const UNKNOWN: &str = "synthetic-unconfigured-desktop-route-marker";
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::new(ReplyMode::History).await?;
    fixture.configure_desktop()?;
    let mut body = lifecycle_input(false);
    body["model"] = json!(UNKNOWN);
    let before = transport.transport_counts();
    let (status, wire, failed) = collect_wire(
        fixture
            .send_route(&body, "/claude-desktop/v1/messages", Some(DESKTOP_AUTH))
            .await?,
    )
    .await?;
    assert!(
        !status.is_success() && !failed,
        "m3_desktop_resolver_unknown_not_rejected"
    );
    assert!(
        transport.transport_counts() == before,
        "m3_desktop_resolver_unknown_reached_transport"
    );
    assert!(
        !String::from_utf8_lossy(&wire).contains(UNKNOWN),
        "m3_desktop_resolver_unknown_echoed_request_model"
    );
    assert_no_private_wire(&wire)?;
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m3_desktop_resolver_unknown_retained_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_desktop_resolver_suffix_only_legacy_capacity_precedes_mapping() -> TestResult
{
    const LIMIT: usize = 1024 * 1024;
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = HttpFixture::with_budget_limit(ReplyMode::History, LIMIT).await?;
    fixture.configure_desktop()?;
    let mut provider = fixture.provider.clone();
    provider.meta = Some(serde_json::from_value(json!({
        "apiFormat":"anthropic", "claudeDesktopMode":"proxy", "claudeDesktopModelRoutes":{
            "claude-sonnet-fixture":{"model":"synthetic-m3-model"},
            "   ":{"model":"x".repeat(2 * LIMIT)}
        }
    }))?);
    fixture
        .state
        .db
        .save_provider("claude-desktop", &provider)?;
    let mut body = lifecycle_input(false);
    body["model"] = json!("[1m]");
    let mapping = super::http_restoration::mapping_copy_test_support::install();
    let before = transport.transport_counts();
    let (status, wire, failed) = collect_wire(
        fixture
            .send_route(&body, "/claude-desktop/v1/messages", Some(DESKTOP_AUTH))
            .await?,
    )
    .await?;
    assert!(
        mapping.snapshot().attempts == 0,
        "m3_desktop_resolver_legacy_model_copied_before_denial"
    );
    assert!(
        transport.transport_counts() == before,
        "m3_desktop_resolver_legacy_capacity_reached_transport"
    );
    assert!(
        !status.is_success() && !failed,
        "m3_desktop_resolver_legacy_capacity_not_rejected"
    );
    assert_no_private_wire(&wire)?;
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m3_desktop_resolver_legacy_capacity_retained_budget"
    );
    Ok(())
}

async fn assert_vendor_immutable_history(desktop: bool, model: &str, redacted: bool) -> TestResult {
    let transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for streaming in [false, true] {
        for sensitive in [false, true] {
            let fixture = HttpFixture::new(ReplyMode::History).await?;
            if desktop {
                fixture.configure_desktop_model(model)?;
            }
            let immutable = if redacted {
                json!({"type":"redacted_thinking","data":if sensitive {CONTACT} else {"synthetic-opaque-history"},
                    "extension":{"types":[true,null,7]}})
            } else {
                json!({"type":"thinking","thinking":if sensitive {CONTACT} else {"Synthetic signed reasoning."},
                    "signature":"synthetic-m3-vendor-signature","extension":{"types":[true,null,7]}})
            };
            let body = json!({"model":if desktop {DESKTOP_MODEL} else {model},"max_tokens":64,"stream":streaming,
                "messages":[{"role":"user","content":CONTACT},
                {"role":"assistant","content":[immutable,{"type":"tool_use","id":"synthetic-vendor-call",
                    "name":"synthetic_lookup","input":{"value":"Synthetic input."}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"synthetic-vendor-call","content":"Synthetic result."}]}]});
            let before_transport = transport.transport_counts();
            let before_publication = publication_test_support::counts(&fixture.runtime.store)?;
            let before_budget = fixture.budget.used();
            let response = if desktop {
                fixture
                    .send_route(&body, "/claude-desktop/v1/messages", Some(DESKTOP_AUTH))
                    .await?
            } else {
                fixture.send(&body).await?
            };
            let (status, wire, failed) = collect_wire(response).await?;
            let records = fixture
                .capture
                .0
                .lock()
                .map_err(|_| "m3_vendor_capture_poisoned")?;
            if status == StatusCode::OK {
                assert!(
                    !sensitive
                        && !failed
                        && records.len() == 1
                        && records[0]["messages"][1]["content"] == body["messages"][1]["content"],
                    "m3_vendor_normalizer_rewrote_immutable_history"
                );
            } else {
                assert!(
                    !failed
                        && records.is_empty()
                        && transport.transport_counts() == before_transport,
                    "m3_vendor_immutable_refusal_reached_transport"
                );
                let text = std::str::from_utf8(&wire)?;
                assert!(
                    !text.contains(CONTACT)
                        && !text.contains("synthetic-m3-vendor-signature")
                        && !text.contains("synthetic-opaque-history")
                        && !text.contains("rredact-"),
                    "m3_vendor_immutable_refusal_leaked_payload"
                );
                let error: Value = serde_json::from_slice(&wire)?;
                // Still a fixed, sanitized projection, but it names the cause the caller
                // can act on. Status, code and message must agree.
                assert_eq!(status, StatusCode::BAD_REQUEST, "m3_vendor_status: {text}");
                assert!(
                    error["error"]["type"] == "invalid_request_error"
                        && error["error"]["code"] == "dlp_restore_unsupported"
                        && error["error"]["message"] == "This request contains signed or otherwise immutable content that reversible DLP cannot modify. Remove that content, or disable reversible DLP for this provider.",
                    "m3_vendor_immutable_refusal_not_fixed: {text}"
                );
                assert!(
                    publication_test_support::counts(&fixture.runtime.store)? == before_publication,
                    "m3_vendor_immutable_refusal_published"
                );
            }
            assert!(
                fixture.budget.used() == before_budget,
                "m3_vendor_immutable_budget_not_refunded"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_vendor_immutable_claude_mimo_signed() -> TestResult {
    assert_vendor_immutable_history(false, "synthetic-mimo-model", false).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_vendor_immutable_claude_mimo_redacted() -> TestResult {
    assert_vendor_immutable_history(false, "synthetic-mimo-model", true).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_vendor_immutable_claude_deepseek_signed() -> TestResult {
    assert_vendor_immutable_history(false, "synthetic-deepseek-model", false).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_vendor_immutable_claude_deepseek_redacted() -> TestResult {
    assert_vendor_immutable_history(false, "synthetic-deepseek-model", true).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_vendor_immutable_desktop_deepseek_signed() -> TestResult {
    assert_vendor_immutable_history(true, "synthetic-deepseek-model", false).await
}
#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m3_anthropic_vendor_immutable_desktop_deepseek_redacted() -> TestResult {
    assert_vendor_immutable_history(true, "synthetic-deepseek-model", true).await
}
