//! Direct Gemini HTTP integration with synthetic, dynamically bound upstreams.
use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use super::{
    http_restoration::RestorationGateway,
    payload_budget::PayloadBudget,
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

const JSON_ROUTE: &str = "/v1beta/models/synthetic-m4-model:generateContent";
const SSE_ROUTE: &str = "/v1beta/models/synthetic-m4-model:streamGenerateContent?alt=sse";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Value>>>);

#[derive(Clone, Copy)]
enum Mode {
    Echo,
    MaxTokens,
    MaxTokensPublic,
    BlockedPrompt,
    InlineMedia,
    Listing,
    UpstreamError,
}

#[derive(Clone)]
struct Script {
    capture: Capture,
    queries: Arc<Mutex<Vec<String>>>,
    mode: Mode,
}

async fn gemini_upstream(
    State(script): State<Script>,
    uri: Uri,
    body: Option<Json<Value>>,
) -> Response {
    if let Ok(mut queries) = script.queries.lock() {
        queries.push(uri.query().unwrap_or_default().to_string());
    }
    if matches!(script.mode, Mode::UpstreamError) {
        return (
            StatusCode::BAD_GATEWAY,
            Json(
                json!({"error":{"code":502,"message":"Synthetic upstream failure.",
                "status":"UNAVAILABLE"}}),
            ),
        )
            .into_response();
    }
    if matches!(script.mode, Mode::Listing) {
        return Json(json!({"models":[{"name":"models/synthetic-m4-model"}]})).into_response();
    }
    if matches!(script.mode, Mode::BlockedPrompt) {
        return Json(json!({"promptFeedback":{"blockReason":"SAFETY"},
            "usageMetadata":{"promptTokenCount":1}}))
        .into_response();
    }
    let Some(Json(body)) = body else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    // Native Parts may carry only structured carriers; those tests assert request
    // tokenization, so a fixed public response is enough here.
    let text = if matches!(script.mode, Mode::MaxTokensPublic) {
        // Token-free upstream answer: nothing for restoration to do.
        "Synthetic public truncated answer."
    } else {
        body.pointer("/contents/0/parts/0/text")
            .and_then(Value::as_str)
            .unwrap_or("Synthetic public response.")
    };
    match script.capture.0.lock() {
        Ok(mut requests) => requests.push(body.clone()),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    if uri.query().unwrap_or_default().contains("alt=sse") {
        let content =
            json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":text}]}}]});
        let usage = json!({"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":2}});
        let terminal = if matches!(script.mode, Mode::MaxTokens | Mode::MaxTokensPublic) {
            json!({"candidates":[{"index":0,"finishReason":"MAX_TOKENS"}]})
        } else {
            json!({"candidates":[{"index":0,"finishReason":"STOP"}]})
        };
        (
            [("content-type", "text/event-stream")],
            format!("data: {content}\n\ndata: {usage}\n\ndata: {terminal}\n\n"),
        )
            .into_response()
    } else {
        let part = if matches!(script.mode, Mode::InlineMedia) {
            json!({"inlineData":{"mimeType":"image/png","data":"c3ludGhldGlj"}})
        } else {
            json!({"text":text})
        };
        let finish = if matches!(script.mode, Mode::MaxTokensPublic) {
            "MAX_TOKENS"
        } else {
            "STOP"
        };
        Json(
            json!({"candidates":[{"index":0,"content":{"role":"model","parts":[part]},
            "finishReason":finish}],"usageMetadata":{"promptTokenCount":1,
            "candidatesTokenCount":2,"totalTokenCount":3},"responseId":"synthetic-m4-response",
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

struct GeminiFixture {
    runtime: ResponsesFixture,
    state: ProxyState,
    capture: Capture,
    queries: Arc<Mutex<Vec<String>>>,
    budget: PayloadBudget,
    _task: UpstreamTask,
}

impl GeminiFixture {
    async fn new(mode: Mode) -> TestResult<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut runtime = ResponsesFixture::new();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        runtime.gateway = RestorationGateway::with_test_store_and_rules(
            runtime.store.clone(),
            &["chinese_phone_number"],
            budget.clone(),
        );
        let state = proxy_state(Arc::new(Database::memory()?), &runtime);
        let capture = Capture::default();
        let queries = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = Router::new()
            .fallback(any(gemini_upstream))
            .with_state(Script {
                capture: capture.clone(),
                queries: queries.clone(),
                mode,
            });
        let task = UpstreamTask(tokio::spawn(
            async move { axum::serve(listener, router).await },
        ));
        let provider = Provider::with_id(
            "synthetic-m4-provider".into(),
            "Synthetic M4 upstream".into(),
            json!({"base_url":format!("http://{address}"),"api_key":"synthetic-m4-key",
                "api_format":"gemini_native"}),
            None,
        );
        state.db.save_provider("gemini", &provider)?;
        state.db.set_current_provider("gemini", &provider.id)?;
        Ok(Self {
            runtime,
            state,
            capture,
            queries,
            budget,
            _task: task,
        })
    }

    async fn send(&self, route: &str, body: &Value) -> TestResult<Response> {
        let uri: Uri = route.parse()?;
        let request = Request::builder()
            .method("POST")
            .uri(route)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(body)?))?;
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            handlers::handle_gemini(State(self.state.clone()), uri, request),
        )
        .await?
        .map_err(|_| "m4_gemini_handler_failed".into())
    }

    async fn send_outcome(
        &self,
        route: &str,
        body: &Value,
    ) -> TestResult<Result<Response, crate::proxy::error::ProxyError>> {
        let uri: Uri = route.parse()?;
        let request = Request::builder()
            .method("POST")
            .uri(route)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(body)?))?;
        Ok(tokio::time::timeout(
            std::time::Duration::from_secs(20),
            handlers::handle_gemini(State(self.state.clone()), uri, request),
        )
        .await?)
    }

    async fn list_models(&self, route: &str) -> TestResult<Response> {
        let uri: Uri = route.parse()?;
        let request = Request::builder()
            .method("GET")
            .uri(route)
            .body(Body::empty())?;
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            handlers::handle_gemini(State(self.state.clone()), uri, request),
        )
        .await?
        .map_err(|_| "m4_gemini_listing_failed".into())
    }

    fn requests(&self) -> TestResult<std::sync::MutexGuard<'_, Vec<Value>>> {
        self.capture
            .0
            .lock()
            .map_err(|_| "m4_gemini_capture_poisoned".into())
    }
}

