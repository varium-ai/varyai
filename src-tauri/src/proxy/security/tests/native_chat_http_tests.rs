//! Direct Chat HTTP integration with synthetic, dynamically bound upstreams.
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

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Value>>>);

async fn chat_upstream(State(capture): State<Capture>, Json(body): Json<Value>) -> Response {
    let text = match body["messages"][0]["content"].as_str() {
        Some(text) => text,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };
    match capture.0.lock() {
        Ok(mut requests) => requests.push(body.clone()),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    if body["stream"] == true {
        let chunk = json!({"id":"synthetic-m2-response", "object":"chat.completion.chunk",
            "model":"synthetic-m2-model", "choices":[{"index":0,
            "delta":{"role":"assistant","content":text}, "finish_reason":null}]});
        let terminal = json!({"id":"synthetic-m2-response", "object":"chat.completion.chunk",
            "model":"synthetic-m2-model", "choices":[{"index":0,
            "delta":{}, "finish_reason":"stop"}]});
        (
            [("content-type", "text/event-stream")],
            format!("data: {chunk}\n\ndata: {terminal}\n\ndata: [DONE]\n\n"),
        )
            .into_response()
    } else {
        Json(
            json!({"id":"synthetic-m2-response", "object":"chat.completion",
            "model":"synthetic-m2-model", "choices":[{"index":0,
            "message":{"role":"assistant","content":text}, "finish_reason":"stop"}],
            "extension":{"types":[true,null,7]}}),
        )
        .into_response()
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

async fn direct_roundtrip(stream: bool) -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = ResponsesFixture::new();
    let state = proxy_state(Arc::new(Database::memory()?), &fixture);
    let capture = Capture::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = Router::new()
        .route("/v1/chat/completions", post(chat_upstream))
        .with_state(capture.clone());
    let _task = UpstreamTask(tokio::spawn(async move {
        axum::serve(listener, upstream).await
    }));
    let provider = Provider::with_id(
        "synthetic-m2-provider".into(),
        "Synthetic M2 upstream".into(),
        json!({"base_url":format!("http://{address}"), "api_key":"synthetic-m2-key",
            "api_format":"chat"}),
        None,
    );
    state.db.save_provider("codex", &provider)?;
    state.db.set_current_provider("codex", &provider.id)?;
    let body = json!({"model":"synthetic-m2-model", "stream":stream,
        "messages":[{"role":"user","content":CONTACT}],
        "extension":{"keep":true,"types":[true,null,7]}});
    let request = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("session-id", "synthetic-m2-session")
        .body(Body::from(serde_json::to_vec(&body)?))?;
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        handlers::handle_chat_completions(State(state), request),
    )
    .await?
    .map_err(|_| "m2_chat_handler_failed")?;
    assert!(
        response.status() == StatusCode::OK,
        "m2_direct_chat_not_admitted"
    );
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        response.into_body().collect(),
    )
    .await??
    .to_bytes();
    let wire = std::str::from_utf8(&bytes)?;
    assert!(wire.contains(CONTACT), "m2_chat_response_not_restored");
    assert!(!wire.contains("rredact-"), "m2_chat_reference_escaped");
    if stream {
        assert!(wire.contains("[DONE]"), "m2_chat_terminal_missing");
    } else {
        let delivered: Value = serde_json::from_slice(&bytes)?;
        assert!(
            delivered["extension"]["types"] == json!([true, null, 7]),
            "m2_chat_unknown_response_types_changed"
        );
    }
    let requests = capture.0.lock().map_err(|_| "m2_capture_poisoned")?;
    assert_eq!(requests.len(), 1, "m2_chat_unexpected_attempt_count");
    let outbound = requests[0]["messages"][0]["content"]
        .as_str()
        .ok_or("m2_chat_outbound_text_missing")?;
    assert!(
        !outbound.contains(CONTACT) && outbound.contains("rredact-cn_phone-"),
        "m2_chat_sensitive_input_forwarded"
    );
    assert!(
        requests[0]["extension"] == body["extension"],
        "m2_chat_unknown_request_types_changed"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_direct_chat_json_redacts_upstream_and_restores_final_client_body() -> TestResult {
    direct_roundtrip(false).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_direct_chat_sse_redacts_upstream_and_restores_final_client_stream() -> TestResult {
    direct_roundtrip(true).await
}

// The scripted upstream records actual HTTP requests, never grants restoration authority.
const HIDDEN_CONTACT: &str = "13800000000";
const TOOL_NAME: &str = "synthetic_m2_nonexecuting_tool";
const SESSION: &str = "synthetic-m2-session";
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Clone, Copy)]
enum ReplyMode {
    Echo,
    EchoSse,
    History,
    HistorySse,
    MixedHistory,
    ExactJson,
    InvalidJson,
    GzipJson,
    GzipPending,
    GzipReadError,
    GzipCorrupt,
    GzipLayered,
    GzipExactJson,
    GzipAtLimit,
    GzipOversized,
    PendingJsonHeaders,
    Redirect,
    EntityValidators,
    HttpError,
    InvalidFraming,
    ErrorAfterTerminal,
    NativeSseError,
    NativeSsePrivateError,
    NativeSseDataError,
    NativeSseToolError,
    NativeSseMultiChoiceError,
    PendingAfterTerminal,
}

#[derive(Clone)]
struct ScriptedUpstream {
    capture: Capture,
    mode: ReplyMode,
    terminal_sent: Arc<tokio::sync::Notify>,
    release_error: Arc<tokio::sync::Notify>,
    compressed_len: Arc<std::sync::atomic::AtomicUsize>,
    uris: Arc<Mutex<Vec<String>>>,
    redirect_hits: Arc<std::sync::atomic::AtomicUsize>,
}

fn tool_completion(text: &str) -> Value {
    let arguments = json!({"contact":text}).to_string();
    json!({"id":"synthetic-m2-history-response", "object":"chat.completion",
    "model":"synthetic-m2-model", "choices":[
        {"index":4,"message":{"role":"assistant","content":text,"tool_calls":[
            {"id":"synthetic-m2-call-a","type":"function","function":{
                "name":TOOL_NAME,"arguments":arguments}},
            {"id":"synthetic-m2-call-b","type":"function","function":{
                "name":TOOL_NAME,"arguments":"{}"}}
        ]},"finish_reason":"tool_calls"},
        {"index":2,"message":{"role":"assistant","content":"Synthetic other choice."},
            "finish_reason":"stop"}
    ]})
}

fn tool_sse(text: &str) -> String {
    let chunk = json!({"id":"synthetic-m2-stream", "object":"chat.completion.chunk",
        "model":"synthetic-m2-model", "choices":[{"index":0,"delta":{
            "role":"assistant","content":text,"tool_calls":[{"index":0,
                "id":"synthetic-m2-stream-call","type":"function","function":{
                    "name":TOOL_NAME,"arguments":json!({"contact":text}).to_string()}}]},
            "finish_reason":null}]});
    let terminal = json!({"id":"synthetic-m2-stream", "object":"chat.completion.chunk",
        "model":"synthetic-m2-model", "choices":[{"index":0,"delta":{},
            "finish_reason":"tool_calls"}]});
    format!("data: {chunk}\n\ndata: {terminal}\n\ndata: [DONE]\n\n")
}

fn exact_json_wire() -> &'static str {
    " { \"id\":\"synthetic-m2-exact\", \"object\":\"chat.completion\",\n\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"Synthetic unchanged output\"},\"finish_reason\":\"stop\"}],\"extension\":[true,null,7] }\n"
}

