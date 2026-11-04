//! Synthetic in-memory error responses; transport injection never opens a socket.
use super::{
    http_restoration::{admit_http_request, RestorationAdmission, RestorationGateway},
    runtime,
};
use crate::{
    app_config::AppType,
    database::Database,
    provider::Provider,
    proxy::{
        failover_switch::FailoverSwitchManager,
        forwarder::{upstream_error_test_support, RequestForwarder},
        hyper_client::ProxyResponse,
        provider_router::ProviderRouter,
        providers::{codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore},
        types::{CopilotOptimizerConfig, OptimizerConfig, ProxyStatus, RectifierConfig},
    },
};
use axum::response::IntoResponse;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Reset(runtime::SecurityRuntimeConfig);
impl Drop for Reset {
    fn drop(&mut self) {
        runtime::set_security_runtime_for_test(self.0.clone());
    }
}

async fn forwarded_error(
    response: ProxyResponse,
) -> Result<(http::StatusCode, Value), Box<dyn std::error::Error>> {
    forwarded_error_options(response, false).await
}
async fn forwarded_error_options(
    response: ProxyResponse,
    official: bool,
) -> Result<(http::StatusCode, Value), Box<dyn std::error::Error>> {
    forwarded_error_scenario(response, official, false).await
}
async fn forwarded_error_scenario(
    response: ProxyResponse,
    official: bool,
    empty: bool,
) -> Result<(http::StatusCode, Value), Box<dyn std::error::Error>> {
    forwarded_error_with_auth(response, official, empty, None).await
}
async fn forwarded_error_with_auth(
    response: ProxyResponse,
    official: bool,
    empty: bool,
    sent_authorization: Option<http::HeaderValue>,
) -> Result<(http::StatusCode, Value), Box<dyn std::error::Error>> {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let db = Arc::new(Database::memory()?);
    let mut provider = Provider::with_id(
        "synthetic-error".into(),
        "Synthetic error".into(),
        json!({"base_url":"http://127.0.0.1:1", "api_key":"synthetic-error-credential"}),
        None,
    );
    let mut headers = http::HeaderMap::new();
    if official {
        provider.category = Some("official".into());
        provider.settings_config = json!({"auth":{}});
        headers.insert(
            http::header::AUTHORIZATION,
            sent_authorization.unwrap_or_else(|| {
                http::HeaderValue::from_static(concat!("Bearer synthetic-inbound-", "opaque"))
            }),
        );
    }
    let semantic_error = response.status().is_success();
    if semantic_error {
        provider.settings_config["api_format"] = json!("anthropic");
    }
    let mut backup = provider.clone();
    backup.id = "synthetic-backup".into();
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let mut body = json!({"model":"synthetic-model", "input":"call 13900001234", "stream":false});
    if empty {
        body["input"] = json!("synthetic-safe-input");
    }
    let request =
        match admit_http_request(&gateway, &AppType::Codex, true, false, &mut body, &headers)? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic_admission_failed".into()),
        };
    let status = Arc::new(RwLock::new(ProxyStatus::default()));
    let forwarder = RequestForwarder::new(
        Arc::new(ProviderRouter::new(db.clone())),
        1,
        status.clone(),
        Arc::default(),
        Arc::new(GeminiShadowStore::default()),
        Arc::new(CodexChatHistoryStore::default()),
        Arc::new(FailoverSwitchManager::new(db)),
        None,
        provider.id.clone(),
        "synthetic-error-session".into(),
        true,
        1,
        1,
        RectifierConfig::default(),
        OptimizerConfig::default(),
        CopilotOptimizerConfig::default(),
        1,
    )
    .with_dlp_request(Some(request));
    let transport = upstream_error_test_support::install(response);
    let failure = match forwarder
        .forward_with_retry(
            &AppType::Codex,
            http::Method::POST,
            "/v1/responses",
            body,
            headers,
            Default::default(),
            vec![provider, backup],
        )
        .await
    {
        Err(failure) => failure,
        Ok(_) => return Err("synthetic_error_accepted".into()),
    };
    assert!(transport.attempts() == 1, "unexpected_retry");
    if semantic_error {
        assert!(failure.provider.is_none(), "local_error_misattributed");
    } else {
        assert!(
            failure
                .provider
                .as_ref()
                .is_some_and(|p| p.id == "synthetic-error"),
            "attempted_provider_identity_lost"
        );
    }
    assert!(
        !failure
            .error
            .to_string()
            .contains("fixture-provider-reason"),
        "diagnostic_body_exposure"
    );
    assert!(
        !format!("{:?}", failure.error).contains("fixture-provider-reason"),
        "debug_body_exposure"
    );
    if semantic_error {
        assert!(
            matches!(
                &failure.error,
                crate::proxy::error::ProxyError::DlpForward(
                    super::http_restoration::DlpForwardError::RestoreFailedAt(
                        super::restore_diagnostics::RestoreStage::SuccessValidation
                    )
                )
            ),
            "semantic_error_not_isolated"
        );
    } else {
        assert!(
            matches!(
                &failure.error,
                crate::proxy::error::ProxyError::SafeUpstream(_)
            ),
            "prepared_error_not_isolated"
        );
    }
    assert!(
        !status
            .read()
            .await
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("fixture-provider-reason"),
        "status_body_exposure"
    );
    let response = failure.error.into_response();
    let status = response.status();
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok((status, serde_json::from_slice(&bytes)?))
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_safe_json_keeps_status_and_fields() -> TestResult {
    let expected = json!({"error":{"message":"fixture-provider-reason: unsupported model", "type":"invalid_request_error",
        "code":"model_not_found", "param":"model", "safe_extension":{"retryable":false}}});
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    let (status, actual) = forwarded_error(ProxyResponse::buffered(
        http::StatusCode::BAD_REQUEST,
        headers,
        bytes::Bytes::from(serde_json::to_vec(&expected)?),
    ))
    .await?;
    assert!(
        status == http::StatusCode::BAD_REQUEST,
        "upstream_status_changed"
    );
    assert!(actual == expected, "safe_upstream_error_not_preserved");
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_safe_text_keeps_reason() -> TestResult {
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/plain"),
    );
    let (status, actual) = forwarded_error(ProxyResponse::buffered(
        http::StatusCode::TOO_MANY_REQUESTS,
        headers,
        bytes::Bytes::from_static(b"fixture-provider-reason: quota exhausted"),
    ))
    .await?;
    assert!(
        status == http::StatusCode::TOO_MANY_REQUESTS,
        "upstream_status_changed"
    );
    assert!(
        actual["error"]["message"] == "fixture-provider-reason: quota exhausted",
        "safe_upstream_text_not_preserved"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_sensitive_nested_value_preserves_safe_reason() -> TestResult {
    let expected = json!({"error":{"message":"fixture-provider-reason: invalid parameter", "type":"invalid_request_error",
        "code":"invalid_parameter", "param":"input", "details":{"echo":"synthetic-error-credential"}}});
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    let (status, actual) = forwarded_error(ProxyResponse::buffered(
        http::StatusCode::UNPROCESSABLE_ENTITY,
        headers,
        bytes::Bytes::from(serde_json::to_vec(&expected)?),
    ))
    .await?;
    assert!(
        status == http::StatusCode::UNPROCESSABLE_ENTITY,
        "upstream_status_changed"
    );
    assert!(
        actual["error"]["message"] == expected["error"]["message"],
        "safe_reason_lost"
    );
    assert!(
        actual["error"]["details"]["echo"] == "[REDACTED]",
        "credential_not_redacted"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_json_key_cannot_echo_sensitive_content() -> TestResult {
    let expected = json!({"error":{"message":"fixture-provider-reason: invalid parameter", "details":{"synthetic-error-credential":"value"}}});
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    let (status, actual) = forwarded_error(ProxyResponse::buffered(
        http::StatusCode::BAD_REQUEST,
        headers,
        bytes::Bytes::from(serde_json::to_vec(&expected)?),
    ))
    .await?;
    assert!(
        status == http::StatusCode::BAD_REQUEST,
        "upstream_status_changed"
    );
    assert!(
        !actual.to_string().contains("synthetic-error-credential"),
        "sensitive_key_exposed"
    );
    assert!(
        actual["error"]["message"] == expected["error"]["message"],
        "safe_reason_lost"
    );
    Ok(())
}

async fn checked_fixture(request: Value, provider: Value, payload: Value) -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        upstream_error::{read_error, ErrorSafetyContext},
    };
    let runtime = runtime::SecurityRuntimeConfig::default();
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let credentials = Vec::new();
    let response = ProxyResponse::buffered(
        http::StatusCode::BAD_REQUEST,
        http::HeaderMap::new(),
        bytes::Bytes::from(serde_json::to_vec(&payload)?),
    );
    let error = read_error(
        response,
        ErrorSafetyContext {
            runtime: &runtime,
            request: &request,
            provider: &provider,
            credentials: &credentials,
            sent_credentials: None,
            budget: budget.clone(),
        },
        std::time::Duration::from_secs(1),
    )
    .await;
    let response = error.to_response();
    assert!(
        response.status() == http::StatusCode::BAD_REQUEST,
        "upstream_status_changed"
    );
    let bytes = response.into_body().collect().await?.to_bytes();
    let actual: Value = serde_json::from_slice(&bytes)?;
    assert!(
        actual["error"]["message"] == "fixture-provider-reason",
        "safe_reason_lost"
    );
    assert!(
        actual["error"]["details"]["echo"] == "[REDACTED]",
        "source_privacy_exposed"
    );
    drop(bytes);
    drop(error);
    assert!(budget.used() == 0, "budget_owner_retained");
    Ok(())
}

#[tokio::test]
async fn upstream_error_passthrough_tool_argument_member_is_private() -> TestResult {
    checked_fixture(json!({"input":[{"type":"function_call", "arguments":"{\"path\":\"synthetic-private-path\"}"}]}),
        json!({}), json!({"error":{"message":"fixture-provider-reason", "details":{"echo":"synthetic-private-path"}}})).await
}

#[tokio::test]
async fn upstream_error_passthrough_private_numeric_scalar_is_private() -> TestResult {
    checked_fixture(
        json!({"input":[{"type":"function_call", "arguments":{"count":736281}}]}),
        json!({}),
        json!({"error":{"message":"fixture-provider-reason", "details":{"echo":736281}}}),
    )
    .await
}

#[tokio::test]
async fn upstream_error_passthrough_nested_config_model_is_private() -> TestResult {
    checked_fixture(json!({}), json!({"config":{"model":"synthetic-private-model"}}),
        json!({"error":{"message":"fixture-provider-reason", "details":{"echo":"synthetic-private-model"}}})).await
}

#[test]
fn upstream_error_passthrough_legacy_diagnostics_do_not_contain_body() {
    let error = crate::proxy::error::ProxyError::UpstreamError {
        status: 400,
        body: Some("fixture-provider-reason".into()),
    };
    assert!(
        !error.to_string().contains("fixture-provider-reason"),
        "diagnostic_body_exposure"
    );
    assert!(
        !format!("{error:?}").contains("fixture-provider-reason"),
        "debug_body_exposure"
    );
}

#[tokio::test]
async fn upstream_error_passthrough_escape_dense_serialization_is_linear() -> TestResult {
    use super::payload_buffer::test_support::{self, Action, Phase};
    use std::{cell::Cell, rc::Rc};
    let copied = Rc::new(Cell::new(0usize));
    let count = copied.clone();
    let _guard = test_support::install(move |event| {
        if event.phase == Phase::BeforeCopy {
            count.set(count.get() + event.bytes_to_copy);
        }
        Action::Continue
    });
    let text = "\"\\\n".repeat(2048);
    let expected = json!({"error":{"message":text}});
    let wire = serde_json::to_vec(&expected)?;
    let size = wire.len();
    let (status, actual) = forwarded_error(ProxyResponse::buffered(
        http::StatusCode::BAD_REQUEST,
        http::HeaderMap::new(),
        bytes::Bytes::from(wire),
    ))
    .await?;
    assert!(
        status == http::StatusCode::BAD_REQUEST && actual == expected,
        "escape_dense_body_changed"
    );
    assert!(
        copied.get() <= size * 16,
        "serialization_copy_work_unbounded"
    );
    Ok(())
}

#[test]
fn upstream_error_passthrough_error_mapper_does_not_persist_body() {
    let error = crate::proxy::error::ProxyError::UpstreamError {
        status: 400,
        body: Some("fixture-provider-reason".into()),
    };
    assert!(
        !crate::proxy::error_mapper::get_error_message(&error).contains("fixture-provider-reason"),
        "diagnostic_body_exposure"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_actual_inbound_credential_is_private() -> TestResult {
    let expected = json!({"error":{"message":"fixture-provider-reason", "details":{"echo":concat!("synthetic-inbound-", "opaque")}}});
    let (_, actual) = forwarded_error_options(
        ProxyResponse::buffered(
            http::StatusCode::BAD_REQUEST,
            http::HeaderMap::new(),
            bytes::Bytes::from(serde_json::to_vec(&expected)?),
        ),
        true,
    )
    .await?;
    assert!(
        actual["error"]["message"] == expected["error"]["message"],
        "safe_reason_lost"
    );
    assert!(
        actual["error"]["details"]["echo"] == "[REDACTED]",
        "actual_credential_exposed"
    );
    Ok(())
}

#[test]
fn upstream_error_passthrough_retry_summary_does_not_contain_body() {
    let error = crate::proxy::error::ProxyError::UpstreamError {
        status: 503,
        body: Some("fixture-provider-reason".into()),
    };
    assert!(
        !upstream_error_test_support::diagnostic_summary(&error)
            .contains("fixture-provider-reason"),
        "retry_diagnostic_body_exposure"
    );
}

async fn read_fixture(
    response: ProxyResponse,
    budget: super::payload_budget::PayloadBudget,
    timeout: std::time::Duration,
) -> super::upstream_error::SafeUpstreamError {
    let runtime = runtime::SecurityRuntimeConfig::default();
    let request = json!({});
    let provider = json!({});
    super::upstream_error::read_error(
        response,
        super::upstream_error::ErrorSafetyContext {
            runtime: &runtime,
            request: &request,
            provider: &provider,
            credentials: &[],
            sent_credentials: None,
            budget,
        },
        timeout,
    )
    .await
}
async fn assert_unavailable(
    response: ProxyResponse,
    limit: usize,
    timeout: std::time::Duration,
) -> TestResult {
    let status = response.status();
    let budget = super::payload_budget::PayloadBudget::with_limit(limit);
    let error = read_fixture(response, budget.clone(), timeout).await;
    let response = error.to_response();
    assert!(response.status() == status, "upstream_status_changed");
    let bytes = response.into_body().collect().await?.to_bytes();
    let value: Value = serde_json::from_slice(&bytes)?;
    assert!(
        value["error"]["code"] == "upstream_error_body_unavailable",
        "unsafe_partial_body_returned"
    );
    drop(bytes);
    drop(error);
    assert!(budget.used() == 0, "budget_owner_retained");
    Ok(())
}
#[tokio::test]
async fn upstream_error_passthrough_unavailable_matrix_preserves_status() -> TestResult {
    use bytes::Bytes;
    use http::{header, HeaderMap, HeaderValue, StatusCode};
    let timeout = std::time::Duration::from_secs(1);
    for wire in [
        Bytes::from_static(b"{\"error\":"),
        Bytes::from_static(b"{\"a\":1,\"a\":2}"),
        Bytes::from_static(&[255]),
        Bytes::from(vec![b'x'; 1024 * 1024 + 1]),
    ] {
        assert_unavailable(
            ProxyResponse::buffered(StatusCode::BAD_REQUEST, HeaderMap::new(), wire),
            16 * 1024 * 1024,
            timeout,
        )
        .await?;
    }
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
    assert_unavailable(
        ProxyResponse::buffered(
            StatusCode::TOO_MANY_REQUESTS,
            headers,
            Bytes::from_static(b"truncated"),
        ),
        16 * 1024 * 1024,
        timeout,
    )
    .await?;
    assert_unavailable(
        ProxyResponse::buffered(
            StatusCode::SERVICE_UNAVAILABLE,
            HeaderMap::new(),
            Bytes::from_static(b"safe"),
        ),
        1,
        timeout,
    )
    .await?;
    let stream = futures::stream::pending::<Result<Bytes, std::io::Error>>();
    assert_unavailable(
        ProxyResponse::streamed(StatusCode::GATEWAY_TIMEOUT, HeaderMap::new(), stream),
        16 * 1024 * 1024,
        std::time::Duration::from_millis(1),
    )
    .await?;
    let stream = futures::stream::iter([
        Ok(Bytes::from_static(b"partial")),
        Err(std::io::Error::other("synthetic_read_failure")),
    ]);
    assert_unavailable(
        ProxyResponse::streamed(StatusCode::BAD_GATEWAY, HeaderMap::new(), stream),
        16 * 1024 * 1024,
        timeout,
    )
    .await?;
    Ok(())
}
#[tokio::test]
async fn upstream_error_passthrough_cancellation_releases_budget() -> TestResult {
    use futures::FutureExt;
    let budget = super::payload_budget::PayloadBudget::with_limit(16 * 1024 * 1024);
    let stream = futures::stream::pending::<Result<bytes::Bytes, std::io::Error>>();
    let response = ProxyResponse::streamed(
        http::StatusCode::BAD_REQUEST,
        http::HeaderMap::new(),
        stream,
    );
    let mut operation = Box::pin(read_fixture(
        response,
        budget.clone(),
        std::time::Duration::from_secs(1),
    ));
    assert!(
        operation.as_mut().now_or_never().is_none(),
        "fixture_not_pending"
    );
    assert!(budget.used() > 0, "fixture_owner_missing");
    drop(operation);
    assert!(budget.used() == 0, "cancelled_owner_retained");
    Ok(())
}
#[tokio::test]
async fn upstream_error_passthrough_gzip_sse_error_is_checked() -> TestResult {
    use std::io::Write;
    let value = json!({"error":{"message":"fixture-provider-reason"}});
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&serde_json::to_vec(&value)?)?;
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_ENCODING,
        http::HeaderValue::from_static("gzip"),
    );
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/event-stream"),
    );
    let response = ProxyResponse::buffered(
        http::StatusCode::BAD_REQUEST,
        headers,
        bytes::Bytes::from(encoder.finish()?),
    );
    let budget = super::payload_budget::PayloadBudget::with_limit(16 * 1024 * 1024);
    let error = read_fixture(response, budget.clone(), std::time::Duration::from_secs(1)).await;
    let bytes = error.to_response().into_body().collect().await?.to_bytes();
    assert!(
        serde_json::from_slice::<Value>(&bytes)? == value,
        "compressed_safe_reason_lost"
    );
    drop(error);
    assert!(budget.used() > 0, "response_owner_missing");
    drop(bytes);
    assert!(budget.used() == 0, "response_owner_retained");
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_empty_lease_uses_safe_carrier() -> TestResult {
    let expected =
        json!({"error":{"message":"fixture-provider-reason", "code":"invalid_parameter"}});
    let (_, actual) = forwarded_error_scenario(
        ProxyResponse::buffered(
            http::StatusCode::BAD_REQUEST,
            http::HeaderMap::new(),
            bytes::Bytes::from(serde_json::to_vec(&expected)?),
        ),
        false,
        true,
    )
    .await?;
    assert!(actual == expected, "empty_lease_safe_reason_lost");
    Ok(())
}

#[tokio::test]
async fn upstream_error_passthrough_encoded_private_echo_is_redacted() -> TestResult {
    use base64::Engine;
    let private = "synthetic-private-encoded-path";
    let encoded = base64::engine::general_purpose::STANDARD.encode(private);
    checked_fixture(
        json!({"input":private}),
        json!({}),
        json!({"error":{"message":"fixture-provider-reason", "details":{"echo":encoded}}}),
    )
    .await
}
#[tokio::test]
async fn upstream_error_passthrough_escaped_nested_private_echo_is_redacted() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        upstream_error::{read_error, ErrorSafetyContext},
    };
    let private = "synthetic/hidden\"path";
    let request = json!({"input":private});
    let provider = json!({});
    let encoded = serde_json::to_string(&json!({"echo":private}))?;
    let payload =
        json!({"error":{"message":"fixture-provider-reason", "details":{"echo":encoded}}});
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let runtime = runtime::SecurityRuntimeConfig::default();
    let response = ProxyResponse::buffered(
        http::StatusCode::BAD_REQUEST,
        http::HeaderMap::new(),
        bytes::Bytes::from(serde_json::to_vec(&payload)?),
    );
    let error = read_error(
        response,
        ErrorSafetyContext {
            runtime: &runtime,
            request: &request,
            provider: &provider,
            credentials: &[],
            sent_credentials: None,
            budget: budget.clone(),
        },
        std::time::Duration::from_secs(1),
    )
    .await;
    let bytes = error.to_response().into_body().collect().await?.to_bytes();
    let actual: Value = serde_json::from_slice(&bytes)?;
    assert!(
        actual["error"]["message"] == "fixture-provider-reason",
        "safe_reason_lost"
    );
    let echo = actual["error"]["details"]["echo"]
        .as_str()
        .ok_or("echo_shape_changed")?;
    let nested: Value = serde_json::from_str(echo)?;
    assert!(nested["echo"] == "[REDACTED]", "source_privacy_exposed");
    drop(bytes);
    drop(error);
    assert!(budget.used() == 0, "budget_owner_retained");
    Ok(())
}
#[tokio::test]
async fn upstream_error_passthrough_source_work_limit_is_fixed() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        upstream_error::{read_error, ErrorSafetyContext},
    };
    let request = json!({"input":vec!["synthetic-unmatched-long-source"; 8192]});
    let provider = json!({});
    let runtime = runtime::SecurityRuntimeConfig::default();
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let response = ProxyResponse::buffered(
        http::StatusCode::BAD_REQUEST,
        http::HeaderMap::new(),
        bytes::Bytes::from_static(b"safe model not found"),
    );
    let error = read_error(
        response,
        ErrorSafetyContext {
            runtime: &runtime,
            request: &request,
            provider: &provider,
            credentials: &[],
            sent_credentials: None,
            budget: budget.clone(),
        },
        std::time::Duration::from_secs(1),
    )
    .await;
    let bytes = error.to_response().into_body().collect().await?.to_bytes();
    let actual: Value = serde_json::from_slice(&bytes)?;
    assert!(
        actual["error"]["code"] == "upstream_error_body_unavailable",
        "source_work_not_bounded"
    );
    drop(bytes);
    drop(error);
    assert!(budget.used() == 0, "budget_owner_retained");
    Ok(())
}