async fn collect_body(response: Response) -> TestResult<(StatusCode, Vec<u8>)> {
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|_| "m4_gemini_body_failed")?
        .to_bytes();
    Ok((status, bytes.to_vec()))
}

fn generate_body() -> Value {
    json!({"contents":[{"role":"user","parts":[{"text":CONTACT}]}],
        "generationConfig":{"temperature":0}})
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_native_json_tokenizes_request_and_restores_response() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::Echo).await?;
    let (status, wire) = collect_body(fixture.send(JSON_ROUTE, &generate_body()).await?).await?;
    assert!(
        status == StatusCode::OK,
        "m4_gemini_json_request_failed: {status} {}",
        String::from_utf8_lossy(&wire)
    );
    let records = fixture.requests()?;
    assert!(
        records.len() == 1
            && !records[0]
                .pointer("/contents/0/parts/0/text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains(CONTACT)),
        "m4_gemini_native_request_not_tokenized"
    );
    let response: Value = serde_json::from_slice(&wire).map_err(|_| "m4_gemini_json_invalid")?;
    assert!(
        response
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(Value::as_str)
            == Some(CONTACT),
        "m4_gemini_native_response_not_restored"
    );
    drop(records);
    assert!(
        fixture.budget.used() == 0,
        "m4_gemini_json_budget_not_refunded"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_native_sse_tokenizes_request_and_restores_frames() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::Echo).await?;
    let (status, wire) = collect_body(fixture.send(SSE_ROUTE, &generate_body()).await?).await?;
    assert!(
        status == StatusCode::OK,
        "m4_gemini_sse_request_failed: {status} {}",
        String::from_utf8_lossy(&wire)
    );
    let records = fixture.requests()?;
    assert!(
        records.len() == 1
            && !records[0]
                .pointer("/contents/0/parts/0/text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains(CONTACT)),
        "m4_gemini_native_sse_request_not_tokenized"
    );
    drop(records);
    let text = std::str::from_utf8(&wire).map_err(|_| "m4_gemini_sse_invalid")?;
    assert!(
        text.contains(CONTACT) && !text.contains("rredact-"),
        "m4_gemini_native_sse_response_not_restored"
    );
    assert!(
        fixture.budget.used() == 0,
        "m4_gemini_sse_budget_not_refunded"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_unsupported_framing_with_restore_rule_is_refused() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::Echo).await?;
    let (status, wire) = collect_body(
        fixture
            .send(
                "/v1beta/models/synthetic-m4-model:streamGenerateContent",
                &generate_body(),
            )
            .await?,
    )
    .await?;
    // A direct native route with a framing DLP cannot restore is a caller error.
    assert!(
        status == StatusCode::BAD_REQUEST,
        "m4_gemini_unsupported_framing_admitted: {status} {}",
        String::from_utf8_lossy(&wire)
    );
    assert!(
        fixture.requests()?.is_empty(),
        "m4_gemini_unsupported_framing_reached_transport"
    );
    let text = std::str::from_utf8(&wire).map_err(|_| "m4_gemini_unsupported_invalid")?;
    assert!(
        !text.contains(CONTACT)
            && text.contains("streamGenerateContent")
            && text.contains("not supported by reversible DLP")
            && text.contains("INVALID_ARGUMENT"),
        "m4_gemini_unsupported_framing_not_actionable: {text}"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_unsupported_framing_without_match_forwards_untouched() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::Echo).await?;
    let body = json!({"contents":[{"role":"user","parts":[{"text":"Synthetic public input."}]}]});
    let (status, wire) = collect_body(
        fixture
            .send(
                "/v1beta/models/synthetic-m4-model:streamGenerateContent",
                &body,
            )
            .await?,
    )
    .await?;
    assert!(
        status == StatusCode::OK,
        "m4_gemini_nonreversible_failed: {status} {}",
        String::from_utf8_lossy(&wire)
    );
    let records = fixture.requests()?;
    assert!(
        records.len() == 1 && records[0] == body,
        "m4_gemini_nonreversible_body_changed"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_model_listing_is_not_admitted() -> TestResult {
    let fixture = GeminiFixture::new(Mode::Listing).await?;
    let (status, wire) = collect_body(fixture.list_models("/v1beta/models").await?).await?;
    assert!(status == StatusCode::OK, "m4_gemini_listing_failed");
    assert!(
        std::str::from_utf8(&wire)
            .map_err(|_| "m4_gemini_listing_invalid")?
            .contains("models/synthetic-m4-model"),
        "m4_gemini_listing_changed"
    );
    assert!(fixture.requests()?.is_empty(), "m4_gemini_listing_captured");
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_query_is_preserved_and_framing_comes_from_uri() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::Echo).await?;
    // `body.stream` is not a Gemini signal: this JSON route stays JSON and the extra
    // query parameter must survive to the actual upstream.
    let body = json!({"stream":true,"contents":[{"role":"user","parts":[{"text":CONTACT}]}]});
    let route = "/v1beta/models/synthetic-m4-model:generateContent?alt=json&synthetic=1";
    let (status, wire) = collect_body(fixture.send(route, &body).await?).await?;
    assert!(
        status == StatusCode::OK,
        "m4_gemini_query_route_failed: {status} {}",
        String::from_utf8_lossy(&wire)
    );
    assert!(
        fixture
            .queries
            .lock()
            .map_err(|_| "m4_gemini_query_poisoned")?
            .as_slice()
            == ["alt=json&synthetic=1".to_string()],
        "m4_gemini_query_not_preserved"
    );
    let response: Value = serde_json::from_slice(&wire).map_err(|_| "m4_gemini_json_garbled")?;
    assert!(
        response
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(Value::as_str)
            == Some(CONTACT),
        "m4_gemini_body_stream_selected_framing"
    );
    assert!(
        fixture.budget.used() == 0,
        "m4_gemini_query_budget_retained"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_signed_part_is_never_rewritten() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::Echo).await?;
    let signed = json!({"text":CONTACT,"thoughtSignature":"synthetic-m4-signature"});
    let body = json!({"contents":[{"role":"user","parts":[signed]}]});
    let (status, wire) = collect_body(fixture.send(JSON_ROUTE, &body).await?).await?;
    // A caller-actionable refusal: nothing was sent, and the client is told why.
    assert!(
        status == StatusCode::BAD_REQUEST,
        "m4_gemini_signed_part_admitted: {status} {}",
        String::from_utf8_lossy(&wire)
    );
    assert!(
        fixture.requests()?.is_empty(),
        "m4_gemini_signed_part_reached_transport"
    );
    let text = std::str::from_utf8(&wire).map_err(|_| "m4_gemini_signed_invalid")?;
    assert!(
        !text.contains(CONTACT) && !text.contains("synthetic-m4-signature"),
        "m4_gemini_signed_part_leaked: {text}"
    );
    let error: Value = serde_json::from_slice(&wire).map_err(|_| "m4_gemini_signed_garbled")?;
    assert!(
        error["error"]["code"] == 400
            && error["error"]["status"] == "INVALID_ARGUMENT"
            && error["error"]["message"]
                == "This request contains signed or otherwise immutable content that reversible DLP cannot modify. Remove that content, or disable reversible DLP for this provider.",
        "m4_gemini_signed_part_not_actionable: {text}"
    );
    assert!(
        fixture.budget.used() == 0,
        "m4_gemini_signed_budget_retained"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_current_function_response_text_is_tokenized() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::Echo).await?;
    let body = json!({"contents":[
        {"role":"user","parts":[{"functionResponse":{"name":"synthetic_lookup",
            "response":{"value":CONTACT,"nested":[CONTACT,7,true,null]}}}]}
    ]});
    let (status, wire) = collect_body(fixture.send(JSON_ROUTE, &body).await?).await?;
    assert!(
        status == StatusCode::OK,
        "m4_gemini_function_response_failed: {status} {}",
        String::from_utf8_lossy(&wire)
    );
    let records = fixture.requests()?;
    let sent = serde_json::to_string(&records[0])?;
    assert!(
        records.len() == 1 && !sent.contains(CONTACT),
        "m4_gemini_function_response_not_tokenized"
    );
    drop(records);
    assert!(
        fixture.budget.used() == 0,
        "m4_gemini_function_budget_retained"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_sse_incomplete_terminal_returns_fixed_error_frame() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::MaxTokens).await?;
    let (status, wire) = collect_body(fixture.send(SSE_ROUTE, &generate_body()).await?).await?;
    assert!(status == StatusCode::OK, "m4_gemini_sse_status_changed");
    // Still a single fixed frame, but it now carries a specific code and cause rather
    // than a generic processing failure.
    let frame = std::str::from_utf8(&wire).map_err(|_| "m4_gemini_sse_garbled")?;
    assert!(
        frame.starts_with("data: {") && frame.ends_with("\n\n"),
        "m4_gemini_sse_frame_shape_changed: {frame}"
    );
    assert!(
        frame.contains("\"code\":502")
            && frame.contains("local DLP placeholders that could not be restored safely")
            && frame.contains("disable reversible DLP for this provider")
            && !frame.contains("proxy_processing_failed")
            && !frame.contains(CONTACT),
        "m4_gemini_sse_incomplete_terminal_not_actionable: {frame}"
    );
    assert!(
        fixture.budget.used() == 0,
        "m4_gemini_sse_incomplete_budget_retained"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_blocked_prompt_degrades_to_the_upstream_signal() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::BlockedPrompt).await?;
    let (status, wire) = collect_body(fixture.send(JSON_ROUTE, &generate_body()).await?).await?;
    // An upstream safety block is a real answer for the caller; a token-free body has
    // nothing to restore, so it must not be replaced by a proxy error.
    assert!(
        status == StatusCode::OK,
        "m4_gemini_blocked_degradation_failed: {status} {}",
        String::from_utf8_lossy(&wire)
    );
    let response: Value = serde_json::from_slice(&wire).map_err(|_| "m4_gemini_blocked_garbled")?;
    assert!(
        response
            .pointer("/promptFeedback/blockReason")
            .and_then(Value::as_str)
            == Some("SAFETY"),
        "m4_gemini_blocked_signal_lost: {}",
        String::from_utf8_lossy(&wire)
    );
    assert!(
        fixture.budget.used() == 0,
        "m4_gemini_blocked_budget_retained"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_truncated_public_answer_degrades_instead_of_erroring() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    for sse in [false, true] {
        let fixture = GeminiFixture::new(Mode::MaxTokensPublic).await?;
        let route = if sse { SSE_ROUTE } else { JSON_ROUTE };
        let (status, wire) = collect_body(fixture.send(route, &generate_body()).await?).await?;
        assert!(
            status == StatusCode::OK,
            "m4_gemini_truncated_degradation_failed: sse={sse} {status} {}",
            String::from_utf8_lossy(&wire)
        );
        let text = std::str::from_utf8(&wire).map_err(|_| "m4_gemini_truncated_garbled")?;
        assert!(
            text.contains("MAX_TOKENS") && text.contains("Synthetic public truncated answer."),
            "m4_gemini_truncated_content_lost: sse={sse} {text}"
        );
        assert!(
            fixture.budget.used() == 0,
            "m4_gemini_truncated_budget_retained: sse={sse}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_inline_media_response_degrades_without_restoration() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::InlineMedia).await?;
    let (status, wire) = collect_body(fixture.send(JSON_ROUTE, &generate_body()).await?).await?;
    // Nothing in this response can be restored, so the user keeps the upstream answer
    // instead of receiving a generic 500 for a capability the proxy never touches.
    assert!(
        status == StatusCode::OK,
        "m4_gemini_media_degradation_failed: {status} {}",
        String::from_utf8_lossy(&wire)
    );
    let response: Value = serde_json::from_slice(&wire).map_err(|_| "m4_gemini_media_garbled")?;
    assert!(
        response
            .pointer("/candidates/0/content/parts/0/inlineData/data")
            .and_then(Value::as_str)
            == Some("c3ludGhldGlj"),
        "m4_gemini_media_not_passed_through: {}",
        String::from_utf8_lossy(&wire)
    );
    assert!(
        fixture.budget.used() == 0,
        "m4_gemini_media_budget_retained"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn m4_gemini_upstream_error_propagates_without_sensitive_plaintext() -> TestResult {
    let _transport = crate::proxy::forwarder::final_send_binding_test_support::install(|_, _| {});
    let fixture = GeminiFixture::new(Mode::UpstreamError).await?;
    let response = match fixture.send_outcome(JSON_ROUTE, &generate_body()).await? {
        Ok(response) => response,
        Err(error) => axum::response::IntoResponse::into_response(error),
    };
    let (status, wire) = collect_body(response).await?;
    assert!(
        status != StatusCode::OK,
        "m4_gemini_upstream_error_admitted: {status} {}",
        String::from_utf8_lossy(&wire)
    );
    // The upstream's own diagnostic may pass through, but never matched plaintext
    // or credentials; the status and its body stay a genuine upstream error.
    let text = std::str::from_utf8(&wire).map_err(|_| "m4_gemini_upstream_error_garbled")?;
    assert!(
        !text.contains(CONTACT) && text.contains("Synthetic upstream failure."),
        "m4_gemini_upstream_error_unsanitized: {text}"
    );
    assert!(
        fixture.budget.used() == 0,
        "m4_gemini_upstream_error_budget_retained"
    );
    Ok(())
}