async fn scripted_upstream(
    State(script): State<ScriptedUpstream>,
    uri: axum::http::Uri,
    Json(body): Json<Value>,
) -> Response {
    match script.uris.lock() {
        Ok(mut uris) => uris.push(uri.to_string()),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    let (round, first) = match script.capture.0.lock() {
        Ok(mut requests) => {
            requests.push(body.clone());
            (requests.len(), requests[0].clone())
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let text = match body["messages"][0]["content"].as_str() {
        Some(text) => text,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };
    match script.mode {
        ReplyMode::MixedHistory => {
            let selected = if round == 1 {
                Some(text)
            } else {
                first["messages"][round - 1]["content"].as_str()
            };
            let Some(selected) = selected else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            if body["stream"] == true {
                ([("content-type", "text/event-stream")], tool_sse(selected)).into_response()
            } else {
                Json(tool_completion(selected)).into_response()
            }
        }
        ReplyMode::History if round == 1 => Json(tool_completion(text)).into_response(),
        ReplyMode::HistorySse if round == 1 => {
            ([("content-type", "text/event-stream")], tool_sse(text)).into_response()
        }
        ReplyMode::EchoSse => {
            ([("content-type", "text/event-stream")], tool_sse(text)).into_response()
        }
        ReplyMode::ExactJson => {
            ([("content-type", "application/json")], exact_json_wire()).into_response()
        }
        ReplyMode::Redirect => (
            StatusCode::TEMPORARY_REDIRECT,
            [("location", "/synthetic-redirect-sink")],
            "Synthetic redirect",
        )
            .into_response(),
        ReplyMode::PendingJsonHeaders => {
            script.terminal_sent.notify_one();
            std::future::pending::<Response>().await
        }
        ReplyMode::GzipJson
        | ReplyMode::GzipPending
        | ReplyMode::GzipReadError
        | ReplyMode::GzipCorrupt
        | ReplyMode::GzipLayered
        | ReplyMode::GzipExactJson
        | ReplyMode::GzipAtLimit
        | ReplyMode::GzipOversized => {
            use std::io::Write;
            let wire = match script.mode {
                ReplyMode::GzipExactJson => exact_json_wire().as_bytes().to_vec(),
                ReplyMode::GzipAtLimit | ReplyMode::GzipOversized => {
                    let mut wire = exact_json_wire().as_bytes().to_vec();
                    wire.resize(
                        1024 * 1024 + usize::from(matches!(script.mode, ReplyMode::GzipOversized)),
                        b' ',
                    );
                    wire
                }
                _ => match serde_json::to_vec(&tool_completion(text)) {
                    Ok(wire) => wire,
                    Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                },
            };
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            if encoder.write_all(&wire).is_err() {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            let mut wire = match encoder.finish() {
                Ok(wire) => wire,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
            if matches!(script.mode, ReplyMode::GzipCorrupt) {
                wire.truncate(wire.len().saturating_sub(8));
            }
            let layered = matches!(script.mode, ReplyMode::GzipLayered);
            if layered {
                let mut encoder =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                if encoder.write_all(&wire).is_err() {
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
                wire = match encoder.finish() {
                    Ok(wire) => wire,
                    Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                };
            }
            script
                .compressed_len
                .store(wire.len(), std::sync::atomic::Ordering::SeqCst);
            let body = if matches!(
                script.mode,
                ReplyMode::GzipPending | ReplyMode::GzipReadError
            ) {
                let stream = async_stream::stream! {
                    yield Ok::<_, std::io::Error>(bytes::Bytes::from(wire));
                    script.terminal_sent.notify_one();
                    if matches!(script.mode, ReplyMode::GzipReadError) {
                        // The test releases this only after capture has consumed all bytes and polled Pending.
                        script.release_error.notified().await;
                        yield Err(std::io::Error::other("Synthetic upstream stream failure."));
                    } else {
                        std::future::pending::<()>().await;
                    }
                };
                Body::from_stream(stream)
            } else {
                Body::from(wire)
            };
            let mut response = Response::new(body);
            response.headers_mut().insert(
                "content-type",
                axum::http::HeaderValue::from_static("application/json"),
            );
            response.headers_mut().insert(
                "content-encoding",
                axum::http::HeaderValue::from_static(if layered {
                    "gzip, deflate"
                } else {
                    "gzip"
                }),
            );
            response
        }
        ReplyMode::EntityValidators => {
            let mut response = Json(tool_completion(text)).into_response();
            for name in [
                "etag",
                "digest",
                "content-md5",
                "content-digest",
                "repr-digest",
            ] {
                response.headers_mut().insert(
                    axum::http::HeaderName::from_static(name),
                    axum::http::HeaderValue::from_static("synthetic-stale-validator"),
                );
            }
            response
        }
        ReplyMode::InvalidJson => {
            ([("content-type", "application/json")], "{\"choices\":[").into_response()
        }
        ReplyMode::Echo | ReplyMode::History | ReplyMode::HistorySse => {
            // Only the first request contains this dependency; the delivered assistant
            // does not. Re-scanning visible replay text cannot authorize this response.
            let restored = if round > 1 {
                first["messages"][1]["content"]
                    .as_str()
                    .or_else(|| first["messages"][0]["content"].as_str())
            } else {
                Some(text)
            };
            match restored {
                Some(restored) => Json(json!({"id":"synthetic-m2-next-response",
                    "object":"chat.completion","model":"synthetic-m2-model",
                    "choices":[{"index":0,"message":{"role":"assistant","content":restored},
                        "finish_reason":"stop"}]}))
                .into_response(),
                None => StatusCode::BAD_REQUEST.into_response(),
            }
        }
        ReplyMode::HttpError => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"message":format!(
                "synthetic-m2-private-error {CONTACT} {text} synthetic-m2-key"),
                "type":"invalid_request_error","code":"synthetic_error"}})),
        )
            .into_response(),
        ReplyMode::InvalidFraming => {
            let wire = tool_sse(text).replacen("\n\n", "\n", 1);
            ([("content-type", "text/event-stream")], wire).into_response()
        }
        ReplyMode::NativeSseError
        | ReplyMode::NativeSsePrivateError
        | ReplyMode::NativeSseDataError
        | ReplyMode::NativeSseToolError
        | ReplyMode::NativeSseMultiChoiceError => {
            let message = if matches!(script.mode, ReplyMode::NativeSseError) {
                "Synthetic upstream quota exhausted.".to_string()
            } else {
                format!("Synthetic upstream quota exhausted. {CONTACT} {text} synthetic-m2-key")
            };
            let error = json!({"error":{"message":message,"type":"rate_limit_error",
                "code":"synthetic_quota","param":null}});
            let prefix = if matches!(script.mode, ReplyMode::NativeSseMultiChoiceError) {
                // One choice has ended, but another still owns an incomplete privileged tool.
                let chunk = json!({"id":"synthetic-m2-multichoice-error","object":"chat.completion.chunk",
                    "choices":[{"index":0,"delta":{"role":"assistant","content":"Synthetic finished choice."},"finish_reason":"stop"},
                    {"index":1,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"synthetic-m2-incomplete-call",
                        "type":"function","function":{"name":TOOL_NAME,"arguments":text}}]},"finish_reason":null}]});
                format!("data: {chunk}\n\n")
            } else if matches!(script.mode, ReplyMode::NativeSseToolError) {
                let chunk = json!({"id":"synthetic-m2-error-stream","choices":[{"index":0,
                    "delta":{"role":"assistant","tool_calls":[{"index":0,"id":"synthetic-m2-error-call",
                        "type":"function","function":{"name":TOOL_NAME,"arguments":text}}]},"finish_reason":null}]});
                format!("data: {chunk}\n\n")
            } else {
                String::new()
            };
            (
                [("content-type", "text/event-stream")],
                format!(
                    "{prefix}{}data: {error}\n\n",
                    if matches!(script.mode, ReplyMode::NativeSseDataError) {
                        ""
                    } else {
                        "event: error\n"
                    }
                ),
            )
                .into_response()
        }
        ReplyMode::ErrorAfterTerminal => {
            let error = json!({"error":{"message":CONTACT,"type":"synthetic_error"}});
            let wire = format!("{}event: error\ndata: {error}\n\n", tool_sse(text));
            ([("content-type", "text/event-stream")], wire).into_response()
        }
        ReplyMode::PendingAfterTerminal => {
            let wire = bytes::Bytes::from(tool_sse(text));
            let stream = async_stream::stream! {
                yield Ok::<_, std::io::Error>(wire);
                script.terminal_sent.notify_one();
                std::future::pending::<()>().await;
            };
            let mut response = Response::new(Body::from_stream(stream));
            response.headers_mut().insert(
                "content-type",
                axum::http::HeaderValue::from_static("text/event-stream"),
            );
            response
        }
    }
}

struct HttpFixture {
    state: ProxyState,
    script: ScriptedUpstream,
    budget: PayloadBudget,
    _task: UpstreamTask,
    _transport: crate::proxy::forwarder::final_send_binding_test_support::Guard,
    runtime: ResponsesFixture,
}

