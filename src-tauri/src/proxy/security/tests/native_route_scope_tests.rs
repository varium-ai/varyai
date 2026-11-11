//! Trusted route admission controls; only integrated direct-native cells are admitted.
use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use super::{
    http_restoration::{
        admit_http_request, admit_http_request_with_scope, route_scope_test_support,
        RestorationAdmission, RestoreUnsupportedCause, CLIENT_PROCESSING_ERROR_CODE,
        CLIENT_PROCESSING_ERROR_MESSAGE, CLIENT_UNSUPPORTED_ENDPOINT_CODE,
    },
    unified_test_support::{
        decode_json, prepare_attempt, ResponsesFixture, TestResult, CONTACT, TARGET,
    },
};
use crate::{
    app_config::AppType,
    database::Database,
    provider::Provider,
    proxy::{
        failover_switch::FailoverSwitchManager,
        gateway::{self, KeyPolicy, Period},
        handler_context::{
            RequestIngress, RestorationFraming, RestorationOperation, RestorationRouteScope,
        },
        handlers,
        protocol_bridge::Protocol,
        provider_router::ProviderRouter,
        providers::{codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore},
        server::ProxyState,
        types::{ProxyConfig, ProxyStatus},
    },
};

fn scope(
    client: Protocol,
    ingress: RequestIngress,
    operation: RestorationOperation,
    framing: RestorationFraming,
) -> RestorationRouteScope {
    RestorationRouteScope::new(client, ingress, operation, framing)
}