#[tokio::test]
async fn upstream_error_passthrough_private_tool_key_is_redacted() -> TestResult {
    checked_fixture(json!({"input":{"synthetic-private-key":"value"}}), json!({}),
        json!({"error":{"message":"fixture-provider-reason", "details":{"echo":"synthetic-private-key"}}})).await
}
#[tokio::test]
async fn upstream_error_passthrough_cookie_member_is_private() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        upstream_error::{read_error, ErrorCredentials, ErrorSafetyContext},
    };
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let runtime = runtime::SecurityRuntimeConfig::default();
    let request = json!({});
    let provider = json!({});
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::COOKIE,
        http::HeaderValue::from_static("session=synthetic-cookie-member; mode=safe"),
    );
    let credentials = ErrorCredentials::capture(&headers, budget.clone())
        .map_err(|_| "credential_capture_failed")?;
    let response = ProxyResponse::buffered(
        http::StatusCode::BAD_REQUEST,
        http::HeaderMap::new(),
        bytes::Bytes::from_static(b"synthetic-cookie-member"),
    );
    let error = read_error(
        response,
        ErrorSafetyContext {
            runtime: &runtime,
            request: &request,
            provider: &provider,
            credentials: &[],
            sent_credentials: Some(&credentials),
            budget: budget.clone(),
        },
        std::time::Duration::from_secs(1),
    )
    .await;
    let bytes = error.to_response().into_body().collect().await?.to_bytes();
    let actual: Value = serde_json::from_slice(&bytes)?;
    assert!(
        actual["error"]["message"] == "[REDACTED]",
        "wrapped_credential_exposed"
    );
    drop(bytes);
    drop(error);
    drop(credentials);
    assert!(budget.used() == 0, "budget_owner_retained");
    Ok(())
}
#[tokio::test]
async fn upstream_error_passthrough_error_guard_stops_at_wire_limit() -> TestResult {
    use futures::StreamExt;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    let stream = futures::stream::iter(
        (0..2048).map(|_| Ok::<_, std::io::Error>(bytes::Bytes::from(vec![b'x'; 1024]))),
    )
    .inspect(move |_| {
        observed.fetch_add(1, Ordering::Relaxed);
    });
    assert_unavailable(
        ProxyResponse::streamed(
            http::StatusCode::BAD_REQUEST,
            http::HeaderMap::new(),
            stream,
        ),
        16 * 1024 * 1024,
        std::time::Duration::from_secs(1),
    )
    .await?;
    assert!(
        polls.load(Ordering::Relaxed) <= 1025,
        "wire_limit_not_enforced"
    );
    Ok(())
}
#[tokio::test]
async fn upstream_error_passthrough_error_guard_small_chunks_are_linear() -> TestResult {
    use super::payload_buffer::test_support::{self, Action, Phase};
    use std::{cell::Cell, rc::Rc};
    let copied = Rc::new(Cell::new(0usize));
    let count = copied.clone();
    let _guard = test_support::install(move |event| {
        if event.phase == Phase::BeforeCopy {
            count.set(count.get() + event.bytes_to_copy);
        }
        Action::Continue
    });
    let stream = futures::stream::iter(
        (0..1024).map(|_| Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"a"))),
    );
    let budget = super::payload_budget::PayloadBudget::with_limit(16 * 1024 * 1024);
    let error = read_fixture(
        ProxyResponse::streamed(
            http::StatusCode::BAD_REQUEST,
            http::HeaderMap::new(),
            stream,
        ),
        budget.clone(),
        std::time::Duration::from_secs(1),
    )
    .await;
    let bytes = error.to_response().into_body().collect().await?.to_bytes();
    let actual: Value = serde_json::from_slice(&bytes)?;
    assert!(
        actual["error"]["message"]
            .as_str()
            .is_some_and(|s| s.len() == 1024),
        "small_chunk_body_changed"
    );
    assert!(copied.get() <= 16 * 1024, "guard_copy_work_unbounded");
    drop(bytes);
    drop(error);
    assert!(budget.used() == 0, "budget_owner_retained");
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_empty_lease_semantic_error_is_fixed() -> TestResult {
    let payload = json!({"type":"error","error":{"type":"invalid_request_error","message":"fixture-provider-reason"}});
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    let (_, body) = forwarded_error_scenario(
        ProxyResponse::buffered(
            http::StatusCode::OK,
            headers,
            bytes::Bytes::from(serde_json::to_vec(&payload)?),
        ),
        false,
        true,
    )
    .await?;
    assert!(
        !body.to_string().contains("fixture-provider-reason"),
        "semantic_client_body_exposure"
    );
    Ok(())
}
#[test]
fn upstream_error_passthrough_empty_lease_suppresses_payload_diagnostics() -> TestResult {
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let mut body = json!({"model":"synthetic-model","input":"synthetic-safe-input"});
    let request = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut body,
        &http::HeaderMap::new(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic_admission_failed".into()),
    };
    let attempt = request
        .prepare_attempt(
            "synthetic-provider",
            "openai_responses",
            "http://127.0.0.1:1/v1/responses",
            None,
            &mut body,
            |prepared| Ok(prepared.clone()),
        )
        .map_err(|_| "synthetic_prepare_failed")?;
    assert!(!attempt.has_restoration_lease(), "fixture_not_empty");
    assert!(
        attempt.suppress_payload_diagnostics(),
        "empty_lease_diagnostics_enabled"
    );
    Ok(())
}