impl HttpFixture {
    async fn new(mode: ReplyMode, api_format: &str) -> TestResult<Self> {
        // Keep dynamic loopback traffic independent of real and system proxy configuration.
        let transport =
            crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
        let runtime = ResponsesFixture::new();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let mut state = proxy_state(Arc::new(Database::memory()?), &runtime);
        state.restoration_gateway = RestorationGateway::with_test_store_and_rules(
            runtime.store.clone(),
            &["chinese_phone_number"],
            budget.clone(),
        );
        let script = ScriptedUpstream {
            capture: Capture::default(),
            mode,
            terminal_sent: Arc::new(tokio::sync::Notify::new()),
            release_error: Arc::new(tokio::sync::Notify::new()),
            compressed_len: Arc::default(),
            uris: Arc::default(),
            redirect_hits: Arc::default(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let upstream = Router::new()
            .route("/v1/chat/completions", post(scripted_upstream))
            .route("/v1/responses", post(scripted_upstream))
            .route(
                "/synthetic-redirect-sink",
                axum::routing::any(|State(script): State<ScriptedUpstream>| async move {
                    script
                        .redirect_hits
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Json(tool_completion(CONTACT))
                }),
            )
            .fallback(scripted_upstream)
            .with_state(script.clone());
        let task = UpstreamTask(tokio::spawn(async move {
            axum::serve(listener, upstream).await
        }));
        let provider = Provider::with_id(
            "synthetic-m2-provider".into(),
            "Synthetic M2 scripted upstream".into(),
            json!({"base_url":format!("http://{address}"),"api_key":"synthetic-m2-key",
                "api_format":api_format}),
            None,
        );
        state.db.save_provider("codex", &provider)?;
        state.db.set_current_provider("codex", &provider.id)?;
        Ok(Self {
            state,
            script,
            budget,
            _task: task,
            _transport: transport,
            runtime,
        })
    }

    async fn send(&self, request: Request<Body>) -> TestResult<Response> {
        Ok(tokio::time::timeout(
            DEADLINE,
            handlers::handle_chat_completions(State(self.state.clone()), request),
        )
        .await
        .map_err(|_| "m2_script_handler_timeout")?
        .unwrap_or_else(IntoResponse::into_response))
    }

    fn requests(&self) -> TestResult<Vec<Value>> {
        Ok(self
            .script
            .capture
            .0
            .lock()
            .map_err(|_| "m2_script_capture_poisoned")?
            .clone())
    }

    fn assert_no_publication(&self) -> TestResult {
        let (published, pending, pins, _) = publication_test_support::counts(&self.runtime.store)?;
        assert!(
            (published, pending, pins) == (0, 0, 0),
            "m2_failed_or_anonymous_attempt_retained_history_owner"
        );
        Ok(())
    }
}

fn chat_request(body: &Value, session: Option<&str>) -> TestResult<Request<Body>> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json");
    if let Some(session) = session {
        builder = builder.header("session-id", session);
    }
    Ok(builder.body(Body::from(serde_json::to_vec(body)?))?)
}

fn current_input(stream: bool) -> Value {
    json!({"model":"synthetic-m2-model","stream":stream,
        "messages":[{"role":"user","content":CONTACT}]})
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
    .await
    .map_err(|_| "m2_script_body_timeout")?;
    Ok((status, wire, failed))
}

fn assert_private_body_absent(wire: &[u8]) {
    let text = String::from_utf8_lossy(wire);
    assert!(
        !text.contains(CONTACT)
            && !text.contains(HIDDEN_CONTACT)
            && !text.contains("rredact-")
            && !text.contains(TOOL_NAME),
        "m2_rejected_response_released_private_content_or_tools"
    );
}

async fn assert_current_only_identity(invalid: bool) -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::Echo, "chat").await?;
    let mut body = current_input(false);
    body["messages"]
        .as_array_mut()
        .ok_or("m2_identity_messages_missing")?
        .push(json!({"role":"developer","content":HIDDEN_CONTACT}));
    if invalid {
        body["metadata"] = json!({"session_id":"synthetic-m2-fallback"});
    }
    let mut request = chat_request(&body, invalid.then_some("synthetic-m2-fallback"))?;
    if invalid {
        // A present invalid higher-priority source cannot fall back to either source.
        request.headers_mut().insert(
            "thread-id",
            axum::http::HeaderValue::from_static("synthetic invalid identity"),
        );
    }
    let (status, wire, failed) = collect_wire(fixture.send(request).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m2_anonymous_current_restoration_unavailable"
    );
    let delivered: Value =
        serde_json::from_slice(&wire).map_err(|_| "m2_identity_response_invalid")?;
    assert!(
        delivered["choices"][0]["message"]["content"] == CONTACT,
        "m2_anonymous_current_restoration_missing"
    );
    let requests = fixture.requests()?;
    assert!(requests.len() == 1, "m2_identity_attempt_count_changed");
    assert!(
        !serde_json::to_string(&requests[0])?.contains(CONTACT)
            && !serde_json::to_string(&requests[0])?.contains(HIDDEN_CONTACT),
        "m2_identity_sensitive_input_forwarded"
    );
    let hidden_reference = requests[0]["messages"][1]["content"]
        .as_str()
        .ok_or("m2_identity_hidden_reference_missing")?;
    assert!(
        hidden_reference.contains("rredact-cn_phone-"),
        "m2_identity_dependency_not_tokenized"
    );
    let replay = json!({"model":"synthetic-m2-model","messages":[
        {"role":"user","content":"Synthetic follow-up."},
        delivered["choices"][0]["message"].clone()
    ]});
    let (status, wire, failed) = collect_wire(
        fixture
            .send(chat_request(&replay, Some("synthetic-m2-fallback"))?)
            .await?,
    )
    .await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m2_identity_unknown_literal_rejected"
    );
    let next: Value = serde_json::from_slice(&wire).map_err(|_| "m2_identity_next_invalid")?;
    assert!(
        next["choices"][0]["message"]["content"] == hidden_reference
            && !String::from_utf8_lossy(&wire).contains(HIDDEN_CONTACT),
        "m2_anonymous_delivery_granted_historical_privilege"
    );
    let (_, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (pending, pins) == (0, 0),
        "m2_identity_retained_pending_owner"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_missing_identity_restores_current_request_without_history_privilege() -> TestResult
{
    assert_current_only_identity(false).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_invalid_priority_identity_cannot_fall_back_or_publish_history() -> TestResult {
    assert_current_only_identity(true).await
}

async fn first_history_round(fixture: &HttpFixture) -> TestResult<Vec<Value>> {
    let mut body = current_input(false);
    body["messages"]
        .as_array_mut()
        .ok_or("m2_history_messages_missing")?
        .push(json!({"role":"developer","content":HIDDEN_CONTACT}));
    let (status, wire, failed) =
        collect_wire(fixture.send(chat_request(&body, Some(SESSION))?).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m2_history_first_round_rejected"
    );
    let delivered: Value =
        serde_json::from_slice(&wire).map_err(|_| "m2_history_first_response_invalid")?;
    assert!(
        !String::from_utf8_lossy(&wire).contains("rredact-")
            && !String::from_utf8_lossy(&wire).contains(HIDDEN_CONTACT),
        "m2_history_first_reference_escaped"
    );
    let assistants = delivered["choices"]
        .as_array()
        .ok_or("m2_history_choices_missing")?
        .iter()
        .map(|choice| choice["message"].clone())
        .collect::<Vec<_>>();
    assert!(
        assistants.len() == 2
            && assistants[0] == tool_completion(CONTACT)["choices"][0]["message"]
            && assistants[1] == tool_completion(CONTACT)["choices"][1]["message"],
        "m2_history_first_native_assistant_or_calls_changed"
    );
    let (published, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (published, pending, pins) == (1, 0, 0),
        "m2_history_first_delivery_not_published"
    );
    Ok(assistants)
}

fn history_replay(assistants: &[Value]) -> Value {
    let mut messages = vec![json!({"role":"user","content":"Synthetic prior request."})];
    for assistant in assistants {
        messages.push(assistant.clone());
        if let Some(calls) = assistant["tool_calls"].as_array() {
            for call in calls {
                messages.push(json!({"role":"tool","tool_call_id":call["id"],
                    "content":CONTACT}));
            }
        }
    }
    messages.push(json!({"role":"user","content":"Synthetic continuation."}));
    json!({"model":"synthetic-m2-model","messages":messages,"stream":false})
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_two_round_exact_native_assistant_and_tool_replay_imports_dependencies(
) -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::History, "chat").await?;
    let assistants = first_history_round(&fixture).await?;
    let (status, wire, failed) = collect_wire(
        fixture
            .send(chat_request(&history_replay(&assistants), Some(SESSION))?)
            .await?,
    )
    .await?;
    let requests = fixture.requests()?;
    assert!(requests.len() == 2, "m2_history_second_attempt_missing");
    let outbound = serde_json::to_string(&requests[1])?;
    assert!(
        !outbound.contains(CONTACT)
            && !outbound.contains(HIDDEN_CONTACT)
            && requests[1].get("input").is_none()
            && requests[1]["messages"][1]["tool_calls"][0]["id"] == "synthetic-m2-call-a"
            && requests[1]["messages"][1]["tool_calls"][1]["id"] == "synthetic-m2-call-b",
        "m2_history_native_messages_or_tool_results_not_preserved_safely"
    );
    assert!(
        status == StatusCode::OK && !failed,
        "m2_history_native_dependency_import_missing"
    );
    let delivered: Value =
        serde_json::from_slice(&wire).map_err(|_| "m2_history_second_response_invalid")?;
    assert!(
        delivered["choices"][0]["message"]["content"] == HIDDEN_CONTACT
            && !String::from_utf8_lossy(&wire).contains("rredact-"),
        "m2_history_import_did_not_restore_unrepeated_dependency"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_edited_reordered_or_truncated_native_replay_cannot_import_dependencies(
) -> TestResult {
    for case in 0..4 {
        let fixture = HttpFixture::new(ReplyMode::History, "chat").await?;
        let mut assistants = first_history_round(&fixture).await?;
        match case {
            0 => assistants[0]["tool_calls"][0]["function"]["arguments"] = json!("{}"),
            1 => assistants.reverse(),
            2 => {
                assistants[0]["tool_calls"]
                    .as_array_mut()
                    .ok_or("m2_history_calls_missing")?
                    .reverse();
            }
            _ => assistants.truncate(1),
        }
        let (status, wire, failed) = collect_wire(
            fixture
                .send(chat_request(&history_replay(&assistants), Some(SESSION))?)
                .await?,
        )
        .await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_history_unknown_literal_rejected"
        );
        let requests = fixture.requests()?;
        let hidden_reference = requests[0]["messages"][1]["content"]
            .as_str()
            .ok_or("m2_history_hidden_reference_missing")?;
        let next: Value =
            serde_json::from_slice(&wire).map_err(|_| "m2_history_nonexact_invalid")?;
        assert!(
            next["choices"][0]["message"]["content"] == hidden_reference
                && !String::from_utf8_lossy(&wire).contains(HIDDEN_CONTACT),
            "m2_history_nonexact_replay_imported_dependencies"
        );
        let (_, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (pending, pins) == (0, 0),
            "m2_history_literal_delivery_retained_owner"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_gateway_cannot_reuse_direct_native_history_or_forged_ingress() -> TestResult {
    use crate::proxy::gateway::{self, KeyPolicy, Period};
    let fixture = HttpFixture::new(ReplyMode::History, "chat").await?;
    let assistants = first_history_round(&fixture).await?;
    let created = fixture.state.db.create_gateway_key(KeyPolicy {
        name: "Synthetic M2 isolation".into(),
        models: vec!["synthetic-m2-model".into()],
        all_models: false,
        token_limit: None,
        period: Period::Never,
    })?;
    let mut request = chat_request(&history_replay(&assistants), Some(SESSION))?;
    *request.uri_mut() = "/gateway/v1/chat/completions".parse()?;
    request
        .headers_mut()
        .insert("x-api-key", created.secret.parse()?);
    request.headers_mut().insert(
        "x-varyai-ingress",
        axum::http::HeaderValue::from_static("direct"),
    );
    let response = tokio::time::timeout(
        DEADLINE,
        gateway::handle(State(fixture.state.clone()), request),
    )
    .await
    .map_err(|_| "m2_gateway_handler_timeout")?;
    let (status, wire, failed) = collect_wire(response).await?;
    assert!(
        !status.is_success() || failed,
        "m2_gateway_reused_direct_native_history"
    );
    assert_private_body_absent(&wire);
    assert!(
        fixture.requests()?.len() == 1,
        "m2_disabled_gateway_native_scope_reached_upstream"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_unsupported_native_provider_fails_before_any_upstream_request() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::Echo, "responses").await?;
    let baseline = fixture.budget.used();
    let (status, wire, failed) = collect_wire(
        fixture
            .send(chat_request(&current_input(false), Some(SESSION))?)
            .await?,
    )
    .await?;
    assert!(
        !status.is_success() || failed,
        "m2_unsupported_native_provider_admitted"
    );
    assert_private_body_absent(&wire);
    assert!(
        fixture.requests()?.is_empty(),
        "m2_unsupported_provider_received_request"
    );
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == baseline,
        "m2_unsupported_provider_retained_payload_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_upstream_http_error_never_exposes_private_body_or_credentials() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::HttpError, "chat").await?;
    let baseline = fixture.budget.used();
    let (status, wire, failed) = collect_wire(
        fixture
            .send(chat_request(&current_input(false), Some(SESSION))?)
            .await?,
    )
    .await?;
    assert!(
        !status.is_success() || failed,
        "m2_upstream_http_error_accepted"
    );
    assert_private_body_absent(&wire);
    let text = String::from_utf8_lossy(&wire);
    assert!(
        !text.contains("synthetic-m2-key"),
        "m2_upstream_error_credential_escaped"
    );
    let requests = fixture.requests()?;
    assert!(requests.len() == 1, "m2_http_error_unexpected_retry");
    assert!(
        !serde_json::to_string(&requests[0])?.contains(CONTACT),
        "m2_http_error_input_was_not_redacted"
    );
    let status = fixture.state.status.read().await;
    assert!(
        status.last_error.as_deref().is_none_or(|error| {
            !error.contains(CONTACT)
                && !error.contains("synthetic-m2-private-error")
                && !error.contains("synthetic-m2-key")
        }),
        "m2_upstream_error_status_exposed_private_content"
    );
    drop(status);
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == baseline,
        "m2_http_error_retained_payload_budget"
    );
    Ok(())
}

async fn assert_sse_rejected(mode: ReplyMode) -> TestResult {
    let fixture = HttpFixture::new(mode, "chat").await?;
    let baseline = fixture.budget.used();
    let (status, wire, failed) = collect_wire(
        fixture
            .send(chat_request(&current_input(true), Some(SESSION))?)
            .await?,
    )
    .await?;
    assert!(
        !status.is_success() || failed,
        "m2_invalid_sse_was_accepted"
    );
    assert_private_body_absent(&wire);
    assert!(fixture.requests()?.len() == 1, "m2_sse_attempt_missing");
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == baseline,
        "m2_invalid_sse_retained_payload_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_sse_invalid_framing_never_releases_privileged_tools_or_plaintext() -> TestResult {
    assert_sse_rejected(ReplyMode::InvalidFraming).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_sse_error_after_terminal_never_releases_privileged_tools_or_plaintext(
) -> TestResult {
    assert_sse_rejected(ReplyMode::ErrorAfterTerminal).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_sse_body_drop_before_upstream_eof_discards_pending_history_and_budget(
) -> TestResult {
    use futures::StreamExt;
    let fixture = HttpFixture::new(ReplyMode::PendingAfterTerminal, "chat").await?;
    let baseline = fixture.budget.used();
    let response = fixture
        .send(chat_request(&current_input(true), Some(SESSION))?)
        .await?;
    assert!(
        response.status() == StatusCode::OK,
        "m2_cancel_sse_setup_rejected"
    );
    let mut body = response.into_body().into_data_stream();
    {
        let next = body.next();
        tokio::pin!(next);
        tokio::select! {
            biased;
            _ = &mut next => return Err("m2_sse_released_before_original_eof".into()),
            notice = tokio::time::timeout(DEADLINE, fixture.script.terminal_sent.notified()) => {
                notice.map_err(|_| "m2_cancel_upstream_terminal_timeout")?;
            }
        }
        assert!(
            futures::poll!(next.as_mut()).is_pending(),
            "m2_sse_terminal_marker_substituted_for_original_eof"
        );
    }
    drop(body);
    tokio::task::yield_now().await;
    fixture.assert_no_publication()?;
    assert!(
        fixture.requests()?.len() == 1,
        "m2_cancel_dynamic_upstream_not_exercised"
    );
    assert!(
        fixture.budget.used() == baseline,
        "m2_cancelled_body_retained_payload_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_json_activates_before_body_return_and_drop_refunds_payload() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::Echo, "chat").await?;
    let baseline = fixture.budget.used();
    let response = fixture
        .send(chat_request(&current_input(false), Some(SESSION))?)
        .await?;
    assert!(
        response.status() == StatusCode::OK,
        "m2_json_activation_setup_failed"
    );
    let (published, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (published, pending, pins) == (1, 0, 0),
        "m2_json_not_active_before_body_return"
    );
    assert!(
        fixture.budget.used() > baseline,
        "m2_json_wire_has_no_budget_owner"
    );
    drop(response);
    assert!(
        fixture.budget.used() == baseline,
        "m2_json_drop_retained_payload_budget"
    );
    Ok(())
}

async fn sse_delivery_boundary(continue_poll: bool) -> TestResult {
    use futures::StreamExt;
    let fixture = HttpFixture::new(ReplyMode::EchoSse, "chat").await?;
    let baseline = fixture.budget.used();
    let response = fixture
        .send(chat_request(&current_input(true), Some(SESSION))?)
        .await?;
    assert!(
        response.status() == StatusCode::OK,
        "m2_sse_activation_setup_failed"
    );
    fixture.assert_no_publication()?;
    let mut body = response.into_body().into_data_stream();
    let mut wire = Vec::new();
    loop {
        let chunk = tokio::time::timeout(DEADLINE, body.next())
            .await?
            .ok_or("m2_sse_ended_before_terminal")??;
        wire.extend_from_slice(&chunk);
        if String::from_utf8_lossy(&wire).contains("[DONE]") {
            break;
        }
    }
    let (published, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (published, pending, pins) == (0, 1, 1),
        "m2_sse_activated_at_final_yield"
    );
    assert!(
        String::from_utf8_lossy(&wire).contains(CONTACT)
            && !String::from_utf8_lossy(&wire).contains("rredact-"),
        "m2_sse_final_body_not_restored"
    );
    if continue_poll {
        assert!(
            tokio::time::timeout(DEADLINE, body.next()).await?.is_none(),
            "m2_sse_continuation_did_not_end"
        );
        let (published, pending, pins, _) =
            publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (published, pending, pins) == (1, 0, 0),
            "m2_sse_continuation_not_active"
        );
    }
    drop(body);
    if !continue_poll {
        fixture.assert_no_publication()?;
    }
    assert!(
        fixture.budget.used() == baseline,
        "m2_sse_delivery_retained_payload_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_sse_last_yield_stays_pending_until_continuation_poll() -> TestResult {
    sse_delivery_boundary(true).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_sse_drop_after_last_yield_discards_pending_history_and_budget() -> TestResult {
    sse_delivery_boundary(false).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_sse_exact_assistant_and_call_evidence_imports_hidden_dependency() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::HistorySse, "chat").await?;
    let mut first = current_input(true);
    first["messages"]
        .as_array_mut()
        .ok_or("m2_sse_history_messages_missing")?
        .push(json!({"role":"developer","content":HIDDEN_CONTACT}));
    let (status, wire, failed) =
        collect_wire(fixture.send(chat_request(&first, Some(SESSION))?).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m2_sse_history_first_failed"
    );
    let text = std::str::from_utf8(&wire)?;
    let chunk = text
        .split("\n\n")
        .find_map(|frame| frame.strip_prefix("data: "))
        .ok_or("m2_sse_history_first_chunk_missing")?;
    let chunk: Value = serde_json::from_str(chunk)?;
    let delta = &chunk["choices"][0]["delta"];
    let mut call = delta["tool_calls"][0].clone();
    call.as_object_mut()
        .ok_or("m2_sse_history_call_invalid")?
        .shift_remove("index");
    let assistant = json!({"role":"assistant","content":delta["content"],"tool_calls":[call]});
    assert!(
        assistant["content"] == CONTACT && !text.contains(HIDDEN_CONTACT),
        "m2_sse_history_first_dependency_changed"
    );
    let replay = history_replay(&[assistant]);
    let (status, wire, failed) =
        collect_wire(fixture.send(chat_request(&replay, Some(SESSION))?).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m2_sse_history_second_failed"
    );
    let next: Value = serde_json::from_slice(&wire)?;
    assert!(
        next["choices"][0]["message"]["content"] == HIDDEN_CONTACT,
        "m2_sse_history_hidden_dependency_not_imported"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_unmodified_json_preserves_original_bytes_with_empty_lease() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::ExactJson, "chat").await?;
    let baseline = fixture.budget.used();
    let input = json!({"model":"synthetic-m2-model","messages":[{"role":"user","content":"Synthetic ordinary input"}]});
    let (status, wire, failed) =
        collect_wire(fixture.send(chat_request(&input, Some(SESSION))?).await?).await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m2_empty_lease_response_rejected"
    );
    assert!(
        wire == exact_json_wire().as_bytes(),
        "m2_unmodified_json_original_bytes_changed"
    );
    assert!(
        fixture.budget.used() == baseline,
        "m2_empty_lease_retained_payload_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_invalid_json_never_falls_back_and_refunds_payload_budget() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::InvalidJson, "chat").await?;
    let baseline = fixture.budget.used();
    let (status, wire, failed) = collect_wire(
        fixture
            .send(chat_request(&current_input(false), Some(SESSION))?)
            .await?,
    )
    .await?;
    assert!(!status.is_success() || failed, "m2_invalid_json_accepted");
    assert_private_body_absent(&wire);
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == baseline,
        "m2_invalid_json_retained_payload_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_feedback_gzip_json_uses_real_eof_then_decoded_native_proof() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::GzipJson, "chat").await?;
    let response = fixture
        .send(chat_request(&current_input(false), Some(SESSION))?)
        .await?;
    assert!(
        response.status() == StatusCode::OK,
        "m2_gzip_native_json_rejected"
    );
    assert!(
        !response.headers().contains_key("content-encoding"),
        "m2_gzip_encoding_header_stale"
    );
    let (_, wire, failed) = collect_wire(response).await?;
    assert!(!failed, "m2_gzip_native_body_failed");
    let delivered: Value = serde_json::from_slice(&wire).map_err(|_| "m2_gzip_json_invalid")?;
    assert!(
        delivered["choices"][0]["message"]["content"] == CONTACT,
        "m2_gzip_native_restoration_missing"
    );
    assert!(fixture.budget.used() == 0, "m2_gzip_payload_not_refunded");
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_feedback_rewritten_body_drops_stale_entity_validators() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::EntityValidators, "chat").await?;
    let response = fixture
        .send(chat_request(&current_input(false), Some(SESSION))?)
        .await?;
    assert!(
        response.status() == StatusCode::OK,
        "m2_validator_fixture_rejected"
    );
    for name in [
        "etag",
        "digest",
        "content-md5",
        "content-digest",
        "repr-digest",
    ] {
        assert!(
            !response.headers().contains_key(name),
            "m2_restored_body_retained_stale_entity_validator"
        );
    }
    let (_, wire, failed) = collect_wire(response).await?;
    assert!(
        !failed
            && serde_json::from_slice::<Value>(&wire)?["choices"][0]["message"]["content"]
                == CONTACT,
        "m2_validator_restoration_missing"
    );
    assert!(
        fixture.budget.used() == 0,
        "m2_validator_payload_not_refunded"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn m2_chat_feedback_empty_lease_outbound_wire_owns_independent_budget() -> TestResult {
    use super::http_restoration::{admit_http_request_with_scope, RestorationAdmission};
    use crate::{
        app_config::AppType,
        proxy::{
            handler_context::{
                RequestIngress, RestorationFraming, RestorationOperation, RestorationRouteScope,
            },
            protocol_bridge::Protocol,
        },
    };
    let fixture = ResponsesFixture::new();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let gateway = RestorationGateway::with_test_store_and_rules(
        fixture.store.clone(),
        &["chinese_phone_number"],
        budget.clone(),
    );
    let mut body = json!({"messages":[{"role":"user","content":"Synthetic ordinary text"}]});
    let admission = admit_http_request_with_scope(
        &gateway,
        &AppType::Codex,
        RestorationRouteScope::new(
            Protocol::Chat,
            RequestIngress::Direct,
            RestorationOperation::ChatCompletions,
            RestorationFraming::Json,
        ),
        &mut body,
        &axum::http::HeaderMap::new(),
    )?;
    let RestorationAdmission::Admitted(request) = admission else {
        return Err("m2_empty_wire_not_admitted".into());
    };
    let attempt = request
        .prepare_attempt(
            "synthetic-m2-wire",
            "openai_chat",
            "https://synthetic.invalid/v1/chat/completions",
            Some("synthetic-principal"),
            &mut body,
            |candidate| Ok(candidate.clone()),
        )
        .map_err(|_| "m2_empty_wire_prepare_failed")?;
    assert!(
        !attempt.has_restoration_lease(),
        "m2_empty_wire_fixture_created_mapping"
    );
    let before = budget.used();
    let wire = attempt
        .encode_outbound_body(&body)
        .map_err(|_| "m2_empty_wire_encode_failed")?
        .ok_or("m2_empty_native_wire_bypassed_budget")?;
    assert!(
        budget.used() >= before + wire.len(),
        "m2_empty_native_wire_not_independently_charged"
    );
    let cloned = wire.clone();
    drop(wire);
    assert!(
        budget.used() >= before + cloned.len(),
        "m2_empty_wire_clone_lost_charge"
    );
    drop(cloned);
    assert!(budget.used() == before, "m2_empty_wire_drop_did_not_refund");
    drop(attempt);
    drop(request);
    assert!(budget.used() == 0, "m2_empty_wire_request_owner_retained");
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_hidden_history_isolated_by_session_provider_target_and_principal() -> TestResult {
    for case in 0..4 {
        let fixture = HttpFixture::new(ReplyMode::History, "chat").await?;
        let assistants = first_history_round(&fixture).await?;
        let mut provider = fixture
            .state
            .db
            .get_provider_by_id("synthetic-m2-provider", "codex")?
            .ok_or("m2_isolation_provider_missing")?;
        match case {
            1 => provider.id = "synthetic-m2-other-provider".into(),
            2 => {
                let base = provider.settings_config["base_url"]
                    .as_str()
                    .ok_or("m2_isolation_base_missing")?
                    .to_string();
                provider.settings_config["base_url"] =
                    json!(format!("{base}/synthetic-other-target"));
            }
            3 => provider.settings_config["api_key"] = json!("synthetic-m2-rotated-key"),
            _ => {}
        }
        fixture.state.db.save_provider("codex", &provider)?;
        fixture
            .state
            .db
            .set_current_provider("codex", &provider.id)?;
        let session = if case == 0 {
            "synthetic-m2-other-session"
        } else {
            SESSION
        };
        let (status, wire, failed) = collect_wire(
            fixture
                .send(chat_request(&history_replay(&assistants), Some(session))?)
                .await?,
        )
        .await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_isolation_literal_delivery_failed"
        );
        let requests = fixture.requests()?;
        assert!(
            requests.len() == 2,
            "m2_isolation_actual_second_send_missing"
        );
        let hidden = requests[0]["messages"][1]["content"]
            .as_str()
            .ok_or("m2_isolation_hidden_reference_missing")?;
        let next: Value = serde_json::from_slice(&wire)?;
        assert!(
            next["choices"][0]["message"]["content"] == hidden
                && !String::from_utf8_lossy(&wire).contains(HIDDEN_CONTACT),
            "m2_isolation_imported_foreign_hidden_dependency"
        );
        let (_, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (pending, pins) == (0, 0),
            "m2_isolation_retained_pending_owner"
        );
        assert!(
            fixture.budget.used() == 0,
            "m2_isolation_payload_not_refunded"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_tool_result_alone_and_duplicate_identity_cannot_import_hidden_history(
) -> TestResult {
    for duplicate_identity in [false, true] {
        let fixture = HttpFixture::new(ReplyMode::History, "chat").await?;
        let assistants = first_history_round(&fixture).await?;
        let replay = if duplicate_identity {
            history_replay(&assistants)
        } else {
            json!({"model":"synthetic-m2-model","messages":[
                {"role":"user","content":"Synthetic continuation."},
                {"role":"tool","tool_call_id":"synthetic-m2-call-a",
                    "content":serde_json::to_string(&assistants)?}
            ]})
        };
        let mut request = chat_request(&replay, Some(SESSION))?;
        if duplicate_identity {
            request.headers_mut().append("session-id", SESSION.parse()?);
        }
        let (status, wire, failed) = collect_wire(fixture.send(request).await?).await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_untrusted_history_literal_failed"
        );
        let requests = fixture.requests()?;
        assert!(
            requests.len() == 2,
            "m2_untrusted_history_second_send_missing"
        );
        let hidden = requests[0]["messages"][1]["content"]
            .as_str()
            .ok_or("m2_untrusted_hidden_reference_missing")?;
        let next: Value = serde_json::from_slice(&wire)?;
        assert!(
            next["choices"][0]["message"]["content"] == hidden
                && !String::from_utf8_lossy(&wire).contains(HIDDEN_CONTACT),
            "m2_untrusted_history_granted_hidden_dependency"
        );
        let (_, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (pending, pins) == (0, 0),
            "m2_untrusted_history_retained_owner"
        );
        assert!(
            fixture.budget.used() == 0,
            "m2_untrusted_history_payload_not_refunded"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_unpolled_sse_drop_has_no_publication_and_refunds_payload() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::EchoSse, "chat").await?;
    let response = fixture
        .send(chat_request(&current_input(true), Some(SESSION))?)
        .await?;
    assert!(
        response.status() == StatusCode::OK,
        "m2_unpolled_sse_setup_failed"
    );
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() > 0,
        "m2_unpolled_sse_missing_payload_owner"
    );
    drop(response);
    tokio::task::yield_now().await;
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m2_unpolled_sse_drop_retained_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_sse_publication_failure_after_last_yield_discards_pending_owner() -> TestResult {
    use futures::StreamExt;
    let fixture = HttpFixture::new(ReplyMode::EchoSse, "chat").await?;
    let response = fixture
        .send(chat_request(&current_input(true), Some(SESSION))?)
        .await?;
    assert!(
        response.status() == StatusCode::OK,
        "m2_publication_failure_setup_failed"
    );
    let mut body = response.into_body().into_data_stream();
    let mut wire = Vec::new();
    loop {
        let chunk = tokio::time::timeout(DEADLINE, body.next())
            .await?
            .ok_or("m2_publication_missing_terminal")??;
        wire.extend_from_slice(&chunk);
        if String::from_utf8_lossy(&wire).contains("[DONE]") {
            break;
        }
    }
    let (published, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (published, pending, pins) == (0, 1, 1),
        "m2_publication_activated_before_continuation"
    );
    publication_test_support::unavailable(&fixture.runtime.store)?;
    assert!(
        tokio::time::timeout(DEADLINE, body.next())
            .await?
            .is_some_and(|item| item.is_err()),
        "m2_publication_failure_reported_clean_eof"
    );
    assert!(
        tokio::time::timeout(DEADLINE, body.next()).await?.is_none(),
        "m2_publication_failure_stream_continued"
    );
    drop(body);
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m2_publication_failure_retained_payload"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_real_router_four_aliases_restore_json_and_sse() -> TestResult {
    use crate::proxy::server::ProxyServer;
    for alias in [
        "/chat/completions",
        "/v1/chat/completions",
        "/v1/v1/chat/completions",
        "/codex/v1/chat/completions",
    ] {
        for stream in [false, true] {
            let fixture = HttpFixture::new(
                if stream {
                    ReplyMode::EchoSse
                } else {
                    ReplyMode::Echo
                },
                "chat",
            )
            .await?;
            let router = ProxyServer::new(
                ProxyConfig {
                    listen_port: 0,
                    ..Default::default()
                },
                fixture.state.db.clone(),
                None,
            )
            .with_restoration_gateway(fixture.state.restoration_gateway.clone())
            .build_router();
            // Exercise the production router over an ephemeral listener, never launch the app.
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let _task = UpstreamTask(tokio::spawn(
                async move { axum::serve(listener, router).await },
            ));
            let response = reqwest::Client::new()
                .post(format!("http://{address}{alias}?synthetic=m2"))
                .header("session-id", SESSION)
                .json(&current_input(stream))
                .send()
                .await?;
            assert!(
                response.status() == StatusCode::OK,
                "m2_router_alias_rejected"
            );
            let wire = tokio::time::timeout(DEADLINE, response.bytes()).await??;
            let text = std::str::from_utf8(&wire)?;
            assert!(
                text.contains(CONTACT) && !text.contains("rredact-"),
                "m2_router_alias_not_restored"
            );
            assert!(
                fixture.requests()?.len() == 1,
                "m2_router_alias_send_missing"
            );
            let uris = fixture
                .script
                .uris
                .lock()
                .map_err(|_| "m2_uri_capture_poisoned")?;
            assert!(
                uris.as_slice() == ["/v1/chat/completions?synthetic=m2"],
                "m2_router_alias_query_not_preserved"
            );
            assert!(
                fixture.budget.used() == 0,
                "m2_router_alias_payload_not_refunded"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_http_each_payload_buffer_capacity_failure_is_atomic_and_refunded() -> TestResult {
    use super::payload_buffer::test_support::{self, Action, Phase};
    use std::{cell::Cell, rc::Rc};
    for stream in [false, true] {
        let mode = if stream {
            ReplyMode::EchoSse
        } else {
            ReplyMode::History
        };
        let count = Rc::new(Cell::new(0usize));
        let observed = count.clone();
        let fixture = HttpFixture::new(mode, "chat").await?;
        let hook = test_support::install(move |event| {
            if event.phase == Phase::CapacityObserved {
                observed.set(observed.get() + 1);
            }
            Action::Continue
        });
        let (status, _, failed) = collect_wire(
            fixture
                .send(chat_request(&current_input(stream), Some(SESSION))?)
                .await?,
        )
        .await?;
        drop(hook);
        assert!(
            status == StatusCode::OK && !failed,
            "m2_capacity_sweep_baseline_failed"
        );
        let allocations = count.get();
        assert!(allocations > 0, "m2_capacity_sweep_zero_allocations");
        drop(fixture);
        for fail_at in 1..=allocations {
            let fixture = HttpFixture::new(mode, "chat").await?;
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
                    .send(chat_request(&current_input(stream), Some(SESSION))?)
                    .await?,
            )
            .await?;
            drop(hook);
            assert!(fired.get(), "m2_capacity_sweep_injection_not_reached");
            assert!(
                !status.is_success() || failed,
                "m2_capacity_sweep_failure_ignored"
            );
            assert_private_body_absent(&wire);
            fixture.assert_no_publication()?;
            assert!(
                fixture.budget.used() == 0,
                "m2_capacity_sweep_retained_payload_budget"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_admission_budget_exhaustion_never_sends_or_publishes() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::Echo, "chat").await?;
    let saturation = fixture.budget.reserve(fixture.budget.limit())?;
    let (status, wire, failed) = collect_wire(
        fixture
            .send(chat_request(&current_input(false), Some(SESSION))?)
            .await?,
    )
    .await?;
    assert!(
        !status.is_success() || failed,
        "m2_exhausted_admission_was_accepted"
    );
    assert_private_body_absent(&wire);
    assert!(
        fixture.requests()?.is_empty(),
        "m2_exhausted_admission_reached_upstream"
    );
    fixture.assert_no_publication()?;
    drop(saturation);
    assert!(
        fixture.budget.used() == 0,
        "m2_exhausted_admission_retained_payload"
    );
    Ok(())
}

async fn cancel_json_before_completion(mode: ReplyMode) -> TestResult {
    let fixture = HttpFixture::new(mode, "chat").await?;
    let baseline = fixture.budget.used();
    let (_capture_guard, capture) = super::http_restoration::capture_test_support::install();
    {
        let send = fixture.send(chat_request(&current_input(false), Some(SESSION))?);
        tokio::pin!(send);
        tokio::select! {
            biased;
            _ = &mut send => return Err("m2_json_completed_before_real_upstream_eof".into()),
            notice = tokio::time::timeout(DEADLINE, async {
                if matches!(mode, ReplyMode::GzipPending) {
                    capture.after_bytes(&fixture.script.compressed_len).await;
                } else {
                    fixture.script.terminal_sent.notified().await;
                }
            }) => {
                notice.map_err(|_| "m2_json_cancel_upstream_notice_timeout")?;
            }
        }
        assert!(
            futures::poll!(send.as_mut()).is_pending(),
            "m2_json_pending_transport_gained_completion"
        );
        fixture.assert_no_publication()?;
        assert!(
            fixture.budget.used() > baseline,
            "m2_json_pending_request_has_no_budget_owner"
        );
    }
    tokio::task::yield_now().await;
    assert!(
        fixture.requests()?.len() == 1,
        "m2_json_cancel_upstream_not_exercised"
    );
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == baseline,
        "m2_json_cancel_retained_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_gzip_complete_bytes_without_real_eof_remain_pending_and_cancel_refunds(
) -> TestResult {
    cancel_json_before_completion(ReplyMode::GzipPending).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_cancel_while_waiting_for_upstream_headers_refunds_without_publication(
) -> TestResult {
    cancel_json_before_completion(ReplyMode::PendingJsonHeaders).await
}

async fn assert_compressed_json_rejected(mode: ReplyMode) -> TestResult {
    let fixture = HttpFixture::new(mode, "chat").await?;
    let (status, wire, failed) = collect_wire(
        fixture
            .send(chat_request(&current_input(false), Some(SESSION))?)
            .await?,
    )
    .await?;
    assert!(
        !status.is_success() || failed,
        "m2_compressed_invalid_entity_accepted"
    );
    assert_private_body_absent(&wire);
    assert!(
        fixture.requests()?.len() == 1,
        "m2_compressed_rejection_missing_attempt"
    );
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m2_compressed_failure_retained_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_gzip_read_error_after_complete_bytes_never_proves_eof() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::GzipReadError, "chat").await?;
    let (_capture_guard, capture) = super::http_restoration::capture_test_support::install();
    let send = fixture.send(chat_request(&current_input(false), Some(SESSION))?);
    tokio::pin!(send);
    tokio::select! {
        biased;
        _ = &mut send => return Err("m2_gzip_read_error_completed_before_release".into()),
        notice = tokio::time::timeout(DEADLINE, capture.after_bytes(&fixture.script.compressed_len)) => {
            notice.map_err(|_| "m2_gzip_complete_bytes_not_captured_pending")?;
        }
    }
    assert!(
        futures::poll!(send.as_mut()).is_pending(),
        "m2_gzip_complete_bytes_forged_eof"
    );
    fixture.assert_no_publication()?;
    fixture.script.release_error.notify_one();
    let (status, wire, failed) = collect_wire(send.await?).await?;
    assert!(
        !status.is_success() || failed,
        "m2_gzip_read_error_accepted"
    );
    assert_private_body_absent(&wire);
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m2_gzip_read_error_retained_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_gzip_corrupt_trailer_fails_closed_and_refunds() -> TestResult {
    assert_compressed_json_rejected(ReplyMode::GzipCorrupt).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_gzip_entity_above_one_mib_fails_closed_and_refunds() -> TestResult {
    assert_compressed_json_rejected(ReplyMode::GzipOversized).await
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_layered_content_codings_restore_only_after_real_eof() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::GzipLayered, "chat").await?;
    let response = fixture
        .send(chat_request(&current_input(false), Some(SESSION))?)
        .await?;
    assert!(
        response.status() == StatusCode::OK,
        "m2_layered_native_json_rejected"
    );
    assert!(
        !response.headers().contains_key("content-encoding"),
        "m2_layered_encoding_header_stale"
    );
    let (_, wire, failed) = collect_wire(response).await?;
    assert!(!failed, "m2_layered_native_body_failed");
    let delivered: Value = serde_json::from_slice(&wire)?;
    assert!(
        delivered == tool_completion(CONTACT),
        "m2_layered_native_types_or_tools_changed"
    );
    let (published, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
    assert!(
        (published, pending, pins) == (1, 0, 0),
        "m2_layered_delivery_not_active"
    );
    assert!(
        fixture.budget.used() == 0,
        "m2_layered_payload_not_refunded"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_gzip_empty_lease_preserves_decoded_original_json_bytes() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::GzipExactJson, "chat").await?;
    let body = json!({"model":"synthetic-m2-model", "messages":[{"role":"user","content":"Synthetic ordinary text"}]});
    let response = fixture.send(chat_request(&body, Some(SESSION))?).await?;
    assert!(
        response.status() == StatusCode::OK,
        "m2_exact_compressed_json_rejected"
    );
    assert!(
        !response.headers().contains_key("content-encoding"),
        "m2_exact_compressed_encoding_stale"
    );
    let (_, wire, failed) = collect_wire(response).await?;
    assert!(
        !failed && wire == exact_json_wire().as_bytes(),
        "m2_empty_lease_decoded_original_bytes_changed"
    );
    assert!(
        fixture.runtime.store.stats()?.entries == 0,
        "m2_empty_lease_created_mapping"
    );
    assert!(
        fixture.budget.used() == 0,
        "m2_exact_compressed_payload_not_refunded"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_decoded_entity_at_cap_still_obeys_frozen_parser_peak_limit() -> TestResult {
    // The entity ceiling is not a promise that every JSON shape fits parser scratch.
    assert_compressed_json_rejected(ReplyMode::GzipAtLimit).await
}

#[test]
#[serial_test::serial]
fn m2_chat_decoder_exact_cap_and_overrun_are_distinct_and_refunded() -> TestResult {
    use super::{payload_buffer::PayloadBuffer, upstream_guard::NativeJsonDecoder};
    use std::io::Write;
    const LIMIT: usize = 1024 * 1024;
    for overrun in [false, true] {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let mut source = exact_json_wire().as_bytes().to_vec();
        source.resize(LIMIT + usize::from(overrun), b' ');
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&source)?;
        let wire = encoder.finish()?;
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "content-encoding",
            axum::http::HeaderValue::from_static("gzip"),
        );
        let captured = PayloadBuffer::try_from_slice(&wire, budget.clone(), LIMIT)?;
        let decoded = NativeJsonDecoder::new(&headers)
            .map_err(|_| "m2_decoder_headers_invalid")?
            .decode(captured, &budget, LIMIT);
        if overrun {
            assert!(
                matches!(
                    decoded,
                    Err(super::http_restoration::DlpForwardError::CapacityExceeded)
                ),
                "m2_decoder_accepted_cap_overrun"
            );
        } else {
            let decoded = decoded.map_err(|_| "m2_decoder_rejected_exact_cap")?;
            assert!(
                decoded.as_slice() == source && budget.used() >= LIMIT,
                "m2_decoder_exact_cap_bytes_or_charge_changed"
            );
            drop(decoded);
        }
        assert!(budget.used() == 0, "m2_decoder_cap_result_retained_budget");
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_layered_decode_each_capacity_failure_is_atomic_and_refunded() -> TestResult {
    use super::payload_buffer::test_support::{self, Action, Phase};
    use std::{cell::Cell, rc::Rc};
    let count = Rc::new(Cell::new(0usize));
    let observed = count.clone();
    let fixture = HttpFixture::new(ReplyMode::GzipLayered, "chat").await?;
    let hook = test_support::install(move |event| {
        if event.phase == Phase::CapacityObserved {
            observed.set(observed.get() + 1);
        }
        Action::Continue
    });
    let (status, _, failed) = collect_wire(
        fixture
            .send(chat_request(&current_input(false), Some(SESSION))?)
            .await?,
    )
    .await?;
    drop(hook);
    assert!(
        status == StatusCode::OK && !failed,
        "m2_layered_sweep_baseline_failed"
    );
    let allocations = count.get();
    assert!(allocations > 0, "m2_layered_sweep_zero_allocations");
    drop(fixture);
    for fail_at in 1..=allocations {
        let fixture = HttpFixture::new(ReplyMode::GzipLayered, "chat").await?;
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
                .send(chat_request(&current_input(false), Some(SESSION))?)
                .await?,
        )
        .await?;
        drop(hook);
        assert!(fired.get(), "m2_layered_sweep_injection_not_reached");
        assert!(
            !status.is_success() || failed,
            "m2_layered_capacity_failure_ignored"
        );
        assert_private_body_absent(&wire);
        fixture.assert_no_publication()?;
        assert!(
            fixture.budget.used() == 0,
            "m2_layered_capacity_failure_retained_budget"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_redirect_never_reaches_unbound_sink_even_with_empty_lease() -> TestResult {
    for mapped in [false, true] {
        let fixture = HttpFixture::new(ReplyMode::Redirect, "chat").await?;
        let mut body = current_input(false);
        if !mapped {
            body["messages"][0]["content"] = json!("Synthetic ordinary input");
        }
        let (status, wire, failed) =
            collect_wire(fixture.send(chat_request(&body, Some(SESSION))?).await?).await?;
        assert!(!status.is_success() || failed, "m2_bound_redirect_accepted");
        assert_private_body_absent(&wire);
        assert!(
            fixture.requests()?.len() == 1,
            "m2_redirect_initial_attempt_missing"
        );
        assert!(
            fixture
                .script
                .redirect_hits
                .load(std::sync::atomic::Ordering::SeqCst)
                == 0,
            "m2_redirect_sink_received_request"
        );
        fixture.assert_no_publication()?;
        assert!(fixture.budget.used() == 0, "m2_redirect_retained_budget");
    }
    Ok(())
}

// Read one complete synthetic HTTP request, then close without response headers.
// This exercises a real retryable transport failure, not a DLP failure reclassified for a test.
async fn install_failed_first_provider(
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
                .ok_or_else(|| std::io::Error::other("Synthetic request length missing."))?;
            if wire.len() < end + 4 + length {
                continue;
            }
            let body: Value = serde_json::from_slice(&wire[end + 4..end + 4 + length])
                .map_err(|_| std::io::Error::other("Synthetic request JSON invalid."))?;
            observed
                .0
                .lock()
                .map_err(|_| std::io::Error::other("Synthetic capture unavailable."))?
                .push(body);
            return Ok(());
        }
    }));
    let first = Provider::with_id(
        "synthetic-m2-failed-provider".into(),
        "Synthetic failed transport".into(),
        json!({"base_url":format!("http://{address}"),"api_key":"synthetic-m2-first-key","api_format":"chat"}),
        None,
    );
    fixture.state.db.save_provider("codex", &first)?;
    fixture.state.db.add_to_failover_queue("codex", &first.id)?;
    fixture
        .state
        .db
        .add_to_failover_queue("codex", "synthetic-m2-provider")?;
    let mut config = fixture.state.db.get_proxy_config_for_app("codex").await?;
    config.auto_failover_enabled = true;
    config.max_retries = 1;
    fixture.state.db.update_proxy_config_for_app(config).await?;
    Ok((task, capture))
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_transport_retry_uses_actual_second_native_provider_for_json_and_sse() -> TestResult
{
    for stream in [false, true] {
        let fixture = HttpFixture::new(
            if stream {
                ReplyMode::EchoSse
            } else {
                ReplyMode::Echo
            },
            "chat",
        )
        .await?;
        let (task, first) = install_failed_first_provider(&fixture).await?;
        let (status, wire, failed) = collect_wire(
            fixture
                .send(chat_request(&current_input(stream), Some(SESSION))?)
                .await?,
        )
        .await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_native_retry_second_provider_rejected"
        );
        let text = std::str::from_utf8(&wire)?;
        assert!(
            text.contains(CONTACT) && !text.contains("rredact-"),
            "m2_native_retry_not_restored"
        );
        assert!(
            task.0.is_finished(),
            "m2_failed_transport_task_not_terminal"
        );
        assert!(
            fixture._transport.transport_counts() == (2, 0),
            "m2_retry_did_not_send_two_real_native_attempts"
        );
        let first = first.0.lock().map_err(|_| "m2_failed_capture_poisoned")?;
        let second = fixture.requests()?;
        assert!(
            first.len() == 1 && second.len() == 1,
            "m2_native_retry_attempt_count_wrong"
        );
        for request in first.iter().chain(second.iter()) {
            let text = request["messages"][0]["content"]
                .as_str()
                .ok_or("m2_retry_text_missing")?;
            assert!(
                !text.contains(CONTACT) && text.contains("rredact-cn_phone-"),
                "m2_retry_forwarded_private_input"
            );
        }
        let (published, pending, pins, _) =
            publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (published, pending, pins) == (1, 0, 0),
            "m2_retry_published_failed_attempt_or_not_delivered"
        );
        assert!(
            fixture.budget.used() == 0,
            "m2_native_retry_retained_budget"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_retry_to_unsupported_protocol_stops_before_second_send() -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::Echo, "responses").await?;
    let (_task, first) = install_failed_first_provider(&fixture).await?;
    let (status, wire, failed) = collect_wire(
        fixture
            .send(chat_request(&current_input(false), Some(SESSION))?)
            .await?,
    )
    .await?;
    assert!(
        !status.is_success() || failed,
        "m2_retry_unsupported_protocol_accepted"
    );
    assert_private_body_absent(&wire);
    assert!(
        first
            .0
            .lock()
            .map_err(|_| "m2_failed_capture_poisoned")?
            .len()
            == 1,
        "m2_retry_first_transport_not_exercised"
    );
    assert!(
        fixture.requests()?.is_empty(),
        "m2_retry_unsupported_protocol_sent_request"
    );
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m2_unsupported_retry_retained_budget"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_final_target_or_principal_mutation_rejects_before_transport() -> TestResult {
    use crate::proxy::forwarder::final_send_binding_test_support;
    for principal in [false, true] {
        let fixture = HttpFixture::new(ReplyMode::Echo, "chat").await?;
        let hook = final_send_binding_test_support::install(move |url, headers| {
            if principal {
                headers.insert(
                    "authorization",
                    axum::http::HeaderValue::from_static("Bearer synthetic-m2-mutated"),
                );
            } else {
                url.push_str("?synthetic=unbound");
            }
        });
        let (status, wire, failed) = collect_wire(
            fixture
                .send(chat_request(&current_input(false), Some(SESSION))?)
                .await?,
        )
        .await?;
        assert!(!status.is_success() || failed, "m2_final_mutation_accepted");
        assert_private_body_absent(&wire);
        assert!(
            hook.transport_counts() == (0, 0) && fixture.requests()?.is_empty(),
            "m2_final_mutation_reached_transport"
        );
        fixture.assert_no_publication()?;
        assert!(
            fixture.budget.used() == 0,
            "m2_final_mutation_retained_budget"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_four_round_json_sse_history_preserves_transitive_hidden_dependencies() -> TestResult
{
    const HIDDEN: [&str; 3] = ["13800000000", "13700000000", "13600000000"];
    let fixture = HttpFixture::new(ReplyMode::MixedHistory, "chat").await?;
    let mut assistants = Vec::new();
    for round in 0..4 {
        let stream = round % 2 == 1;
        let mut body = if round == 0 {
            let mut body = current_input(stream);
            for hidden in HIDDEN {
                body["messages"]
                    .as_array_mut()
                    .ok_or("m2_mixed_messages_missing")?
                    .push(json!({"role":"developer","content":hidden}));
            }
            body
        } else {
            history_replay(&assistants)
        };
        body["stream"] = json!(stream);
        let (status, wire, failed) =
            collect_wire(fixture.send(chat_request(&body, Some(SESSION))?).await?).await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_mixed_history_round_rejected"
        );
        let text = std::str::from_utf8(&wire)?;
        assert!(
            !text.contains("rredact-"),
            "m2_mixed_history_reference_escaped"
        );
        assistants = if stream {
            let frame = text
                .split("\n\n")
                .find_map(|frame| frame.strip_prefix("data: "))
                .ok_or("m2_mixed_sse_frame_missing")?;
            let frame: Value = serde_json::from_str(frame)?;
            let delta = &frame["choices"][0]["delta"];
            let mut call = delta["tool_calls"][0].clone();
            call.as_object_mut()
                .ok_or("m2_mixed_sse_call_missing")?
                .shift_remove("index");
            vec![json!({"role":"assistant","content":delta["content"],"tool_calls":[call]})]
        } else {
            let response: Value = serde_json::from_slice(&wire)?;
            response["choices"]
                .as_array()
                .ok_or("m2_mixed_choices_missing")?
                .iter()
                .map(|choice| choice["message"].clone())
                .collect()
        };
        let expected = if round == 0 {
            CONTACT
        } else {
            HIDDEN[round - 1]
        };
        assert!(
            assistants[0]["content"] == expected,
            "m2_mixed_transitive_dependency_not_restored"
        );
        for hidden in HIDDEN.iter().skip(round) {
            assert!(
                !text.contains(hidden),
                "m2_mixed_future_dependency_released"
            );
        }
        let requests = fixture.requests()?;
        assert!(requests.len() == round + 1, "m2_mixed_attempt_count_wrong");
        let outbound = serde_json::to_string(&requests[round])?;
        assert!(
            !outbound.contains(CONTACT) && HIDDEN.iter().all(|hidden| !outbound.contains(hidden)),
            "m2_mixed_private_input_forwarded"
        );
        let (published, pending, pins, _) =
            publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (published, pending, pins) == (round + 1, 0, 0),
            "m2_mixed_publication_owner_wrong"
        );
        assert!(fixture.budget.used() == 0, "m2_mixed_round_retained_budget");
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_feedback_appended_json_sse_transcript_preserves_complete_batches() -> TestResult {
    const HIDDEN: [&str; 3] = ["13800000000", "13700000000", "13600000000"];
    let fixture = HttpFixture::new(ReplyMode::MixedHistory, "chat").await?;
    let mut assistants = Vec::new();
    for round in 0..4 {
        let stream = round % 2 == 1;
        let mut body = if round == 0 {
            let mut body = current_input(stream);
            for hidden in HIDDEN {
                body["messages"]
                    .as_array_mut()
                    .ok_or("m2_mixed_messages_missing")?
                    .push(json!({"role":"developer","content":hidden}));
            }
            body
        } else {
            history_replay(&assistants)
        };
        body["stream"] = json!(stream);
        let (status, wire, failed) =
            collect_wire(fixture.send(chat_request(&body, Some(SESSION))?).await?).await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_mixed_history_round_rejected"
        );
        let text = std::str::from_utf8(&wire)?;
        assert!(
            !text.contains("rredact-"),
            "m2_mixed_history_reference_escaped"
        );
        let delivered = if stream {
            let frame = text
                .split("\n\n")
                .find_map(|frame| frame.strip_prefix("data: "))
                .ok_or("m2_mixed_sse_frame_missing")?;
            let frame: Value = serde_json::from_str(frame)?;
            let delta = &frame["choices"][0]["delta"];
            let mut call = delta["tool_calls"][0].clone();
            call.as_object_mut()
                .ok_or("m2_mixed_sse_call_missing")?
                .shift_remove("index");
            vec![json!({"role":"assistant","content":delta["content"],"tool_calls":[call]})]
        } else {
            let response: Value = serde_json::from_slice(&wire)?;
            response["choices"]
                .as_array()
                .ok_or("m2_mixed_choices_missing")?
                .iter()
                .map(|choice| choice["message"].clone())
                .collect()
        };
        let expected = if round == 0 {
            CONTACT
        } else {
            HIDDEN[round - 1]
        };
        assert!(
            delivered[0]["content"] == expected,
            "m2_mixed_transitive_dependency_not_restored"
        );
        for hidden in HIDDEN.iter().skip(round) {
            assert!(
                !text.contains(hidden),
                "m2_mixed_future_dependency_released"
            );
        }
        assistants.extend(delivered);
        let requests = fixture.requests()?;
        assert!(requests.len() == round + 1, "m2_mixed_attempt_count_wrong");
        let outbound = serde_json::to_string(&requests[round])?;
        assert!(
            !outbound.contains(CONTACT) && HIDDEN.iter().all(|hidden| !outbound.contains(hidden)),
            "m2_mixed_private_input_forwarded"
        );
        let (published, pending, pins, _) =
            publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (published, pending, pins) == (round + 1, 0, 0),
            "m2_mixed_publication_owner_wrong"
        );
        assert!(fixture.budget.used() == 0, "m2_mixed_round_retained_budget");
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_feedback_native_sse_errors_preserve_safe_semantics_without_completion(
) -> TestResult {
    for mode in [
        ReplyMode::NativeSseError,
        ReplyMode::NativeSsePrivateError,
        ReplyMode::NativeSseToolError,
        ReplyMode::NativeSseDataError,
    ] {
        let fixture = HttpFixture::new(mode, "chat").await?;
        let (status, wire, failed) = collect_wire(
            fixture
                .send(chat_request(&current_input(true), Some(SESSION))?)
                .await?,
        )
        .await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_native_sse_error_was_transport_failure"
        );
        let text = std::str::from_utf8(&wire)?;
        assert!(
            text.contains("synthetic_quota")
                && text.contains("rate_limit_error")
                && text.contains("Synthetic upstream quota exhausted."),
            "m2_native_sse_error_semantics_lost"
        );
        assert!(
            !text.contains(CONTACT)
                && !text.contains("synthetic-m2-key")
                && !text.contains("rredact-")
                && !text.contains("[DONE]")
                && !text.contains("tool_calls")
                && !text.contains(TOOL_NAME)
                && !text.contains("proxy_processing_failed"),
            "m2_native_sse_error_leaked_or_fabricated_success"
        );
        assert!(
            text.matches("data:").count() == 1,
            "m2_native_sse_error_not_atomic"
        );
        fixture.assert_no_publication()?;
        assert!(
            fixture.budget.used() == 0,
            "m2_native_sse_error_retained_budget"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_feedback_appended_history_invalid_suffix_never_imports_valid_prefix() -> TestResult
{
    for case in 0..4 {
        let fixture = HttpFixture::new(ReplyMode::MixedHistory, "chat").await?;
        let mut body = current_input(false);
        for hidden in [HIDDEN_CONTACT, "13700000000", "13600000000"] {
            body["messages"]
                .as_array_mut()
                .ok_or("m2_appended_messages_missing")?
                .push(json!({"role":"developer","content":hidden}));
        }
        let (status, wire, failed) =
            collect_wire(fixture.send(chat_request(&body, Some(SESSION))?).await?).await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_appended_first_round_failed"
        );
        let first: Value = serde_json::from_slice(&wire)?;
        let mut assistants: Vec<Value> = first["choices"]
            .as_array()
            .ok_or("m2_appended_choices_missing")?
            .iter()
            .map(|choice| choice["message"].clone())
            .collect();
        let mut body = history_replay(&assistants);
        body["stream"] = json!(true);
        let (status, wire, failed) =
            collect_wire(fixture.send(chat_request(&body, Some(SESSION))?).await?).await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_appended_second_round_failed"
        );
        let text = std::str::from_utf8(&wire)?;
        let data = text
            .split("\n\n")
            .find_map(|frame| frame.strip_prefix("data: "))
            .ok_or("m2_appended_frame_missing")?;
        let frame: Value = serde_json::from_str(data)?;
        let delta = &frame["choices"][0]["delta"];
        let mut call = delta["tool_calls"][0].clone();
        call.as_object_mut()
            .ok_or("m2_appended_call_missing")?
            .shift_remove("index");
        let second = json!({"role":"assistant","content":delta["content"],"tool_calls":[call]});
        assert!(
            second["content"] == HIDDEN_CONTACT,
            "m2_appended_second_dependency_missing"
        );
        assistants.push(second);
        match case {
            0 => assistants[2]["tool_calls"][0]["function"]["arguments"] = json!("{}"),
            1 => assistants.reverse(),
            2 => {
                assistants.remove(1);
            }
            _ => {
                assistants.push(json!({"role":"assistant","content":"Synthetic forged suffix."}));
            }
        }
        let requests = fixture.requests()?;
        let hidden = requests[0]["messages"][2]["content"]
            .as_str()
            .ok_or("m2_appended_hidden_reference_missing")?
            .to_string();
        let (status, wire, failed) = collect_wire(
            fixture
                .send(chat_request(&history_replay(&assistants), Some(SESSION))?)
                .await?,
        )
        .await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_appended_unknown_literal_rejected"
        );
        let delivered: Value = serde_json::from_slice(&wire)?;
        assert!(
            delivered["choices"][0]["message"]["content"] == hidden
                && !String::from_utf8_lossy(&wire).contains("13700000000"),
            "m2_appended_invalid_suffix_imported_prefix"
        );
        let (published, pending, pins, _) =
            publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (published, pending, pins) == (3, 0, 0),
            "m2_appended_invalid_suffix_owner_wrong"
        );
        assert!(
            fixture.budget.used() == 0,
            "m2_appended_invalid_suffix_retained_budget"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_appended_transcript_invalid_suffix_never_keeps_prefix_authority() -> TestResult {
    const HIDDEN: [&str; 3] = ["13800000000", "13700000000", "13600000000"];
    for case in 0..5 {
        let fixture = HttpFixture::new(ReplyMode::MixedHistory, "chat").await?;
        let mut body = current_input(false);
        for hidden in HIDDEN {
            body["messages"]
                .as_array_mut()
                .ok_or("m2_append_messages_missing")?
                .push(json!({"role":"developer","content":hidden}));
        }
        let (status, wire, failed) =
            collect_wire(fixture.send(chat_request(&body, Some(SESSION))?).await?).await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_append_first_round_failed"
        );
        let delivered: Value = serde_json::from_slice(&wire)?;
        let first: Vec<Value> = delivered["choices"]
            .as_array()
            .ok_or("m2_append_choices_missing")?
            .iter()
            .map(|choice| choice["message"].clone())
            .collect();
        let mut body = history_replay(&first);
        body["stream"] = json!(true);
        let (status, wire, failed) =
            collect_wire(fixture.send(chat_request(&body, Some(SESSION))?).await?).await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_append_second_round_failed"
        );
        let text = std::str::from_utf8(&wire)?;
        let frame = text
            .split("\n\n")
            .find_map(|frame| frame.strip_prefix("data: "))
            .ok_or("m2_append_sse_frame_missing")?;
        let chunk: Value = serde_json::from_str(frame)?;
        assert!(
            chunk["choices"][0]["delta"]["content"] == HIDDEN[0],
            "m2_append_second_round_not_restored"
        );
        let delta = &chunk["choices"][0]["delta"];
        let mut call = delta["tool_calls"][0].clone();
        call.as_object_mut()
            .ok_or("m2_append_call_missing")?
            .shift_remove("index");
        let second = json!({"role":"assistant","content":delta["content"],"tool_calls":[call]});
        let mut transcript = first.clone();
        transcript.push(second.clone());
        match case {
            0 => transcript[2]["tool_calls"][0]["function"]["arguments"] = json!("{}"),
            1 => {
                transcript.clear();
                transcript.push(second);
                transcript.extend(first);
            }
            2 => {
                transcript.extend(first);
            }
            3 => {
                transcript.remove(1);
            }
            _ => {
                transcript[2]
                    .as_object_mut()
                    .ok_or("m2_append_assistant_missing")?
                    .shift_remove("tool_calls");
            }
        }
        let (status, wire, failed) = collect_wire(
            fixture
                .send(chat_request(&history_replay(&transcript), Some(SESSION))?)
                .await?,
        )
        .await?;
        assert!(
            status == StatusCode::OK && !failed,
            "m2_append_invalid_history_independent_request_failed"
        );
        let requests = fixture.requests()?;
        let reference = requests[0]["messages"][2]["content"]
            .as_str()
            .ok_or("m2_append_hidden_reference_missing")?;
        let delivered: Value = serde_json::from_slice(&wire)?;
        assert!(
            delivered["choices"][0]["message"]["content"] == reference
                && !String::from_utf8_lossy(&wire).contains(HIDDEN[1]),
            "m2_append_invalid_suffix_kept_partial_authority"
        );
        assert!(
            requests.len() == 3
                && requests.iter().all(|request| {
                    let wire = request.to_string();
                    !wire.contains(CONTACT) && HIDDEN.iter().all(|hidden| !wire.contains(hidden))
                }),
            "m2_append_invalid_history_leaked_upstream"
        );
        let (_, pending, pins, _) = publication_test_support::counts(&fixture.runtime.store)?;
        assert!(
            (pending, pins) == (0, 0) && fixture.budget.used() == 0,
            "m2_append_invalid_history_retained_owner"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m2_chat_sol_multichoice_upstream_error_preserves_semantics_without_completion(
) -> TestResult {
    let fixture = HttpFixture::new(ReplyMode::NativeSseMultiChoiceError, "chat").await?;
    let (status, wire, failed) = collect_wire(
        fixture
            .send(chat_request(&current_input(true), Some(SESSION))?)
            .await?,
    )
    .await?;
    assert!(
        status == StatusCode::OK && !failed,
        "m2_multichoice_native_error_was_transport_failure"
    );
    let text = std::str::from_utf8(&wire)?;
    assert!(
        text.contains("synthetic_quota")
            && text.contains("rate_limit_error")
            && text.contains("Synthetic upstream quota exhausted."),
        "m2_multichoice_native_error_semantics_lost"
    );
    assert!(
        !text.contains(CONTACT)
            && !text.contains("synthetic-m2-key")
            && !text.contains("rredact-")
            && !text.contains("[DONE]")
            && !text.contains("finish_reason")
            && !text.contains("tool_calls")
            && !text.contains(TOOL_NAME)
            && !text.contains("proxy_processing_failed"),
        "m2_multichoice_native_error_leaked_or_fabricated_success"
    );
    assert!(
        text.matches("data:").count() == 1,
        "m2_multichoice_native_error_not_atomic"
    );
    fixture.assert_no_publication()?;
    assert!(
        fixture.budget.used() == 0,
        "m2_multichoice_native_error_retained_budget"
    );
    Ok(())
}