async fn assert_fixed_scope_error(admission: RestorationAdmission, app: &AppType) -> TestResult {
    let RestorationAdmission::Intercepted(response) = admission else {
        return Err("c4_disabled_scope_was_not_intercepted".into());
    };
    let (status, body) = decode_json(response).await?;
    // Only two sanctioned projections exist, and each must pair its own status, code and
    // message. An actionable route/framing refusal must never be reported as a generic
    // processing failure, and vice versa.
    let message = body
        .pointer("/error/message")
        .and_then(Value::as_str)
        .ok_or("c4_scope_error_message_missing")?;
    assert!(
        !message.contains(CONTACT) && !message.contains("rredact-"),
        "c4_scope_error_leaked_content"
    );
    match app {
        AppType::Gemini => {
            let code = body.pointer("/error/code").and_then(Value::as_u64);
            let state = body.pointer("/error/status").and_then(Value::as_str);
            if message == RestoreUnsupportedCause::UnsupportedEndpoint.client_message() {
                assert_eq!(status, StatusCode::BAD_REQUEST, "c4_gemini_cause_status");
                assert_eq!(code, Some(400), "c4_gemini_cause_code");
                assert_eq!(state, Some("INVALID_ARGUMENT"), "c4_gemini_cause_state");
            } else {
                assert_eq!(message, CLIENT_PROCESSING_ERROR_MESSAGE);
                assert_eq!(
                    status,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "c4_gemini_generic_status"
                );
                assert_eq!(code, Some(500), "c4_gemini_generic_code");
                assert_eq!(state, Some("UNAVAILABLE"), "c4_gemini_generic_state");
            }
        }
        _ => {
            let code = body.pointer("/error/code").and_then(Value::as_str);
            let kind = body.pointer("/error/type").and_then(Value::as_str);
            if message == RestoreUnsupportedCause::UnsupportedEndpoint.client_message() {
                assert_eq!(status, StatusCode::BAD_REQUEST, "c4_cause_status");
                assert_eq!(
                    code,
                    Some(CLIENT_UNSUPPORTED_ENDPOINT_CODE),
                    "c4_cause_code"
                );
                assert_eq!(kind, Some("invalid_request_error"), "c4_cause_type");
            } else {
                assert_eq!(message, CLIENT_PROCESSING_ERROR_MESSAGE);
                assert_eq!(
                    status,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "c4_generic_status"
                );
                assert_eq!(code, Some(CLIENT_PROCESSING_ERROR_CODE), "c4_generic_code");
            }
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn legacy_responses_and_compact_facade_preserve_zero_mapping_attempts() -> TestResult {
    let fixture = ResponsesFixture::new();
    for compact in [false, true] {
        let mut body = json!({"input":"Synthetic ordinary request","extension":{"keep":true}});
        let original = body.clone();
        let admission = admit_http_request(
            &fixture.gateway,
            &AppType::Codex,
            true,
            compact,
            false,
            &mut body,
            &HeaderMap::new(),
        )?;
        let RestorationAdmission::Admitted(request) = admission else {
            return Err("c4_legacy_responses_admission_changed".into());
        };
        let attempt = prepare_attempt(
            &request,
            &mut body,
            "synthetic-scope-provider",
            if compact {
                "https://synthetic.invalid/v1/responses/compact"
            } else {
                TARGET
            },
        )?;
        assert!(!attempt.has_active_tokens(), "c4_zero_mapping_changed");
        assert_eq!(attempt.endpoint_is_compact, compact);
        assert!(body == original, "c4_legacy_request_changed");
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn explicit_responses_and_compact_preserve_direct_and_gateway_admission() -> TestResult {
    let fixture = ResponsesFixture::new();
    for ingress in [RequestIngress::Direct, RequestIngress::Gateway] {
        for (operation, framing, compact) in [
            (
                RestorationOperation::Responses,
                RestorationFraming::Json,
                false,
            ),
            (
                RestorationOperation::Responses,
                RestorationFraming::Sse,
                false,
            ),
            (
                RestorationOperation::ResponsesCompact,
                RestorationFraming::Json,
                true,
            ),
        ] {
            let mut body = json!({"input":"Synthetic ordinary request"});
            let original = body.clone();
            let admission = admit_http_request_with_scope(
                &fixture.gateway,
                &AppType::Codex,
                scope(Protocol::Responses, ingress, operation, framing),
                &mut body,
                &HeaderMap::new(),
            )?;
            let RestorationAdmission::Admitted(request) = admission else {
                return Err("c4_existing_responses_scope_lost".into());
            };
            let attempt = prepare_attempt(
                &request,
                &mut body,
                "synthetic-scope-provider",
                if compact {
                    "https://synthetic.invalid/v1/responses/compact"
                } else {
                    TARGET
                },
            )?;
            assert!(!attempt.has_active_tokens(), "c4_scope_created_mapping");
            assert_eq!(attempt.endpoint_is_compact, compact);
            assert!(body == original, "c4_responses_scope_changed_request");
        }
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn native_scopes_admit_only_integrated_direct_protocols() -> TestResult {
    let fixture = ResponsesFixture::new();
    for ingress in [RequestIngress::Direct, RequestIngress::Gateway] {
        for framing in [RestorationFraming::Json, RestorationFraming::Sse] {
            for (app, client, operation, mut body) in [
                (
                    AppType::Codex,
                    Protocol::Chat,
                    RestorationOperation::ChatCompletions,
                    json!({"messages":[{"role":"user","content":CONTACT}]}),
                ),
                (
                    AppType::Claude,
                    Protocol::Anthropic,
                    RestorationOperation::Messages,
                    json!({"messages":[{"role":"user","content":CONTACT}]}),
                ),
                (
                    AppType::ClaudeDesktop,
                    Protocol::Anthropic,
                    RestorationOperation::Messages,
                    json!({"messages":[{"role":"user","content":CONTACT}]}),
                ),
                (
                    AppType::Gemini,
                    Protocol::Gemini,
                    if framing == RestorationFraming::Sse {
                        RestorationOperation::StreamGenerateContent
                    } else {
                        RestorationOperation::GenerateContent
                    },
                    json!({"contents":[{"role":"user","parts":[{"text":CONTACT}]}]}),
                ),
            ] {
                let original = body.clone();
                let admission = admit_http_request_with_scope(
                    &fixture.gateway,
                    &app,
                    scope(client, ingress, operation, framing),
                    &mut body,
                    &HeaderMap::new(),
                )?;
                if matches!(
                    client,
                    Protocol::Chat | Protocol::Anthropic | Protocol::Gemini
                ) && ingress == RequestIngress::Direct
                {
                    let RestorationAdmission::Admitted(request) = admission else {
                        return Err("m2_direct_chat_scope_not_admitted".into());
                    };
                    assert!(
                        (client == Protocol::Chat && request.is_native_chat())
                            || (client == Protocol::Anthropic && request.is_native_anthropic())
                            || (client == Protocol::Gemini && request.is_native_gemini()),
                        "m3_native_protocol_borrowed_other_authority"
                    );
                } else {
                    assert_fixed_scope_error(admission, &app).await?;
                }
                assert!(body == original, "c4_native_admission_request_mutated");
                assert_eq!(fixture.store.stats()?.entries, 0);
            }
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn ordinary_native_input_admits_only_integrated_direct_protocols() -> TestResult {
    let fixture = ResponsesFixture::new();
    for ingress in [RequestIngress::Direct, RequestIngress::Gateway] {
        for (app, client, operation, mut body) in [
            (
                AppType::Codex,
                Protocol::Chat,
                RestorationOperation::ChatCompletions,
                json!({"messages":[{"role":"user","content":"Synthetic ordinary input"}]}),
            ),
            (
                AppType::Claude,
                Protocol::Anthropic,
                RestorationOperation::Messages,
                json!({"messages":[{"role":"user","content":"Synthetic ordinary input"}]}),
            ),
            (
                AppType::ClaudeDesktop,
                Protocol::Anthropic,
                RestorationOperation::Messages,
                json!({"messages":[{"role":"user","content":"Synthetic ordinary input"}]}),
            ),
            (
                AppType::Gemini,
                Protocol::Gemini,
                RestorationOperation::GenerateContent,
                json!({"contents":[{"role":"user","parts":[{"text":"Synthetic ordinary input"}]}]}),
            ),
        ] {
            let original = body.clone();
            let admission = admit_http_request_with_scope(
                &fixture.gateway,
                &app,
                scope(client, ingress, operation, RestorationFraming::Json),
                &mut body,
                &HeaderMap::new(),
            )?;
            if matches!(
                client,
                Protocol::Chat | Protocol::Anthropic | Protocol::Gemini
            ) && ingress == RequestIngress::Direct
            {
                let RestorationAdmission::Admitted(request) = admission else {
                    return Err("m2_ordinary_direct_chat_not_admitted".into());
                };
                assert!(
                    (client == Protocol::Chat && request.is_native_chat())
                        || (client == Protocol::Anthropic && request.is_native_anthropic())
                        || (client == Protocol::Gemini && request.is_native_gemini()),
                    "m3_ordinary_native_borrowed_other_authority"
                );
            } else {
                assert!(
                    matches!(admission, RestorationAdmission::Passthrough),
                    "c4_ordinary_unsupported_native_gained_authority_or_failed"
                );
            }
            assert!(body == original, "c4_ordinary_native_input_changed");
            assert_eq!(fixture.store.stats()?.entries, 0);
        }
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn forged_ingress_headers_cannot_enable_gateway_native_admission() -> TestResult {
    let fixture = ResponsesFixture::new();
    for claimed_ingress in ["direct", "gateway", "responses"] {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-varyai-ingress",
            HeaderValue::from_static(claimed_ingress),
        );
        headers.insert(
            "x-varyai-client-protocol",
            HeaderValue::from_static("openai_responses"),
        );
        let mut body = json!({"messages":[{"role":"user","content":CONTACT}]});
        let original = body.clone();
        let admission = admit_http_request_with_scope(
            &fixture.gateway,
            &AppType::Codex,
            scope(
                Protocol::Chat,
                RequestIngress::Gateway,
                RestorationOperation::ChatCompletions,
                RestorationFraming::Json,
            ),
            &mut body,
            &headers,
        )?;
        assert_fixed_scope_error(admission, &AppType::Codex).await?;
        assert!(body == original, "c4_forged_header_changed_native_input");
        assert_eq!(fixture.store.stats()?.entries, 0);
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn forged_ingress_headers_do_not_reclassify_legacy_responses() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut headers = HeaderMap::new();
    headers.insert("x-varyai-ingress", HeaderValue::from_static("gateway"));
    headers.insert(
        "x-varyai-client-protocol",
        HeaderValue::from_static("gemini_native"),
    );
    let mut body = json!({"input":"Synthetic ordinary input"});
    let admission = admit_http_request_with_scope(
        &fixture.gateway,
        &AppType::Codex,
        scope(
            Protocol::Responses,
            RequestIngress::Direct,
            RestorationOperation::Responses,
            RestorationFraming::Json,
        ),
        &mut body,
        &headers,
    )?;
    assert!(
        matches!(admission, RestorationAdmission::Admitted(_)),
        "c4_forged_header_reclassified_responses"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn explicit_chat_client_cannot_borrow_codex_responses_authority() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":CONTACT});
    let original = body.clone();
    let admission = admit_http_request_with_scope(
        &fixture.gateway,
        &AppType::Codex,
        scope(
            Protocol::Chat,
            RequestIngress::Direct,
            RestorationOperation::Responses,
            RestorationFraming::Json,
        ),
        &mut body,
        &HeaderMap::new(),
    )?;
    assert!(
        matches!(&admission, RestorationAdmission::Intercepted(_)),
        "c4_explicit_chat_client_received_responses_authority"
    );
    assert_fixed_scope_error(admission, &AppType::Codex).await?;
    assert!(body == original, "c4_wrong_client_request_mutated");
    assert_eq!(fixture.store.stats()?.entries, 0);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn compact_sse_scope_cannot_borrow_json_authority() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":CONTACT,"stream":true});
    let original = body.clone();
    let admission = admit_http_request_with_scope(
        &fixture.gateway,
        &AppType::Codex,
        scope(
            Protocol::Responses,
            RequestIngress::Direct,
            RestorationOperation::ResponsesCompact,
            RestorationFraming::Sse,
        ),
        &mut body,
        &HeaderMap::new(),
    )?;
    assert!(
        matches!(&admission, RestorationAdmission::Intercepted(_)),
        "c4_compact_sse_received_json_authority"
    );
    assert_fixed_scope_error(admission, &AppType::Codex).await?;
    assert!(body == original, "c4_unsupported_framing_request_mutated");
    assert_eq!(fixture.store.stats()?.entries, 0);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn codex_attribution_cannot_enable_anthropic_or_gemini_clients() -> TestResult {
    let fixture = ResponsesFixture::new();
    for (client, operation, mut body) in [
        (
            Protocol::Anthropic,
            RestorationOperation::Messages,
            json!({"messages":[{"role":"user","content":CONTACT}]}),
        ),
        (
            Protocol::Gemini,
            RestorationOperation::GenerateContent,
            json!({"contents":[{"role":"user","parts":[{"text":CONTACT}]}]}),
        ),
    ] {
        let original = body.clone();
        let admission = admit_http_request_with_scope(
            &fixture.gateway,
            &AppType::Codex,
            scope(
                client,
                RequestIngress::Direct,
                operation,
                RestorationFraming::Json,
            ),
            &mut body,
            &HeaderMap::new(),
        )?;
        assert_fixed_scope_error(admission, &AppType::Codex).await?;
        assert!(body == original, "c4_app_attribution_rewrote_native_input");
        assert_eq!(fixture.store.stats()?.entries, 0);
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn gemini_unsupported_operation_and_framing_refuse_required_restoration() -> TestResult {
    let fixture = ResponsesFixture::new();
    for (operation, framing) in [
        (RestorationOperation::Unsupported, RestorationFraming::Json),
        (
            RestorationOperation::GenerateContent,
            RestorationFraming::Sse,
        ),
        (
            RestorationOperation::StreamGenerateContent,
            RestorationFraming::Json,
        ),
        (
            RestorationOperation::StreamGenerateContent,
            RestorationFraming::Unsupported,
        ),
    ] {
        let mut body = json!({"contents":[{"role":"user","parts":[{"text":CONTACT}]}]});
        let original = body.clone();
        let admission = admit_http_request_with_scope(
            &fixture.gateway,
            &AppType::Gemini,
            scope(Protocol::Gemini, RequestIngress::Direct, operation, framing),
            &mut body,
            &HeaderMap::new(),
        )?;
        assert_fixed_scope_error(admission, &AppType::Gemini).await?;
        assert!(body == original, "c4_unsupported_gemini_input_mutated");
        assert_eq!(fixture.store.stats()?.entries, 0);
    }
    Ok(())
}

fn dispatch_state(db: Arc<Database>, fixture: &ResponsesFixture) -> ProxyState {
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

fn gateway_dispatch_fixture(fixture: &ResponsesFixture) -> TestResult<(ProxyState, String)> {
    let db = Arc::new(Database::memory()?);
    let created = db.create_gateway_key(KeyPolicy {
        name: "Synthetic C4 dispatch".into(),
        models: vec!["synthetic-scope-model".into()],
        all_models: false,
        token_limit: None,
        period: Period::Never,
    })?;
    Ok((dispatch_state(db, fixture), created.secret))
}

fn dispatch_request(method: &str, uri: &str, body: &Value) -> TestResult<axum::extract::Request> {
    Ok(axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("session-id", "synthetic-c4-scope-session")
        .header("x-session-id", "synthetic-c4-gemini-session")
        // Forged hints must not replace the trusted handler-owned classification.
        .header("x-varyai-ingress", "direct")
        .header("x-varyai-client-protocol", "openai_responses")
        .body(Body::from(serde_json::to_vec(body)?))?)
}

fn assert_recorded_scope(
    observation: &route_scope_test_support::Observation,
    expected: RestorationRouteScope,
) {
    assert_eq!(
        observation.recorded(),
        Some((
            expected.client,
            expected.ingress,
            expected.operation,
            expected.framing
        )),
        "c4_real_dispatch_scope_mismatch"
    );
}

fn assert_gemini_classification(
    method: &str,
    uri: &str,
    operation: RestorationOperation,
    framing: RestorationFraming,
) -> TestResult {
    for ingress in [RequestIngress::Direct, RequestIngress::Gateway] {
        let classified =
            RestorationRouteScope::gemini_endpoint(ingress, &method.parse()?, &uri.parse()?);
        assert_eq!(
            classified.client,
            Protocol::Gemini,
            "c4_gemini_client_changed"
        );
        assert_eq!(classified.ingress, ingress, "c4_gemini_ingress_changed");
        assert_eq!(
            classified.operation, operation,
            "c4_gemini_operation_mismatch"
        );
        assert_eq!(classified.framing, framing, "c4_gemini_framing_mismatch");
    }
    Ok(())
}

#[test]
fn c4_classifier_gemini_registered_routes_are_exact() -> TestResult {
    for prefix in [
        "/v1beta/models/",
        "/gemini/v1beta/models/",
        "/gemini/v1/models/",
    ] {
        for (suffix, operation, framing) in [
            (
                "generateContent",
                RestorationOperation::GenerateContent,
                RestorationFraming::Json,
            ),
            (
                "streamGenerateContent?alt=sse",
                RestorationOperation::StreamGenerateContent,
                RestorationFraming::Sse,
            ),
        ] {
            assert_gemini_classification(
                "POST",
                &format!("{prefix}synthetic-scope-model:{suffix}"),
                operation,
                framing,
            )?;
        }
    }
    for (method, uri) in [
        (
            "GET",
            "/v1beta/models/synthetic-scope-model:generateContent",
        ),
        (
            "PUT",
            "/v1beta/models/synthetic-scope-model:generateContent",
        ),
        (
            "DELETE",
            "/v1beta/models/synthetic-scope-model:generateContent",
        ),
        ("POST", "/v1/models/synthetic-scope-model:generateContent"),
        (
            "POST",
            "/gateway/v1beta/models/synthetic-scope-model:generateContent",
        ),
        ("POST", "/v1beta/models/:generateContent"),
        (
            "POST",
            "/v1beta/models/nested/synthetic-scope-model:generateContent",
        ),
        (
            "POST",
            "/v1beta/models/synthetic-scope-model:generateContent/extra",
        ),
        ("POST", "/v1beta/models/synthetic-scope-model:countTokens"),
        ("GET", "/v1beta/models"),
    ] {
        assert_gemini_classification(
            method,
            uri,
            RestorationOperation::Unsupported,
            RestorationFraming::Unsupported,
        )?;
    }
    Ok(())
}

#[test]
fn c4_classifier_gemini_generate_content_rejects_sse_alt() -> TestResult {
    assert_gemini_classification(
        "POST",
        "/v1beta/models/synthetic-scope-model:generateContent?alt=sse",
        RestorationOperation::GenerateContent,
        RestorationFraming::Unsupported,
    )
}

#[test]
fn c4_classifier_gemini_streaming_rejects_duplicate_or_malformed_alt() -> TestResult {
    for query in [
        "alt=sse&alt",
        "alt&alt=sse",
        "alt=sse&alt=sse",
        "alt=json&alt=sse",
        "alt=sse&alt=",
        "alt",
        "alt=",
        "alt=json",
        "alt=SSE",
        "alt=sse=extra",
        "alt=%73se",
        "",
    ] {
        assert_gemini_classification(
            "POST",
            &format!("/v1beta/models/synthetic-scope-model:streamGenerateContent?{query}"),
            RestorationOperation::StreamGenerateContent,
            RestorationFraming::Unsupported,
        )?;
    }
    assert_gemini_classification(
        "POST",
        "/v1beta/models/synthetic-scope-model:streamGenerateContent",
        RestorationOperation::StreamGenerateContent,
        RestorationFraming::Unsupported,
    )
}

#[test]
fn c4_classifier_gemini_streaming_allows_unrelated_query_options() -> TestResult {
    for query in [
        "alt=sse&synthetic=option",
        "synthetic=option&alt=sse",
        "synthetic&alt=sse",
    ] {
        assert_gemini_classification(
            "POST",
            &format!("/v1beta/models/synthetic-scope-model:streamGenerateContent?{query}"),
            RestorationOperation::StreamGenerateContent,
            RestorationFraming::Sse,
        )?;
    }
    assert_gemini_classification(
        "POST",
        "/v1beta/models/synthetic-scope-model:generateContent?synthetic=option",
        RestorationOperation::GenerateContent,
        RestorationFraming::Json,
    )
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn c4_dispatch_direct_native_wrappers_keep_direct_scope() -> TestResult {
    let fixture = ResponsesFixture::new();
    let state = dispatch_state(Arc::new(Database::memory()?), &fixture);
    for stream in [false, true] {
        for (_app, client, operation, uri) in [
            (
                AppType::Codex,
                Protocol::Chat,
                RestorationOperation::ChatCompletions,
                "/v1/chat/completions",
            ),
            (
                AppType::Claude,
                Protocol::Anthropic,
                RestorationOperation::Messages,
                "/v1/messages",
            ),
        ] {
            let baseline = fixture.store.stats()?;
            let before_publication =
                super::tokenization::publication_test_support::counts(&fixture.store)?;
            let body = json!({"model":"synthetic-scope-model", "stream":stream,
                "messages":[{"role":"user","content":CONTACT}]});
            let mut request = dispatch_request("POST", uri, &body)?;
            request
                .headers_mut()
                .insert("x-varyai-ingress", HeaderValue::from_static("gateway"));
            let observation = route_scope_test_support::observe();
            let response = match client {
                Protocol::Chat => {
                    handlers::handle_chat_completions(State(state.clone()), request).await
                }
                _ => handlers::handle_messages(State(state.clone()), request).await,
            };
            assert!(
                matches!(
                    response,
                    Err(crate::proxy::error::ProxyError::NoProvidersConfigured)
                ),
                "m3_integrated_direct_native_did_not_reach_empty_provider_selection"
            );
            assert_recorded_scope(
                &observation,
                scope(
                    client,
                    RequestIngress::Direct,
                    operation,
                    if stream {
                        RestorationFraming::Sse
                    } else {
                        RestorationFraming::Json
                    },
                ),
            );
            let after = fixture.store.stats()?;
            if matches!(client, Protocol::Chat | Protocol::Anthropic) {
                assert!(
                    after.entries == baseline.entries
                        && after.plaintext_bytes == baseline.plaintext_bytes,
                    "m2_admission_without_provider_created_mapping"
                );
                assert!(
                    super::tokenization::publication_test_support::counts(&fixture.store)?
                        == before_publication,
                    "m2_admission_without_provider_published_history"
                );
            } else {
                assert_eq!(after, baseline, "c4_later_native_gained_namespace");
            }
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn c4_dispatch_gateway_native_wrappers_use_gateway_scope() -> TestResult {
    let fixture = ResponsesFixture::new();
    let (state, secret) = gateway_dispatch_fixture(&fixture)?;
    let baseline = fixture.store.stats()?;
    for stream in [false, true] {
        for (app, client, operation, uri) in [
            (
                AppType::Codex,
                Protocol::Chat,
                RestorationOperation::ChatCompletions,
                "/gateway/v1/chat/completions",
            ),
            (
                AppType::Claude,
                Protocol::Anthropic,
                RestorationOperation::Messages,
                "/gateway/v1/messages",
            ),
        ] {
            let body = json!({"model":"synthetic-scope-model", "stream":stream,
                "messages":[{"role":"user","content":CONTACT}]});
            let mut request = dispatch_request("POST", uri, &body)?;
            request.headers_mut().insert("x-api-key", secret.parse()?);
            let observation = route_scope_test_support::observe();
            let response = gateway::handle(State(state.clone()), request).await;
            assert_fixed_scope_error(RestorationAdmission::Intercepted(response), &app).await?;
            assert_recorded_scope(
                &observation,
                scope(
                    client,
                    RequestIngress::Gateway,
                    operation,
                    if stream {
                        RestorationFraming::Sse
                    } else {
                        RestorationFraming::Json
                    },
                ),
            );
            assert_eq!(
                fixture.store.stats()?,
                baseline,
                "c4_gateway_native_gained_namespace"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn c4_dispatch_gateway_gemini_generation_uses_gateway_scope() -> TestResult {
    let fixture = ResponsesFixture::new();
    let (state, secret) = gateway_dispatch_fixture(&fixture)?;
    let baseline = fixture.store.stats()?;
    for (suffix, operation, framing) in [
        (
            "generateContent",
            RestorationOperation::GenerateContent,
            RestorationFraming::Json,
        ),
        (
            "streamGenerateContent?alt=sse",
            RestorationOperation::StreamGenerateContent,
            RestorationFraming::Sse,
        ),
    ] {
        let uri = format!("/gateway/v1beta/models/synthetic-scope-model:{suffix}");
        let body = json!({"contents":[{"role":"user","parts":[{"text":CONTACT}]}]});
        let mut request = dispatch_request("POST", &uri, &body)?;
        request.headers_mut().insert("x-api-key", secret.parse()?);
        let observation = route_scope_test_support::observe();
        let response = gateway::handle(State(state.clone()), request).await;
        assert_fixed_scope_error(
            RestorationAdmission::Intercepted(response),
            &AppType::Gemini,
        )
        .await?;
        assert_recorded_scope(
            &observation,
            scope(
                Protocol::Gemini,
                RequestIngress::Gateway,
                operation,
                framing,
            ),
        );
        assert_eq!(
            fixture.store.stats()?,
            baseline,
            "c4_gateway_gemini_gained_namespace"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn c4_dispatch_gemini_registered_generation_is_admitted_natively() -> TestResult {
    let fixture = ResponsesFixture::new();
    let state = dispatch_state(Arc::new(Database::memory()?), &fixture);
    let baseline = fixture.store.stats()?;
    for prefix in [
        "/v1beta/models/",
        "/gemini/v1beta/models/",
        "/gemini/v1/models/",
    ] {
        for (suffix, operation, framing, body_stream) in [
            (
                "generateContent",
                RestorationOperation::GenerateContent,
                RestorationFraming::Json,
                true,
            ),
            (
                "streamGenerateContent?alt=sse",
                RestorationOperation::StreamGenerateContent,
                RestorationFraming::Sse,
                false,
            ),
        ] {
            let uri = format!("{prefix}synthetic-scope-model:{suffix}");
            // Gemini framing is URI-owned, not borrowed from Chat's stream flag.
            let body = json!({"stream":body_stream,"contents":[{"role":"user","parts":[{"text":CONTACT}]}]});
            let request = dispatch_request("POST", &uri, &body)?;
            let observation = route_scope_test_support::observe();
            // M4 admits these canonical generation pairs natively; only DLP refusal
            // would keep the request out of provider selection.
            // The fixture configures no provider, so native admission is proven by the
            // handler reaching that later provider-selection refusal instead of DLP's.
            match handlers::handle_gemini(State(state.clone()), uri.parse()?, request).await {
                Err(crate::proxy::error::ProxyError::NoProvidersConfigured) => {}
                Err(crate::proxy::error::ProxyError::DlpForward(_)) => {
                    return Err("c4_direct_gemini_refused_by_dlp".into())
                }
                _ => return Err("c4_direct_gemini_unexpected_outcome".into()),
            }
            assert_recorded_scope(
                &observation,
                scope(Protocol::Gemini, RequestIngress::Direct, operation, framing),
            );
            assert_eq!(
                fixture.store.stats()?.entries,
                baseline.entries,
                "c4_direct_gemini_published_without_tokens"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn c4_dispatch_gemini_unsupported_method_operation_and_framing_stays_disabled() -> TestResult
{
    let fixture = ResponsesFixture::new();
    let state = dispatch_state(Arc::new(Database::memory()?), &fixture);
    let baseline = fixture.store.stats()?;
    for (method, uri, operation, framing) in [
        (
            "GET",
            "/v1beta/models/synthetic-scope-model:generateContent",
            RestorationOperation::Unsupported,
            RestorationFraming::Unsupported,
        ),
        (
            "POST",
            "/v1beta/models/synthetic-scope-model:countTokens",
            RestorationOperation::Unsupported,
            RestorationFraming::Unsupported,
        ),
        (
            "POST",
            "/v1beta/models/synthetic-scope-model:streamGenerateContent",
            RestorationOperation::StreamGenerateContent,
            RestorationFraming::Unsupported,
        ),
        (
            "POST",
            "/v1beta/models/synthetic-scope-model:streamGenerateContent?alt=sse&alt",
            RestorationOperation::StreamGenerateContent,
            RestorationFraming::Unsupported,
        ),
        (
            "POST",
            "/v1beta/models/synthetic-scope-model:generateContent?alt=sse",
            RestorationOperation::GenerateContent,
            RestorationFraming::Unsupported,
        ),
    ] {
        let body = json!({"stream":true,"contents":[{"role":"user","parts":[{"text":CONTACT}]}]});
        let request = dispatch_request(method, uri, &body)?;
        let observation = route_scope_test_support::observe();
        let response = handlers::handle_gemini(State(state.clone()), uri.parse()?, request)
            .await
            .map_err(|_| "c4_unsupported_gemini_reached_provider_selection")?;
        assert_fixed_scope_error(
            RestorationAdmission::Intercepted(response),
            &AppType::Gemini,
        )
        .await?;
        assert_recorded_scope(
            &observation,
            scope(Protocol::Gemini, RequestIngress::Direct, operation, framing),
        );
        assert_eq!(
            fixture.store.stats()?,
            baseline,
            "c4_unsupported_gemini_gained_namespace"
        );
    }
    Ok(())
}

#[derive(Clone, Default)]
struct ResponsesDispatchCapture(Arc<Mutex<Vec<Value>>>);

async fn responses_dispatch_upstream(
    State(capture): State<ResponsesDispatchCapture>,
    Json(body): Json<Value>,
) -> Response {
    if let Ok(mut requests) = capture.0.lock() {
        requests.push(body.clone());
    }
    let text = body["input"].as_str().unwrap_or_default();
    let completed = json!({
        "id":"synthetic-c4-response", "object":"response", "status":"completed",
        "model":"synthetic-scope-model", "output":[{
            "id":"synthetic-c4-message", "type":"message", "role":"assistant", "status":"completed",
            "content":[{"type":"output_text","text":text,"annotations":[]}]
        }]
    });
    if body["stream"] == true {
        let event = json!({"type":"response.completed","response":completed});
        (
            [("content-type", "text/event-stream")],
            format!("event: response.completed\r\ndata: {event}\r\n\r\ndata: [DONE]\r\n\r\n"),
        )
            .into_response()
    } else {
        Json(completed).into_response()
    }
}

// The task only serves a synthetic upstream on a dynamically allocated loopback port.
struct DispatchUpstreamTask(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for DispatchUpstreamTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn c4_dispatch_gateway_responses_preserves_json_and_sse_restoration() -> TestResult {
    let fixture = ResponsesFixture::new();
    let (state, secret) = gateway_dispatch_fixture(&fixture)?;
    let capture = ResponsesDispatchCapture::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = Router::new()
        .route("/v1/responses", post(responses_dispatch_upstream))
        .with_state(capture.clone());
    let _task = DispatchUpstreamTask(tokio::spawn(async move {
        axum::serve(listener, upstream).await
    }));
    let provider = Provider::with_id(
        "synthetic-c4-provider".into(),
        "Synthetic C4 upstream".into(),
        json!({"base_url":format!("http://{address}"), "api_key":"synthetic-c4-upstream-key"}),
        None,
    );
    state.db.save_provider("codex", &provider)?;
    state.db.set_current_provider("codex", &provider.id)?;
    for stream in [false, true] {
        let body = json!({"model":"synthetic-scope-model","input":CONTACT,"stream":stream});
        let mut request = dispatch_request("POST", "/gateway/v1/responses", &body)?;
        request.headers_mut().insert("x-api-key", secret.parse()?);
        request.headers_mut().insert(
            "x-varyai-client-protocol",
            HeaderValue::from_static("gemini_native"),
        );
        let observation = route_scope_test_support::observe();
        let response = gateway::handle(State(state.clone()), request).await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "c4_gateway_responses_status_changed"
        );
        let bytes = response.into_body().collect().await?.to_bytes();
        let wire = std::str::from_utf8(&bytes)?;
        assert!(wire.contains(CONTACT), "c4_gateway_responses_not_restored");
        assert!(
            !wire.contains("rredact-"),
            "c4_gateway_responses_leaked_reference"
        );
        if stream {
            assert!(
                wire.contains("response.completed"),
                "c4_gateway_sse_completion_lost"
            );
            assert!(!wire.contains("response.failed"), "c4_gateway_sse_failed");
        }
        let requests = capture
            .0
            .lock()
            .map_err(|_| "c4_upstream_capture_poisoned")?;
        assert_eq!(
            requests.len(),
            if stream { 2 } else { 1 },
            "c4_gateway_retry_changed"
        );
        let outbound = requests
            .last()
            .and_then(|request| request["input"].as_str())
            .ok_or("c4_missing_upstream_input")?;
        assert!(
            !outbound.contains(CONTACT),
            "c4_gateway_forwarded_sensitive_input"
        );
        assert!(
            outbound.contains("rredact-cn_phone-"),
            "c4_gateway_mapping_lost"
        );
        drop(requests);
        assert_recorded_scope(
            &observation,
            scope(
                Protocol::Responses,
                RequestIngress::Gateway,
                RestorationOperation::Responses,
                if stream {
                    RestorationFraming::Sse
                } else {
                    RestorationFraming::Json
                },
            ),
        );
    }
    Ok(())
}

// Characterization only: exercise existing failover behavior without enabling native admission.
mod c4_actual_cell_characterization {
    use super::*;
    use futures::FutureExt;
    use std::{
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        time::Duration,
    };

    use super::super::{
        http_restoration::{source_copy_test_support, DlpForwardError, DlpPreparedAttempt},
        response_restoration::ArgumentNormalizationMode,
    };
    use crate::proxy::{
        error::ProxyError,
        forwarder::{final_send_binding_test_support, RequestForwarder},
        protocol_bridge::Bridge,
    };

    const SAFE_INPUT: &str = "Synthetic ordinary failover input";
    const MODEL: &str = "synthetic-scope-model";
    const QUERY: &str = "operation=synthetic-c4-failover";

    struct ActualCellUpstream {
        base: String,
        hits: Arc<AtomicUsize>,
        valid: Arc<AtomicUsize>,
        disconnect: Arc<AtomicBool>,
        task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
    }

    impl ActualCellUpstream {
        async fn start(
            disconnect: bool,
            compact: bool,
            stream: bool,
            active: bool,
            key: &'static str,
        ) -> TestResult<Self> {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|_| "c4_actual_cell_listener_failed")?;
            let base = format!(
                "http://{}",
                listener
                    .local_addr()
                    .map_err(|_| "c4_actual_cell_address_failed")?
            );
            let hits = Arc::new(AtomicUsize::new(0));
            let valid = Arc::new(AtomicUsize::new(0));
            let captured_hits = hits.clone();
            let captured_valid = valid.clone();
            let disconnect = Arc::new(AtomicBool::new(disconnect));
            let captured_disconnect = disconnect.clone();
            let task = tokio::spawn(async move {
                // One task owns all connections, including intentional disconnects.
                loop {
                    let (socket, _) = listener.accept().await?;
                    let hits = captured_hits.clone();
                    let valid = captured_valid.clone();
                    let disconnect = captured_disconnect.clone();
                    let service = hyper::service::service_fn(
                        move |request: http::Request<hyper::body::Incoming>| {
                            let hits = hits.clone();
                            let valid = valid.clone();
                            let disconnect = disconnect.clone();
                            async move {
                                hits.fetch_add(1, Ordering::SeqCst);
                                let path = if compact {
                                    "/v1/responses/compact"
                                } else {
                                    "/v1/responses"
                                };
                                let target_ok = request.method() == http::Method::POST
                                    && request.uri().path_and_query().is_some_and(|target| {
                                        target.as_str() == format!("{path}?{QUERY}")
                                    });
                                let headers_ok = request
                                    .headers()
                                    .get(http::header::AUTHORIZATION)
                                    .is_some_and(|value| value == format!("Bearer {key}").as_str());
                                let bytes =
                                    axum::body::to_bytes(Body::new(request.into_body()), 64 * 1024)
                                        .await
                                        .map_err(|_| {
                                            std::io::Error::other("c4_actual_cell_body_failed")
                                        })?;
                                let body: Value = serde_json::from_slice(&bytes).map_err(|_| {
                                    std::io::Error::other("c4_actual_cell_json_failed")
                                })?;
                                let text = body["input"].as_str().ok_or_else(|| {
                                    std::io::Error::other("c4_actual_cell_input_missing")
                                })?;
                                let input_ok = if active {
                                    text.contains("rredact-cn_phone-") && !text.contains(CONTACT)
                                } else {
                                    text == SAFE_INPUT && !text.contains("rredact-")
                                };
                                if !(target_ok
                                    && headers_ok
                                    && body["model"] == MODEL
                                    && body["stream"] == stream
                                    && input_ok)
                                {
                                    return Err(std::io::Error::other(
                                        "c4_actual_cell_request_invalid",
                                    ));
                                }
                                valid.fetch_add(1, Ordering::SeqCst);
                                if disconnect.load(Ordering::SeqCst) {
                                    // Receive the entire prepared request, then close before headers.
                                    return Err(std::io::Error::other(
                                        "c4_actual_cell_disconnect_before_headers",
                                    ));
                                }
                                let mut completed = json!({
                                    "id":"synthetic-c4-failover-response", "object":"response",
                                    "status":"completed", "model":MODEL, "output":[{
                                        "id":"synthetic-c4-failover-message", "type":"message",
                                        "role":"assistant", "status":"completed", "content":[{
                                            "type":"output_text", "text":text, "annotations":[]
                                        }]
                                    }]
                                });
                                if compact {
                                    completed["object"] = json!("response.compaction");
                                    completed["created_at"] = json!(1);
                                    completed["output"]
                                        .as_array_mut()
                                        .ok_or_else(|| {
                                            std::io::Error::other("c4_actual_cell_output_missing")
                                        })?
                                        .push(json!({
                                            "id":"synthetic-c4-compaction", "type":"compaction",
                                            "encrypted_content":"synthetic-c4-opaque"
                                        }));
                                }
                                let mut response = if stream {
                                    let event =
                                        json!({"type":"response.completed","response":completed});
                                    (
                                        [("content-type", "text/event-stream")],
                                        format!("event: response.completed\ndata: {event}\n\ndata: [DONE]\n\n"),
                                    )
                                        .into_response()
                                } else {
                                    Json(completed).into_response()
                                };
                                response.headers_mut().insert(
                                    http::header::CONNECTION,
                                    HeaderValue::from_static("close"),
                                );
                                Ok::<_, std::io::Error>(response)
                            }
                        },
                    );
                    // A service error deliberately disconnects; it is not a synthetic HTTP 5xx.
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(socket), service)
                        .await;
                }
            });
            Ok(Self {
                base,
                hits,
                valid,
                disconnect,
                task: Some(task),
            })
        }

        async fn finish_at(mut self, deadline: tokio::time::Instant) -> TestResult {
            let task = self.task.take().ok_or("c4_actual_cell_task_missing")?;
            task.abort();
            match tokio::time::timeout_at(deadline, task).await {
                Ok(Err(error)) if error.is_cancelled() => Ok(()),
                _ => Err("c4_actual_cell_cleanup_failed".into()),
            }
        }
    }

    impl Drop for ActualCellUpstream {
        fn drop(&mut self) {
            if let Some(task) = &self.task {
                task.abort();
            }
        }
    }

    // Mirror the production principal digest using synthetic fixture headers only.
    fn synthetic_principal(key: &str) -> String {
        use sha2::{Digest, Sha256};
        let name = "authorization";
        let value = format!("Bearer {key}");
        let mut digest = Sha256::new();
        digest.update((name.len() as u64).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
        format!("{:x}", digest.finalize())
    }

    fn assert_actual_binding(
        attempt: &DlpPreparedAttempt,
        provider: &Provider,
        target: &str,
        principal: &str,
        active: bool,
    ) {
        assert!(
            attempt
                .validate_final_send_binding(
                    &provider.id,
                    "openai_responses",
                    target,
                    Some(principal),
                )
                .is_ok(),
            "c4_actual_cell_exact_binding_refused"
        );
        for (id, protocol, target, principal) in [
            (
                "synthetic-c4-other-provider",
                "openai_responses",
                target.to_owned(),
                principal,
            ),
            (
                provider.id.as_str(),
                "gemini_native",
                target.to_owned(),
                principal,
            ),
            (
                &provider.id,
                "openai_responses",
                format!("{target}&unexpected=1"),
                principal,
            ),
            (
                &provider.id,
                "openai_responses",
                target.to_owned(),
                "synthetic-c4-other-principal",
            ),
        ] {
            assert!(
                attempt
                    .validate_final_send_binding(id, protocol, &target, Some(principal))
                    .is_err(),
                "c4_actual_cell_binding_dimension_not_enforced"
            );
        }
        assert!(
            attempt.has_active_tokens() == active,
            "c4_actual_cell_mapping_state_changed"
        );
    }

    // Ingress is supplied through the real scoped admission facade, not HTTP dispatch/auth.
    // Network work uses real reqwest sends; no synthetic transport responses are injected.
    async fn actual_failover_cell(
        ingress: RequestIngress,
        compact: bool,
        stream: bool,
        active: bool,
        unsupported: bool,
    ) -> TestResult {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
        let work_deadline = deadline - Duration::from_secs(1);
        let _ = rustls::crypto::ring::default_provider().install_default();
        let transport = final_send_binding_test_support::install(|_, _| {});
        let fixture = ResponsesFixture::new();
        let state = dispatch_state(Arc::new(Database::memory()?), &fixture);
        let first = tokio::time::timeout_at(
            work_deadline,
            ActualCellUpstream::start(false, compact, stream, active, "synthetic-c4-first-key"),
        )
        .await
        .map_err(|_| "c4_actual_cell_start_deadline_exceeded")??;
        // Every fallible operation after spawning is covered by the shared cleanup below.
        let mut backup = None;
        let work = async {
            backup = Some(
                ActualCellUpstream::start(
                    false,
                    compact,
                    stream,
                    active,
                    "synthetic-c4-backup-key",
                )
                .await?,
            );
            let backup = backup.as_ref().ok_or("c4_actual_cell_backup_missing")?;
            let provider_a = Provider::with_id(
                "synthetic-c4-failover-a".into(),
                "Synthetic C4 failover".into(),
                json!({"base_url":first.base, "api_key":"synthetic-c4-first-key"}),
                None,
            );
            let provider_b = Provider::with_id(
                "synthetic-c4-failover-b".into(),
                provider_a.name.clone(),
                json!({"base_url":backup.base, "api_key":"synthetic-c4-backup-key",
                    "api_format":if unsupported {"gemini_native"} else {"responses"}}),
                None,
            );
            let mut tail = provider_b.clone();
            tail.id = "synthetic-c4-failover-tail".into();
            tail.settings_config["api_format"] = json!("responses");
            let endpoint = if compact {
                "/responses/compact"
            } else {
                "/responses"
            };
            let endpoint = format!("{endpoint}?{QUERY}");
            let target_a = format!("{}/v1{endpoint}", first.base);
            let target_b = format!("{}/v1{endpoint}", backup.base);
            // Gemini is a real missing bridge edge, unlike supported Chat/Anthropic conversions.
            if unsupported {
                let bridge = Bridge::route(&AppType::Codex, &endpoint, &provider_b)
                    .ok_or("c4_actual_cell_missing_unsupported_bridge")?;
                assert!(
                    bridge.input == Protocol::Responses && bridge.output == Protocol::Gemini,
                    "c4_actual_cell_unsupported_route_misclassified"
                );
            }
            let headers = HeaderMap::new();
            let mut body = json!({"model":MODEL,
                "input":if active {CONTACT} else {SAFE_INPUT}, "stream":stream});
            let expected_scope = scope(
                Protocol::Responses,
                ingress,
                if compact {
                    RestorationOperation::ResponsesCompact
                } else {
                    RestorationOperation::Responses
                },
                if stream {
                    RestorationFraming::Sse
                } else {
                    RestorationFraming::Json
                },
            );
            let observation = route_scope_test_support::observe();
            let admission = admit_http_request_with_scope(
                &fixture.gateway,
                &AppType::Codex,
                expected_scope,
                &mut body,
                &headers,
            )?;
            let RestorationAdmission::Admitted(request) = admission else {
                return Err("c4_actual_cell_responses_not_admitted".into());
            };
            assert_recorded_scope(&observation, expected_scope);
            let forwarder = RequestForwarder::new(
                state.provider_router.clone(),
                3,
                state.status.clone(),
                state.current_providers.clone(),
                state.gemini_shadow.clone(),
                state.codex_chat_history.clone(),
                state.failover_manager.clone(),
                None,
                provider_a.id.clone(),
                "synthetic-c4-failover-session".into(),
                false,
                3,
                3,
                Default::default(),
                Default::default(),
                Default::default(),
                2,
            )
            .with_dlp_request(Some(request));
            // Obtain a real sent attempt on this exact admitted request before forcing failure.
            // Failed attempts are not returned by the forwarder; the retry below independently
            // checks its final target/header hook and complete socket request.
            let control_transport = final_send_binding_test_support::install(|_, _| {});
            let mut control = forwarder
                .forward_with_retry(
                    &AppType::Codex,
                    http::Method::POST,
                    &endpoint,
                    body.clone(),
                    headers.clone(),
                    Default::default(),
                    vec![provider_a.clone()],
                )
                .await
                .map_err(|_| "c4_actual_cell_first_control_failed")?;
            assert!(
                control.provider.id == provider_a.id
                    && control.response.status() == StatusCode::OK
                    && control_transport.transport_counts() == (1, 0)
                    && first.hits.load(Ordering::SeqCst) == 1
                    && first.valid.load(Ordering::SeqCst) == 1,
                "c4_actual_cell_first_control_not_sent"
            );
            let control_attempt = control
                .dlp_attempt
                .take()
                .ok_or("c4_actual_cell_first_attempt_missing")?;
            assert_actual_binding(
                &control_attempt,
                &provider_a,
                &target_a,
                &synthetic_principal("synthetic-c4-first-key"),
                active,
            );
            drop(control_attempt);
            control
                .response
                .bytes_with_limit(64 * 1024)
                .await
                .map_err(|_| "c4_actual_cell_first_control_incomplete")?;
            drop(control_transport);
            first.hits.store(0, Ordering::SeqCst);
            first.valid.store(0, Ordering::SeqCst);
            first.disconnect.store(true, Ordering::SeqCst);
            let first_target_valid = Arc::new(AtomicBool::new(false));
            let captured_target_valid = first_target_valid.clone();
            let observed_target = target_a.clone();
            let first_transport =
                final_send_binding_test_support::install(move |target, headers| {
                    captured_target_valid.store(
                        *target == observed_target
                            && headers
                                .get(http::header::AUTHORIZATION)
                                .is_some_and(|value| value == "Bearer synthetic-c4-first-key"),
                        Ordering::SeqCst,
                    );
                });
            let source = source_copy_test_support::install();
            let result = forwarder
                .forward_with_retry(
                    &AppType::Codex,
                    http::Method::POST,
                    &endpoint,
                    body,
                    headers,
                    Default::default(),
                    vec![provider_a.clone(), provider_b.clone(), tail],
                )
                .await;
            assert!(
                source.snapshot().attempts == 2
                    && first.hits.load(Ordering::SeqCst) == 1
                    && first.valid.load(Ordering::SeqCst) == 1
                    && first_target_valid.load(Ordering::SeqCst),
                "c4_actual_cell_failover_not_executed"
            );
            let expected_transports = if unsupported { (1, 0) } else { (2, 0) };
            assert!(
                first_transport.transport_counts() == expected_transports,
                "c4_actual_cell_transport_count_changed"
            );
            assert!(
                state.status.read().await.current_provider_id.as_deref()
                    == Some(provider_b.id.as_str()),
                "c4_actual_cell_backup_not_selected_or_tail_reached"
            );
            if unsupported {
                // The admitted request reached the fallback selection, not disabled admission.
                // The bridge refuses before owned preparation/send, including zero mappings.
                let error = match result {
                    Err(error) => error,
                    Ok(_) => return Err("c4_actual_cell_unsupported_fallback_sent".into()),
                };
                assert!(
                    matches!(
                        error.error,
                        ProxyError::DlpForward(DlpForwardError::RestoreUnsupported)
                    ) && error.provider.is_none()
                        && backup.hits.load(Ordering::SeqCst) == 0
                        && backup.valid.load(Ordering::SeqCst) == 0,
                    "c4_actual_cell_unsupported_barrier_changed"
                );
            } else {
                let mut delivered =
                    result.map_err(|_| "c4_actual_cell_supported_fallback_failed")?;
                assert!(
                    delivered.provider.id == provider_b.id
                        && delivered.response.status() == StatusCode::OK
                        && backup.hits.load(Ordering::SeqCst) == 1
                        && backup.valid.load(Ordering::SeqCst) == 1,
                    "c4_actual_cell_supported_fallback_not_delivered"
                );
                let attempt = delivered
                    .dlp_attempt
                    .take()
                    .ok_or("c4_actual_cell_backup_attempt_missing")?;
                assert_actual_binding(
                    &attempt,
                    &provider_b,
                    &target_b,
                    &synthetic_principal("synthetic-c4-backup-key"),
                    active,
                );
                assert!(
                    attempt.endpoint_is_compact == compact,
                    "c4_actual_cell_operation_changed"
                );
                let status = delivered.response.status();
                let response_headers = delivered.response.headers().clone();
                let bytes = delivered
                    .response
                    .bytes_with_limit(64 * 1024)
                    .await
                    .map_err(|_| "c4_actual_cell_response_incomplete")?;
                let mut response = Response::new(Body::from(bytes));
                *response.status_mut() = status;
                *response.headers_mut() = response_headers;
                let restored = attempt
                    .restore_client_http_response(response, ArgumentNormalizationMode::Exact, None)
                    .await;
                assert!(
                    restored.status() == StatusCode::OK,
                    "c4_actual_cell_restore_failed"
                );
                let bytes = axum::body::to_bytes(restored.into_body(), 64 * 1024)
                    .await
                    .map_err(|_| "c4_actual_cell_restored_body_failed")?;
                let wire = std::str::from_utf8(&bytes)
                    .map_err(|_| "c4_actual_cell_restored_encoding_failed")?;
                assert!(
                    wire.contains(if active { CONTACT } else { SAFE_INPUT })
                        && !wire.contains("rredact-")
                        && !wire.contains("response.failed")
                        && (!stream || wire.contains("response.completed")),
                    "c4_actual_cell_supported_restoration_changed"
                );
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        };
        let outcome = tokio::time::timeout_at(
            work_deadline,
            std::panic::AssertUnwindSafe(work).catch_unwind(),
        )
        .await;
        // Abort and join both task-owned connection loops on errors, panic, or timeout.
        let cleanup_first = first.finish_at(deadline).await;
        let cleanup_backup = match backup {
            Some(backup) => backup.finish_at(deadline).await,
            None => Ok(()),
        };
        cleanup_first?;
        cleanup_backup?;
        assert!(
            transport.transport_counts() == (0, 0),
            "c4_actual_cell_transport_guard_leaked"
        );
        match outcome.map_err(|_| "c4_actual_cell_work_deadline_exceeded")? {
            Ok(result) => result?,
            Err(panic) => std::panic::resume_unwind(panic),
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial]
    async fn c4_characterization_actual_failover_rejects_gemini_backup_active_and_zero_mapping(
    ) -> TestResult {
        for ingress in [RequestIngress::Direct, RequestIngress::Gateway] {
            for stream in [false, true] {
                for active in [false, true] {
                    actual_failover_cell(ingress, false, stream, active, true).await?;
                }
            }
        }
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial]
    async fn c4_characterization_actual_failover_supported_responses_cells_restore_active_and_zero_mapping(
    ) -> TestResult {
        for ingress in [RequestIngress::Direct, RequestIngress::Gateway] {
            for (compact, stream) in [(false, false), (false, true), (true, false)] {
                for active in [false, true] {
                    actual_failover_cell(ingress, compact, stream, active, false).await?;
                }
            }
        }
        Ok(())
    }
}

#[tokio::test]
#[serial_test::serial]
async fn m3_anthropic_trusted_scope_cannot_be_selected_by_headers_or_wrong_cells() -> TestResult {
    let fixture = ResponsesFixture::new();
    for app in [AppType::Claude, AppType::ClaudeDesktop] {
        for (ingress, operation, framing) in [
            (
                RequestIngress::Gateway,
                RestorationOperation::Messages,
                RestorationFraming::Json,
            ),
            (
                RequestIngress::Gateway,
                RestorationOperation::Messages,
                RestorationFraming::Sse,
            ),
            (
                RequestIngress::Direct,
                RestorationOperation::Responses,
                RestorationFraming::Json,
            ),
            (
                RequestIngress::Direct,
                RestorationOperation::ChatCompletions,
                RestorationFraming::Sse,
            ),
            (
                RequestIngress::Direct,
                RestorationOperation::Unsupported,
                RestorationFraming::Json,
            ),
            (
                RequestIngress::Direct,
                RestorationOperation::Messages,
                RestorationFraming::Unsupported,
            ),
        ] {
            for claimed_ingress in ["direct", "gateway", "responses"] {
                let mut headers = HeaderMap::new();
                headers.insert(
                    "x-varyai-ingress",
                    HeaderValue::from_static(claimed_ingress),
                );
                headers.insert(
                    "x-varyai-client-protocol",
                    HeaderValue::from_static("anthropic"),
                );
                let mut body = json!({"messages":[{"role":"user","content":CONTACT}]});
                let original = body.clone();
                let admission = admit_http_request_with_scope(
                    &fixture.gateway,
                    &app,
                    scope(Protocol::Anthropic, ingress, operation, framing),
                    &mut body,
                    &headers,
                )?;
                assert_fixed_scope_error(admission, &app).await?;
                assert!(body == original, "m3_wrong_anthropic_scope_mutated_request");
                assert_eq!(fixture.store.stats()?.entries, 0);
            }
        }
    }
    Ok(())
}