#[tokio::test]
async fn upstream_error_passthrough_protocol_named_private_subtree_is_private() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        upstream_error::{read_error, ErrorSafetyContext},
    };
    let request = json!({"input":[{"type":"function_call","arguments":{"type":"message","role":{"opaque":"synthetic-private-role"}}}]});
    let provider = json!({});
    let payload =
        json!({"error":{"code":"safe_reason","details":{"echo":"synthetic-private-role"}}});
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let runtime = runtime::SecurityRuntimeConfig::default();
    let error = read_error(
        ProxyResponse::buffered(
            http::StatusCode::BAD_REQUEST,
            http::HeaderMap::new(),
            bytes::Bytes::from(serde_json::to_vec(&payload)?),
        ),
        ErrorSafetyContext {
            runtime: &runtime,
            request: &request,
            provider: &provider,
            credentials: &[],
            sent_credentials: None,
            budget: budget.clone(),
        },
        std::time::Duration::from_secs(1),
    )
    .await;
    let bytes = error.to_response().into_body().collect().await?.to_bytes();
    let actual: Value = serde_json::from_slice(&bytes)?;
    assert!(actual["error"]["code"] == "safe_reason", "safe_reason_lost");
    assert!(
        actual["error"]["details"]["echo"] == "[REDACTED]",
        "source_privacy_exposed"
    );
    drop(bytes);
    drop(error);
    assert!(budget.used() == 0, "budget_owner_retained");
    Ok(())
}
#[tokio::test]
async fn upstream_error_passthrough_url_components_are_private() -> TestResult {
    for private in [
        "synthetic-private-host.invalid",
        "synthetic user",
        "synthetic/path",
    ] {
        checked_fixture(json!({}),json!({"base_url":"https://synthetic%20user:synthetic-password@synthetic-private-host.invalid/synthetic%2Fpath"}),json!({"error":{"message":"fixture-provider-reason","details":{"echo":private}}})).await?;
    }
    Ok(())
}
#[tokio::test]
async fn upstream_error_passthrough_encoding_equivalents_are_private() -> TestResult {
    use base64::Engine;
    let private = "synthetic/hidden?";
    for encoded in [
        "synthetic%2fhidden%3f".to_owned(),
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(private),
    ] {
        checked_fixture(
            json!({"input":private}),
            json!({}),
            json!({"error":{"message":"fixture-provider-reason","details":{"echo":encoded}}}),
        )
        .await?;
    }
    Ok(())
}
#[tokio::test]
async fn upstream_error_passthrough_basic_credentials_are_private() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        upstream_error::{read_error, ErrorCredentials, ErrorSafetyContext},
    };
    use base64::Engine;
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let runtime = runtime::SecurityRuntimeConfig::default();
    let request = json!({});
    let provider = json!({});
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::AUTHORIZATION,
        http::HeaderValue::from_str(&format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD
                .encode("synthetic-user:synthetic-basic-password")
        ))?,
    );
    let credentials = ErrorCredentials::capture(&headers, budget.clone())
        .map_err(|_| "credential_capture_failed")?;
    let response = ProxyResponse::buffered(
        http::StatusCode::BAD_REQUEST,
        http::HeaderMap::new(),
        bytes::Bytes::from_static(b"synthetic-basic-password"),
    );
    let error = read_error(
        response,
        ErrorSafetyContext {
            runtime: &runtime,
            request: &request,
            provider: &provider,
            credentials: &[],
            sent_credentials: Some(&credentials),
            budget: budget.clone(),
        },
        std::time::Duration::from_secs(1),
    )
    .await;
    let bytes = error.to_response().into_body().collect().await?.to_bytes();
    let body: Value = serde_json::from_slice(&bytes)?;
    assert!(
        body["error"]["message"] == "[REDACTED]",
        "basic_credential_exposed"
    );
    drop(bytes);
    drop(error);
    drop(credentials);
    assert!(budget.used() == 0, "budget_owner_retained");
    Ok(())
}
#[tokio::test]
async fn upstream_error_passthrough_no_progress_stream_is_bounded() -> TestResult {
    use futures::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    let stream =
        futures::stream::iter((0..8192).map(|_| Ok::<_, std::io::Error>(bytes::Bytes::new())))
            .inspect(move |_| {
                observed.fetch_add(1, Ordering::Relaxed);
            });
    assert_unavailable(
        ProxyResponse::streamed(
            http::StatusCode::BAD_REQUEST,
            http::HeaderMap::new(),
            stream,
        ),
        16 * 1024 * 1024,
        std::time::Duration::from_secs(1),
    )
    .await?;
    assert!(
        polls.load(Ordering::Relaxed) <= 1025,
        "no_progress_work_unbounded"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_encoded_pii_without_source_plaintext_is_private() -> TestResult
{
    use base64::Engine;
    let private = "13900001234";
    for encoded in [
        base64::engine::general_purpose::STANDARD.encode(private),
        "%31%33%39%30%30%30%30%31%32%33%34".to_owned(),
    ] {
        let payload =
            json!({"error":{"message":"fixture-provider-reason","details":{"echo":encoded}}});
        let (_, body) = forwarded_error(ProxyResponse::buffered(
            http::StatusCode::BAD_REQUEST,
            http::HeaderMap::new(),
            bytes::Bytes::from(serde_json::to_vec(&payload)?),
        ))
        .await?;
        assert!(
            body["error"]["message"] == "fixture-provider-reason",
            "safe_reason_lost"
        );
        assert!(
            body["error"]["details"]["echo"] == "[REDACTED]",
            "encoded_pii_exposed"
        );
    }
    Ok(())
}

#[tokio::test]
async fn upstream_error_passthrough_review_private_item_and_part_keys() -> TestResult {
    for request in [
        json!({"input":[{"type":"message","role":"user","synthetic-extension-key":"value"}]}),
        json!({"input":[{"type":"message","role":"user","content":[{"type":"input_text","synthetic-extension-key":"value"}]}]}),
    ] {
        checked_fixture(request,json!({}),json!({"error":{"message":"fixture-provider-reason","details":{"echo":"synthetic-extension-key"}}})).await?;
    }
    Ok(())
}

#[tokio::test]
async fn upstream_error_passthrough_review_nested_unicode_escape() -> TestResult {
    checked_fixture(json!({"input":"synthetic-private-path"}),json!({}),
        json!({"error":{"message":"fixture-provider-reason","details":{"echo":r#"{"echo":"\u0073ynthetic-private-path"}"#}}})).await
}

#[tokio::test]
async fn upstream_error_passthrough_review_basic_whitespace() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        upstream_error::{read_error, ErrorCredentials, ErrorSafetyContext},
    };
    use base64::Engine;
    for separator in ["\t", "  "] {
        let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
        let runtime = runtime::SecurityRuntimeConfig::default();
        let request = json!({});
        let provider = json!({});
        let mut headers = http::HeaderMap::new();
        let encoded = base64::engine::general_purpose::STANDARD
            .encode("synthetic-user:synthetic-password-component");
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_str(&format!("Basic{separator}{encoded}"))?,
        );
        let credentials = ErrorCredentials::capture(&headers, budget.clone())
            .map_err(|_| "credential_capture_failed")?;
        let response = ProxyResponse::buffered(
            http::StatusCode::UNAUTHORIZED,
            http::HeaderMap::new(),
            bytes::Bytes::from_static(b"synthetic-password-component"),
        );
        let error = read_error(
            response,
            ErrorSafetyContext {
                runtime: &runtime,
                request: &request,
                provider: &provider,
                credentials: &[],
                sent_credentials: Some(&credentials),
                budget: budget.clone(),
            },
            std::time::Duration::from_secs(1),
        )
        .await;
        let bytes = error.to_response().into_body().collect().await?.to_bytes();
        let actual: Value = serde_json::from_slice(&bytes)?;
        assert!(
            actual["error"]["message"] == "[REDACTED]",
            "basic_component_exposed"
        );
        drop(bytes);
        drop(error);
        drop(credentials);
        assert!(budget.used() == 0, "budget_owner_retained");
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_reference_error_has_no_completion_authority() -> TestResult {
    use super::{payload_budget::PayloadBudget, tokenization::DlpTokenStore};
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    for malformed in [false, true] {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        let gateway = RestorationGateway::with_test_store_and_rules(
            store.clone(),
            &["chinese_phone_number"],
            budget,
        );
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "session-id",
            http::HeaderValue::from_static("synthetic-error-session"),
        );
        let mut body = json!({"model":"synthetic-model","input":"call 13900001234","stream":false});
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            &mut body,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic_admission_failed".into()),
        };
        let mut reference = String::new();
        super::tokenization::visit_token_references(
            body["input"].as_str().ok_or("input_missing")?,
            |range| {
                reference = body["input"]
                    .as_str()
                    .ok_or(super::tokenization::TokenError::RedactionFailed)?[range]
                    .to_owned();
                Ok(())
            },
        )?;
        assert!(!reference.is_empty(), "reference_missing");
        let mut echoed = reference.clone();
        if malformed {
            echoed.pop();
        }
        let provider = Provider::with_id(
            "synthetic-error".into(),
            "Synthetic error".into(),
            json!({"base_url":"http://127.0.0.1:1","api_key":"synthetic-error-credential"}),
            None,
        );
        let db = Arc::new(Database::memory()?);
        let forwarder = RequestForwarder::new(
            Arc::new(ProviderRouter::new(db.clone())),
            1,
            Arc::default(),
            Arc::default(),
            Arc::new(GeminiShadowStore::default()),
            Arc::new(CodexChatHistoryStore::default()),
            Arc::new(FailoverSwitchManager::new(db)),
            None,
            provider.id.clone(),
            "synthetic-error-session".into(),
            true,
            1,
            1,
            RectifierConfig::default(),
            OptimizerConfig::default(),
            CopilotOptimizerConfig::default(),
            1,
        )
        .with_dlp_request(Some(request));
        let payload = json!({"id":"synthetic-error-response","error":{"message":format!("fixture-provider-reason {echoed}")}});
        let transport = upstream_error_test_support::install(ProxyResponse::buffered(
            http::StatusCode::BAD_REQUEST,
            headers.clone(),
            bytes::Bytes::from(payload.to_string()),
        ));
        let failure = match forwarder
            .forward_with_retry(
                &AppType::Codex,
                http::Method::POST,
                "/v1/responses",
                body,
                headers,
                Default::default(),
                vec![provider.clone()],
            )
            .await
        {
            Err(failure) => failure,
            Ok(_) => return Err("synthetic_error_accepted".into()),
        };
        assert!(transport.attempts() == 1, "unexpected_retry");
        let bytes = failure
            .error
            .into_response()
            .into_body()
            .collect()
            .await?
            .to_bytes();
        let text = std::str::from_utf8(&bytes)?;
        assert!(!text.contains("rredact-"), "reference_exposed");
        assert!(!text.contains("13900001234"), "plaintext_exposed");
        for continuation in [
            json!({"model":"synthetic-model","previous_response_id":"synthetic-error-response","input":"continue"}),
            json!({"model":"synthetic-model","input":reference}),
        ] {
            let mut continuation = continuation;
            let mut continuation_headers = http::HeaderMap::new();
            continuation_headers.insert(
                "session-id",
                http::HeaderValue::from_static("synthetic-error-session"),
            );
            let admission = admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                &mut continuation,
                &continuation_headers,
            );
            if let Ok(RestorationAdmission::Admitted(request)) = admission {
                if continuation.get("previous_response_id").is_none() {
                    // Literal compatibility is not completed-response or restore authority.
                    let original = continuation.clone();
                    let attempt = request
                        .prepare_attempt(
                            &provider.id,
                            "openai_responses",
                            "http://127.0.0.1:1/v1/responses",
                            Some("synthetic-error-credential"),
                            &mut continuation,
                            |candidate| Ok(candidate.clone()),
                        )
                        .map_err(|_| "error_literal_rejected")?;
                    assert!(
                        continuation == original && !attempt.has_active_tokens(),
                        "error_published_authority"
                    );
                    assert!(
                        attempt.lease.resolve(&reference).is_err(),
                        "error_reference_resolved"
                    );
                    assert!(transport.attempts() == 1, "literal_probe_reached_transport");
                    continue;
                }
                let original = continuation.clone();
                let mut attempt = request
                    .prepare_attempt(
                        &provider.id,
                        "openai_responses",
                        "http://127.0.0.1:1/v1/responses",
                        Some("synthetic-error-credential"),
                        &mut continuation,
                        |candidate| Ok(candidate.clone()),
                    )
                    .map_err(|_| "error_cursor_preparation_rejected")?;
                assert!(
                    continuation == original && !attempt.has_active_tokens(),
                    "error_published_authority"
                );
                let mut resume_reason = None;
                assert_eq!(
                    attempt
                        .lease
                        .resume_completed_context_diagnosed(
                            &attempt.binding,
                            Some("synthetic-error-response"),
                            &[],
                            &[],
                            &mut resume_reason,
                        )
                        .err(),
                    Some(super::tokenization::TokenError::ContextUnavailable),
                    "error_cursor_gained_completion_authority"
                );
                drop(attempt);
                let continuation_transport =
                    upstream_error_test_support::install(ProxyResponse::buffered(
                        http::StatusCode::BAD_REQUEST,
                        continuation_headers.clone(),
                        bytes::Bytes::from(payload.to_string()),
                    ));
                let db = Arc::new(Database::memory()?);
                let next = RequestForwarder::new(
                    Arc::new(ProviderRouter::new(db.clone())),
                    1,
                    Arc::default(),
                    Arc::default(),
                    Arc::new(GeminiShadowStore::default()),
                    Arc::new(CodexChatHistoryStore::default()),
                    Arc::new(FailoverSwitchManager::new(db)),
                    None,
                    provider.id.clone(),
                    "synthetic-error-session".into(),
                    true,
                    1,
                    1,
                    RectifierConfig::default(),
                    OptimizerConfig::default(),
                    CopilotOptimizerConfig::default(),
                    1,
                )
                .with_dlp_request(Some(request));
                let denied = match next
                    .forward_with_retry(
                        &AppType::Codex,
                        http::Method::POST,
                        "/v1/responses",
                        continuation,
                        continuation_headers,
                        Default::default(),
                        vec![provider.clone()],
                    )
                    .await
                {
                    Err(failure) => failure,
                    Ok(_) => return Err("error_published_authority".into()),
                };
                assert_eq!(
                    denied.error.into_response().status(),
                    http::StatusCode::BAD_REQUEST,
                    "continuation must preserve the real upstream failure"
                );
                assert_eq!(
                    continuation_transport.attempts(),
                    1,
                    "missing error completion must not block the continuation"
                );
            }
        }
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_skipped_circuit_keeps_actual_provider() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let db = Arc::new(Database::memory()?);
    let router = Arc::new(ProviderRouter::new(db.clone()));
    let skipped = Provider::with_id(
        "synthetic-skipped".into(),
        "Synthetic skipped".into(),
        json!({"base_url":"http://127.0.0.1:1","api_key":"synthetic-credential"}),
        None,
    );
    let mut actual = skipped.clone();
    actual.id = "synthetic-actual".into();
    db.save_provider("codex", &skipped)?;
    db.save_provider("codex", &actual)?;
    let _ = router.allow_provider_request(&skipped.id, "codex").await;
    router
        .update_app_configs(
            "codex",
            crate::proxy::circuit_breaker::CircuitBreakerConfig {
                failure_threshold: 1,
                timeout_seconds: 3600,
                ..Default::default()
            },
        )
        .await;
    router
        .record_result(&skipped.id, "codex", false, false, None)
        .await?;
    assert!(
        !router
            .allow_provider_request(&skipped.id, "codex")
            .await
            .allowed,
        "circuit_not_open"
    );
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let mut body = json!({"model":"synthetic-model","input":"call 13900001234"});
    let request = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut body,
        &Default::default(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic_admission_failed".into()),
    };
    let forwarder = RequestForwarder::new(
        router,
        5,
        Arc::default(),
        Arc::default(),
        Arc::new(GeminiShadowStore::default()),
        Arc::new(CodexChatHistoryStore::default()),
        Arc::new(FailoverSwitchManager::new(db)),
        None,
        skipped.id.clone(),
        "synthetic-error-session".into(),
        true,
        1,
        1,
        RectifierConfig::default(),
        OptimizerConfig::default(),
        CopilotOptimizerConfig::default(),
        1,
    )
    .with_dlp_request(Some(request));
    let transport = upstream_error_test_support::install(ProxyResponse::buffered(
        http::StatusCode::SERVICE_UNAVAILABLE,
        http::HeaderMap::new(),
        bytes::Bytes::from_static(b"fixture-provider-reason"),
    ));
    let failure = match forwarder
        .forward_with_retry(
            &AppType::Codex,
            http::Method::POST,
            "/v1/responses",
            body,
            Default::default(),
            Default::default(),
            vec![skipped, actual],
        )
        .await
    {
        Err(failure) => failure,
        Ok(_) => return Err("synthetic_error_accepted".into()),
    };
    assert!(transport.attempts() == 1, "unexpected_retry");
    assert!(
        failure
            .provider
            .as_ref()
            .is_some_and(|p| p.id == "synthetic-actual"),
        "attempted_provider_identity_lost"
    );
    assert!(
        matches!(
            failure.error,
            crate::proxy::error::ProxyError::SafeUpstream(_)
        ),
        "unsafe_error_carrier"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn upstream_error_passthrough_review_sent_credential_json_slash_escape() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        upstream_error::{read_error, ErrorCredentials, ErrorSafetyContext},
    };
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let runtime = runtime::SecurityRuntimeConfig::default();
    let request = json!({});
    let provider = json!({});
    let secret = "syntheticSent/segmentAlpha/segmentBeta";
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::AUTHORIZATION,
        http::HeaderValue::from_str(&format!("Bearer {secret}"))?,
    );
    let credentials = ErrorCredentials::capture(&headers, budget.clone())
        .map_err(|_| "credential_capture_failed")?;
    let echo = format!(r#"{{"echo":"{}"}}"#, secret.replace('/', r"\/"));
    let payload = json!({"error":{"message":"fixture-provider-reason","details":{"echo":echo}}});
    let error = read_error(
        ProxyResponse::buffered(
            http::StatusCode::UNAUTHORIZED,
            http::HeaderMap::new(),
            bytes::Bytes::from(serde_json::to_vec(&payload)?),
        ),
        ErrorSafetyContext {
            runtime: &runtime,
            request: &request,
            provider: &provider,
            credentials: &[],
            sent_credentials: Some(&credentials),
            budget: budget.clone(),
        },
        std::time::Duration::from_secs(1),
    )
    .await;
    let response = error.to_response();
    assert!(
        response.status() == http::StatusCode::UNAUTHORIZED,
        "upstream_status_changed"
    );
    let bytes = response.into_body().collect().await?.to_bytes();
    let actual: Value = serde_json::from_slice(&bytes)?;
    assert!(
        actual["error"]["message"] == "fixture-provider-reason",
        "safe_reason_lost"
    );
    assert!(
        actual["error"]["details"]["echo"] == "[REDACTED]",
        "sent_credential_json_escape_exposed"
    );
    drop(bytes);
    drop(error);
    drop(credentials);
    assert!(budget.used() == 0, "budget_owner_retained");
    let (status, actual) = forwarded_error_with_auth(
        ProxyResponse::buffered(
            http::StatusCode::UNAUTHORIZED,
            http::HeaderMap::new(),
            bytes::Bytes::from(serde_json::to_vec(&payload)?),
        ),
        true,
        false,
        headers.get(http::header::AUTHORIZATION).cloned(),
    )
    .await?;
    assert!(
        status == http::StatusCode::UNAUTHORIZED,
        "upstream_status_changed"
    );
    assert!(
        actual["error"]["message"] == "fixture-provider-reason",
        "safe_reason_lost"
    );
    assert!(
        actual["error"]["details"]["echo"] == "[REDACTED]",
        "sent_credential_json_escape_exposed"
    );
    Ok(())
}
